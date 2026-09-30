use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::process_env::ProcessEnv;

pub const DEFAULT_ANT_PROFILE: &str = "contents-title";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const TOKEN_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    ApiKey(String),
    OAuth(String),
}

impl Credential {
    pub fn header(&self) -> (&'static str, String) {
        match self {
            Self::ApiKey(key) => ("x-api-key", key.clone()),
            Self::OAuth(token) => ("authorization", format!("Bearer {token}")),
        }
    }

    pub fn beta(&self) -> Option<&'static str> {
        matches!(self, Self::OAuth(_)).then_some(OAUTH_BETA)
    }
}

#[async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn credential(&self) -> AppResult<Credential>;
    fn describe(&self) -> String;
}

pub struct StaticApiKey(String);

#[async_trait]
impl CredentialProvider for StaticApiKey {
    async fn credential(&self) -> AppResult<Credential> {
        Ok(Credential::ApiKey(self.0.clone()))
    }

    fn describe(&self) -> String {
        "ANTHROPIC_API_KEY".into()
    }
}

/// Short-lived OAuth token minted by `ant auth print-credentials`, which refreshes as needed.
pub struct AntProfile {
    env: ProcessEnv,
    profile: String,
    cached: Mutex<Option<(String, Instant)>>,
}

impl AntProfile {
    pub fn new(env: ProcessEnv, profile: impl Into<String>) -> Self {
        Self {
            env,
            profile: profile.into(),
            cached: Mutex::new(None),
        }
    }

    async fn mint(&self) -> AppResult<String> {
        let output = self
            .env
            .command("ant")
            .args(["--profile", &self.profile, "auth", "print-credentials", "--access-token"])
            .output()
            .await
            .map_err(|e| AppError::Auth(format!("cannot run `ant`: {e}")))?;
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if output.status.success() && !token.is_empty() {
            Ok(token)
        } else {
            Err(AppError::Auth(format!(
                "`ant` profile '{}' has no usable token; run `ant auth login --profile {}` ({})",
                self.profile,
                self.profile,
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

#[async_trait]
impl CredentialProvider for AntProfile {
    async fn credential(&self) -> AppResult<Credential> {
        let mut cached = self.cached.lock().await;
        if let Some((token, minted)) = cached.as_ref() {
            if minted.elapsed() < TOKEN_TTL {
                return Ok(Credential::OAuth(token.clone()));
            }
        }
        let token = self.mint().await?;
        *cached = Some((token.clone(), Instant::now()));
        Ok(Credential::OAuth(token))
    }

    fn describe(&self) -> String {
        format!("ant profile '{}'", self.profile)
    }
}

pub fn resolve_provider(env: &ProcessEnv) -> Box<dyn CredentialProvider> {
    match env.var("ANTHROPIC_API_KEY") {
        Some(key) => Box::new(StaticApiKey(key.to_string())),
        None => {
            let profile = env.var("ANTHROPIC_PROFILE").unwrap_or(DEFAULT_ANT_PROFILE);
            Box::new(AntProfile::new(env.clone(), profile))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_uses_bearer_and_beta() {
        let cred = Credential::OAuth("t".into());
        assert_eq!(cred.header(), ("authorization", "Bearer t".to_string()));
        assert_eq!(cred.beta(), Some(OAUTH_BETA));
    }

    #[test]
    fn provider_prefers_api_key_then_named_then_default_profile() {
        let describe = |env: ProcessEnv| resolve_provider(&env).describe();
        assert_eq!(
            describe(ProcessEnv::from_vars([("ANTHROPIC_API_KEY", "k"), ("ANTHROPIC_PROFILE", "p")])),
            "ANTHROPIC_API_KEY"
        );
        assert_eq!(describe(ProcessEnv::from_vars([("ANTHROPIC_PROFILE", "p")])), "ant profile 'p'");
        assert_eq!(
            describe(ProcessEnv::from_vars([("ANTHROPIC_API_KEY", "")])),
            format!("ant profile '{DEFAULT_ANT_PROFILE}'")
        );
    }

    #[test]
    fn api_key_uses_x_api_key_without_beta() {
        let cred = Credential::ApiKey("k".into());
        assert_eq!(cred.header(), ("x-api-key", "k".to_string()));
        assert_eq!(cred.beta(), None);
    }
}
