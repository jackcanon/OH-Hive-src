//! Local Bots bridge. Account identity is obtained once through whoami, never from UI author IDs.
//! Subsequent SQLite operations run on blocking workers and do not send chat content to a hub.
use crate::bots_storage::BotsStorage;
use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::{bots::*, hub::HubClient, nodeconfig};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, uniffi::Record)]
pub struct BotsAgent {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub runtime_kind: String,
    pub preferred_host: Option<String>,
    /// The realm: the display name of `preferred_host` in this vault. Read-only and derived --
    /// the app shows it, never edits it. `None` for an agent that answers hub-side on no
    /// particular machine, or one pinned to a host the vault has no row for.
    pub host_name: Option<String>,
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
                AgentRuntimeKind::AnthropicByok => "anthropic_byok",
                AgentRuntimeKind::NousByok => "nous_byok",
            }
            .into(),
            preferred_host: a.preferred_host.map(|v| v.to_string()),
            host_name: a.host_name,
            role_revision: a.role_revision,
            capability_policy_ref: a.capability_policy_ref,
            memory_namespace: a.memory_namespace,
            archived: a.archived,
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct BotsConversation {
    pub title: Option<String>,
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
            title: c.title,
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

#[derive(Clone, uniffi::Record)]
pub struct BotsMentions {
    pub recipient_ids: Vec<String>,
    pub unresolved: Vec<String>,
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
    store: BotsStorage,
    owner: Uuid,
    host: Uuid,
    // Detect local unpair/account changes. No credential crosses the foreign-language boundary.
    connection: Option<(String, String)>,
    private_key: Option<String>,
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
        self.store.validate_selection().map_err(storage)?;
        if let Some((url, key)) = &self.connection {
            let cfg = nodeconfig::load().map_err(HiveError::from)?;
            if cfg.hub_url != *url || cfg.node_key.as_ref() != Some(key) {
                return Err(fail("Account changed. Reopen Bots."));
            }
        }
        if let Some(key) = &self.private_key {
            if nodeconfig::get_extra("HIVE_VAULT_SELF_KEY").as_ref() != Some(key) {
                return Err(fail("Private Fleet account changed. Reopen Bots."));
            }
            let identity = self
                .store
                .local()
                .map_err(storage)?
                .connect(key)
                .map_err(storage)?
                .private_fleet_identity()
                .map_err(storage)?
                .ok_or_else(|| fail("Private Fleet enrollment is required"))?;
            if identity.owner_id != self.owner || identity.node_id != self.host {
                return Err(fail("Private Fleet account changed. Reopen Bots."));
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
                    && c.storage_scope == StorageScope::LocalOnly
            })
            .ok_or_else(|| fail("Message requires an owned local conversation"))?;
        let recipients: Vec<_> = draft
            .recipient_ids
            .iter()
            .map(|v| id(v))
            .collect::<Result<_, _>>()?;
        if conversation.kind == ConversationKind::AgentDm
            && (recipients.len() != 1 || conversation.coordinator != recipients.first().copied())
        {
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
                if let Some((selection, wire)) = crate::private_fleet::selected()? {
                    let client = selection.connect().await.map_err(storage)?.into_transport();
                    return Ok(Arc::new(BotsSession {
                        store: BotsStorage::Remote {
                            client,
                            selection: wire,
                        },
                        owner: selection.owner_id,
                        host: selection.node_id,
                        connection: None,
                        private_key: None,
                    }));
                }
                let node = self.clone();
                let private = RUNTIME
                    .spawn_blocking(move || node.private_bots_context())
                    .await
                    .map_err(|_| fail("Cannot open Private Fleet"))??;
                if let Some((store, owner, host, key)) = private {
                    return Ok(Arc::new(BotsSession {
                        store: BotsStorage::Local(store),
                        owner,
                        host,
                        connection: None,
                        private_key: Some(key),
                    }));
                }
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .ok_or_else(|| fail("Pair this node before opening Bots"))?;
                let me = HubClient::new(&cfg.hub_url, &cfg.anon_key, key.clone())
                    .whoami()
                    .await
                    .map_err(HiveError::from)?;
                let store = RUNTIME
                    .spawn_blocking(move || self.bind_bots_owner(me.member_id))
                    .await
                    .map_err(|_| fail("Cannot open Bots store"))??;
                Ok(Arc::new(BotsSession {
                    store: BotsStorage::Local(store),
                    owner: me.member_id,
                    host: me.node_id,
                    connection: Some((cfg.hub_url, key)),
                    private_key: None,
                }))
            })
            .await
            .map_err(|_| fail("Bots connection stopped"))?
    }
}
#[uniffi::export]
impl BotsSession {
    pub async fn conversation_deliveries(self: Arc<Self>, conversation_id: String) -> Result<String, HiveError> {
        self.call(move |s| {
            let id = Uuid::parse_str(&conversation_id).map_err(|_| fail("Invalid conversation"))?;
            let rows = s.store.conversation_deliveries(s.owner,id).map_err(storage)?;
            serde_json::to_string(&rows).map_err(|_| fail("Cannot read reply status"))
        }).await
    }
    pub async fn host_agent_deleted(self: Arc<Self>) -> Result<bool,HiveError> { self.call(|s| s.store.agent_was_archived(s.owner,"local".into(),Some(s.host)).map_err(storage)).await }
    pub async fn agent_bio_get(self: Arc<Self>, agent_id: String) -> Result<String,HiveError> { self.call(move |s| { let p=s.store.agent_bio_get(s.owner,id(&agent_id)?).map_err(storage)?; serde_json::to_string(&p).map_err(|_|fail("Cannot read agent profile")) }).await }
    pub async fn agent_bio_set(self: Arc<Self>, agent_id: String, name: String, profile: String) -> Result<(),HiveError> { self.call(move |s| { let p: AgentBio=serde_json::from_str(&profile).map_err(|_|fail("Invalid agent profile"))?; s.store.agent_bio_set(s.owner,id(&agent_id)?,name,p).map(|_|()).map_err(storage) }).await }
    pub async fn agents_archive(self: Arc<Self>, agent_id: String) -> Result<(),HiveError> { self.call(move |s| s.store.agent_archive(s.owner,id(&agent_id)?).map_err(storage)).await }
    pub async fn user_profile_get(self: Arc<Self>) -> Result<String, HiveError> {
        self.call(|s| { let p = s.store.user_profile_get(s.owner).map_err(storage)?; serde_json::to_string(&p).map_err(|_| fail("Cannot read profile")) }).await
    }
    pub async fn user_profile_set(self: Arc<Self>, preferred_name: String, about: String) -> Result<(), HiveError> {
        self.call(move |s| s.store.user_profile_set(s.owner, UserProfile { preferred_name, about }).map(|_| ()).map_err(storage)).await
    }
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
                // Works for a remote primary too. `self.host` is the node id the hub issued
                // for THIS machine at enrollment -- the vault's namespace, which is what
                // `preferred_host` is compared against on the far side. Passing this machine's
                // Hive account node id instead would have it drain nothing and say nothing,
                // which is the exact shape of the bug that cost an evening on the CLI.
                let store = self.store.delivery_store();
                let local = crate::model_pref().and_then(|model| {
                    LocalModelTurnRunner::loopback(self.host, model, &cfg.llama_url).ok()
                });
                let mut executor = match local {
                    Some(runner) => {
                        DeliveryExecutor::new(store, Arc::new(runner), self.host, self.owner)
                    }
                    None => DeliveryExecutor::without_local_runner(store, self.host, self.owner),
                };
                // Only a community session authenticated by whoami authorizes this node key.
                // Private-fleet identities must never borrow unrelated community credentials.
                if let Some((hub_url, key)) = &self.connection {
                    if let Ok(cloud) =
                        CloudTurnRunner::new(hub_url, cfg.anon_key, key.clone(), self.owner)
                    {
                        executor = executor.with_cloud_runner(Arc::new(cloud));
                    }
                }
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

