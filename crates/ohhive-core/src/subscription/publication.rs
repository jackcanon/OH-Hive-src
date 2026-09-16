//! Retryable local publication of completed subscription output. No provider I/O.
//! Operation IDs for Bots turns MUST be the triggering message ID; the binding
//! identifies its agent and room. Caller must resolve current host authority.
use super::{journal::Binding, results::DurableResults, runner::RunError};
use crate::{bots::MessageId, local_hub::LocalHubStore};
use std::sync::Arc;
use uuid::Uuid;
impl DurableResults {
    /// Atomically publish a reply and finish its claimed Bots delivery. Safe to
    /// repeat after a lost response/restart. No recipients are woken in this slice.
    pub async fn publish_to_bots(
        &self,
        store: Arc<LocalHubStore>,
        binding: &Binding,
        operation: Uuid,
        delivery_generation: u64,
    ) -> Result<MessageId, RunError> {
        let reply = self.completed(binding, operation).await?;
        let binding = binding.clone();
        tokio::task::spawn_blocking(move || {
            store
                .bots_publish_subscription(&binding, operation, delivery_generation, reply)
                .map_err(|_| RunError::Persistence)
        })
        .await
        .map_err(|_| RunError::Persistence)?
    }
}
