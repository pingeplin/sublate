use std::future::Future;
use std::time::Duration;

use crate::error::{AppError, AppResult};

/// How one attempt at a request failed.
pub enum Failure {
    /// May succeed on a later attempt; the server may say when to try again.
    Transient {
        error: AppError,
        retry_after: Option<Duration>,
    },
    Fatal(AppError),
}

impl Failure {
    fn into_error(self) -> AppError {
        match self {
            Self::Transient { error, .. } | Self::Fatal(error) => error,
        }
    }
}

/// Decides whether a failed attempt is tried again and how long to wait first.
#[derive(Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    /// The wait after attempt `n` when the server names no time.
    pub backoff: fn(u32) -> Duration,
}

impl RetryPolicy {
    pub async fn run<T, F, Fut>(&self, mut attempt_once: F) -> AppResult<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, Failure>>,
    {
        let mut attempt = 1;
        loop {
            let failure = match attempt_once().await {
                Ok(value) => return Ok(value),
                Err(failure) => failure,
            };
            match self.delay(attempt, &failure) {
                Some(wait) => tokio::time::sleep(wait).await,
                None => return Err(failure.into_error()),
            }
            attempt += 1;
        }
    }

    /// The wait before the next attempt, or `None` when `failure` ends the request.
    fn delay(&self, attempt: u32, failure: &Failure) -> Option<Duration> {
        match failure {
            Failure::Transient { retry_after, .. } if attempt < self.max_attempts => {
                Some(retry_after.unwrap_or_else(|| (self.backoff)(attempt)))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    const POLICY: RetryPolicy = RetryPolicy {
        max_attempts: 3,
        backoff: |attempt| Duration::from_millis(u64::from(attempt)),
    };

    fn transient(retry_after: Option<Duration>) -> Failure {
        Failure::Transient {
            error: AppError::Api("busy".into()),
            retry_after,
        }
    }

    fn fatal() -> Failure {
        Failure::Fatal(AppError::Api("denied".into()))
    }

    #[test]
    fn the_server_s_retry_after_wins_over_the_backoff() {
        let wait = Duration::from_millis(250);
        assert_eq!(POLICY.delay(1, &transient(Some(wait))), Some(wait));
        assert_eq!(POLICY.delay(2, &transient(None)), Some(Duration::from_millis(2)));
    }

    #[test]
    fn the_last_attempt_and_a_fatal_failure_are_never_retried() {
        assert_eq!(POLICY.delay(3, &transient(None)), None);
        assert_eq!(POLICY.delay(1, &fatal()), None);
    }

    #[tokio::test]
    async fn a_transient_failure_is_retried_until_it_succeeds() {
        let attempts = AtomicU32::new(0);

        let result = POLICY
            .run(|| async {
                match attempts.fetch_add(1, Ordering::SeqCst) {
                    0 => Err(transient(Some(Duration::ZERO))),
                    _ => Ok("done"),
                }
            })
            .await;

        assert_eq!(result.unwrap(), "done");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retrying_stops_at_the_attempt_limit_with_the_last_error() {
        let attempts = AtomicU32::new(0);

        let result: AppResult<()> = POLICY
            .run(|| async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(transient(Some(Duration::ZERO)))
            })
            .await;

        assert_eq!(result.unwrap_err().to_string(), "Claude API error: busy");
        assert_eq!(attempts.load(Ordering::SeqCst), POLICY.max_attempts);
    }

    #[tokio::test]
    async fn a_fatal_failure_ends_the_request_at_once() {
        let attempts = AtomicU32::new(0);

        let result: AppResult<()> = POLICY
            .run(|| async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(fatal())
            })
            .await;

        assert_eq!(result.unwrap_err().to_string(), "Claude API error: denied");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
