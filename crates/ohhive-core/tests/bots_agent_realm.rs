//! The realm: an agent's `host_name` is the display name of the computer it runs on, derived on
//! read from the vault's `nodes` row rather than copied onto the agent when it is registered.
//!
//! Derived, not stored, is the point. `0ff03190` was a stored id that had drifted out of the
//! vault's namespace; three machine renames on 2026-09-18 then left the vault's own node names
//! stale. A copy taken at registration time would reproduce both failures one level up -- rename
//! the computer and every agent on it would still introduce itself by the old realm.
#![cfg(all(feature = "bots", feature = "local-hub"))]

use hive_core::bots::{AgentRuntimeKind, NewAgentProfile};
use hive_core::local_hub::LocalHubStore;
use uuid::Uuid;

fn register(store: &LocalHubStore, owner: Uuid, name: &str, host: Option<Uuid>) -> Option<String> {
    store
        .bots_agents_create(NewAgentProfile {
            owner,
            name: name.into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: host,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: format!("agent:{name}"),
        })
        .expect("agent")
        .host_name
}

#[test]
fn host_name_is_the_vault_s_name_for_the_agent_s_computer() {
    let store = LocalHubStore::in_memory().expect("store");
    let owner = Uuid::new_v4();
    let midgaard = store.enroll_owner("Midgaard").expect("node").node_id;

    assert_eq!(
        register(&store, owner, "Odin", Some(midgaard)).as_deref(),
        Some("Midgaard")
    );
    // No computer claimed: a BYOK agent answers hub-side, on no particular machine.
    assert_eq!(register(&store, owner, "Claude", None), None);
    // Pinned to a computer this vault has no row for -- `0ff03190`'s shape. It must read as
    // "unknown", never as a name the vault invented.
    assert_eq!(register(&store, owner, "Ghost", Some(Uuid::new_v4())), None);
}

#[test]
fn renaming_the_computer_renames_the_realm_without_touching_the_agent() {
    let store = LocalHubStore::in_memory().expect("store");
    let owner = Uuid::new_v4();
    let node = store.enroll_owner("Overgaard").expect("node").node_id;
    let agent = store
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Tyr".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: Some(node),
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "agent:tyr".into(),
        })
        .expect("agent");
    assert_eq!(agent.host_name.as_deref(), Some("Overgaard"));

    store.rename_node(node, "Niflheim").expect("rename");

    let listed = store.bots_agents_list(owner).expect("list");
    let tyr = listed.iter().find(|a| a.id == agent.id).expect("Tyr");
    assert_eq!(
        tyr.host_name.as_deref(),
        Some("Niflheim"),
        "the realm must follow the computer, not a copy taken at registration"
    );
}
