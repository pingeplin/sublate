pub mod claude;

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::{StreamExt, TryStreamExt};

use crate::error::{AppError, AppResult};
use crate::subtitle::Cue;

#[derive(Clone, Copy)]
pub struct BatchRequest<'a> {
    pub source_language: &'a str,
    pub target_language: &'a str,
    pub context: &'a [String],
    pub lines: &'a [String],
}

#[async_trait]
pub trait Translator: Send + Sync {
    /// Returns exactly one translation per input line, in order.
    async fn translate(&self, request: BatchRequest<'_>) -> AppResult<Vec<String>>;
}

#[derive(Debug, Clone, Copy)]
pub struct TranslationPlan {
    pub batch_size: usize,
    pub context_size: usize,
    pub concurrency: usize,
    pub max_attempts: usize,
}

impl Default for TranslationPlan {
    fn default() -> Self {
        Self {
            batch_size: 40,
            context_size: 6,
            concurrency: 4,
            max_attempts: 3,
        }
    }
}

pub async fn translate_cues(
    translator: &dyn Translator,
    cues: &[Cue],
    source_language: &str,
    target_language: &str,
    plan: TranslationPlan,
    on_progress: impl Fn(usize, usize) + Sync,
) -> AppResult<Vec<Cue>> {
    let lines: Vec<String> = cues.iter().map(|c| c.text.clone()).collect();
    let job = Job {
        translator,
        lines: &lines,
        source_language,
        target_language,
        plan,
    };
    let batch_size = plan.batch_size.max(1);
    let batches = lines.len().div_ceil(batch_size);
    let total = lines.len();
    let done = AtomicUsize::new(0);
    on_progress(0, total);

    let mut translated: Vec<Vec<String>> = vec![Vec::new(); batches];
    let mut completed = futures::stream::iter(0..batches)
        .map(|batch| {
            let range = batch * batch_size..((batch + 1) * batch_size).min(total);
            let (job, done, on_progress) = (&job, &done, &on_progress);
            async move {
                let result = job.translate(range).await?;
                let count = result.len();
                on_progress(done.fetch_add(count, Ordering::SeqCst) + count, total);
                Ok::<_, AppError>((batch, result))
            }
        })
        .buffer_unordered(plan.concurrency.max(1));
    while let Some((batch, result)) = completed.try_next().await? {
        translated[batch] = result;
    }

    Ok(cues
        .iter()
        .zip(translated.into_iter().flatten())
        .map(|(cue, text)| Cue { text, ..cue.clone() })
        .collect())
}

struct Job<'a> {
    translator: &'a dyn Translator,
    lines: &'a [String],
    source_language: &'a str,
    target_language: &'a str,
    plan: TranslationPlan,
}

impl Job<'_> {
    /// Retries malformed output. A response cut off at the output limit is split in half
    /// instead, since resending the same batch would most likely be cut off again.
    fn translate(&self, range: Range<usize>) -> BoxFuture<'_, AppResult<Vec<String>>> {
        Box::pin(async move {
            let mut attempt = 1;
            loop {
                let result = self
                    .translator
                    .translate(self.request(range.clone()))
                    .await
                    .and_then(|out| ensure_aligned(out, range.len()));
                match result {
                    Err(AppError::Truncated) if range.len() > 1 => {
                        let mid = range.start + range.len() / 2;
                        let mut lines = self.translate(range.start..mid).await?;
                        lines.extend(self.translate(mid..range.end).await?);
                        return Ok(lines);
                    }
                    Err(e) if e.is_retryable() && attempt < self.plan.max_attempts => attempt += 1,
                    other => return other,
                }
            }
        })
    }

    fn request(&self, range: Range<usize>) -> BatchRequest<'_> {
        BatchRequest {
            source_language: self.source_language,
            target_language: self.target_language,
            context: &self.lines[range.start.saturating_sub(self.plan.context_size)..range.start],
            lines: &self.lines[range],
        }
    }
}

fn ensure_aligned(out: Vec<String>, expected: usize) -> AppResult<Vec<String>> {
    if out.len() == expected {
        Ok(out)
    } else {
        Err(AppError::Translation(format!(
            "expected {expected} lines, got {}",
            out.len()
        )))
    }
}
