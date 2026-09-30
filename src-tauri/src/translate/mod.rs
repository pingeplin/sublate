pub mod claude;

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
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
    let batch_size = plan.batch_size.max(1);
    let total = lines.len().div_ceil(batch_size);
    let done = AtomicUsize::new(0);

    let translated: Vec<Vec<String>> = futures::stream::iter(0..total)
        .map(|batch| {
            let start = batch * batch_size;
            let end = (start + batch_size).min(lines.len());
            let request = BatchRequest {
                source_language,
                target_language,
                context: &lines[start.saturating_sub(plan.context_size)..start],
                lines: &lines[start..end],
            };
            let done = &done;
            let on_progress = &on_progress;
            async move {
                let result = translate_with_retry(translator, request, plan.max_attempts).await?;
                on_progress(done.fetch_add(1, Ordering::SeqCst) + 1, total);
                Ok::<_, AppError>(result)
            }
        })
        .buffered(plan.concurrency.max(1))
        .try_collect()
        .await?;

    Ok(cues
        .iter()
        .zip(translated.into_iter().flatten())
        .map(|(cue, text)| Cue { text, ..cue.clone() })
        .collect())
}

async fn translate_with_retry(
    translator: &dyn Translator,
    request: BatchRequest<'_>,
    max_attempts: usize,
) -> AppResult<Vec<String>> {
    let mut attempt = 1;
    loop {
        let result = translator
            .translate(request)
            .await
            .and_then(|out| ensure_aligned(out, request.lines.len()));
        match result {
            Err(AppError::Translation(_)) if attempt < max_attempts => attempt += 1,
            other => return other,
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
