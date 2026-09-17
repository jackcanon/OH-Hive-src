//! The storage surface `DeliveryExecutor` needs, as a trait, so the executor can drain against
//! either the machine's own vault or a hub on another machine.
//!
//! WHY THIS EXISTS. `DeliveryExecutor` held an `Arc<LocalHubStore>`. That was right while a
//! machine could only ever answer for agents in its own SQLite file -- and it is exactly what
//! stopped a second machine's agent from appearing in the Den running on the hub machine, since
//! the delivery loop had no way to reach across.
//!
//! Track E item 4 exposed these operations over `local_hub::transport` with host authorization,
//! and `RemoteLocalHub` gained the client half. This trait is the last piece: one surface both
//! satisfy, so the executor stops naming a concrete store.
//!
//! ON ERROR TYPES. The executor previously mixed two: `BotsResult` from the `BotsService`
//! methods it awaited, and `HubError` from the inherent store methods it called synchronously.
//! Everything here is `BotsResult`, using the existing `impl From<HubError> for BotsError`. That
//! is not cosmetic -- a trait cannot be implemented for both stores while half its methods carry
//! a `local_hub`-specific error, and the executor's own handling already treated a storage
//! failure the same way whichever type it arrived as.
//!
//! ON WHAT IS *NOT* HERE. No method takes a node id. The remote implementations send none: the
//! hub resolves the calling host from the session's bearer key and refuses anything that is not
//! that host's to touch. A node id in this trait would be a parameter the server must ignore,
//! and a reader could easily mistake it for something that grants authority.

