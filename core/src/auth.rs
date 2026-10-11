use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Mutex, OnceCell};

use crate::error::{AppError, AppResult};
use crate::process_env::ProcessEnv;

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

/// A provider whose origin the UI can name.
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

/// What the user saved in the app, a key or the name of a profile. Blank text counts as nothing.
#[derive(Default)]
pub struct SavedSetting(RwLock<Option<String>>);

impl SavedSetting {
    fn set(&self, text: Option<String>) {
        let text = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        *self.0.write().expect("saved setting lock") = text;
    }

    fn get(&self) -> Option<String> {
        self.0.read().expect("saved setting lock").clone()
    }
}

struct Minted {
    token: String,
    profile: Option<String>,
    at: Instant,
}

/// Short-lived OAuth token minted by `ant auth print-credentials`, which refreshes as needed.
/// The profile chosen in the app is shared with whoever saves it, so a change reaches a
/// provider that is already in use.
pub struct AntProfile {
    env: ProcessEnv,
    chosen: Arc<SavedSetting>,
    cached: Mutex<Option<Minted>>,
}

impl AntProfile {
    pub fn new(env: ProcessEnv, chosen: Arc<SavedSetting>) -> Self {
        Self {
            env,
            chosen,
            cached: Mutex::new(None),
        }
    }

    /// The profile chosen in the app, else the one the shell names. With neither, `ant` is
    /// left to use its own active profile.
    fn profile(&self) -> Option<String> {
        self.chosen.get().or_else(|| self.env.var("ANTHROPIC_PROFILE").map(str::to_string))
    }

