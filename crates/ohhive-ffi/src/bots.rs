//! Local Bots bridge. Account identity is obtained once through whoami, never from UI author IDs.
//! Subsequent SQLite operations run on blocking workers and do not send chat content to a hub.
use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::{bots::*, hub::HubClient, local_hub::LocalHubStore, nodeconfig};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, uniffi::Record)]
pub struct BotsAgent {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub runtime_kind: String,
    pub preferred_host: Option<String>,
    pub role_revision: u32,
    pub capability_policy_ref: String,
    pub memory_namespace: String,
    pub archived: bool,
}
impl From<AgentProfile> for BotsAgent {
    fn from(a: AgentProfile) -> Self {
        Self {
            id: a.id.to_string(),
            owner: a.owner.to_string(),
            name: a.name,
            runtime_kind: match a.runtime_kind {
                AgentRuntimeKind::Local => "local",
                AgentRuntimeKind::ChatgptSubscription => "chatgpt_subscription",
                AgentRuntimeKind::CopilotSubscription => "copilot_subscription",
                AgentRuntimeKind::GrokSubscription => "grok_subscription",
            }
            .into(),
            preferred_host: a.preferred_host.map(|v| v.to_string()),
            role_revision: a.role_revision,
            capability_policy_ref: a.capability_policy_ref,
            memory_namespace: a.memory_namespace,
            archived: a.archived,
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct BotsConversation {
    pub id: String,
    pub owner: String,
    pub kind: String,
    pub project_id: Option<String>,
    pub coordinator: Option<String>,
    pub storage_scope: String,
    pub policy_revision: u32,
    pub created_at: String,
}
impl From<Conversation> for BotsConversation {
    fn from(c: Conversation) -> Self {
        Self {
            id: c.id.to_string(),
            owner: c.owner.to_string(),
            kind: match c.kind {
                ConversationKind::AgentDm => "agent_dm",
                ConversationKind::Team => "team",
                ConversationKind::Project => "project",
            }
            .into(),
            project_id: c.project_id.map(|v| v.to_string()),
            coordinator: c.coordinator.map(|v| v.to_string()),
            storage_scope: match c.storage_scope {
                StorageScope::LocalOnly => "local_only",
                StorageScope::HubBacked => "hub_backed",
            }
            .into(),
            policy_revision: c.policy_revision,
            created_at: c.created_at.to_rfc3339(),
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct BotsMessage {
    pub id: String,
    pub conversation_id: String,
    pub thread_root: Option<String>,
    pub author_kind: String,
    pub author_id: String,
    pub server_sequence: u64,
    pub client_request_id: String,
    pub kind: String,
    pub body: Option<String>,
    pub attachment_refs: Vec<String>,
    pub task_ref: Option<String>,
    pub turn_ref: Option<String>,
    pub source_event_ref: Option<String>,
    pub created_at: String,
}
impl From<Message> for BotsMessage {
    fn from(m: Message) -> Self {
        let (kind, id) = match m.author {
            Principal::User(id) => ("user", id),
            Principal::Agent(id) => ("agent", id),
        };
        Self {
            id: m.id.to_string(),
            conversation_id: m.conversation_id.to_string(),
            thread_root: m.thread_root.map(|v| v.to_string()),
            author_kind: kind.into(),
            author_id: id.to_string(),
            server_sequence: m.server_sequence,
            client_request_id: m.client_request_id,
            kind: match m.kind {
                MessageKind::Text => "text",
                MessageKind::System => "system",
                MessageKind::TaskReceipt => "task_receipt",
            }
            .into(),
            body: m.body,
            attachment_refs: m.attachment_refs,
            task_ref: m.task_ref.map(|v| v.to_string()),
            turn_ref: m.turn_ref,
            source_event_ref: m.source_event_ref,
            created_at: m.created_at.to_rfc3339(),
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct BotsPage {
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub limit: u32,
}
#[derive(Clone, uniffi::Record)]
pub struct BotsSend {
    pub conversation_id: String,
    pub client_request_id: String,
    pub expected_policy_revision: u32,
    pub recipient_ids: Vec<String>,
    pub body: String,
    pub thread_root: Option<String>,
}

// One drain at a time in this process, even if a UI reopens its session mid-turn.
static DRAIN_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, uniffi::Record)]
pub struct BotsDrain {
    pub delivered: u32,
    pub failed: u32,
    pub requeued: u32,
}

#[derive(uniffi::Object)]
pub struct BotsSession {
    store: LocalHubStore,
    owner: Uuid,
    host: Uuid,
    // Detect local unpair/account changes. No credential crosses the foreign-language boundary.
    connection: Option<(String, String)>,
}
fn fail(s: &str) -> HiveError {
    HiveError::Failed(s.into())
}
fn id(s: &str) -> Result<Uuid, HiveError> {
    Uuid::parse_str(s).map_err(|_| fail("Invalid Bots identifier"))
}
fn storage(e: hive_core::hub::HubError) -> HiveError {
    HiveError::Failed(e.to_string())
}
impl BotsSession {
    fn validate(&self) -> Result<(), HiveError> {
        if let Some((url, key)) = &self.connection {
            let cfg = nodeconfig::load().map_err(HiveError::from)?;
            if cfg.hub_url != *url || cfg.node_key.as_ref() != Some(key) {
                return Err(fail("Account changed. Reopen Bots."));
            }
        }
        Ok(())
    }
    async fn call<T: Send + 'static>(
        self: Arc<Self>,
        op: impl FnOnce(&Self) -> Result<T, HiveError> + Send + 'static,
    ) -> Result<T, HiveError> {
        RUNTIME
            .spawn_blocking(move || {
                self.validate()?;
                op(&self)
            })
            .await
            .map_err(|_| fail("Bots service stopped"))?
    }
    fn owned_agent(&self, agent: Uuid) -> Result<(), HiveError> {
        if self
            .store
            .bots_agents_list(self.owner)
            .map_err(storage)?
            .iter()
            .any(|a| a.id == agent && !a.archived)
        {
            Ok(())
        } else {
            Err(fail("Agent is not an active agent owned by this account"))
        }
    }
    fn send(&self, draft: BotsSend) -> Result<BotsMessage, HiveError> {
        if draft.body.trim().is_empty()
            || draft.body.len() > 64 * 1024
            || draft.client_request_id.is_empty()
            || draft.client_request_id.len() > 200
            || draft.recipient_ids.len() > 16
        {
            return Err(fail("Invalid message size, recipients or request ID"));
        }
        let conversation_id = id(&draft.conversation_id)?;
        let conversation = self
            .store
            .bots_conversations_list(Principal::User(self.owner))
            .map_err(storage)?
            .into_iter()
            .find(|c| {
                c.id == conversation_id
                    && c.owner == self.owner
                    && c.kind == ConversationKind::AgentDm
                    && c.storage_scope == StorageScope::LocalOnly
            })
            .ok_or_else(|| fail("Message requires an owned local DM"))?;
        let recipients: Vec<_> = draft
            .recipient_ids
            .iter()
            .map(|v| id(v))
            .collect::<Result<_, _>>()?;
        if recipients.len() != 1 || conversation.coordinator != recipients.first().copied() {
            return Err(fail("DM recipient must be its coordinator"));
        }
        for recipient in &recipients {
            self.owned_agent(*recipient)?;
        }
        if let Some(root) = &draft.thread_root {
            let message = self.store.bots_message_get(id(root)?).map_err(storage)?;
            if message.conversation_id != conversation_id {
                return Err(fail("Thread belongs to another conversation"));
            }
        }
        self.store
            .bots_message_send(
                Principal::User(self.owner),
                id(&draft.conversation_id)?,
                draft.client_request_id,
                draft.expected_policy_revision,
                recipients,
                NewMessage {
                    thread_root: draft.thread_root.as_deref().map(id).transpose()?,
                    kind: MessageKind::Text,
                    body: Some(draft.body),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .map(Into::into)
            .map_err(storage)
    }
}
#[uniffi::export]
impl HiveNode {
    /// Explicit connection step; whoami sends account authentication only, never message content.
    pub async fn bots_open(self: Arc<Self>) -> Result<Arc<BotsSession>, HiveError> {
        RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .ok_or_else(|| fail("Pair this node before opening Bots"))?;
                let me = HubClient::new(&cfg.hub_url, &cfg.anon_key, key.clone())
                    .whoami()
                    .await
                    .map_err(HiveError::from)?;
                let store = RUNTIME
                    .spawn_blocking(|| {
                        LocalHubStore::open(nodeconfig::path().with_file_name("vault-host.sqlite3"))
                    })
                    .await
                    .map_err(|_| fail("Cannot open Bots store"))?
                    .map_err(storage)?;
                Ok(Arc::new(BotsSession {
                    store,
                    owner: me.member_id,
                    host: me.node_id,
                    connection: Some((cfg.hub_url, key)),
                }))
            })
            .await
            .map_err(|_| fail("Bots connection stopped"))?
    }
}
#[uniffi::export]
impl BotsSession {
    /// Runs a bounded core drain pass. The app owns polling; no detached infinite Rust loop.
    /// Cancellation of the UI waiter does not abort a claimed delivery mid-write.
    pub async fn drain_once(self: Arc<Self>) -> Result<BotsDrain, HiveError> {
        RUNTIME
            .spawn(async move {
                let _guard = DRAIN_GATE
                    .try_lock()
                    .map_err(|_| fail("Another Bots reply is running"))?;
                self.validate()?;
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let model = crate::model_pref()
                    .ok_or_else(|| fail("Choose a local model in Settings to enable replies"))?;
                let runner = LocalModelTurnRunner::loopback(self.host, model, &cfg.llama_url)
                    .map_err(|_| fail("Bots replies require a local model on this Mac"))?;
                let executor = DeliveryExecutor::new(
                    Arc::new(self.store.clone()),
                    Arc::new(runner),
                    self.host,
                    self.owner,
                );
                let result = executor.drain_once().await;
                Ok(BotsDrain {
                    delivered: result.delivered as u32,
                    failed: result.failed as u32,
                    requeued: result.requeued as u32,
                })
            })
            .await
            .map_err(|_| fail("Bots reply worker stopped"))?
    }

    pub fn owner_id(&self) -> String {
        self.owner.to_string()
    }
    pub fn host_id(&self) -> String {
        self.host.to_string()
    }
    pub async fn agents_list(self: Arc<Self>) -> Result<Vec<BotsAgent>, HiveError> {
        self.call(|s| {
            s.store
                .bots_agents_list(s.owner)
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(storage)
        })
        .await
    }
    /// Registers a Local agent on this authenticated node, matching the CLI's current policy placeholder.
    pub async fn agents_create(self: Arc<Self>, name: String) -> Result<BotsAgent, HiveError> {
        self.call(move |s| {
            if name.trim().is_empty() || name.len() > 256 {
                return Err(fail("Agent name is required and must be at most 256 bytes"));
            }
            s.store
                .bots_agents_create(NewAgentProfile {
                    owner: s.owner,
                    name,
                    runtime_kind: AgentRuntimeKind::Local,
                    preferred_host: Some(s.host),
                    capability_policy_ref: "default".into(),
                    provider_account_ref: None,
                    memory_namespace: format!("agent:{}", Uuid::new_v4()),
                })
                .map(Into::into)
                .map_err(storage)
        })
        .await
    }
    pub async fn conversations_list(self: Arc<Self>) -> Result<Vec<BotsConversation>, HiveError> {
        self.call(|s| {
            s.store
                .bots_conversations_list(Principal::User(s.owner))
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(storage)
        })
        .await
    }
    /// C1 creates local DMs only; room policy and hub-backed conversations remain C2.
    pub async fn conversations_create(
        self: Arc<Self>,
        agent_id: String,
    ) -> Result<BotsConversation, HiveError> {
        self.call(move |s| {
            let agent = id(&agent_id)?;
            s.owned_agent(agent)?;
            s.store
                .bots_conversations_create(NewConversation {
                    owner: s.owner,
                    kind: ConversationKind::AgentDm,
                    project_id: None,
                    coordinator: Some(agent),
                    storage_scope: StorageScope::LocalOnly,
                })
                .map(Into::into)
                .map_err(storage)
        })
        .await
    }
    pub async fn conversations_join(
        self: Arc<Self>,
        conversation_id: String,
    ) -> Result<(), HiveError> {
        self.call(move |s| {
            let conversation_id = id(&conversation_id)?;
            let visible = s
                .store
                .bots_conversations_list(Principal::User(s.owner))
                .map_err(storage)?;
            if !visible
                .iter()
                .any(|c| c.id == conversation_id && c.owner == s.owner)
            {
                return Err(fail("Cannot join another account's conversation"));
            }
            s.store
                .bots_conversations_join(Principal::User(s.owner), conversation_id)
                .map(|_| ())
                .map_err(storage)
        })
        .await
    }
    pub async fn messages_list(
        self: Arc<Self>,
        conversation_id: String,
        page: BotsPage,
    ) -> Result<Vec<BotsMessage>, HiveError> {
        self.call(move |s| {
            if page.limit == 0
                || page.limit > 200
                || (page.before.is_some() && page.after.is_some())
            {
                return Err(fail("Choose one message cursor and a limit from 1 to 200"));
            }
            s.store
                .bots_messages_list(
                    Principal::User(s.owner),
                    id(&conversation_id)?,
                    MessagePage {
                        before: page.before,
                        after: page.after,
                        limit: page.limit,
                    },
                )
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(storage)
        })
        .await
    }
    pub async fn message_send(self: Arc<Self>, draft: BotsSend) -> Result<BotsMessage, HiveError> {
        self.call(move |s| s.send(draft)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(store: LocalHubStore) -> Arc<BotsSession> {
        Arc::new(BotsSession {
            store,
            owner: Uuid::new_v4(),
            host: Uuid::new_v4(),
            connection: None,
        })
    }
    fn page() -> BotsPage {
        BotsPage {
            before: None,
            after: None,
            limit: 50,
        }
    }
    fn draft(c: &BotsConversation, a: &BotsAgent) -> BotsSend {
        BotsSend {
            conversation_id: c.id.clone(),
            client_request_id: Uuid::new_v4().to_string(),
            expected_policy_revision: c.policy_revision,
            recipient_ids: vec![a.id.clone()],
            body: "Hello local agent".into(),
            thread_root: None,
        }
    }
    #[tokio::test]
    async fn overlapping_drain_is_rejected_before_claiming() {
        let _guard = DRAIN_GATE.lock().await;
        let s = session(LocalHubStore::in_memory().unwrap());
        assert!(s.drain_once().await.is_err());
    }

    #[tokio::test]
    async fn dm_roundtrip_and_retry() {
        let s = session(LocalHubStore::in_memory().unwrap());
        let a = s.clone().agents_create("Test agent".into()).await.unwrap();
        assert_eq!(a.owner, s.owner_id());
        assert_eq!(a.preferred_host, Some(s.host_id()));
        assert_eq!(s.clone().agents_list().await.unwrap().len(), 1);
        let c = s.clone().conversations_create(a.id.clone()).await.unwrap();
        s.clone().conversations_join(c.id.clone()).await.unwrap();
        let d = draft(&c, &a);
        let m = s.clone().message_send(d.clone()).await.unwrap();
        let retry = s.clone().message_send(d).await.unwrap();
        assert_eq!(m.id, retry.id);
        assert_eq!(m.author_id, s.owner_id());
        assert_eq!(m.author_kind, "user");
        assert_eq!(m.server_sequence, 1);
        let messages = s.clone().messages_list(c.id.clone(), page()).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body.as_deref(), Some("Hello local agent"));
        assert_eq!(s.clone().conversations_list().await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn isolates_accounts_and_dm_recipients() {
        let store = LocalHubStore::in_memory().unwrap();
        let s = session(store.clone());
        let other = session(store);
        let a = s.clone().agents_create("One".into()).await.unwrap();
        let c = s.clone().conversations_create(a.id.clone()).await.unwrap();
        assert!(other.clone().agents_list().await.unwrap().is_empty());
        assert!(other.clone().conversations_list().await.unwrap().is_empty());
        assert!(other
            .clone()
            .conversations_create(a.id.clone())
            .await
            .is_err());
        assert!(other
            .clone()
            .conversations_join(c.id.clone())
            .await
            .is_err());
        assert!(other
            .clone()
            .messages_list(c.id.clone(), page())
            .await
            .is_err());
        assert!(other.clone().message_send(draft(&c, &a)).await.is_err());
        let b = s.clone().agents_create("Two".into()).await.unwrap();
        assert!(s.clone().message_send(draft(&c, &b)).await.is_err());
        let c2 = s.clone().conversations_create(b.id.clone()).await.unwrap();
        let m = s.clone().message_send(draft(&c2, &b)).await.unwrap();
        let mut d = draft(&c, &a);
        d.thread_root = Some(m.id);
        assert!(s.clone().message_send(d).await.is_err());
    }
    #[tokio::test]
    async fn rejects_invalid_input_and_stale_policy() {
        let s = session(LocalHubStore::in_memory().unwrap());
        assert!(s.clone().agents_create(" ".into()).await.is_err());
        assert!(s
            .clone()
            .conversations_create("not-a-uuid".into())
            .await
            .is_err());
        let a = s.clone().agents_create("One".into()).await.unwrap();
        let c = s.clone().conversations_create(a.id.clone()).await.unwrap();
        let mut d = draft(&c, &a);
        d.expected_policy_revision += 1;
        assert!(s.clone().message_send(d).await.is_err());
        let mut d = draft(&c, &a);
        d.body = " ".into();
        assert!(s.clone().message_send(d).await.is_err());
        let mut d = draft(&c, &a);
        d.body = "x".repeat(65537);
        assert!(s.clone().message_send(d).await.is_err());
        assert!(s
            .clone()
            .messages_list(
                c.id.clone(),
                BotsPage {
                    before: Some(2),
                    after: Some(1),
                    limit: 10
                }
            )
            .await
            .is_err());
        assert!(s
            .clone()
            .messages_list(
                c.id.clone(),
                BotsPage {
                    before: None,
                    after: None,
                    limit: 201
                }
            )
            .await
            .is_err());
        assert!(s
            .clone()
            .messages_list(c.id, page())
            .await
            .unwrap()
            .is_empty());
    }
}
