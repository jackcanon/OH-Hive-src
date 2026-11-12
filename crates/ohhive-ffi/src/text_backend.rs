//! Owner-selected local inference. No credential contents cross FFI.
use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::backend::{text_profile::TextBackendProfile, Backend};
use std::sync::Arc;
#[uniffi::export]
impl HiveNode {
    pub fn text_backend_profile(&self) -> Result<String, HiveError> {
        serde_json::to_string(&TextBackendProfile::load()?)
            .map_err(|_| HiveError::Failed("Cannot inspect text backend profile".into()))
    }
    pub async fn text_backend_select(
        self: Arc<Self>,
        profile: String,
    ) -> Result<String, HiveError> {
        RUNTIME.spawn(async move {
            let _gate = self.fleet.gate.try_lock().map_err(|_|HiveError::Failed("Wait for active private work to finish".into()))?;
            if *self.busy.lock().await { return Err(HiveError::Failed("Stop the compute worker first".into())); }
            let parsed: TextBackendProfile = serde_json::from_str(&profile).map_err(|_|HiveError::Failed("Invalid text backend profile".into()))?;
            let cfg = hive_core::nodeconfig::load()?;
            let backend = parsed.backend(&cfg.llama_url)?;
            let caps = backend.capabilities().await.map_err(|_|HiveError::Failed("Selected model server is unavailable or unauthorized; previous selection retained".into()))?;
            if caps.models.is_empty() { return Err(HiveError::Failed("Selected server advertises no models; previous selection retained".into())); }
            parsed.save(&cfg.llama_url)?;
            serde_json::to_string(&parsed).map_err(|_|HiveError::Failed("Cannot inspect selected backend".into()))
        }).await.map_err(|_|HiveError::Failed("Backend selection stopped".into()))?
    }
}