    pub fn uses_remote_primary(&self) -> bool {
        self.store.is_remote()
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
    /// Auto-provisions a Bots agent for every BYOK provider (Anthropic, Nous) the member has a
    /// key on file for in Settings, so "you have a key configured" becomes "there's an agent
    /// you can DM" without a separate manual register step per provider. Idempotent -- skips
    /// any provider that already has a non-archived agent of the matching runtime kind, so
    /// it's safe to call every time the Bots screen opens (`BotsModel.refreshAgents()` and its
    /// Tauri/CLI equivalents are expected to call this before `agents_list`, not the other way
    /// around, so a freshly-provisioned agent shows up in the same load rather than needing a
    /// second refresh). Local agents (`agents_create`) are untouched. No reply capability yet
    /// -- this only creates the identity; a cloud turn runner to actually answer as one of
    /// these agents is separate, not-yet-built work (see
    /// docs/LOKI-BOTS-C2-GROUP-AND-ADAPTER-AGENTS-PLAN-2026-09-15.md).
    pub async fn ensure_provider_agents(self: Arc<Self>) -> Result<Vec<BotsAgent>, HiveError> {
        // Private enrollment does not authorize community BYOK queries. Local Bots must
        // remain usable offline and without a community node key.
        if self.private_key.is_some() || self.store.is_remote() {
            return self.call(|_| Ok(Vec::new())).await;
        }
        // Sif's review (SIF-AGENT-INSPECTOR-IMPLEMENTATION-2026-09-15.md): the hub round trip
        // belongs on the owned runtime like every other network FFI export (bots_open's
        // pattern), and session identity must be checked before it, not only before the local
        // store write that follows.
        let session = self.clone();
        let status = RUNTIME
            .spawn(async move {
                session.validate()?;
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| fail("Pair this node before opening Bots"))?;
                HubClient::new(&cfg.hub_url, &cfg.anon_key, key)
                    .member_key_status()
                    .await
                    .map_err(HiveError::from)
            })
            .await
            .map_err(|_| fail("Bots provisioning check stopped"))??;
        let wanted: Vec<(AgentRuntimeKind, &str, bool)> = vec![
            (
                AgentRuntimeKind::AnthropicByok,
                "Claude",
                status.anthropic.is_some(),
            ),
            (AgentRuntimeKind::NousByok, "Nous", status.nous.is_some()),
        ];
        self.call(move |s| {
            let existing = s.store.bots_agents_list(s.owner).map_err(storage)?;
            let mut created = Vec::new();
            for (kind, default_name, has_key) in wanted {
                if !has_key {
                    continue;
                }
                if existing
                    .iter()
                    .any(|a| a.runtime_kind == kind)
                {
                    continue;
                }
                let runtime = serde_json::to_value(kind).map_err(|_|fail("Invalid runtime"))?.as_str().unwrap_or_default().to_string();
                if s.store.agent_was_archived(s.owner,runtime,None).map_err(storage)? { continue; }
                let agent = s
                    .store
                    .bots_agents_create(NewAgentProfile {
                        owner: s.owner,
                        name: default_name.to_string(),
                        runtime_kind: kind,
                        preferred_host: None,
                        capability_policy_ref: "default".into(),
                        provider_account_ref: None,
                        memory_namespace: format!("agent:{}", Uuid::new_v4()),
                    })
                    .map_err(storage)?;
                created.push(agent.into());
            }
            Ok(created)
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
    /// Edits metadata only. Policy references do not grant or restrict tools yet.
    pub async fn agents_update(
        self: Arc<Self>,
        agent_id: String,
        name: Option<String>,
        capability_policy_ref: Option<String>,
    ) -> Result<BotsAgent, HiveError> {
        self.call(move |s| {
            let agent = id(&agent_id)?;
            s.owned_agent(agent)?;
            if name
                .as_ref()
                .is_some_and(|v| v.trim().is_empty() || v.len() > 200)
            {
                return Err(fail("Agent name must be 1–200 bytes"));
            }
            if capability_policy_ref
                .as_ref()
                .is_some_and(|v| v.trim().is_empty() || v.len() > 500)
            {
                return Err(fail("Capability policy reference must be 1–500 bytes"));
            }
            s.store
                .bots_agents_update(
                    s.owner,
                    agent,
                    AgentProfilePatch {
                        name,
                        capability_policy_ref,
                        preferred_host: None,
                        memory_namespace: None,
                    },
                )
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
                    title: None,
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
    /// Human-driven rooms. Agent replies still have no recipients.
    pub async fn rooms_create(
        self: Arc<Self>,
        request_id: String,
        title: String,
        kind: String,
        agent_ids: Vec<String>,
        project_id: Option<String>,
        coordinator_id: Option<String>,
    ) -> Result<BotsConversation, HiveError> {
        self.call(move |s| {
            let kind = match kind.as_str() {
                "team" => ConversationKind::Team,
                "project" => ConversationKind::Project,
                _ => return Err(fail("Choose team or project")),
            };
            if title.trim().is_empty()
                || title.len() > 200
                || agent_ids.is_empty()
                || agent_ids.len() > 16
            {
                return Err(fail("Name the room and choose 1–16 agents"));
            }
            if (kind == ConversationKind::Project) != project_id.is_some() {
                return Err(fail("Only project rooms require a project"));
            }
            let project_id = project_id.as_deref().map(id).transpose()?;
            let agents = agent_ids
                .iter()
                .map(|v| id(v))
                .collect::<Result<Vec<_>, _>>()?;
            let coordinator = coordinator_id.as_deref().map(id).transpose()?;
            if coordinator.is_some_and(|a| !agents.contains(&a)) {
                return Err(fail("Coordinator must be a selected room member"));
            }
            let room = s
                .store
                .bots_rooms_create(
                    id(&request_id)?,
                    NewConversation {
                        title: Some(title.trim().into()),
                        owner: s.owner,
                        kind,
                        project_id,
                        coordinator,
                        storage_scope: StorageScope::LocalOnly,
                    },
                    agents,
                )
                .map_err(storage)?;
            Ok(room.into())
        })
        .await
    }
    pub async fn room_agents(
        self: Arc<Self>,
        conversation_id: String,
    ) -> Result<Vec<BotsAgent>, HiveError> {
        self.call(move |s| {
            s.store
                .bots_room_agents(Principal::User(s.owner), id(&conversation_id)?)
                .map(|a| a.into_iter().map(Into::into).collect())
                .map_err(storage)
        })
        .await
    }
    pub async fn mentions_resolve(
        self: Arc<Self>,
        conversation_id: String,
        body: String,
    ) -> Result<BotsMentions, HiveError> {
        self.call(move |s| {
            if body.len() > 65536 {
                return Err(fail("Message is too long"));
            }
            let roster = s
                .store
                .bots_room_agents(Principal::User(s.owner), id(&conversation_id)?)
                .map_err(storage)?;
            let mentions = resolve_mentions(&body, &roster, Principal::User(s.owner));
            Ok(BotsMentions {
                recipient_ids: mentions
                    .recipients
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                unresolved: mentions.unresolved,
            })
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
    use hive_core::local_hub::LocalHubStore;
    fn session(store: LocalHubStore) -> Arc<BotsSession> {
        Arc::new(BotsSession {
            store: BotsStorage::Local(store),
            owner: Uuid::new_v4(),
            host: Uuid::new_v4(),
            connection: None,
            private_key: None,
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
    async fn metadata_update_roundtrip_and_account_isolation() {
        let store = LocalHubStore::in_memory().unwrap();
        let s = session(store.clone());
        let other = session(store);
        let a = s.clone().agents_create("Before".into()).await.unwrap();
        assert!(other
            .agents_update(a.id.clone(), Some("Intruder".into()), None)
            .await
            .is_err());
        let updated = s
            .clone()
            .agents_update(
                a.id.clone(),
                Some("After".into()),
                Some("future-policy".into()),
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "After");
        assert_eq!(updated.capability_policy_ref, "future-policy");
        assert_eq!(updated.preferred_host, a.preferred_host);
        assert_eq!(updated.memory_namespace, a.memory_namespace);
        let listed = s.clone().agents_list().await.unwrap();
        assert_eq!(listed[0].name, "After");
        assert_eq!(listed[0].capability_policy_ref, "future-policy");
        assert!(s
            .clone()
            .agents_update(a.id.clone(), Some(" ".into()), None)
            .await
            .is_err());
        assert!(s
            .clone()
            .agents_update(a.id.clone(), Some("x".repeat(201)), None)
            .await
            .is_err());
        assert!(s
            .clone()
            .agents_update(a.id.clone(), None, Some(" ".into()))
            .await
            .is_err());
        assert!(s
            .clone()
            .agents_update(a.id.clone(), None, Some("x".repeat(501)))
            .await
            .is_err());
        let retained = s.agents_update(a.id, None, None).await.unwrap();
        assert_eq!(retained.name, "After");
        assert_eq!(retained.capability_policy_ref, "future-policy");
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
    async fn rooms_preserve_metadata_resolve_members_and_allow_quiet_posts() {
        let store = LocalHubStore::in_memory().unwrap();
        let s = session(store.clone());
        let a = s.clone().agents_create("Sif".into()).await.unwrap();
        let b = s.clone().agents_create("Nous".into()).await.unwrap();
        let outsider = s.clone().agents_create("Outside".into()).await.unwrap();
        let project = Uuid::new_v4().to_string();
        let room = s
            .clone()
            .rooms_create(
                Uuid::new_v4().to_string(),
                "Launch".into(),
                "project".into(),
                vec![a.id.clone(), b.id.clone()],
                Some(project.clone()),
                Some(a.id.clone()),
            )
            .await
            .unwrap();
        let listed = s.clone().conversations_list().await.unwrap();
        assert_eq!(listed[0].title.as_deref(), Some("Launch"));
        assert_eq!(listed[0].project_id.as_deref(), Some(project.as_str()));
        assert_eq!(
            s.clone().room_agents(room.id.clone()).await.unwrap().len(),
            2
        );
        let mentions = s
            .clone()
            .mentions_resolve(room.id.clone(), "@sif @Nous @Outside".into())
            .await
            .unwrap();
        assert_eq!(mentions.recipient_ids.len(), 2);
        assert_eq!(mentions.unresolved, vec!["Outside"]);
        let mut d = draft(&room, &a);
        d.recipient_ids = mentions.recipient_ids;
        let sent = s.clone().message_send(d.clone()).await.unwrap();
        assert_eq!(s.clone().message_send(d).await.unwrap().id, sent.id);
        for agent in [&a, &b] {
            assert_eq!(
                store
                    .bots_deliveries_pending_for_agent(id(&agent.id).unwrap(), 10)
                    .unwrap()
                    .len(),
                1
            );
        }
        let mut quiet = draft(&room, &a);
        quiet.recipient_ids.clear();
        s.clone().message_send(quiet).await.unwrap();
        assert_eq!(
            store
                .bots_deliveries_pending_for_agent(id(&a.id).unwrap(), 10)
                .unwrap()
                .len(),
            1
        );
        assert!(s
            .clone()
            .message_send(draft(&room, &outsider))
            .await
            .is_err());
        let other = session(store);
        assert!(other.clone().room_agents(room.id.clone()).await.is_err());
        assert!(other
            .clone()
            .mentions_resolve(room.id, "@everyone".into())
            .await
            .is_err());
        assert!(other
            .rooms_create(
                Uuid::new_v4().to_string(),
                "Bad".into(),
                "team".into(),
                vec![a.id.clone()],
                None,
                None
            )
            .await
            .is_err());
        assert!(s
            .clone()
            .rooms_create(
                Uuid::new_v4().to_string(),
                "Bad".into(),
                "team".into(),
                vec![a.id.clone()],
                Some(project),
                None
            )
            .await
            .is_err());
        assert!(s
            .rooms_create(
                Uuid::new_v4().to_string(),
                "Bad".into(),
                "team".into(),
                vec![a.id],
                None,
                Some(outsider.id)
            )
            .await
            .is_err());
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