use super::*;
use crate::local_hub::{LocalHubStore, RemoteLocalHub};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Everything one drain pass touches. Implemented for the local vault and for a remote hub.
#[async_trait]
pub trait DeliveryStore: Send + Sync {
    async fn agents_list(&self, owner: UserId) -> BotsResult<Vec<AgentProfile>>;
    async fn conversations_list(&self, actor: Principal) -> BotsResult<Vec<Conversation>>;
    async fn message_get(&self, id: MessageId) -> BotsResult<Message>;
    async fn messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> BotsResult<Vec<Message>>;
    async fn room_agents(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> BotsResult<Vec<AgentProfile>>;
    async fn deliveries_pending_for_agent(
        &self,
        agent_id: AgentId,
        limit: u32,
    ) -> BotsResult<Vec<AgentDelivery>>;
    async fn delivery_claim(&self, key: DeliveryKey) -> BotsResult<AgentDelivery>;
    async fn delivery_complete(
        &self,
        key: DeliveryKey,
        lease_generation: u64,
    ) -> BotsResult<AgentDelivery>;
    async fn delivery_fail(
        &self,
        key: DeliveryKey,
        lease_generation: u64,
        retry_after: Option<DateTime<Utc>>,
    ) -> BotsResult<AgentDelivery>;
    #[allow(clippy::too_many_arguments)]
    async fn message_send_with_cause(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
        cause: Option<DeliveryCause>,
        hold: bool,
    ) -> BotsResult<Message>;
    async fn turns_for_root(&self, root_message_id: MessageId) -> BotsResult<u32>;
    /// How many of this agent's deliveries are already `running`, so one machine does not start
    /// a second concurrent turn for the same agent.
    async fn active_turns_for_agent(&self, agent_id: AgentId) -> BotsResult<u32>;
    async fn deliveries_release_root(&self, root_message_id: MessageId) -> BotsResult<u32>;
    /// `owner` and `host` are the executor's own, and the LOCAL implementation needs them because
    /// a bare `LocalHubStore` has no session to derive them from. The remote implementation
    /// deliberately drops them -- the hub takes both from the bearer key, precisely so a node
    /// cannot write off another machine's agents as unreachable. See
    /// `LocalHub::bots_report_unroutable`.
    async fn report_unroutable(
        &self,
        owner: UserId,
        host: Uuid,
        local_ready: bool,
    ) -> BotsResult<usize>;
}

#[async_trait]
impl DeliveryStore for LocalHubStore {
    async fn agents_list(&self, owner: UserId) -> BotsResult<Vec<AgentProfile>> {
        LocalHubStore::bots_agents_list(self, owner).map_err(Into::into)
    }
    async fn conversations_list(&self, actor: Principal) -> BotsResult<Vec<Conversation>> {
        LocalHubStore::bots_conversations_list(self, actor).map_err(Into::into)
    }
    async fn message_get(&self, id: MessageId) -> BotsResult<Message> {
        LocalHubStore::bots_message_get(self, id).map_err(Into::into)
    }
    async fn messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> BotsResult<Vec<Message>> {
        LocalHubStore::bots_messages_list(self, actor, conversation_id, page).map_err(Into::into)
    }
    async fn room_agents(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> BotsResult<Vec<AgentProfile>> {
        LocalHubStore::bots_room_agents(self, actor, conversation_id).map_err(Into::into)
    }
    async fn deliveries_pending_for_agent(
        &self,
        agent_id: AgentId,
        limit: u32,
    ) -> BotsResult<Vec<AgentDelivery>> {
        LocalHubStore::bots_deliveries_pending_for_agent(self, agent_id, limit).map_err(Into::into)
    }
    async fn delivery_claim(&self, key: DeliveryKey) -> BotsResult<AgentDelivery> {
        LocalHubStore::bots_delivery_claim(self, key).map_err(Into::into)
    }
    async fn delivery_complete(
        &self,
        key: DeliveryKey,
        lease_generation: u64,
    ) -> BotsResult<AgentDelivery> {
        LocalHubStore::bots_delivery_complete(self, key, lease_generation).map_err(Into::into)
    }
    async fn delivery_fail(
        &self,
        key: DeliveryKey,
        lease_generation: u64,
        retry_after: Option<DateTime<Utc>>,
    ) -> BotsResult<AgentDelivery> {
        LocalHubStore::bots_delivery_fail(self, key, lease_generation, retry_after)
            .map_err(Into::into)
    }
    async fn message_send_with_cause(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
        cause: Option<DeliveryCause>,
        hold: bool,
    ) -> BotsResult<Message> {
        LocalHubStore::bots_message_send_with_cause(
            self,
            actor,
            conversation_id,
            client_request_id,
            expected_policy_revision,
            recipient_ids,
            draft,
            cause,
            hold,
        )
        .map_err(Into::into)
    }
    async fn turns_for_root(&self, root_message_id: MessageId) -> BotsResult<u32> {
        LocalHubStore::bots_turns_for_root(self, root_message_id).map_err(Into::into)
    }
    async fn active_turns_for_agent(&self, agent_id: AgentId) -> BotsResult<u32> {
        LocalHubStore::bots_active_turns_for_agent(self, agent_id).map_err(Into::into)
    }
    async fn deliveries_release_root(&self, root_message_id: MessageId) -> BotsResult<u32> {
        LocalHubStore::bots_deliveries_release_root(self, root_message_id).map_err(Into::into)
    }
    async fn report_unroutable(
        &self,
        owner: UserId,
        host: Uuid,
        local_ready: bool,
    ) -> BotsResult<usize> {
        LocalHubStore::bots_report_unroutable(self, owner, host, local_ready).map_err(Into::into)
    }
}

#[async_trait]
impl DeliveryStore for RemoteLocalHub {
    async fn agents_list(&self, _owner: UserId) -> BotsResult<Vec<AgentProfile>> {
        // The hub scopes to the session's owner. Passing one would be ignored, so it is dropped
        // here rather than sent and silently overridden.
        self.bots_agents_list().await.map_err(Into::into)
    }
    async fn conversations_list(&self, actor: Principal) -> BotsResult<Vec<Conversation>> {
        self.bots_conversations_list(actor)
            .await
            .map_err(Into::into)
    }
    async fn message_get(&self, id: MessageId) -> BotsResult<Message> {
        self.bots_message_get(id).await.map_err(Into::into)
    }
    async fn messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> BotsResult<Vec<Message>> {
        self.bots_messages_list(actor, conversation_id, page)
            .await
            .map_err(Into::into)
    }
    async fn room_agents(
        &self,
        _actor: Principal,
        conversation_id: ConversationId,
    ) -> BotsResult<Vec<AgentProfile>> {
        // The hub derives the actor from the session, same as elsewhere.
        self.bots_room_agents(conversation_id)
            .await
            .map_err(Into::into)
    }
    async fn deliveries_pending_for_agent(
        &self,
        agent_id: AgentId,
        limit: u32,
    ) -> BotsResult<Vec<AgentDelivery>> {
        self.bots_deliveries_pending_for_agent(agent_id, limit)
            .await
            .map_err(Into::into)
    }
    async fn delivery_claim(&self, key: DeliveryKey) -> BotsResult<AgentDelivery> {
        self.bots_delivery_claim(key).await.map_err(Into::into)
    }
    async fn delivery_complete(
        &self,
        key: DeliveryKey,
        lease_generation: u64,
    ) -> BotsResult<AgentDelivery> {
        self.bots_delivery_complete(key, lease_generation)
            .await
            .map_err(Into::into)
    }
    async fn delivery_fail(
        &self,
        key: DeliveryKey,
        lease_generation: u64,
        retry_after: Option<DateTime<Utc>>,
    ) -> BotsResult<AgentDelivery> {
        self.bots_delivery_fail(key, lease_generation, retry_after)
            .await
            .map_err(Into::into)
    }
    async fn message_send_with_cause(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
        cause: Option<DeliveryCause>,
        hold: bool,
    ) -> BotsResult<Message> {
        RemoteLocalHub::bots_message_send_with_cause(
            self,
            actor,
            conversation_id,
            client_request_id,
            expected_policy_revision,
            recipient_ids,
            draft,
            cause,
            hold,
        )
        .await
        .map_err(Into::into)
    }
    async fn turns_for_root(&self, root_message_id: MessageId) -> BotsResult<u32> {
        self.bots_turns_for_root(root_message_id)
            .await
            .map_err(Into::into)
    }
    async fn active_turns_for_agent(&self, agent_id: AgentId) -> BotsResult<u32> {
        self.bots_active_turns_for_agent(agent_id)
            .await
            .map_err(Into::into)
    }
    async fn deliveries_release_root(&self, root_message_id: MessageId) -> BotsResult<u32> {
        self.bots_deliveries_release_root(root_message_id)
            .await
            .map_err(Into::into)
    }
    async fn report_unroutable(
        &self,
        _owner: UserId,
        _host: Uuid,
        local_ready: bool,
    ) -> BotsResult<usize> {
        // Both dropped on purpose -- see the trait's doc. The hub derives them from the session.
        self.bots_report_unroutable(local_ready)
            .await
            .map_err(Into::into)
    }
}
