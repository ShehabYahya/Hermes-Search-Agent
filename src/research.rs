use std::{sync::Arc, time::Duration};

use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    config::Settings,
    error::SearchError,
    hermes::HermesClient,
    prompts::{self, PromptBundle},
    tools::{DeepResearchArgs, LightSearchArgs, MediumResearchArgs},
};

#[derive(Debug, Clone, Copy)]
enum ResearchMode {
    Light,
    Medium,
    Deep,
}

impl ResearchMode {
    fn name(self) -> &'static str {
        match self {
            Self::Light => "light_search",
            Self::Medium => "medium_research",
            Self::Deep => "deep_research",
        }
    }
}

#[derive(Clone)]
pub struct ResearchRunner {
    hermes: HermesClient,
    light_timeout: Duration,
    medium_timeout: Duration,
    deep_timeout: Duration,
}

impl ResearchRunner {
    pub fn new(hermes: HermesClient, settings: &Settings) -> Self {
        Self {
            hermes,
            light_timeout: settings.light_timeout,
            medium_timeout: settings.medium_timeout,
            deep_timeout: settings.deep_timeout,
        }
    }

    pub async fn light_search(&self, args: LightSearchArgs) -> Result<String, SearchError> {
        self.execute(ResearchMode::Light, prompts::build_light(&args)?)
            .await
    }

    pub async fn medium_research(&self, args: MediumResearchArgs) -> Result<String, SearchError> {
        self.execute(ResearchMode::Medium, prompts::build_medium(&args)?)
            .await
    }

    pub async fn deep_research(&self, args: DeepResearchArgs) -> Result<String, SearchError> {
        self.execute(ResearchMode::Deep, prompts::build_deep(&args)?)
            .await
    }

    async fn execute(
        &self,
        mode: ResearchMode,
        prompt: PromptBundle,
    ) -> Result<String, SearchError> {
        let request_id = Uuid::new_v4();
        let session_id = format!("dsh-research-{request_id}");
        let idempotency_key = request_id.to_string();
        let timeout = self.timeout_for(mode);

        info!(
            mode = mode.name(),
            %request_id,
            timeout_secs = timeout.as_secs(),
            "starting Hermes research run"
        );

        let run_id = self
            .hermes
            .start_run(
                &prompt.input,
                &prompt.instructions,
                &session_id,
                &idempotency_key,
            )
            .await?;

        let mut stop_guard = RunStopGuard::new(self.hermes.clone(), run_id.clone());
        let result = tokio::time::timeout(timeout, self.hermes.wait_for_completion(&run_id)).await;

        match result {
            Ok(Ok(completed)) => {
                stop_guard.disarm();
                info!(mode = mode.name(), %request_id, %run_id, "research run completed");
                Ok(completed.content)
            }
            Ok(Err(error)) => {
                warn!(mode = mode.name(), %request_id, %run_id, %error, "research run failed");
                Err(error)
            }
            Err(_) => {
                warn!(mode = mode.name(), %request_id, %run_id, "research run timed out");
                Err(SearchError::Timeout {
                    seconds: timeout.as_secs(),
                })
            }
        }
    }

    fn timeout_for(&self, mode: ResearchMode) -> Duration {
        match mode {
            ResearchMode::Light => self.light_timeout,
            ResearchMode::Medium => self.medium_timeout,
            ResearchMode::Deep => self.deep_timeout,
        }
    }
}

struct RunStopGuard {
    hermes: Arc<HermesClient>,
    run_id: Option<String>,
}

impl RunStopGuard {
    fn new(hermes: HermesClient, run_id: String) -> Self {
        Self {
            hermes: Arc::new(hermes),
            run_id: Some(run_id),
        }
    }

    fn disarm(&mut self) {
        self.run_id = None;
    }
}

impl Drop for RunStopGuard {
    fn drop(&mut self) {
        let Some(run_id) = self.run_id.take() else {
            return;
        };
        let hermes = Arc::clone(&self.hermes);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if let Err(error) = hermes.stop_run(&run_id).await {
                    tracing::warn!(%run_id, %error, "failed to stop abandoned Hermes run");
                }
            });
        }
    }
}
