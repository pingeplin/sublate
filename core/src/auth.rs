use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Mutex, OnceCell};

use crate::error::{AppError, AppResult};
use crate::process_env::ProcessEnv;

pub const DEFAULT_ANT_PROFILE: &str = "contents-title";
const ANT: &str = "ant";
const SAVED_KEY_SOURCE: &str = "your saved API key";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const TOKEN_TTL: Duration = Duration::from_secs(60);
const MINT_TIMEOUT: Duration = Duration::from_secs(30);

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
}

/// A provider with one fixed origin the UI can name.
pub trait CredentialSource: CredentialProvider {
    fn describe(&self) -> String;
}

pub struct StaticApiKey(String);

#[async_trait]
impl CredentialProvider for StaticApiKey {
    async fn credential(&self) -> AppResult<Credential> {
        Ok(Credential::ApiKey(self.0.clone()))
    }
}

impl CredentialSource for StaticApiKey {
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
        let run = self
            .env
            .command(ANT)
            .args(["--profile", &self.profile, "auth", "print-credentials", "--access-token"])
            .output();
        let output = tokio::time::timeout(MINT_TIMEOUT, run)
            .await
            .map_err(|_| AppError::Auth(format!("`ant` did not respond within {}s", MINT_TIMEOUT.as_secs())))?
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
}

impl CredentialSource for AntProfile {
    fn describe(&self) -> String {
        format!("ant profile '{}'", self.profile)
    }
}

/// What the user's shell offers: an exported key, else an `ant` profile when `ant` is installed.
pub fn resolve_provider(env: &ProcessEnv) -> Option<Box<dyn CredentialSource>> {
    if let Some(key) = env.var("ANTHROPIC_API_KEY") {
        return Some(Box::new(StaticApiKey(key.to_string())));
    }
    env.resolve(ANT)?;
    let profile = env.var("ANTHROPIC_PROFILE").unwrap_or(DEFAULT_ANT_PROFILE);
    Some(Box::new(AntProfile::new(env.clone(), profile)))
}

/// The key saved in the app wins; the shell is only consulted, once, when there is none.
#[derive(Default)]
pub struct CredentialChain {
    saved_key: std::sync::RwLock<Option<String>>,
    shell: OnceCell<Option<Box<dyn CredentialSource>>>,
}

impl CredentialChain {
    #[cfg(test)]
    fn with_shell(shell: Option<Box<dyn CredentialSource>>) -> Self {
        Self {
            saved_key: Default::default(),
            shell: OnceCell::new_with(Some(shell)),
        }
    }

    pub fn save_key(&self, key: Option<String>) {
        let key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        *self.saved_key.write().expect("saved key lock") = key;
    }

    /// Names where the credential comes from, or `None` when the user has yet to provide one.
    pub async fn source(&self) -> Option<String> {
        if self.saved_key().is_some() {
            return Some(SAVED_KEY_SOURCE.into());
        }
        self.shell().await.map(CredentialSource::describe)
    }

    fn saved_key(&self) -> Option<String> {
        self.saved_key.read().expect("saved key lock").clone()
    }

    async fn shell(&self) -> Option<&dyn CredentialSource> {
        let resolved = self.shell.get_or_init(|| async { resolve_provider(&ProcessEnv::load().await) });
        resolved.await.as_deref()
    }
}

#[async_trait]
impl CredentialProvider for CredentialChain {
    async fn credential(&self) -> AppResult<Credential> {
        if let Some(key) = self.saved_key() {
            return Ok(Credential::ApiKey(key));
        }
        match self.shell().await {
            Some(provider) => provider.credential().await,
            None => Err(AppError::Auth("no Anthropic API key; add one in Settings (⌘,)".into())),
        }
    }
}

#[async_trait]
impl<T: CredentialProvider + ?Sized> CredentialProvider for std::sync::Arc<T> {
    async fn credential(&self) -> AppResult<Credential> {
        (**self).credential().await
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

    fn path_with_ant() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(ANT), "").unwrap();
        dir
    }

    #[test]
    fn provider_prefers_api_key_then_named_then_default_profile() {
        let bin = path_with_ant();
        let path = bin.path().to_str().unwrap();
        let describe = |env: ProcessEnv| resolve_provider(&env).unwrap().describe();
        assert_eq!(
            describe(ProcessEnv::from_vars([("ANTHROPIC_API_KEY", "k"), ("ANTHROPIC_PROFILE", "p")])),
            "ANTHROPIC_API_KEY"
        );
        assert_eq!(
            describe(ProcessEnv::from_vars([("ANTHROPIC_PROFILE", "p"), ("PATH", path)])),
            "ant profile 'p'"
        );
        assert_eq!(
            describe(ProcessEnv::from_vars([("ANTHROPIC_API_KEY", ""), ("PATH", path)])),
            format!("ant profile '{DEFAULT_ANT_PROFILE}'")
        );
    }

    #[test]
    fn a_shell_without_a_key_or_ant_offers_nothing() {
        let empty = tempfile::tempdir().unwrap();
        let env = ProcessEnv::from_vars([("ANTHROPIC_PROFILE", "p"), ("PATH", empty.path().to_str().unwrap())]);
        assert!(resolve_provider(&env).is_none());
    }

    fn shell_key(key: &str) -> Option<Box<dyn CredentialSource>> {
        Some(Box::new(StaticApiKey(key.into())))
    }

    #[tokio::test]
    async fn the_saved_key_wins_over_the_shell() {
        let chain = CredentialChain::with_shell(shell_key("from-shell"));
        assert_eq!(chain.source().await.as_deref(), Some("ANTHROPIC_API_KEY"));
        assert!(chain.credential().await.unwrap() == Credential::ApiKey("from-shell".into()));

        chain.save_key(Some("  saved\n".into()));
        assert_eq!(chain.source().await.as_deref(), Some(SAVED_KEY_SOURCE));
        assert!(chain.credential().await.unwrap() == Credential::ApiKey("saved".into()));

        chain.save_key(Some("  ".into()));
        assert_eq!(chain.source().await.as_deref(), Some("ANTHROPIC_API_KEY"));
    }

    #[tokio::test]
    async fn a_saved_key_never_consults_the_shell() {
        let chain = CredentialChain::default();
        chain.save_key(Some("saved".into()));

        assert!(chain.credential().await.unwrap() == Credential::ApiKey("saved".into()));
        assert_eq!(chain.source().await.as_deref(), Some(SAVED_KEY_SOURCE));
        assert!(!chain.shell.initialized());
    }

    #[tokio::test]
    async fn without_any_credential_the_error_points_at_settings() {
        let chain = CredentialChain::with_shell(None);
        assert_eq!(chain.source().await, None);
        let err = chain.credential().await.err().unwrap();
        assert_eq!(
            err.to_string(),
            "authentication failed: no Anthropic API key; add one in Settings (⌘,)"
        );
    }

    #[test]
    fn api_key_uses_x_api_key_without_beta() {
        let cred = Credential::ApiKey("k".into());
        assert_eq!(cred.header(), ("x-api-key", "k".to_string()));
        assert_eq!(cred.beta(), None);
    }
}
