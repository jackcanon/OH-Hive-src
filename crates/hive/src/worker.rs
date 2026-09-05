//! v0 worker loop (ADR-005 pull dispatch, ADR-006 D41 node-owned execution).
//!
//! check-in → every `poll` seconds: heartbeat, claim one card, run it through
//! the text backend with the project goal + dependency outputs as context,
//! report completion (the hub meters and pays). One card at a time per node.
//!
//! Not yet: multi-step agent loop, tools/sandbox, checkpoints, non-text modalities.

use anyhow::Result;
use ohhive_core::backend::Backend;
use ohhive_core::capability::{Capabilities, Requirements};
use ohhive_core::hub::{Claim, ClaimedCard, ClaimedProject, HubClient};
use ohhive_core::job::{Job, JobKind};
use ohhive_core::ledger::Usage;
use std::time::Duration;

pub struct Worker<'a> {
    pub hub: &'a HubClient,
    pub backend: &'a dyn Backend,
    pub caps: &'a Capabilities,
    pub default_model: Option<String>,
}

fn build_prompt(card: &ClaimedCard, project: &ClaimedProject, deps: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut p = String::new();
    p.push_str(&format!("You are a worker node in OH Hive, a community compute network.\n"));
    p.push_str(&format!("Project: {}\nProject goal: {}\n\n", project.title, project.goal));
    if !deps.is_empty() {
        p.push_str("Outputs from cards this one depends on:\n");
        for (k, v) in deps {
            p.push_str(&format!("--- {k} ---\n{}\n", v.as_str().unwrap_or("")));
        }
        p.push('\n');
    }
    p.push_str(&format!("Card: {}\nTask:\n{}\n\nAcceptance criteria: {}\n\nRespond with the deliverable only.",
        card.title, card.inputs, card.acceptance));
    p
}

impl<'a> Worker<'a> {
    /// Run one dispatch cycle. Returns true if a card was executed.
    pub async fn tick(&self) -> Result<bool> {
        match self.hub.claim_card().await? {
            Claim::NothingToDo => Ok(false),
            Claim::NotCheckedIn => {
                tracing::warn!("hub says we are not checked in; re-checking in");
                self.hub.check_in(self.caps, None).await?;
                Ok(false)
            }
            Claim::AlreadyLeased => {
                tracing::warn!("hub says we hold a lease already (previous run died?) — will be reaped");
                Ok(false)
            }
            Claim::Leased { card, project, dep_outputs, lease_expires_at } => {
                tracing::info!(card = %card.key, project = %project.title, expires = %lease_expires_at, "leased card");
                let model = card
                    .required_capabilities
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| self.default_model.clone())
                    .or_else(|| self.caps.models.first().map(|m| m.id.clone()));
                let prompt = build_prompt(&card, &project, &dep_outputs);
                let job = Job {
                    id: uuid::Uuid::new_v4(),
                    kind: JobKind::AgentCard,
                    project_id: project.id,
                    card_id: Some(card.id),
                    parent: None,
                    requirements: Requirements { model_id: model.clone(), ..Default::default() },
                    input: serde_json::json!({ "prompt": prompt, "max_tokens": 512 }),
                    resume_from: None,
                    created_at: chrono::Utc::now(),
                };
                let run = async {
                    let stream = self.backend.run(&job).await?;
                    ohhive_core::backend::collect(stream).await
                };
                match run.await {
                    Ok((text, usage)) => {
                        let text = text.trim().to_string();
                        let done = self.hub.complete_card(card.id, &text, model.as_deref(), usage).await?;
                        tracing::info!(
                            card = %card.key, tokens_out = usage.tokens_out, earned = done.earned_honey,
                            wallet = done.wallet_balance, fund = done.fund_balance, "card complete → review"
                        );
                        println!("\n[{}] {}\n{}\n  → earned {:.4} $honey ({} tokens); wallet {:.2}, project fund {:.2}",
                            project.title, card.title, text, done.earned_honey, usage.tokens_out, done.wallet_balance, done.fund_balance);
                        Ok(true)
                    }
                    Err(e) => {
                        tracing::error!(card = %card.key, "backend failed: {e}");
                        self.hub.fail_card(card.id, &e.to_string()).await?;
                        Ok(true)
                    }
                }
            }
        }
    }

    pub async fn run_forever(&self, poll: Duration, heartbeat_every: u32) -> Result<()> {
        let mut n: u32 = 0;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(poll) => {}
                _ = tokio::signal::ctrl_c() => {
                    let p = self.hub.check_out().await?;
                    println!("\nchecked out ({p})");
                    return Ok(());
                }
            }
            n = n.wrapping_add(1);
            if n % heartbeat_every == 0 {
                if let Err(e) = self.hub.heartbeat().await {
                    tracing::warn!("heartbeat failed: {e}");
                }
            }
            // Drain: keep claiming while there is eligible work.
            loop {
                match self.tick().await {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(e) => {
                        tracing::warn!("tick failed: {e}");
                        break;
                    }
                }
            }
        }
    }
}

#[allow(dead_code)]
fn _usage_type_check(_: Usage) {}
