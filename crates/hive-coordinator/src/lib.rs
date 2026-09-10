//! Scheduler core (ADR-005). Capability-first, region-second, local-first.
//!
//! This crate is deliberately I/O-free: it takes snapshots of nodes and jobs
//! and returns placement decisions. `hive-server` wires it to Postgres and the
//! overlay. That keeps the matching rules testable and the Pi build small.

use hive_core::capability::Capabilities;
use hive_core::job::{Job, JobId};
use hive_core::node::{NodeId, Presence, Region};

/// Minimal view of a node the scheduler needs.
#[derive(Debug, Clone)]
pub struct NodeView {
    pub id: NodeId,
    pub region: Region,
    pub presence: Presence,
    pub capabilities: Capabilities,
    /// Active leases held by this node. v1 rule: one card per node at a time.
    pub active_leases: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    Node(NodeId),
    /// No eligible Hive node; fall through to provider API pool (ADR-002 D17/D24).
    ProviderOverflow,
    /// Card cannot run anywhere right now (e.g. requires internet, no opted-in nodes).
    Starved,
}

/// Pick a node for `job`. Preference order:
/// 1. checked-in, idle, capability match, same region as `preferred_region`
/// 2. checked-in, idle, capability match, any region
/// 3. `ProviderOverflow` if the job's modality is text/code, else `Starved`
pub fn place(job: &Job, nodes: &[NodeView], preferred_region: Option<&Region>) -> Placement {
    let eligible = || {
        nodes.iter().filter(|n| {
            n.presence == Presence::CheckedIn
                && n.active_leases == 0
                && n.capabilities.satisfies(&job.requirements)
        })
    };
    if let Some(r) = preferred_region {
        if let Some(n) = eligible().find(|n| &n.region == r) {
            return Placement::Node(n.id);
        }
    }
    if let Some(n) = eligible().next() {
        return Placement::Node(n.id);
    }
    use hive_core::capability::Modality::{Code, Text};
    match job.requirements.modality {
        Some(Text) | Some(Code) | None => Placement::ProviderOverflow,
        _ => Placement::Starved,
    }
}

/// Which leases have expired as of `now` — the reaper feeds these back to
/// `place()` with `resume_from` set (ADR-006 D42).
pub fn expired_leases(
    leases: &[hive_core::job::Lease],
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<JobId> {
    leases
        .iter()
        .filter(|l| l.is_expired(now))
        .map(|l| l.job_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hive_core::capability::{
        GpuVendor, Hardware, Modality, ModelRef, Requirements, ToolsLevel,
    };
    use hive_core::job::JobKind;
    use uuid::Uuid;

    fn node(region: &str, allow_internet: bool, modalities: Vec<Modality>) -> NodeView {
        NodeView {
            id: Uuid::new_v4(),
            region: Region(region.into()),
            presence: Presence::CheckedIn,
            active_leases: 0,
            capabilities: Capabilities {
                hardware: Hardware {
                    cpu_model: "x".into(),
                    cpu_cores: 8,
                    ram_bytes: 32 << 30,
                    gpu_vendor: GpuVendor::Nvidia,
                    gpu_model: None,
                    vram_bytes: Some(12 << 30),
                    disk_free_bytes: 1 << 40,
                    upload_mbps: None,
                    download_mbps: None,
                },
                modalities,
                models: vec![ModelRef {
                    id: "m".into(),
                    modality: Modality::Text,
                    backend: "llama_cpp".into(),
                }],
                allow_internet,
                tools_level: ToolsLevel::SandboxedTools,
                storage_gb_offered: None,
                shard_capable: None,
            },
        }
    }

    fn job(modality: Modality, internet: bool) -> Job {
        Job {
            id: Uuid::new_v4(),
            kind: JobKind::AgentCard,
            project_id: Uuid::new_v4(),
            card_id: Some(Uuid::new_v4()),
            parent: None,
            requirements: Requirements {
                modality: Some(modality),
                requires_internet: internet,
                ..Default::default()
            },
            input: serde_json::Value::Null,
            resume_from: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn prefers_same_region_then_any() {
        let us = node("us-west", false, vec![Modality::Text]);
        let eu = node("eu-west", false, vec![Modality::Text]);
        let nodes = vec![eu.clone(), us.clone()];
        assert_eq!(
            place(
                &job(Modality::Text, false),
                &nodes,
                Some(&Region("us-west".into()))
            ),
            Placement::Node(us.id)
        );
        assert_eq!(
            place(
                &job(Modality::Text, false),
                &nodes,
                Some(&Region("ap-south".into()))
            ),
            Placement::Node(eu.id)
        );
    }

    #[test]
    fn text_overflows_to_provider_but_video_starves() {
        let nodes = vec![node("us-west", false, vec![Modality::Text])];
        assert_eq!(
            place(&job(Modality::Text, true), &nodes, None),
            Placement::ProviderOverflow
        );
        assert_eq!(
            place(&job(Modality::Video, false), &nodes, None),
            Placement::Starved
        );
    }
}
