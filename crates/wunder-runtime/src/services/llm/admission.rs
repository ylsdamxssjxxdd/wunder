use super::LlmClient;
use anyhow::Result;
use std::{future::Future, pin::Pin, sync::Arc};

pub(super) type RequestAdmission =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

impl LlmClient {
    /// Run immediately before every HTTP attempt, including adapter fallbacks.
    pub(crate) fn with_request_admission<F, Fut>(mut self, admission: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.request_admission = Some(Arc::new(move || Box::pin(admission())));
        self
    }

    pub(super) async fn admit_request(&self) -> Result<()> {
        if let Some(admission) = &self.request_admission {
            admission().await?;
        }
        Ok(())
    }
}
