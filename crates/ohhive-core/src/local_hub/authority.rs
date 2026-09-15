//! Explicit remote Bots selection. Configuration is credential-bearing and must be stored
//! with the node's private configuration, never in logs, project files, or the knowledge vault.
//! No local store is accepted by this client: an offline primary cannot create a second history.
use super::{enrollment::EnrollmentReceipt, transport::RemoteLocalHub, *};
use crate::bots::{AgentProfile, Conversation, Message, MessagePage, NewMessage, Principal};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAuthoritySelection {
    pub endpoint: String,
    pub authority_id: Uuid,
    pub fleet_id: Uuid,
    pub owner_id: Uuid,
    pub node_id: Uuid,
    credential: String,
}
impl RemoteAuthoritySelection {
    /// Call after locally pairing and completing the owner's signed enrollment approval.
    /// The returned identity is rechecked rather than trusting a caller-provided owner UUID.
    pub async fn confirm(
        endpoint: String,
        credential: String,
        expected: EnrollmentReceipt,
    ) -> Result<Self> {
        let selection = Self {
            endpoint,
            credential,
            authority_id: expected.authority_id,
            fleet_id: expected.fleet_id,
            owner_id: expected.owner_id,
            node_id: expected.node_id,
        };
        selection.connect().await?;
        Ok(selection)
    }
    pub async fn connect(&self) -> Result<SelectedBotsAuthority> {
        let client = RemoteLocalHub::new(&self.endpoint, self.credential.clone())?;
        let identity = client
            .private_fleet_identity()
            .await?
            .ok_or_else(|| rejected("selected primary has no verified private fleet"))?;
        if identity.authority_id != self.authority_id
            || identity.fleet_id != self.fleet_id
            || identity.owner_id != self.owner_id
            || identity.node_id != self.node_id
        {
            return Err(rejected(
                "selected primary identity changed; explicit enrollment is required",
            ));
        }
        Ok(SelectedBotsAuthority {
            client,
            owner: self.owner_id,
        })
    }
}

/// A session tied to one selected authority. RPC failure propagates to the caller, which
/// retains the draft/request ID and displays offline/pending state. Never retry on another DB.
pub struct SelectedBotsAuthority {
    client: RemoteLocalHub,
    owner: Uuid,
}
impl SelectedBotsAuthority {
    pub async fn agents(&self) -> Result<Vec<AgentProfile>> {
        self.client.bots_agents_list().await
    }
    pub async fn conversations(&self) -> Result<Vec<Conversation>> {
        self.client
            .bots_conversations_list(Principal::User(self.owner))
            .await
    }
    pub async fn messages(&self, conversation: Uuid, page: MessagePage) -> Result<Vec<Message>> {
        self.client
            .bots_messages_list(Principal::User(self.owner), conversation, page)
            .await
    }
    pub async fn send(
        &self,
        conversation: Uuid,
        request_id: String,
        policy_revision: u32,
        recipients: Vec<Uuid>,
        message: NewMessage,
    ) -> Result<Message> {
        self.client
            .bots_message_send(
                Principal::User(self.owner),
                conversation,
                request_id,
                policy_revision,
                recipients,
                message,
            )
            .await
    }
}