    async fn mint(&self, profile: Option<&str>) -> AppResult<String> {
        let mut command = self.env.command(ANT);
        if let Some(profile) = profile {
            command.args(["--profile", profile]);
        }
        let run = command.args(["auth", "print-credentials", "--access-token"]).output();
        let output = tokio::time::timeout(MINT_TIMEOUT, run)
            .await
            .map_err(|_| AppError::Auth(format!("`ant` did not respond within {}s", MINT_TIMEOUT.as_secs())))?
            .map_err(|e| AppError::Auth(format!("cannot run `ant`: {e}")))?;
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if output.status.success() && !token.is_empty() {
            Ok(token)
        } else {
            Err(AppError::Auth(format!(
                "{} has no usable token; run `{}` ({})",
                describe_profile(profile),
                login_command(profile),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

fn describe_profile(profile: Option<&str>) -> String {
    match profile {
        Some(name) => format!("ant profile '{name}'"),
        None => "the active ant profile".into(),
    }
}

fn login_command(profile: Option<&str>) -> String {
    match profile {
        Some(name) => format!("ant auth login --profile {name}"),
        None => "ant auth login".into(),
    }
}

#[async_trait]
impl CredentialProvider for AntProfile {
    async fn credential(&self) -> AppResult<Credential> {
        let mut cached = self.cached.lock().await;
        let profile = self.profile();
        if let Some(minted) = cached.as_ref() {
            if minted.profile == profile && minted.at.elapsed() < TOKEN_TTL {
                return Ok(Credential::OAuth(minted.token.clone()));
            }
        }
        let token = self.mint(profile.as_deref()).await?;
        *cached = Some(Minted {
            token: token.clone(),
            profile,
            at: Instant::now(),
        });
        Ok(Credential::OAuth(token))
    }
}

impl CredentialSource for AntProfile {
    fn describe(&self) -> String {
        describe_profile(self.profile().as_deref())
    }
}

/// What the user's shell offers: an exported key, else an `ant` profile when `ant` is installed.
pub fn resolve_provider(env: &ProcessEnv, chosen: Arc<SavedSetting>) -> Option<Box<dyn CredentialSource>> {
    if let Some(key) = env.var("ANTHROPIC_API_KEY") {
        return Some(Box::new(StaticApiKey(key.to_string())));
    }
    env.resolve(ANT)?;
    Some(Box::new(AntProfile::new(env.clone(), chosen)))
}

/// The key saved in the app wins; the shell is only consulted, once, when there is none.
#[derive(Default)]
pub struct CredentialChain {
    saved_key: SavedSetting,
    ant_profile: Arc<SavedSetting>,
    shell: OnceCell<Option<Box<dyn CredentialSource>>>,
}

impl CredentialChain {
    #[cfg(test)]
    fn with_shell(shell: Option<Box<dyn CredentialSource>>) -> Self {
        Self {
            shell: OnceCell::new_with(Some(shell)),
            ..Self::default()
        }
    }

    pub fn save_key(&self, key: Option<String>) {
        self.saved_key.set(key);
    }

    /// The `ant` profile to use when the shell offers `ant`; `None` leaves the choice to the
    /// shell, and then to `ant`.
    pub fn save_profile(&self, name: Option<String>) {
        self.ant_profile.set(name);
    }

    /// Names where the credential comes from, or `None` when the user has yet to provide one.
    pub async fn source(&self) -> Option<String> {
        if self.saved_key.get().is_some() {
            return Some(SAVED_KEY_SOURCE.into());
        }
        self.shell().await.map(CredentialSource::describe)
    }

    async fn shell(&self) -> Option<&dyn CredentialSource> {
        let resolve = || async { resolve_provider(&ProcessEnv::load().await, Arc::clone(&self.ant_profile)) };
        self.shell.get_or_init(resolve).await.as_deref()
    }
}

#[async_trait]
impl CredentialProvider for CredentialChain {
    async fn credential(&self) -> AppResult<Credential> {
        if let Some(key) = self.saved_key.get() {
            return Ok(Credential::ApiKey(key));
        }
        match self.shell().await {
            Some(provider) => provider.credential().await,
            None => Err(AppError::Auth("no Anthropic API key; add one in Settings (⌘,)".into())),
        }
    }
}

#[async_trait]
impl<T: CredentialProvider + ?Sized> CredentialProvider for Arc<T> {
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

    fn path_with_scripted_ant(script: &str) -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let ant = dir.path().join(ANT);
        std::fs::write(&ant, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&ant, std::fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    /// A stand-in for `ant` that answers with its own arguments and counts how often it ran.
    fn path_with_ant() -> tempfile::TempDir {
        path_with_scripted_ant("echo run >> \"$0.runs\"; echo \"$*\"")
    }

    fn runs(bin: &tempfile::TempDir) -> usize {
        let log = std::fs::read_to_string(bin.path().join("ant.runs")).unwrap_or_default();
        log.lines().count()
    }

    /// The shell of a user who has `ant`. An empty `profile` stands for no `ANTHROPIC_PROFILE`.
    fn shell_with(bin: &tempfile::TempDir, profile: &str) -> ProcessEnv {
        ProcessEnv::from_vars([("ANTHROPIC_PROFILE", profile), ("PATH", bin.path().to_str().unwrap())])
    }

    fn chosen(name: Option<&str>) -> Arc<SavedSetting> {
        let choice = Arc::new(SavedSetting::default());
        choice.set(name.map(String::from));
        choice
    }

    fn token(arguments: &str) -> Credential {
        Credential::OAuth(arguments.into())
    }

    #[test]
    fn provider_prefers_the_api_key_and_then_ant() {
        let bin = path_with_ant();
        let path = bin.path().to_str().unwrap();
        let describe = |env: ProcessEnv| resolve_provider(&env, chosen(Some("mine"))).unwrap().describe();
        assert_eq!(
            describe(ProcessEnv::from_vars([("ANTHROPIC_API_KEY", "k"), ("PATH", path)])),
            "ANTHROPIC_API_KEY"
        );
        assert_eq!(
            describe(ProcessEnv::from_vars([("ANTHROPIC_API_KEY", ""), ("PATH", path)])),
            "ant profile 'mine'"
        );
    }

    #[test]
    fn a_shell_without_a_key_or_ant_offers_nothing() {
        let empty = tempfile::tempdir().unwrap();
        let env = ProcessEnv::from_vars([("ANTHROPIC_PROFILE", "p"), ("PATH", empty.path().to_str().unwrap())]);
        assert!(resolve_provider(&env, chosen(Some("mine"))).is_none());
    }

    #[tokio::test]
    async fn the_profile_chosen_in_the_app_wins_over_the_one_the_shell_names() {
        let bin = path_with_ant();
        let ant = AntProfile::new(shell_with(&bin, "shell"), chosen(Some("mine")));

        assert_eq!(ant.describe(), "ant profile 'mine'");
        assert!(ant.credential().await.unwrap() == token("--profile mine auth print-credentials --access-token"));
    }

    #[tokio::test]
    async fn without_a_chosen_profile_the_one_the_shell_names_is_used() {
        let bin = path_with_ant();
        let ant = AntProfile::new(shell_with(&bin, "shell"), chosen(None));

        assert_eq!(ant.describe(), "ant profile 'shell'");
        assert!(ant.credential().await.unwrap() == token("--profile shell auth print-credentials --access-token"));
    }

    #[tokio::test]
    async fn without_any_profile_ant_is_left_to_use_its_active_one() {
        let bin = path_with_ant();
        let ant = AntProfile::new(shell_with(&bin, ""), chosen(None));

        assert_eq!(ant.describe(), "the active ant profile");
        assert!(ant.credential().await.unwrap() == token("auth print-credentials --access-token"));
    }

    #[tokio::test]
    async fn a_token_is_reused_until_the_profile_changes() {
        let bin = path_with_ant();
        let choice = chosen(Some("first"));
        let ant = AntProfile::new(shell_with(&bin, ""), Arc::clone(&choice));

        ant.credential().await.unwrap();
        ant.credential().await.unwrap();
        assert_eq!(runs(&bin), 1);

        choice.set(Some("second".into()));
        assert_eq!(ant.describe(), "ant profile 'second'");
        assert!(ant.credential().await.unwrap() == token("--profile second auth print-credentials --access-token"));
        assert_eq!(runs(&bin), 2);
    }

    #[tokio::test]
    async fn a_profile_without_a_token_names_the_login_command() {
        let bin = path_with_scripted_ant("echo 'not logged in' >&2; exit 1");
        let env = shell_with(&bin, "");

        let named = AntProfile::new(env.clone(), chosen(Some("mine"))).credential().await.err().unwrap();
        let active = AntProfile::new(env, chosen(None)).credential().await.err().unwrap();

        assert_eq!(
            named.to_string(),
            "authentication failed: ant profile 'mine' has no usable token; run `ant auth login --profile mine` (not logged in)"
        );
        assert_eq!(
            active.to_string(),
            "authentication failed: the active ant profile has no usable token; run `ant auth login` (not logged in)"
        );
    }

    #[tokio::test]
    async fn a_profile_saved_after_the_shell_was_read_is_used_at_once() {
        let bin = path_with_ant();
        let chain = CredentialChain::default();
        let _ = chain.shell.set(resolve_provider(&shell_with(&bin, ""), Arc::clone(&chain.ant_profile)));
        assert_eq!(chain.source().await.as_deref(), Some("the active ant profile"));

        chain.save_profile(Some("  mine\n".into()));
        assert_eq!(chain.source().await.as_deref(), Some("ant profile 'mine'"));
        assert!(chain.credential().await.unwrap() == token("--profile mine auth print-credentials --access-token"));

        chain.save_profile(Some("  ".into()));
        assert_eq!(chain.source().await.as_deref(), Some("the active ant profile"));
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
