//! A real multi-agent room against a real local model. `#[ignore]`d: it needs a model server.
//!
//! Every other Bots test uses a scripted or mock runner, which is why a defect like `Principal`
//! UUIDs reaching the prompt survived a green suite. This one runs the actual path -- real
//! `LocalModelTurnRunner`, real inference -- and prints the transcript so a human can read whether
//! a three-agent room is coherent, not merely non-crashing.
//!
//! Run it:
//!   HIVE_LIVE_MODEL=llama3.1:8b cargo test -p hive-core --features bots,local-hub,llama-cpp \
//!     --test bots_group_chat_live -- --ignored --nocapture
//!
//! Endpoint defaults to http://127.0.0.1:11434/ (Ollama, which serves the OpenAI-compatible
//! /v1/chat/completions this backend speaks). Uses an in-memory store, so it never touches a real
//! LocalHub database or anyone's live agents.
#![cfg(all(feature = "bots", feature = "local-hub", feature = "llama-cpp"))]

use std::sync::Arc;

use hive_core::bots::executor::DeliveryExecutor;
use hive_core::bots::{
    AgentRuntimeKind, ConversationKind, LocalBotsTurnRunner, LocalModelTurnRunner, MessageKind,
    MessagePage, NewAgentProfile, NewConversation, NewMessage, Principal, StorageScope,
};
use hive_core::local_hub::LocalHubStore;
use uuid::Uuid;

#[tokio::test]
#[ignore = "needs a local model server; run explicitly with --ignored"]
async fn three_agent_room_against_a_real_model() {
    let model = std::env::var("HIVE_LIVE_MODEL").unwrap_or_else(|_| "llama3.1:8b".to_string());
    let endpoint =
        std::env::var("HIVE_LLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434/".to_string());

    let store = Arc::new(LocalHubStore::in_memory().expect("store"));
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();

    // Three agents with distinct, deliberately confusable-if-unnamed roles.
    let roles = [
        ("Scout", "You investigate and report facts plainly."),
        ("Builder", "You propose concrete implementation steps."),
        ("Skeptic", "You look for what will break."),
    ];
    let mut agents = Vec::new();
    for (name, _role) in roles {
        agents.push(
            store
                .bots_agents_create(NewAgentProfile {
                    owner,
                    name: name.to_string(),
                    runtime_kind: AgentRuntimeKind::Local,
                    preferred_host: Some(host),
                    capability_policy_ref: "default".into(),
                    provider_account_ref: None,
                    memory_namespace: name.to_string(),
                })
                .expect("agent"),
        );
    }
    let room = store
        .bots_conversations_create(NewConversation {
            title: Some("Live check".into()),
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        })
        .expect("room");
    for a in &agents {
        store
            .bots_conversations_join(Principal::Agent(a.id), room.id)
            .expect("join");
    }

    // The human addresses two of the three by name. Scout and Builder should answer; Skeptic
    // should stay silent, which is the property a room has to get right.
    let body =
        "@Scout @Builder we need group chat working tonight. One sentence each: what is the \
                single biggest risk?";
    let mentions = hive_core::bots::resolve_mentions(body, &agents, Principal::User(owner));
    assert_eq!(
        mentions.recipients.len(),
        2,
        "the prompt should address exactly two agents"
    );
    store
        .bots_message_send(
            Principal::User(owner),
            room.id,
            "live-1".into(),
            room.policy_revision,
            mentions.recipients.clone(),
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some(body.into()),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .expect("send");

    let runner: Arc<dyn LocalBotsTurnRunner> = Arc::new(
        LocalModelTurnRunner::loopback(host, model.clone(), &endpoint)
            .expect("construct the real runner"),
    );
    let executor = DeliveryExecutor::new(store.clone(), runner, host, owner);

    let summary = executor.drain_once().await;
    println!(
        "\n--- drain: {} delivered, {} failed, {} requeued (model {model} at {endpoint}) ---",
        summary.delivered, summary.failed, summary.requeued
    );

    let messages = store
        .bots_messages_list(
            Principal::User(owner),
            room.id,
            MessagePage {
                before: None,
                after: None,
                limit: 50,
            },
        )
        .expect("list");
    println!("--- transcript ({} messages) ---", messages.len());
    for m in &messages {
        let who = match m.author {
            Principal::User(_) => "the person".to_string(),
            Principal::Agent(id) => agents
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "unknown".into()),
        };
        println!(
            "[{}] {}: {}\n",
            m.kind_label(),
            who,
            m.body.clone().unwrap_or_default().trim()
        );
    }

    assert_eq!(
        summary.failed, 0,
        "a real turn failed -- is the model pulled and the server up?"
    );
    assert_eq!(
        summary.delivered, 2,
        "both addressed agents should have replied"
    );

    let replied: Vec<&str> = agents
        .iter()
        .filter(|a| messages.iter().any(|m| m.author == Principal::Agent(a.id)))
        .map(|a| a.name.as_str())
        .collect();
    assert!(
        replied.contains(&"Scout") && replied.contains(&"Builder"),
        "addressed agents replied: {replied:?}"
    );
    assert!(
        !replied.contains(&"Skeptic"),
        "an unaddressed agent must stay silent: {replied:?}"
    );

    // Fan-out is off by default, so nothing may cascade however the model phrases its reply.
    assert_eq!(
        store.bots_turns_for_root(root_of(&messages)).unwrap_or(0),
        2,
        "the shipped default must not cascade"
    );
}

fn root_of(messages: &[hive_core::bots::Message]) -> Uuid {
    messages.first().map(|m| m.id).unwrap_or_else(Uuid::nil)
}

trait KindLabel {
    fn kind_label(&self) -> &'static str;
}
impl KindLabel for hive_core::bots::Message {
    fn kind_label(&self) -> &'static str {
        match self.kind {
            MessageKind::Text => "text",
            MessageKind::System => "system",
            MessageKind::TaskReceipt => "receipt",
        }
    }
}
