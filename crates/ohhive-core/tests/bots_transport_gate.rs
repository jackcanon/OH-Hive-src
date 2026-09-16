//! Library behavior without unit-test configuration: unbound keys cannot access Bots.
#![cfg(all(feature = "bots", feature = "local-hub"))]
use hive_core::{hub::HubError, local_hub::LocalHubStore};
#[test]
fn owner_binding_is_required_and_cannot_be_reassigned() {
    let store = LocalHubStore::in_memory().unwrap();
    let credentials = store.enroll_owner("test node").unwrap();
    let hub = store.connect(&credentials.raw_key).unwrap();
    assert!(
        matches!(hub.bots_agents_list(), Err(HubError::Rejected(message)) if message.contains("confirmed its Hive account"))
    );
    let owner = uuid::Uuid::new_v4();
    store.set_node_owner(credentials.node_id, owner).unwrap();
    store.set_node_owner(credentials.node_id, owner).unwrap();
    assert!(hub.bots_agents_list().unwrap().is_empty());
    assert!(store
        .set_node_owner(credentials.node_id, uuid::Uuid::new_v4())
        .is_err());
    assert!(store.set_node_owner(uuid::Uuid::new_v4(), owner).is_err());
    assert!(store
        .set_node_owner(credentials.node_id, uuid::Uuid::nil())
        .is_err());
    store.revoke(credentials.node_id).unwrap();
    assert!(matches!(hub.bots_agents_list(), Err(HubError::BadKey)));
}
