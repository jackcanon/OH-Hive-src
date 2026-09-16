//! Audit 3.5 and 3.6: a delivery's terminal state must not be rewritable, and an RPC caller must
//! not be able to author messages as one of the owner's agents.
//!
//! These are preconditions for turning agent-to-agent fan-out on, not cleanups. The Held human
//! gate is built on delivery status: if a stale lease can still write `pending`, a held chain can
//! be walked past its own gate, and if a caller can mint agent-authored messages outside the
//! executor then it never meets a budget check at all.
#![cfg(all(feature = "bots", feature = "local-hub"))]

use hive_core::bots::{
    AgentRuntimeKind, ConversationKind, DeliveryKey, DeliveryStatus, MessageKind, NewAgentProfile,
    NewConversation, NewMessage, Principal, StorageScope,
};
use hive_core::local_hub::LocalHubStore;
use uuid::Uuid;

struct Fixture {
    store: LocalHubStore,
    owner: Uuid,
    agent: Uuid,
    key: DeliveryKey,
}

/// One agent, one room, one pending delivery addressed to it.
fn pending_delivery() -> Fixture {
    let store = LocalHubStore::in_memory().expect("store");
    let owner = Uuid::new_v4();
    let agent = store
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Alpha".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: Some(Uuid::new_v4()),
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "alpha".into(),
        })
        .expect("agent");
    let room = store
        .bots_conversations_create(NewConversation {
            title: Some("Fencing".into()),
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        })
        .expect("room");
    store
        .bots_conversations_join(Principal::Agent(agent.id), room.id)
        .expect("join");
    let message = store
        .bots_message_send(
            Principal::User(owner),
            room.id,
            "ask".into(),
            1,
            vec![agent.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("@Alpha hello".into()),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .expect("send");
    let key = DeliveryKey { message_id: message.id, recipient: agent.id };
    Fixture { store, owner, agent: agent.id, key }
}

/// The sequence the audit named: claim, cancel, then the executor's NoCapacity path fails the
/// delivery with the lease it still holds. Generation fencing alone accepts that and writes
/// `pending`, resurrecting a delivery the user cancelled.
#[test]
fn a_cancelled_delivery_cannot_be_resurrected_by_its_in_flight_lease() {
    let f = pending_delivery();
    let claimed = f.store.bots_delivery_claim(f.key).expect("claim");
    let lease = claimed.lease_generation;

    f.store
        .bots_delivery_cancel(Principal::User(f.owner), f.key)
        .expect("cancel");

    let retry = chrono::Utc::now() + chrono::Duration::seconds(20);
    assert!(
        f.store.bots_delivery_fail(f.key, lease, Some(retry)).is_err(),
        "a stale lease must not rewrite a cancelled delivery back to pending"
    );

    let still = f
        .store
        .bots_deliveries_pending_for_agent(f.agent, 10)
        .expect("pending");
    assert!(still.is_empty(), "and it must not become claimable again");
}

/// Cancel bumps the generation, so even a `complete` from the in-flight turn is refused.
#[test]
fn a_cancelled_delivery_cannot_be_completed_by_its_in_flight_lease() {
    let f = pending_delivery();
    let lease = f.store.bots_delivery_claim(f.key).expect("claim").lease_generation;
    f.store
        .bots_delivery_cancel(Principal::User(f.owner), f.key)
        .expect("cancel");
    assert!(
        f.store.bots_delivery_complete(f.key, lease).is_err(),
        "a turn that finished after the user cancelled must not mark it done"
    );
}

/// Generation 0 is what a never-claimed row carries, so without a status fence
/// `complete(key, 0)` marked a delivery done that no runner ever ran.
#[test]
fn a_never_claimed_delivery_cannot_be_completed() {
    let f = pending_delivery();
    assert!(
        f.store.bots_delivery_complete(f.key, 0).is_err(),
        "a pending delivery must be claimed before it can be resolved"
    );
    let still = f
        .store
        .bots_deliveries_pending_for_agent(f.agent, 10)
        .expect("pending");
    assert_eq!(still.len(), 1, "and it stays pending for a real runner");
}

/// The same fence protects the Held gate. A held delivery is not `running`, so no lease can move
/// it -- which is what stops a stale finish walking a chain past the human gate it is waiting on.
#[test]
fn a_held_delivery_cannot_be_moved_by_any_lease() {
    let f = pending_delivery();
    // Create a second delivery the way the 30-turn gate does: held from birth, never claimed.
    let cause = hive_core::bots::DeliveryCause {
        cause_message_id: f.key.message_id,
        root_message_id: f.key.message_id,
        depth: 1,
    };
    let held_message = f
        .store
        .bots_message_send_with_cause(
            Principal::User(f.owner),
            f.store
                .bots_conversations_list(Principal::User(f.owner))
                .expect("rooms")
                .first()
                .expect("a room")
                .id,
            "gated".into(),
            1,
            vec![f.agent],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("this one is gated".into()),
                attachment_refs: Vec::new(),
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
            Some(cause),
            true,
        )
        .expect("held send");
    let gated = DeliveryKey { message_id: held_message.id, recipient: f.agent };

    let held = f.store.bots_deliveries_held(10).expect("held");
    assert_eq!(held.len(), 1, "the gate produced a held delivery");
    assert_eq!(held[0].status, DeliveryStatus::Held);

    // A held delivery was never claimed, so its generation is 0 -- the value a caller would
    // guess. Status fencing is the only thing refusing these.
    assert!(
        f.store.bots_delivery_complete(gated, 0).is_err(),
        "a held delivery must not be resolvable"
    );
    assert!(
        f.store.bots_delivery_fail(gated, 0, None).is_err(),
        "nor failed out from under the person it is waiting on"
    );
    // And it is not offered as work.
    let pending = f
        .store
        .bots_deliveries_pending_for_agent(f.agent, 10)
        .expect("pending");
    assert!(
        !pending.iter().any(|d| d.key == gated),
        "a held delivery is never drained"
    );
    assert_eq!(f.store.bots_deliveries_held(10).expect("held").len(), 1);

    // Only a human release moves it, and then it is ordinary pending work again.
    assert_eq!(
        f.store.bots_deliveries_release_root(f.key.message_id).expect("release"),
        1
    );
    let pending = f
        .store
        .bots_deliveries_pending_for_agent(f.agent, 10)
        .expect("pending");
    assert!(pending.iter().any(|d| d.key == gated), "release makes it claimable");
}

/// Audit 3.6: over RPC, `bots_actor` only checked that the actor belonged to this node's owner --
/// so any same-owner paired device could post *as any of the owner's agents*, minting
/// agent-authored messages with arbitrary recipients through a path that never touches the
/// executor, and therefore never meets a loop budget. That is why it blocks fan-out.
#[test]
fn rpc_cannot_author_a_message_as_one_of_the_owners_agents() {
    let store = LocalHubStore::in_memory().expect("store");
    let credentials = store.enroll_owner("test node").expect("enroll");
    let owner = Uuid::new_v4();
    store
        .set_node_owner(credentials.node_id, owner)
        .expect("bind owner");
    let hub = store.connect(&credentials.raw_key).expect("connect");

    let agent = hub
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Alpha".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: Some(Uuid::new_v4()),
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "alpha".into(),
        })
        .expect("agent");
    let room = hub
        .bots_conversations_create(NewConversation {
            title: Some("RPC".into()),
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        })
        .expect("room");
    hub.bots_conversations_join(Principal::Agent(agent.id), room.id)
        .expect("join");

    let draft = |body: &str| NewMessage {
        thread_root: None,
        kind: MessageKind::Text,
        body: Some(body.into()),
        attachment_refs: Vec::new(),
        task_ref: None,
        turn_ref: None,
        source_event_ref: None,
    };

    // The attack: speak as the agent, and address the agent, from a device that does not run it.
    let impersonated = hub.bots_message_send(
        Principal::Agent(agent.id),
        room.id,
        "as-the-agent".into(),
        room.policy_revision,
        vec![agent.id],
        draft("I am Alpha and I have decided to keep going"),
    );
    assert!(
        impersonated.is_err(),
        "an RPC caller must not author messages as one of the owner's agents"
    );

    // The owner speaking as themselves is unaffected -- this is the path the apps actually use.
    hub.bots_message_send(
        Principal::User(owner),
        room.id,
        "as-the-person".into(),
        room.policy_revision,
        vec![agent.id],
        draft("@Alpha hello"),
    )
    .expect("a person may still post");

    // And listing/joining as an agent still works: those cannot mint messages, and Sif's room
    // creation depends on them.
    hub.bots_conversations_list(Principal::Agent(agent.id))
        .expect("an agent may still see its own rooms");
}
