use tokio::sync::OnceCell;

use crate::auth::resolve_provider;
use crate::error::AppResult;
use crate::process_env::ProcessEnv;
use crate::translate::claude::ClaudeTranslator;
use crate::translate::Translator;
use crate::ytdlp::YtDlp;

pub struct Services {
    pub ytdlp: YtDlp,
    pub translator: Box<dyn Translator>,
    pub credential_source: String,
}

impl Services {
    async fn load() -> AppResult<Self> {
        let env = ProcessEnv::from_login_shell().await;
        let credentials = resolve_provider(&env);
        let credential_source = credentials.describe();
        Ok(Self {
            translator: Box::new(ClaudeTranslator::new(credentials)?),
            ytdlp: YtDlp::new(env),
            credential_source,
        })
    }
}

/// Built on first use so the login-shell probe never blocks window start-up.
#[derive(Default)]
pub struct AppState(OnceCell<Services>);

impl AppState {
    pub async fn services(&self) -> AppResult<&Services> {
        self.0.get_or_try_init(Services::load).await
    }
}
