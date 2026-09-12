//! FFI wrapper for the "OH Hive" (cloud) side of the Swift app's Kanban view (Good Idea Fairy:
//! "a Kanban in the Swift Hive App... pick for them to be local hive or OH Hive"). Local-only idea
//! cards are handled entirely on the Swift side (`KanbanStore.swift`) -- they never touch the Rust
//! core, since they're not Hive state at all until a member actually pushes one to OH Hive. This
//! module only covers reading real cloud projects, via the new node-scoped
//! `hive.node_projects_overview` RPC (see its migration for why a *node*-scoped read is needed:
//! the Swift app only ever holds a node key, never a member session/JWT, per ADR-004).

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::hub::HubClient;
use hive_core::nodeconfig;
use std::sync::Arc;

#[uniffi::export]
impl HiveNode {
    /// Raw `hive.node_projects_overview` result, passed through as a JSON string (array of
    /// project objects) -- same "pass jsonb through, decode on the Swift side" convention as
    /// `HiveSnapshot::summary_json`, since this shape isn't stable enough yet to model field by
    /// field as a `uniffi::Record`.
    pub async fn kanban_cloud_projects(self: Arc<Self>) -> Result<String, HiveError> {
        RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
                let v = hub
                    .node_projects_overview()
                    .await
                    .map_err(HiveError::from)?;
                Ok(v.to_string())
            })
            .await
            .map_err(|e| HiveError::Failed(format!("kanban_cloud_projects task panicked: {e}")))?
    }
}
