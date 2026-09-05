//! Node identity and registration record (ADR-008, ADR-010 registration flow).

use crate::capability::Capabilities;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type NodeId = Uuid;

/// Coarse region derived from IP at registration, editable (ADR-004 D58).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// Runs inference; may also relay when promoted (ADR-004 D28).
    Compute,
    /// Headless `hive-server`: relay, artifact store, model cache, coordinator candidate.
    RegionalServer,
    /// Desktop app that opted into the server role too (ADR-010 §Server role).
    ComputeAndServer,
}

/// Presence state as tracked by the coordinator (ADR-001 D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    CheckedIn,
    CheckedOut,
    Draining,
}

/// Row shape of `hive.nodes`. Owned by the member identified by `member_id`
/// (a `public.profiles.id`, ADR-008 D31).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: NodeId,
    pub member_id: Uuid,
    pub display_name: String,
    pub role: NodeRole,
    pub region: Region,
    pub capabilities: Capabilities,
    pub presence: Presence,
    /// ToS version accepted at registration (ADR-010 step 10, ADR-011).
    pub tos_version: String,
    pub tos_accepted_at: DateTime<Utc>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
