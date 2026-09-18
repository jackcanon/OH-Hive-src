//! Sync facade used only on the FFI blocking worker. A remote session never owns a local DB.
use crate::RUNTIME;
use hive_core::{
    bots::*,
    hub::HubError,
    local_hub::{LocalHubStore, RemoteLocalHub},
};
use std::sync::Arc;
use uuid::Uuid;
type Result<T> = std::result::Result<T, HubError>;
#[derive(Clone)]
pub(crate) enum BotsStorage {
    Local(LocalHubStore),
    Remote {
        client: RemoteLocalHub,
        selection: String,
    },
}
impl BotsStorage {
    /// The store the delivery executor drains through, for either variant.
    ///
    /// `local()` below refuses for a remote primary, which is why the app could show a peer's
    /// shared history and never answer in it. That refusal made sense when nothing could execute
    /// remotely; it stopped making sense once `RemoteLocalHub` gained the delivery surface and
    /// the CLI started draining a remote hub in the field. The executor takes
    /// `Arc<dyn DeliveryStore>` precisely so it does not care which side of the wire the vault
    /// is on -- the hub authorizes the claim either way, from the session's bearer key rather
    /// than from anything the caller asserts.
    pub fn delivery_store(&self) -> Arc<dyn DeliveryStore> {
        match self {
            Self::Local(s) => Arc::new(s.clone()),
            Self::Remote { client, .. } => Arc::new(client.clone()),
        }
    }
    pub fn local(&self) -> Result<&LocalHubStore> {
        match self { Self::Local(s) => Ok(s), Self::Remote {..} => Err(HubError::Rejected("Remote agent execution is not connected yet; chat history stays on your selected primary".into())) }
    }
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }
    pub fn validate_selection(&self) -> Result<()> {
        let active = crate::private_fleet::selected_wire().map_err(|_| {
            HubError::Rejected("Cannot read primary selection; local fallback is disabled".into())
        })?;
        match self {
            Self::Local(_) if active.is_none() => Ok(()),
            Self::Remote { selection, .. } if active.as_ref() == Some(selection) => Ok(()),
            _ => Err(HubError::Rejected(
                "Primary selection changed. Reopen Bots.".into(),
            )),
        }
    }
    pub fn conversation_deliveries(&self, owner: Uuid, conversation: Uuid) -> Result<Vec<(String,String,String)>> {
        match self { Self::Local(s) => s.bots_conversation_deliveries(owner,conversation), Self::Remote { client, .. } => RUNTIME.block_on(client.bots_conversation_deliveries(conversation)) }
    }
    pub fn agent_was_archived(&self, owner: Uuid, runtime: String, host: Option<Uuid>) -> Result<bool> { match self { Self::Local(s) => s.bots_agent_was_archived(owner,runtime,host), Self::Remote{client,..} => RUNTIME.block_on(client.bots_agent_was_archived(runtime,host)) } }
    pub fn agent_bio_get(&self, owner: Uuid, agent: Uuid) -> Result<AgentBio> { match self { Self::Local(s) => s.bots_agent_bio_get(owner,agent), Self::Remote {client,..} => RUNTIME.block_on(client.bots_agent_bio_get(agent)) } }
    pub fn agent_bio_set(&self, owner: Uuid, agent: Uuid, name: String, profile: AgentBio) -> Result<AgentBio> { match self { Self::Local(s) => s.bots_agent_bio_set(owner,agent,name,profile), Self::Remote {client,..} => RUNTIME.block_on(client.bots_agent_bio_set(agent,name,profile)) } }
    pub fn agent_archive(&self, owner: Uuid, agent: Uuid) -> Result<()> { match self { Self::Local(s) => s.bots_agents_archive(owner,agent), Self::Remote {client,..} => RUNTIME.block_on(client.bots_agents_archive(agent)) } }
    pub fn user_profile_get(&self, owner: Uuid) -> Result<UserProfile> {
        match self { Self::Local(s) => s.bots_user_profile_get(owner), Self::Remote { client, .. } => RUNTIME.block_on(client.bots_user_profile_get()) }
    }
    pub fn user_profile_set(&self, owner: Uuid, profile: UserProfile) -> Result<UserProfile> {
        match self { Self::Local(s) => s.bots_user_profile_set(owner, profile), Self::Remote { client, .. } => RUNTIME.block_on(client.bots_user_profile_set(profile)) }
    }
    pub fn bots_agents_list(&self, owner: Uuid) -> Result<Vec<AgentProfile>> {
        match self {
            Self::Local(s) => s.bots_agents_list(owner),
            Self::Remote { client, .. } => RUNTIME.block_on(client.bots_agents_list()),
        }
    }
    pub fn bots_agents_create(&self, draft: NewAgentProfile) -> Result<AgentProfile> {
        match self {
            Self::Local(s) => s.bots_agents_create(draft),
            Self::Remote { client, .. } => RUNTIME.block_on(client.bots_agents_create(draft)),
        }
    }
    pub fn bots_agents_update(
        &self,
        owner: Uuid,
        agent: Uuid,
        patch: AgentProfilePatch,
    ) -> Result<AgentProfile> {
        match self {
            Self::Local(s) => s.bots_agents_update(owner, agent, patch),
            Self::Remote { client, .. } => {
                RUNTIME.block_on(client.bots_agents_update(agent, patch))
            }
        }
    }
    pub fn bots_room_agents(&self, actor: Principal, id: Uuid) -> Result<Vec<AgentProfile>> {
        match self {
            Self::Local(s) => s.bots_room_agents(actor, id),
            Self::Remote { client, .. } => RUNTIME.block_on(client.bots_room_agents(id)),
        }
    }
    pub fn bots_conversations_list(&self, actor: Principal) -> Result<Vec<Conversation>> {
        match self {
            Self::Local(s) => s.bots_conversations_list(actor),
            Self::Remote { client, .. } => RUNTIME.block_on(client.bots_conversations_list(actor)),
        }
    }
    pub fn bots_rooms_create(
        &self,
        request_id: Uuid,
        draft: NewConversation,
        agents: Vec<AgentId>,
    ) -> Result<Conversation> {
        match self {
            Self::Local(s) => s.bots_rooms_create(request_id, draft, agents),
            Self::Remote { client, .. } => {
                RUNTIME.block_on(client.bots_rooms_create(request_id, draft, agents))
            }
        }
    }
    pub fn bots_conversations_create(&self, draft: NewConversation) -> Result<Conversation> {
        match self {
            Self::Local(s) => s.bots_conversations_create(draft),
            Self::Remote { client, .. } => {
                RUNTIME.block_on(client.bots_conversations_create(draft))
            }
        }
    }
    pub fn bots_conversations_join(
        &self,
        actor: Principal,
        id: Uuid,
    ) -> Result<ConversationMember> {
        match self {
            Self::Local(s) => s.bots_conversations_join(actor, id),
            Self::Remote { client, .. } => {
                RUNTIME.block_on(client.bots_conversations_join(actor, id))
            }
        }
    }
    pub fn bots_messages_list(
        &self,
        actor: Principal,
        id: Uuid,
        page: MessagePage,
    ) -> Result<Vec<Message>> {
        match self {
            Self::Local(s) => s.bots_messages_list(actor, id, page),
            Self::Remote { client, .. } => {
                RUNTIME.block_on(client.bots_messages_list(actor, id, page))
            }
        }
    }
    pub fn bots_message_get(&self, id: Uuid) -> Result<Message> {
        match self {
            Self::Local(s) => s.bots_message_get(id),
            Self::Remote { client, .. } => RUNTIME.block_on(client.bots_message_get(id)),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bots_message_send(
        &self,
        actor: Principal,
        id: Uuid,
        request: String,
        revision: u32,
        recipients: Vec<Uuid>,
        draft: NewMessage,
    ) -> Result<Message> {
        match self {
            Self::Local(s) => s.bots_message_send(actor, id, request, revision, recipients, draft),
            Self::Remote { client, .. } => RUNTIME.block_on(
                client.bots_message_send(actor, id, request, revision, recipients, draft),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn remote_native_storage_routes_chat_and_rejects_foreign_reads_and_revocation() {
        use hive_core::local_hub::serve;
        let store = LocalHubStore::in_memory().unwrap();
        let credentials = store.enroll_owner("secondary fixture").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(credentials.node_id, owner).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (stop, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve(store.clone(), listener, async {
            let _ = rx.await;
        }));
        let storage = BotsStorage::Remote {
            client: RemoteLocalHub::new(&endpoint, credentials.raw_key).unwrap(),
            selection: "fixture".into(),
        };
        let foreign_owner = Uuid::new_v4();
        let foreign = store
            .bots_conversations_create(NewConversation {
                title: None,
                owner: foreign_owner,
                kind: ConversationKind::Team,
                project_id: None,
                coordinator: None,
                storage_scope: StorageScope::LocalOnly,
            })
            .unwrap();
        let foreign_message = store
            .bots_message_send(
                Principal::User(foreign_owner),
                foreign.id,
                "foreign".into(),
                foreign.policy_revision,
                vec![],
                NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some("private".into()),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .unwrap();
        let test_storage = storage.clone();
        RUNTIME
            .spawn_blocking(move || {
                assert!(test_storage.local().is_err());
                // ...but the executor's store is available for a remote primary, which is the
                // whole difference between the app showing a peer's history and answering in
                // it. The app used to branch on `usesRemotePrimary()` and skip the drain
                // entirely, so this assertion is the guard against that returning: if
                // `delivery_store()` ever refuses again, remote agents go quiet in the app and
                // nothing says why.
                let _: std::sync::Arc<dyn DeliveryStore> = test_storage.delivery_store();
                assert!(test_storage.validate_selection().is_err());
                assert!(test_storage.bots_message_get(foreign_message.id).is_err());
                assert!(test_storage
                    .bots_room_agents(Principal::User(owner), foreign.id)
                    .is_err());
                let agent = test_storage
                    .bots_agents_create(NewAgentProfile {
                        owner,
                        name: "Remote agent".into(),
                        runtime_kind: AgentRuntimeKind::Local,
                        preferred_host: Some(credentials.node_id),
                        capability_policy_ref: "default".into(),
                        provider_account_ref: None,
                        memory_namespace: "fixture".into(),
                    })
                    .unwrap();
                test_storage
                    .bots_agents_update(
                        owner,
                        agent.id,
                        AgentProfilePatch {
                            name: Some("Renamed".into()),
                            capability_policy_ref: None,
                            preferred_host: None,
                            memory_namespace: None,
                        },
                    )
                    .unwrap();
                assert_eq!(
                    test_storage.bots_agents_list(owner).unwrap()[0].name,
                    "Renamed"
                );
                let conversation = test_storage
                    .bots_conversations_create(NewConversation {
                        title: None,
                        owner,
                        kind: ConversationKind::AgentDm,
                        project_id: None,
                        coordinator: Some(agent.id),
                        storage_scope: StorageScope::LocalOnly,
                    })
                    .unwrap();
                test_storage
                    .bots_conversations_join(Principal::User(owner), conversation.id)
                    .unwrap();
                assert_eq!(
                    test_storage
                        .bots_conversations_list(Principal::User(owner))
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(
                    test_storage
                        .bots_room_agents(Principal::User(owner), conversation.id)
                        .unwrap()[0]
                        .id,
                    agent.id
                );
                let draft = NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some("from native secondary".into()),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                };
                let first = test_storage
                    .bots_message_send(
                        Principal::User(owner),
                        conversation.id,
                        "same-request".into(),
                        conversation.policy_revision,
                        vec![agent.id],
                        draft.clone(),
                    )
                    .unwrap();
                let retry = test_storage
                    .bots_message_send(
                        Principal::User(owner),
                        conversation.id,
                        "same-request".into(),
                        conversation.policy_revision,
                        vec![agent.id],
                        draft,
                    )
                    .unwrap();
                assert_eq!(first.id, retry.id);
                assert_eq!(
                    test_storage
                        .bots_message_get(first.id)
                        .unwrap()
                        .body
                        .as_deref(),
                    Some("from native secondary")
                );
                assert_eq!(
                    test_storage
                        .bots_messages_list(
                            Principal::User(owner),
                            conversation.id,
                            MessagePage {
                                before: None,
                                after: None,
                                limit: 20
                            }
                        )
                        .unwrap()
                        .len(),
                    1
                );
            })
            .await
            .unwrap();
        store.revoke(credentials.node_id).unwrap();
        let revoked = storage.clone();
        RUNTIME
            .spawn_blocking(move || assert!(revoked.bots_agents_list(owner).is_err()))
            .await
            .unwrap();
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
        RUNTIME
            .spawn_blocking(move || assert!(storage.bots_agents_list(owner).is_err()))
            .await
            .unwrap();
    }
}
