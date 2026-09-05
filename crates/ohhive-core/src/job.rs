//! Cards, jobs, leases, checkpoints (ADR-005, ADR-006 D41–D44).
//!
//! A *card* is what a project owner sees on the kanban. A *job* is the unit the
//! coordinator hands to a node: in v1 one card == one job (D41); child jobs
//! spawned by an agent loop (D44) are jobs with `parent` set.

use crate::capability::Requirements;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type JobId = Uuid;
pub type CardId = Uuid;
pub type ProjectId = Uuid;

/// Job kinds the agent runtime knows how to run. Mirrors `packages/schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Interviewer / planner turns (ADR-006 D37). Hub-orchestrated, node-executed.
    Interview,
    /// Full agent loop over a card (D41).
    AgentCard,
    /// A single inference call, used for child jobs and spot-check replays.
    Inference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub kind: JobKind,
    pub project_id: ProjectId,
    pub card_id: Option<CardId>,
    pub parent: Option<JobId>,
    pub requirements: Requirements,
    /// Opaque, schema-versioned payload; see `packages/schema/job-input.json`.
    pub input: serde_json::Value,
    /// Content hash of the last checkpoint to resume from, if any (D42).
    pub resume_from: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A node's claim on a job. Expiry is per-modality (video >> text) and is
/// extended by heartbeats through the coordinator (ADR-005).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    pub job_id: JobId,
    pub node_id: crate::node::NodeId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Coordinator-minted, short-lived token the node presents on every call
    /// for this lease. Never a raw Supabase JWT (ADR-004).
    pub token: String,
}

impl Lease {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }
}

/// Written at step boundaries so another node can resume (D42). The payload
/// itself lives on a regional server; only the pointer travels here (ADR-007).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub job_id: JobId,
    pub step: u32,
    /// Content hash of the checkpoint blob on the artifact store.
    pub blob_hash: String,
    pub usage_so_far: crate::ledger::Usage,
    pub created_at: DateTime<Utc>,
}

/// Terminal or intermediate outcome reported to the coordinator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum JobOutcome {
    Completed {
        artifact_hashes: Vec<String>,
        usage: crate::ledger::Usage,
    },
    Failed {
        reason: String,
        usage: crate::ledger::Usage,
    },
    /// Node is checking out; last checkpoint is attached for resumption.
    Yielded { checkpoint: Checkpoint },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_expiry_is_inclusive_of_deadline() {
        let now = Utc::now();
        let lease = Lease {
            job_id: Uuid::new_v4(),
            node_id: Uuid::new_v4(),
            issued_at: now,
            expires_at: now,
            token: "t".into(),
        };
        assert!(lease.is_expired(now));
    }
}
