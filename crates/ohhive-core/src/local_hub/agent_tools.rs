//! Owner-saved, resource-scoped agent tools. No prompt or opaque policy reference grants access.
use super::*;
use crate::bots::{AgentId, Handoff, HandoffId, HandoffState, NewHandoff};
use rusqlite::OptionalExtension;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolPolicy {
    pub revision: u32,
    /// Versioned template selection is descriptive; only concrete grants authorize tools.
    pub template: Option<String>,
    pub readable_vaults: Vec<Uuid>,
    /// Hosts the agent may fetch over HTTPS (exact host or a subdomain of it). `None` on the
    /// wire means "leave the saved list alone", so a client that predates this field cannot
    /// wipe it by re-sending a policy without it. Stored JSON always carries `Some`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_hosts: Option<Vec<String>>,
    /// Hosts the agent may POST a JSON body to. Kept separate from `web_hosts` (a read grant
    /// does not imply a write grant) -- see the module doc on `web_post_json` below. Same
    /// "None on the wire leaves the saved list alone" rule as `web_hosts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_post_hosts: Option<Vec<String>>,
    /// Agents this agent may hand work to via the `handoff_create` chat tool. Same "None on
    /// the wire leaves the saved list alone" rule as `web_hosts`/`web_post_hosts` -- an explicit
    /// grant, never a template or a prompt, is what lets one agent hand work to another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_targets: Option<Vec<Uuid>>,
}
impl AgentToolPolicy {
    pub fn web_hosts(&self) -> &[String] {
        self.web_hosts.as_deref().unwrap_or(&[])
    }
    pub fn web_post_hosts(&self) -> &[String] {
        self.web_post_hosts.as_deref().unwrap_or(&[])
    }
    pub fn handoff_targets(&self) -> &[Uuid] {
        self.handoff_targets.as_deref().unwrap_or(&[])
    }
}
/// A single hostname label set: lowercase ASCII letters, digits, `-` and `.`; no scheme,
/// port, path, wildcard or IP literal. Bounded so the allowlist stays a list of names.
/// `localhost` is the one single-label name accepted: it stands for this machine's loopback
/// (127.0.0.1 counts as it), which is how a locally served page or test fixture is allowed.
pub fn valid_web_host(h: &str) -> bool {
    h == "localhost"
        || h.len() <= 253
            && !h.is_empty()
            && !h.starts_with('.')
            && !h.ends_with('.')
            && !h.contains("..")
            && h.split('.').count() >= 2
            && h.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
            && !h.split('.').all(|l| l.bytes().all(|b| b.is_ascii_digit()))
}
/// `host` is allowed when it equals an entry or is a subdomain of one (`docs.a.b` under `a.b`).
pub fn host_allowed(allow: &[String], host: &str) -> bool {
    allow
        .iter()
        .any(|a| host == a || host.strip_suffix(a).is_some_and(|p| p.ends_with('.')))
}
/// Replaces every string value in `body` that is exactly "{{SECRET}}" with `secret` (recursing
/// into arrays and objects; keys are left alone). Exact-match only, on purpose -- no partial
/// substring interpolation, so there is exactly one thing a body author can do with the
/// placeholder and no surprise concatenation. A placeholder left in place when no secret is
/// configured is sent as literal text, which will simply fail the destination's own auth check
/// -- visible in the tool result, not a silent leak.
fn substitute_secret(mut body: serde_json::Value, secret: Option<&str>) -> serde_json::Value {
    let Some(secret) = secret else { return body };
    fn walk(v: &mut serde_json::Value, secret: &str) {
        match v {
            serde_json::Value::String(s) if s == "{{SECRET}}" => *s = secret.to_string(),
            serde_json::Value::Array(items) => items.iter_mut().for_each(|i| walk(i, secret)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|i| walk(i, secret)),
            _ => {}
        }
    }
    walk(&mut body, secret);
    body
}
/// Authority to fetch one URL, issued by the hub before the agent host performs the request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebFetchGrant {
    pub receipt: Uuid,
    pub url: String,
    pub host: String,
}
/// Authority to POST one JSON body to one URL, issued by the hub before the agent host performs
/// the request. `body` travels with the grant (unlike fetch, there is a payload to authorize,
/// not just a destination) so the hub's receipt reflects what was actually sent.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebPostGrant {
    pub receipt: Uuid,
    pub url: String,
    pub host: String,
    pub body: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolCall {
    VaultSearch {
        vault: Uuid,
        query: String,
        limit: u32,
    },
    VaultRead {
        vault: Uuid,
        document: Uuid,
        revision: String,
    },
    /// Authorized by the hub (host allowlist, turn fence, receipt); performed by the agent's
    /// host process, never inside a database transaction.
    WebFetch { url: String },
    /// Same shape as `WebFetch`: authorized by the hub (a separate `web_post_hosts` allowlist),
    /// performed by the agent host. `body` may contain the literal string "{{SECRET}}" in a
    /// string value, substituted on the agent host from a per-(agent,host) stored secret --
    /// the model never sees the real value, only ever writes the placeholder.
    WebPost {
        url: String,
        body: serde_json::Value,
    },
    /// Chat-tool counterpart to `hive hub handoff create`. Authorized by
    /// `bots_agent_handoff_create` against the caller's `handoff_targets` allowlist -- routed
    /// there directly by `library_tools.rs`, never through `bots_agent_tool_execute` (see that
    /// method's own rejection of this variant, matching how it already refuses `WebFetch`/
    /// `WebPost`: those need the agent host to perform the request, this needs the handoff's own
    /// wake-message send, neither fits the vault-shaped read this method executes).
    HandoffCreate {
        target: AgentId,
        task_or_question: String,
        acceptance_criteria: String,
        #[serde(default = "default_handoff_deadline_minutes")]
        deadline_minutes: i64,
    },
    /// Chat-tool counterpart to `hive hub handoff resolve`. Authorized by
    /// `bots_agent_handoff_resolve`, which checks the caller is the handoff's own `target_agent`.
    HandoffResolve {
        handoff_id: HandoffId,
        state: HandoffState,
        summary: String,
    },
}
/// A day: long enough that a coding sub-task doesn't need the model to reason about its own
/// deadline on every call, short enough that a stuck handoff surfaces the same day, not the
/// next sprint.
fn default_handoff_deadline_minutes() -> i64 {
    1440
}
/// Supplied by the delivery executor, never by model-generated arguments.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolTurn {
    pub message: Uuid,
    pub conversation: Uuid,
    pub generation: u64,
    pub conversation_revision: u32,
}
fn turn_check(tx: &Transaction<'_>, agent: Uuid, turn: &AgentToolTurn) -> Result<()> {
    let generation =
        i64::try_from(turn.generation).map_err(|_| rejected("invalid delivery generation"))?;
    // `generation` above is u64 and genuinely can overflow i64, so it stays fallible. A u32
    // revision cannot, and clippy's `unnecessary_fallible_conversions` only fires when the `hive`
    // crate is linted on its own -- different feature unification than the workspace lint, which
    // is exactly the crate-scoped failure `ab011a95` was filed for.
    let revision = i64::from(turn.conversation_revision);
    let actions: Option<String> = tx.query_row(
        "SELECT cm.allowed_actions FROM agent_deliveries d JOIN messages m ON m.id=d.message_id JOIN conversations c ON c.id=m.conversation_id JOIN conversation_members cm ON cm.conversation_id=c.id AND cm.principal_kind='agent' AND cm.principal_id=d.recipient WHERE d.message_id=?1 AND d.recipient=?2 AND c.id=?3 AND d.status='running' AND d.lease_generation=?4 AND d.lease_deadline>?5 AND c.policy_revision=?6 AND m.created_at>=cm.history_boundary",
        params![turn.message.to_string(),agent.to_string(),turn.conversation.to_string(),generation,now(),revision], |r|r.get(0)).optional().map_err(db_error)?;
    let actions: Vec<crate::bots::MemberAction> =
        decode(&actions.ok_or_else(|| rejected("chat attempt is no longer active"))?)?;
    if !actions.contains(&crate::bots::MemberAction::Read) {
        return Err(rejected("conversation read access revoked"));
    }
    let used: i64 = tx.query_row("SELECT count(*) FROM bots_agent_tool_turns WHERE message=?1 AND agent=?2 AND generation=?3",params![turn.message.to_string(),agent.to_string(),generation],|r|r.get(0)).map_err(db_error)?;
    if used >= 8 {
        return Err(rejected("chat library tool limit reached"));
    }
    Ok(())
}
fn owner_check(tx: &Transaction<'_>, node: &str, agent: Uuid) -> Result<()> {
    let allowed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles a JOIN nodes n ON n.owner_member_id=a.owner WHERE a.id=?1 AND n.id=?2 AND a.archived=0)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
    if !allowed {
        return Err(rejected("forbidden: active agent owner"));
    }
    Ok(())
}
fn policy(tx: &Transaction<'_>, agent: Uuid) -> Result<AgentToolPolicy> {
    let saved: Option<String> = tx
        .query_row(
            "SELECT policy FROM bots_agent_tool_policies WHERE agent=?1",
            [agent.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    saved
        .map(|s| decode(&s))
        .transpose()
        .map(|p| p.unwrap_or_default())
}
impl LocalHub {
    pub fn bots_agent_tool_settings(&self, agent: Uuid) -> Result<serde_json::Value> {
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            let current=policy(tx,agent)?;
            let mut q=tx.prepare("SELECT v.id,v.name,v.state,EXISTS(SELECT 1 FROM vault_readers host JOIN agent_profiles a ON a.preferred_host=host.node_id WHERE a.id=?1 AND host.vault_id=v.id) FROM vaults v JOIN vault_readers reader ON reader.vault_id=v.id WHERE reader.node_id=?2 ORDER BY v.name,v.id LIMIT 1000").map_err(db_error)?;
            let libraries=q.query_map(params![agent.to_string(),node],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"host_access":r.get::<_,bool>(3)?}))).map_err(db_error)?.collect::<std::result::Result<Vec<_>,_>>().map_err(db_error)?;
            Ok(serde_json::json!({"policy":current,"libraries":libraries}))
        })
    }

    pub fn bots_agent_tool_policy_get(&self, agent: Uuid) -> Result<AgentToolPolicy> {
        self.with_node(|tx, node| {
            owner_check(tx, node, agent)?;
            policy(tx, agent)
        })
    }
    pub fn bots_agent_tool_policy_set(
        &self,
        agent: Uuid,
        mut next: AgentToolPolicy,
    ) -> Result<AgentToolPolicy> {
        if next.readable_vaults.len() > 32 {
            return Err(rejected("select at most 32 libraries"));
        }
        if next.template.as_deref().is_some_and(|s| {
            !matches!(
                s,
                "assistant-v1"
                    | "researcher-v1"
                    | "librarian-v1"
                    | "developer-v1"
                    | "reviewer-v1"
                    | "integrator-v1"
                    | "coordinator-v1"
                    | "tester-v1"
                    | "designer-v1"
                    | "fleet-operator-v1"
            )
        }) {
            return Err(rejected("unknown agent template"));
        }
        next.readable_vaults.sort();
        next.readable_vaults.dedup();
        if let Some(hosts) = next.web_hosts.as_mut() {
            if hosts.len() > 32 {
                return Err(rejected("select at most 32 web hosts"));
            }
            for h in hosts.iter_mut() {
                *h = h.trim().to_ascii_lowercase();
                if !valid_web_host(h) {
                    return Err(rejected(
                        "web hosts must be bare lowercase host names like example.org",
                    ));
                }
            }
            hosts.sort();
            hosts.dedup();
        }
        if let Some(hosts) = next.web_post_hosts.as_mut() {
            if hosts.len() > 32 {
                return Err(rejected("select at most 32 web post hosts"));
            }
            for h in hosts.iter_mut() {
                *h = h.trim().to_ascii_lowercase();
                if !valid_web_host(h) {
                    return Err(rejected(
                        "web post hosts must be bare lowercase host names like example.org",
                    ));
                }
            }
            hosts.sort();
            hosts.dedup();
        }
        if let Some(targets) = next.handoff_targets.as_mut() {
            if targets.len() > 32 {
                return Err(rejected("select at most 32 handoff targets"));
            }
            if targets.contains(&agent) {
                return Err(rejected("an agent cannot be its own handoff target"));
            }
            targets.sort();
            targets.dedup();
        }
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            let previous=policy(tx,agent)?;
            if next.revision!=previous.revision {return Err(rejected("Tool access changed. Reload before saving."));}
            if next.web_hosts.is_none() { next.web_hosts = Some(previous.web_hosts().to_vec()); }
            if next.web_post_hosts.is_none() { next.web_post_hosts = Some(previous.web_post_hosts().to_vec()); }
            if next.handoff_targets.is_none() { next.handoff_targets = Some(previous.handoff_targets().to_vec()); }
            for vault in &next.readable_vaults {
                let exists: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM vaults WHERE id=?1)",[vault.to_string()],|r|r.get(0)).map_err(db_error)?;
                if !exists {return Err(rejected("library not found"));}
            }
            if let Some(targets) = &next.handoff_targets {
                for target in targets {
                    let exists: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND archived=0)",[target.to_string()],|r|r.get(0)).map_err(db_error)?;
                    if !exists {return Err(rejected("handoff target agent not found"));}
                }
            }
            next.revision=next.revision.checked_add(1).ok_or_else(||rejected("policy revision exhausted"))?;
            tx.execute("INSERT INTO bots_agent_tool_policies(agent,policy) VALUES(?1,?2) ON CONFLICT(agent) DO UPDATE SET policy=excluded.policy",params![agent.to_string(),encode(&next)?]).map_err(db_error)?;
            tx.execute("UPDATE agent_profiles SET role_revision=role_revision+1 WHERE id=?1",[agent.to_string()]).map_err(db_error)?;
            Ok(next.clone())
        })
    }
    /// Same fences as `bots_agent_tool_execute` (owner, assigned host, live turn, policy revision)
    /// plus the host allowlist; records the receipt and turn, then hands the agent host a grant to
    /// perform the request itself. The hub never opens the connection.
    pub fn bots_agent_web_authorize(
        &self,
        agent: Uuid,
        expected_revision: u32,
        turn: &AgentToolTurn,
        url: &str,
    ) -> Result<WebFetchGrant> {
        check_text(url, 2048)?;
        let parsed = url::Url::parse(url).map_err(|_| rejected("invalid url"))?;
        let host = parsed
            .host_str()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| rejected("url has no host"))?;
        let loopback = matches!(host.as_str(), "127.0.0.1" | "localhost");
        if !(parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback)) {
            return Err(rejected("only https urls can be fetched"));
        }
        if parsed.username() != "" || parsed.password().is_some() {
            return Err(rejected("urls with credentials cannot be fetched"));
        }
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            let hosted: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND runtime_kind='local' AND preferred_host=?2)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
            if !hosted {return Err(rejected("tool calls must come from the assigned agent host"));}
            turn_check(tx,agent,turn)?;
            let current=policy(tx,agent)?;
            if current.revision!=expected_revision {return Err(rejected("tool policy changed; reload access"));}
            let effective = if loopback { "localhost" } else { host.as_str() };
            if !host_allowed(current.web_hosts(), effective) {return Err(rejected("agent does not have access to this web host"));}
            let receipt=Uuid::new_v4();
            tx.execute("INSERT INTO bots_agent_tool_receipts(id,agent,node,tool,vault,policy_revision,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt.to_string(),agent.to_string(),node,"web_fetch",host,current.revision,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_tool_turns(receipt,message,conversation,agent,generation) VALUES(?1,?2,?3,?4,?5)",params![receipt.to_string(),turn.message.to_string(),turn.conversation.to_string(),agent.to_string(),turn.generation as i64]).map_err(db_error)?;
            Ok(WebFetchGrant { receipt, url: url.to_string(), host })
        })
    }
    /// Same fences as `bots_agent_web_authorize`, checked against the separate `web_post_hosts`
    /// allowlist -- a fetch grant never implies a post grant. `body` is authorized as given (it
    /// is recorded on the grant and in the receipt) and may contain the literal placeholder
    /// string "{{SECRET}}" in place of a credential; the agent host substitutes that from
    /// `bots_agent_secret_get` right before sending, so the value this call returns (and
    /// anything logged from it) never carries the real secret.
    pub fn bots_agent_web_post_authorize(
        &self,
        agent: Uuid,
        expected_revision: u32,
        turn: &AgentToolTurn,
        url: &str,
        body: serde_json::Value,
    ) -> Result<WebPostGrant> {
        check_text(url, 2048)?;
        if serde_json::to_vec(&body)
            .map(|v| v.len())
            .unwrap_or(usize::MAX)
            > 65536
        {
            return Err(rejected("post body too large"));
        }
        let parsed = url::Url::parse(url).map_err(|_| rejected("invalid url"))?;
        let host = parsed
            .host_str()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| rejected("url has no host"))?;
        let loopback = matches!(host.as_str(), "127.0.0.1" | "localhost");
        if !(parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback)) {
            return Err(rejected("only https urls can be posted to"));
        }
        if parsed.username() != "" || parsed.password().is_some() {
            return Err(rejected("urls with credentials cannot be posted to"));
        }
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            let hosted: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND runtime_kind='local' AND preferred_host=?2)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
            if !hosted {return Err(rejected("tool calls must come from the assigned agent host"));}
            turn_check(tx,agent,turn)?;
            let current=policy(tx,agent)?;
            if current.revision!=expected_revision {return Err(rejected("tool policy changed; reload access"));}
            let effective = if loopback { "localhost" } else { host.as_str() };
            if !host_allowed(current.web_post_hosts(), effective) {return Err(rejected("agent does not have write access to this web host"));}
            // Resolve "{{SECRET}}" here, inside the hub's own trust boundary, so the grant that
            // crosses to the agent host already carries the real value -- the model that
            // composed `body` only ever wrote the placeholder, and never sees this substitution.
            let secret: Option<String> = tx.query_row("SELECT secret FROM agent_tool_secrets WHERE agent=?1 AND host=?2",params![agent.to_string(),host],|r|r.get(0)).optional().map_err(db_error)?;
            let resolved = substitute_secret(body, secret.as_deref());
            let receipt=Uuid::new_v4();
            tx.execute("INSERT INTO bots_agent_tool_receipts(id,agent,node,tool,vault,policy_revision,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt.to_string(),agent.to_string(),node,"web_post_json",host,current.revision,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_tool_turns(receipt,message,conversation,agent,generation) VALUES(?1,?2,?3,?4,?5)",params![receipt.to_string(),turn.message.to_string(),turn.conversation.to_string(),agent.to_string(),turn.generation as i64]).map_err(db_error)?;
            Ok(WebPostGrant { receipt, url: url.to_string(), host, body: resolved })
        })
    }
    /// Lets a locally-hosted agent hand work to another of its owner's agents itself, mid-chat --
    /// the chat-tool counterpart to `hive hub handoff create` (agent_tools.rs top doc: "no prompt
    /// or opaque policy reference grants access"). Same fences as `bots_agent_web_authorize`
    /// (owner, assigned local host, live turn, policy revision) plus the explicit
    /// `handoff_targets` allowlist -- an agent can only hand off to a target its owner has
    /// actually granted, mirroring how `web_hosts` scopes fetches. Authorization and the create
    /// are two separate steps (the policy/turn checks need the node's own connection inside
    /// `with_node`, the wake message needs `bots_message_send`'s own transactions in `bots.rs`),
    /// so this is a thin wrapper: authorize here, then delegate to `bots_handoff_create_and_wake`.
    #[allow(clippy::too_many_arguments)]
    pub fn bots_agent_handoff_create(
        &self,
        agent: Uuid,
        expected_revision: u32,
        turn: &AgentToolTurn,
        target: Uuid,
        task_or_question: String,
        acceptance_criteria: String,
        deadline_minutes: i64,
    ) -> Result<Handoff> {
        check_text(&task_or_question, 4000)?;
        check_text(&acceptance_criteria, 4000)?;
        if !(1..=10_080).contains(&deadline_minutes) {
            return Err(rejected(
                "deadline must be between 1 minute and 7 days (10080 minutes) out",
            ));
        }
        let owner = self.with_node(|tx, node| {
            owner_check(tx, node, agent)?;
            let hosted: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND runtime_kind='local' AND preferred_host=?2)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
            if !hosted {return Err(rejected("tool calls must come from the assigned agent host"));}
            turn_check(tx, agent, turn)?;
            let current = policy(tx, agent)?;
            if current.revision != expected_revision {return Err(rejected("tool policy changed; reload access"));}
            if !current.handoff_targets().contains(&target) {return Err(rejected("agent is not allowed to hand work to this target"));}
            let target_exists: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND archived=0)",[target.to_string()],|r|r.get(0)).map_err(db_error)?;
            if !target_exists {return Err(rejected("handoff target agent not found"));}
            let owner: String = tx.query_row("SELECT owner FROM agent_profiles WHERE id=?1",[agent.to_string()],|r|r.get(0)).map_err(db_error)?;
            let receipt=Uuid::new_v4();
            tx.execute("INSERT INTO bots_agent_tool_receipts(id,agent,node,tool,vault,policy_revision,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt.to_string(),agent.to_string(),node,"handoff_create",target.to_string(),current.revision,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_tool_turns(receipt,message,conversation,agent,generation) VALUES(?1,?2,?3,?4,?5)",params![receipt.to_string(),turn.message.to_string(),turn.conversation.to_string(),agent.to_string(),turn.generation as i64]).map_err(db_error)?;
            Uuid::parse_str(&owner).map_err(|_| rejected("invalid owner"))
        })?;
        self.store.bots_handoff_create_and_wake(
            owner,
            NewHandoff {
                source_agent: agent,
                target_agent: target,
                project_id: None,
                task_or_question,
                acceptance_criteria,
                artifact_refs: vec![],
                allowed_tools: vec![],
                parent_run: None,
                reply_to_thread: Some(turn.message),
                budgets: None,
                deadline: chrono::Utc::now() + chrono::Duration::minutes(deadline_minutes),
            },
        )
    }
    /// Lets the target of a handoff resolve it itself, mid-chat -- the chat-tool counterpart to
    /// `hive hub handoff resolve`. Same fences as `bots_agent_handoff_create`, but the grant
    /// check is "is this agent the handoff's own target" rather than an allowlist: resolving is
    /// answering a task that was already, explicitly, addressed to this agent, not reaching for
    /// a new one.
    pub fn bots_agent_handoff_resolve(
        &self,
        agent: Uuid,
        expected_revision: u32,
        turn: &AgentToolTurn,
        handoff_id: HandoffId,
        state: HandoffState,
        summary: String,
    ) -> Result<Handoff> {
        if !state.is_terminal() {
            return Err(rejected("a handoff must resolve to a terminal state"));
        }
        check_text(&summary, 20_000)?;
        let owner = self.with_node(|tx, node| {
            owner_check(tx, node, agent)?;
            let hosted: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND runtime_kind='local' AND preferred_host=?2)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
            if !hosted {return Err(rejected("tool calls must come from the assigned agent host"));}
            turn_check(tx, agent, turn)?;
            let current = policy(tx, agent)?;
            if current.revision != expected_revision {return Err(rejected("tool policy changed; reload access"));}
            let target: Option<String> = tx.query_row("SELECT target_agent FROM handoffs WHERE id=?1",[handoff_id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            let target = target.ok_or_else(|| rejected("handoff not found"))?;
            if target != agent.to_string() {return Err(rejected("only the handoff's target agent may resolve it"));}
            let owner: String = tx.query_row("SELECT owner FROM agent_profiles WHERE id=?1",[agent.to_string()],|r|r.get(0)).map_err(db_error)?;
            let receipt=Uuid::new_v4();
            tx.execute("INSERT INTO bots_agent_tool_receipts(id,agent,node,tool,vault,policy_revision,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt.to_string(),agent.to_string(),node,"handoff_resolve",handoff_id.to_string(),current.revision,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_tool_turns(receipt,message,conversation,agent,generation) VALUES(?1,?2,?3,?4,?5)",params![receipt.to_string(),turn.message.to_string(),turn.conversation.to_string(),agent.to_string(),turn.generation as i64]).map_err(db_error)?;
            Uuid::parse_str(&owner).map_err(|_| rejected("invalid owner"))
        })?;
        self.store
            .bots_handoff_resolve(owner, handoff_id, state, summary, vec![])
    }
    /// Owner-only: store or replace the secret substituted for "{{SECRET}}" in a `web_post_json`
    /// body sent to `host` on `agent`'s behalf. Never returned by any agent-facing read (see
    /// `bots_agent_tool_settings`, which surfaces policy but not this table).
    pub fn bots_agent_secret_set(&self, agent: Uuid, host: &str, secret: &str) -> Result<()> {
        check_text(host, 253)?;
        check_text(secret, 4096)?;
        let host = host.trim().to_ascii_lowercase();
        if !valid_web_host(&host) {
            return Err(rejected(
                "host must be a bare lowercase host name like example.org",
            ));
        }
        self.with_node(|tx, node| {
            owner_check(tx, node, agent)?;
            let t = now();
            tx.execute("INSERT INTO agent_tool_secrets(agent,host,secret,created_at,updated_at) VALUES(?1,?2,?3,?4,?4) ON CONFLICT(agent,host) DO UPDATE SET secret=excluded.secret,updated_at=excluded.updated_at",params![agent.to_string(),host,secret,t]).map_err(db_error)?;
            Ok(())
        })
    }
    /// Executes on the authority, with the caller authenticated as the agent's assigned host.
    /// Policy, node/library grants, document revision and the read share one transaction.
    pub fn bots_agent_tool_execute(
        &self,
        agent: Uuid,
        expected_revision: u32,
        turn: &AgentToolTurn,
        call: AgentToolCall,
    ) -> Result<serde_json::Value> {
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            let host: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND runtime_kind='local' AND preferred_host=?2)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
            if !host {return Err(rejected("tool calls must come from the assigned agent host"));}
            turn_check(tx,agent,turn)?;
            let current=policy(tx,agent)?;
            if current.revision!=expected_revision {return Err(rejected("tool policy changed; reload access"));}
            let vault=match &call {AgentToolCall::VaultSearch{vault,..}|AgentToolCall::VaultRead{vault,..}=>*vault, AgentToolCall::WebFetch{..}=>return Err(rejected("web fetches are authorized with bots_agent_web_authorize and performed on the agent host")), AgentToolCall::WebPost{..}=>return Err(rejected("web posts are authorized with bots_agent_web_post_authorize and performed on the agent host")), AgentToolCall::HandoffCreate{..}=>return Err(rejected("handoffs are created with bots_agent_handoff_create")), AgentToolCall::HandoffResolve{..}=>return Err(rejected("handoffs are resolved with bots_agent_handoff_resolve"))};
            if !current.readable_vaults.contains(&vault) {return Err(rejected("agent does not have access to this library"));}
            self.vault_access(tx,node,vault,true)?;
            let (tool,result)=match &call {
                AgentToolCall::VaultSearch{query,limit,..}=>{
                    check_text(query,2048)?;
                    if !(1..=20).contains(limit) {return Err(rejected("agent search limit must be 1..20"));}
                    let terms=query.split_whitespace().map(|t|format!("\"{}\"",t.replace('"',"\"\""))).collect::<Vec<_>>().join(" AND ");
                    let mut q=tx.prepare("SELECT d.id,d.path,d.revision,d.title,snippet(vault_fts,1,'','', ' … ',32) FROM vault_fts JOIN vault_documents d ON d.rowid=vault_fts.rowid WHERE vault_fts MATCH ?1 AND d.vault_id=?2 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=d.id AND a.vault_id=d.vault_id) ORDER BY bm25(vault_fts),d.id LIMIT ?3").map_err(db_error)?;
                    let rows=q.query_map(params![terms,vault.to_string(),limit],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"path":r.get::<_,String>(1)?,"revision":r.get::<_,String>(2)?,"title":r.get::<_,String>(3)?,"snippet":r.get::<_,String>(4)?}))).map_err(db_error)?;
                    let hits=rows.collect::<std::result::Result<Vec<_>,_>>().map_err(db_error)?;
                    ("vault_search",serde_json::json!({"hits":hits}))
                },
                AgentToolCall::VaultRead{document,revision,..}=>{
                    check_text(revision,256)?;
                    let row: Option<(String,String,String,String)>=tx.query_row("SELECT path,revision,title,content FROM vault_documents WHERE id=?1 AND vault_id=?2 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=vault_documents.id AND a.vault_id=vault_documents.vault_id)",params![document.to_string(),vault.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
                    let(path,actual,title,mut content)=row.ok_or_else(||rejected("document not found"))?;
                    if &actual!=revision {return Err(rejected("document revision changed; search again"));}
                    let truncated=content.len()>32768;
                    if truncated {let mut n=32768;while !content.is_char_boundary(n){n-=1;}content.truncate(n);}
                    ("vault_read",serde_json::json!({"id":document,"vault":vault,"path":path,"revision":actual,"title":title,"content":content,"truncated":truncated}))
                }
                AgentToolCall::WebFetch{..}|AgentToolCall::WebPost{..}|AgentToolCall::HandoffCreate{..}|AgentToolCall::HandoffResolve{..}=>unreachable!("handled above"),
            };
            let receipt=Uuid::new_v4();
            tx.execute("INSERT INTO bots_agent_tool_receipts(id,agent,node,tool,vault,policy_revision,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt.to_string(),agent.to_string(),node,tool,vault.to_string(),current.revision,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_tool_turns(receipt,message,conversation,agent,generation) VALUES(?1,?2,?3,?4,?5)",params![receipt.to_string(),turn.message.to_string(),turn.conversation.to_string(),agent.to_string(),turn.generation as i64]).map_err(db_error)?;
            Ok(serde_json::json!({"receipt":receipt,"result":result}))
        })
    }
}

/// CLI-only local configuration path, mirroring `schedules.rs::resolve_owner`'s reasoning: the
/// hub-serving machine's own HOME never has a paired Hive-account node key, so a CLI admin
/// action meant to run *there* (configuring what a locally-hosted agent may do) resolves
/// ownership from the vault's own confirmed pairing instead of going through `with_node`'s
/// node-key authentication. This is deliberately separate from `bots_agent_web_post_authorize`,
/// which legitimately does need that authentication -- it is answering "did this request really
/// come from the agent's assigned host", not "is the caller running on the vault's own machine".
impl LocalHubStore {
    fn owned_agent(tx: &Transaction<'_>, agent: Uuid) -> Result<()> {
        let ok: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1)",
                [agent.to_string()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !ok {
            return Err(rejected("agent not found"));
        }
        Ok(())
    }
    /// Merges `hosts` into the agent's `web_post_hosts` allowlist (union, not replace -- run it
    /// again with a new host and the old ones stay granted).
    pub fn bots_agent_web_post_hosts_grant_local(
        &self,
        agent: Uuid,
        hosts: Vec<String>,
    ) -> Result<Vec<String>> {
        let mut hosts = hosts;
        for h in hosts.iter_mut() {
            *h = h.trim().to_ascii_lowercase();
            if !valid_web_host(h) {
                return Err(rejected(
                    "web post hosts must be bare lowercase host names like example.org",
                ));
            }
        }
        self.transaction(|tx| {
            Self::owned_agent(tx, agent)?;
            let mut current = policy(tx, agent)?;
            let mut merged = current.web_post_hosts().to_vec();
            merged.extend(hosts);
            merged.sort();
            merged.dedup();
            if merged.len() > 32 {
                return Err(rejected("select at most 32 web post hosts"));
            }
            current.web_post_hosts = Some(merged.clone());
            current.revision = current.revision.checked_add(1).ok_or_else(|| rejected("policy revision exhausted"))?;
            tx.execute("INSERT INTO bots_agent_tool_policies(agent,policy) VALUES(?1,?2) ON CONFLICT(agent) DO UPDATE SET policy=excluded.policy",params![agent.to_string(),encode(&current)?]).map_err(db_error)?;
            tx.execute("UPDATE agent_profiles SET role_revision=role_revision+1 WHERE id=?1",[agent.to_string()]).map_err(db_error)?;
            Ok(merged)
        })
    }
    /// Store or replace the secret substituted for "{{SECRET}}" in a `web_post_json` body sent
    /// to `host` on `agent`'s behalf.
    pub fn bots_agent_secret_set_local(&self, agent: Uuid, host: &str, secret: &str) -> Result<()> {
        check_text(host, 253)?;
        check_text(secret, 4096)?;
        let host = host.trim().to_ascii_lowercase();
        if !valid_web_host(&host) {
            return Err(rejected(
                "host must be a bare lowercase host name like example.org",
            ));
        }
        self.transaction(|tx| {
            Self::owned_agent(tx, agent)?;
            let t = now();
            tx.execute("INSERT INTO agent_tool_secrets(agent,host,secret,created_at,updated_at) VALUES(?1,?2,?3,?4,?4) ON CONFLICT(agent,host) DO UPDATE SET secret=excluded.secret,updated_at=excluded.updated_at",params![agent.to_string(),host,secret,t]).map_err(db_error)?;
            Ok(())
        })
    }
}

#[cfg(test)]
impl LocalHubStore {
    pub(crate) fn bots_agent_tool_receipt_count(&self, tool: &str, resource: Option<&str>) -> i64 {
        self.transaction(|tx| {
            tx.query_row(
                "SELECT count(*) FROM bots_agent_tool_receipts WHERE tool=?1 AND (?2 IS NULL OR vault=?2)",
                params![tool, resource],
                |r| r.get(0),
            )
            .map_err(db_error)
        })
        .unwrap()
    }
}
#[cfg(test)]
pub(crate) fn test_turn(store: &LocalHubStore, agent: &crate::bots::AgentProfile) -> AgentToolTurn {
    use crate::bots::*;
    let room = store
        .bots_conversations_create(NewConversation {
            title: None,
            owner: agent.owner,
            kind: ConversationKind::AgentDm,
            project_id: None,
            coordinator: Some(agent.id),
            storage_scope: StorageScope::LocalOnly,
        })
        .unwrap();
    store
        .bots_conversations_join(Principal::Agent(agent.id), room.id)
        .unwrap();
    store
        .bots_conversations_join(Principal::User(agent.owner), room.id)
        .unwrap();
    let message = store
        .bots_message_send(
            Principal::User(agent.owner),
            room.id,
            Uuid::new_v4().to_string(),
            room.policy_revision,
            vec![agent.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("Research".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .unwrap();
    let delivery = store
        .bots_delivery_claim(DeliveryKey {
            message_id: message.id,
            recipient: agent.id,
        })
        .unwrap();
    AgentToolTurn {
        message: message.id,
        conversation: room.id,
        generation: delivery.lease_generation,
        conversation_revision: room.policy_revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::{AgentRuntimeKind, NewAgentProfile};
    #[test]
    fn team_roles_persist_without_implicit_library_grants() {
        let store = LocalHubStore::in_memory().unwrap();
        let credentials = store.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(credentials.node_id, owner).unwrap();
        let hub = store.connect(&credentials.raw_key).unwrap();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Team member".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(credentials.node_id),
                capability_policy_ref: "no-tools".into(),
                provider_account_ref: None,
                memory_namespace: "team-role-test".into(),
            })
            .unwrap();
        let mut policy = AgentToolPolicy::default();
        for role in [
            "coordinator-v1",
            "librarian-v1",
            "researcher-v1",
            "developer-v1",
            "reviewer-v1",
            "tester-v1",
            "designer-v1",
            "integrator-v1",
            "fleet-operator-v1",
        ] {
            policy.template = Some(role.into());
            policy = hub.bots_agent_tool_policy_set(agent.id, policy).unwrap();
            let saved = hub.bots_agent_tool_policy_get(agent.id).unwrap();
            assert_eq!(saved.template.as_deref(), Some(role));
            assert!(saved.readable_vaults.is_empty());
        }
        policy.template = Some("unknown-role".into());
        assert!(hub.bots_agent_tool_policy_set(agent.id, policy).is_err());
    }

    #[test]
    fn web_host_grammar_and_matching() {
        for ok in [
            "lokislab.org",
            "ollama.com",
            "raw.githubusercontent.com",
            "a-b.example.co.uk",
            "localhost",
        ] {
            assert!(valid_web_host(ok), "{ok}");
        }
        for bad in [
            "",
            "example",
            "Example.org",
            "https://x.org",
            "x.org/path",
            "x.org:443",
            "*.x.org",
            ".x.org",
            "x..org",
            "127.0.0.1",
            "x.org.",
        ] {
            assert!(!valid_web_host(bad), "{bad}");
        }
        let allow = vec!["lokislab.org".to_string(), "ollama.com".to_string()];
        assert!(host_allowed(&allow, "lokislab.org"));
        assert!(host_allowed(&allow, "www.lokislab.org"));
        assert!(!host_allowed(&allow, "notlokislab.org"));
        assert!(!host_allowed(&allow, "lokislab.org.evil.net"));
        assert!(!host_allowed(&allow, "ollama.co"));
    }

    #[test]
    fn web_authorize_enforces_allowlist_scheme_host_and_turn_and_keeps_hosts_when_omitted() {
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        s.set_node_owner(c.node_id, owner).unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let agent = s
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(c.node_id),
                capability_policy_ref: "all-tools".into(),
                provider_account_ref: None,
                memory_namespace: "test".into(),
            })
            .unwrap();
        let turn = test_turn(&s, &agent);
        // No hosts yet: everything is refused, including loopback.
        assert!(h
            .bots_agent_web_authorize(agent.id, 0, &turn, "https://lokislab.org/")
            .is_err());
        // Bad grammar is rejected at save time; case and whitespace are normalized.
        assert!(h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    web_hosts: Some(vec!["https://x.org".into()]),
                    ..Default::default()
                }
            )
            .is_err());
        let p = h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    web_hosts: Some(vec![
                        " LokisLab.org ".into(),
                        "ollama.com".into(),
                        "ollama.com".into(),
                    ]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p.web_hosts(), ["lokislab.org", "ollama.com"]);
        assert_eq!(p.revision, 1);
        // Allowed host and subdomain; receipt is written with the host in the resource column.
        let g = h
            .bots_agent_web_authorize(agent.id, 1, &turn, "https://www.lokislab.org/articles/x")
            .unwrap();
        assert_eq!(g.host, "www.lokislab.org");
        let receipts = s.bots_agent_tool_receipt_count("web_fetch", Some("www.lokislab.org"));
        assert_eq!(receipts, 1);
        // Wrong host, wrong scheme, credentials, stale revision, lookalike host.
        assert!(h
            .bots_agent_web_authorize(agent.id, 1, &turn, "https://example.org/")
            .is_err());
        assert!(h
            .bots_agent_web_authorize(agent.id, 1, &turn, "http://lokislab.org/")
            .is_err());
        assert!(h
            .bots_agent_web_authorize(agent.id, 1, &turn, "https://user:pw@lokislab.org/")
            .is_err());
        assert!(h
            .bots_agent_web_authorize(agent.id, 0, &turn, "https://lokislab.org/")
            .is_err());
        assert!(h
            .bots_agent_web_authorize(agent.id, 1, &turn, "https://lokislab.org.evil.net/")
            .is_err());
        // A client that omits web_hosts (older UI) keeps the saved list; explicit empty clears it.
        let kept = h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: 1,
                    template: Some("researcher-v1".into()),
                    web_hosts: None,
                    web_post_hosts: None,
                    handoff_targets: None,
                    readable_vaults: vec![],
                },
            )
            .unwrap();
        assert_eq!(kept.web_hosts(), ["lokislab.org", "ollama.com"]);
        let cleared = h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: 2,
                    web_hosts: Some(vec![]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(cleared.web_hosts().is_empty());
        assert!(h
            .bots_agent_web_authorize(agent.id, 3, &turn, "https://lokislab.org/")
            .is_err());
        // Web fetches count against the same 8-call turn limit as library reads.
        h.bots_agent_tool_policy_set(
            agent.id,
            AgentToolPolicy {
                revision: 3,
                web_hosts: Some(vec!["lokislab.org".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..7 {
            h.bots_agent_web_authorize(agent.id, 4, &turn, "https://lokislab.org/")
                .unwrap();
        }
        assert!(h
            .bots_agent_web_authorize(agent.id, 4, &turn, "https://lokislab.org/")
            .is_err());
    }

    #[test]
    fn secret_substitution_is_exact_match_only_and_recurses() {
        let body = serde_json::json!({
            "token": "{{SECRET}}",
            "nested": {"a": "{{SECRET}}", "b": "keep me"},
            "list": ["{{SECRET}}", "not a secret: {{SECRET}}"],
        });
        let resolved = substitute_secret(body.clone(), Some("s3cr3t"));
        assert_eq!(resolved["token"], "s3cr3t");
        assert_eq!(resolved["nested"]["a"], "s3cr3t");
        assert_eq!(resolved["nested"]["b"], "keep me");
        assert_eq!(resolved["list"][0], "s3cr3t");
        // Not an exact match -- left as literal text, never partially interpolated.
        assert_eq!(resolved["list"][1], "not a secret: {{SECRET}}");
        // No secret configured: placeholder passes through untouched.
        let untouched = substitute_secret(body, None);
        assert_eq!(untouched["token"], "{{SECRET}}");
    }

    #[test]
    fn web_post_authorize_uses_a_separate_allowlist_from_fetch_and_resolves_the_secret() {
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        s.set_node_owner(c.node_id, owner).unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let agent = s
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Scanner".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(c.node_id),
                capability_policy_ref: "all-tools".into(),
                provider_account_ref: None,
                memory_namespace: "test".into(),
            })
            .unwrap();
        let turn = test_turn(&s, &agent);
        let body = serde_json::json!({"token": "{{SECRET}}", "ok": true});
        // No web_post_hosts yet, even though this will be a fetch-allowed host below.
        assert!(h
            .bots_agent_web_post_authorize(
                agent.id,
                0,
                &turn,
                "https://script.google.com/x",
                body.clone()
            )
            .is_err());
        // Granting web_hosts (read) does NOT grant web_post_hosts (write).
        let p = h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    web_hosts: Some(vec!["script.google.com".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(h
            .bots_agent_web_post_authorize(
                agent.id,
                p.revision,
                &turn,
                "https://script.google.com/x",
                body.clone()
            )
            .is_err());
        let p = h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: p.revision,
                    web_post_hosts: Some(vec!["Script.Google.com".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(p.web_post_hosts(), ["script.google.com"]);
        // No secret configured yet: placeholder is authorized through as literal text.
        let g = h
            .bots_agent_web_post_authorize(
                agent.id,
                p.revision,
                &turn,
                "https://script.google.com/x",
                body.clone(),
            )
            .unwrap();
        assert_eq!(g.body["token"], "{{SECRET}}");
        // Configure the secret; a fresh authorize now resolves it into the grant's body.
        s.bots_agent_secret_set_local(agent.id, "script.google.com", "shh-its-a-secret")
            .unwrap();
        let g2 = h
            .bots_agent_web_post_authorize(
                agent.id,
                p.revision,
                &turn,
                "https://script.google.com/x",
                body,
            )
            .unwrap();
        assert_eq!(g2.body["token"], "shh-its-a-secret");
        assert_eq!(g2.body["ok"], true);
        let receipts = s.bots_agent_tool_receipt_count("web_post_json", Some("script.google.com"));
        assert_eq!(receipts, 2);
        // A wrong host is refused even though it's on the web_hosts (fetch) allowlist.
        assert!(h
            .bots_agent_web_post_authorize(
                agent.id,
                p.revision,
                &turn,
                "https://example.org/",
                serde_json::json!({})
            )
            .is_err());
    }

    #[test]
    fn handoff_targets_allowlist_gates_agent_initiated_handoff_create_and_resolve() {
        use crate::bots::{HandoffState, Principal};
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        s.set_node_owner(c.node_id, owner).unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let make = |name: &str| {
            s.bots_agents_create(NewAgentProfile {
                owner,
                name: name.into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(c.node_id),
                capability_policy_ref: "all-tools".into(),
                provider_account_ref: None,
                memory_namespace: format!("test-{name}"),
            })
            .unwrap()
        };
        let coordinator = make("Coordinator");
        let coder = make("Coder");
        let bystander = make("Bystander");
        let turn = test_turn(&s, &coordinator);

        // No handoff_targets granted yet: even a real agent is refused.
        assert!(h
            .bots_agent_handoff_create(
                coordinator.id,
                0,
                &turn,
                coder.id,
                "add a --start-at flag".into(),
                "cargo test passes".into(),
                60,
            )
            .is_err());

        let policy = h
            .bots_agent_tool_policy_set(
                coordinator.id,
                AgentToolPolicy {
                    handoff_targets: Some(vec![coder.id]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(policy.handoff_targets(), [coder.id]);

        // Bystander was never granted -- still refused even after coordinator has some grant.
        assert!(h
            .bots_agent_handoff_create(
                coordinator.id,
                1,
                &turn,
                bystander.id,
                "task".into(),
                "criteria".into(),
                60,
            )
            .is_err());

        // An agent cannot grant itself as its own target.
        assert!(h
            .bots_agent_tool_policy_set(
                coordinator.id,
                AgentToolPolicy {
                    revision: 1,
                    handoff_targets: Some(vec![coordinator.id]),
                    ..Default::default()
                },
            )
            .is_err());

        let handoff = h
            .bots_agent_handoff_create(
                coordinator.id,
                1,
                &turn,
                coder.id,
                "add a --start-at flag".into(),
                "cargo test passes".into(),
                60,
            )
            .unwrap();
        assert_eq!(handoff.state, HandoffState::Requested);
        assert_eq!(handoff.source_agent, coordinator.id);
        assert_eq!(handoff.target_agent, coder.id);

        // Only the handoff's own target may resolve it -- the coordinator itself is refused.
        let coordinator_turn = test_turn(&s, &coordinator);
        assert!(h
            .bots_agent_handoff_resolve(
                coordinator.id,
                1,
                &coordinator_turn,
                handoff.id,
                HandoffState::Completed,
                "done".into(),
            )
            .is_err());

        let coder_turn = test_turn(&s, &coder);
        let resolved = h
            .bots_agent_handoff_resolve(
                coder.id,
                0,
                &coder_turn,
                handoff.id,
                HandoffState::Completed,
                "shipped in PR #50".into(),
            )
            .unwrap();
        assert_eq!(resolved.state, HandoffState::Completed);
        assert_eq!(
            resolved.receipt.as_ref().unwrap().summary,
            "shipped in PR #50"
        );

        // Already resolved: a second resolve attempt is refused.
        assert!(h
            .bots_agent_handoff_resolve(
                coder.id,
                0,
                &coder_turn,
                handoff.id,
                HandoffState::Completed,
                "again".into(),
            )
            .is_err());

        // Confirmed via the same status read the CLI uses.
        let status = s
            .bots_handoff_status(Principal::User(owner), handoff.id)
            .unwrap();
        assert_eq!(status.state, HandoffState::Completed);
    }
    #[test]
    fn web_post_hosts_grant_local_merges_and_secret_set_local_requires_a_real_agent() {
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        s.set_node_owner(c.node_id, owner).unwrap();
        let agent = s
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Scanner".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(c.node_id),
                capability_policy_ref: "all-tools".into(),
                provider_account_ref: None,
                memory_namespace: "test".into(),
            })
            .unwrap();
        let granted = s
            .bots_agent_web_post_hosts_grant_local(agent.id, vec!["Script.Google.com".into()])
            .unwrap();
        assert_eq!(granted, ["script.google.com"]);
        let granted = s
            .bots_agent_web_post_hosts_grant_local(
                agent.id,
                vec!["lokislab.org".into(), "script.google.com".into()],
            )
            .unwrap();
        assert_eq!(granted, ["lokislab.org", "script.google.com"]);
        assert!(s
            .bots_agent_secret_set_local(agent.id, "script.google.com", "t")
            .is_ok());
        assert!(s
            .bots_agent_secret_set_local(Uuid::new_v4(), "script.google.com", "t")
            .is_err());
        assert!(s
            .bots_agent_web_post_hosts_grant_local(agent.id, vec!["not a host".into()])
            .is_err());
    }

    #[test]
    fn agent_tool_policy_enforces_scope_host_revision_and_revocation() {
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        s.set_node_owner(c.node_id, owner).unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let agent = s
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(c.node_id),
                capability_policy_ref: "all-tools".into(),
                provider_account_ref: None,
                memory_namespace: "test".into(),
            })
            .unwrap();
        let turn = test_turn(&s, &agent);
        let v = s.vault_create("Allowed").unwrap();
        let other = s.vault_create("Other").unwrap();
        let doc = Uuid::new_v4();
        let rev = s
            .vault_put(v, doc, "notes.md", "Notes", "alpha evidence")
            .unwrap();
        s.vault_set_available(v, true).unwrap();
        s.vault_grant(v, c.node_id, true).unwrap();
        s.vault_set_available(other, true).unwrap();
        s.vault_grant(other, c.node_id, true).unwrap();
        let read = || AgentToolCall::VaultRead {
            vault: v,
            document: doc,
            revision: rev.clone(),
        };
        assert!(h
            .bots_agent_tool_execute(agent.id, 0, &turn, read())
            .is_err());
        let p = h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: 0,
                    template: Some("researcher-v1".into()),
                    web_hosts: None,
                    web_post_hosts: None,
                    handoff_targets: None,
                    readable_vaults: vec![v],
                },
            )
            .unwrap();
        assert_eq!(p.revision, 1);
        assert!(h
            .bots_agent_tool_policy_set(agent.id, AgentToolPolicy::default())
            .is_err());
        let result = h
            .bots_agent_tool_execute(agent.id, 1, &turn, read())
            .unwrap();
        assert_eq!(result["result"]["content"], "alpha evidence");
        let hits = h
            .bots_agent_tool_execute(
                agent.id,
                1,
                &turn,
                AgentToolCall::VaultSearch {
                    vault: v,
                    query: "alpha".into(),
                    limit: 5,
                },
            )
            .unwrap();
        assert_eq!(hits["result"]["hits"].as_array().unwrap().len(), 1);
        assert!(h
            .bots_agent_tool_execute(
                agent.id,
                1,
                &turn,
                AgentToolCall::VaultSearch {
                    vault: other,
                    query: "alpha".into(),
                    limit: 5
                }
            )
            .is_err());
        assert!(h
            .bots_agent_tool_execute(agent.id, 0, &turn, read())
            .is_err());
        assert!(h
            .bots_agent_tool_execute(
                agent.id,
                1,
                &turn,
                AgentToolCall::VaultRead {
                    vault: v,
                    document: doc,
                    revision: "stale".into()
                }
            )
            .is_err());
        s.vault_grant(v, c.node_id, false).unwrap();
        assert!(h
            .bots_agent_tool_execute(agent.id, 1, &turn, read())
            .is_err());
        s.vault_grant(v, c.node_id, true).unwrap();
        let revoked = h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(h
            .bots_agent_tool_execute(agent.id, 1, &turn, read())
            .is_err());
        assert!(h
            .bots_agent_tool_execute(agent.id, revoked.revision, &turn, read())
            .is_err());
        s.transaction(|tx| {
            assert_eq!(
                tx.query_row("SELECT count(*) FROM bots_agent_tool_receipts", [], |r| r
                    .get::<_, u32>(
                    0
                ))
                .unwrap(),
                2
            );
            tx.execute(
                "UPDATE agent_profiles SET preferred_host=NULL WHERE id=?1",
                [agent.id.to_string()],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
        h.bots_agent_tool_policy_set(
            agent.id,
            AgentToolPolicy {
                revision: 2,
                readable_vaults: vec![v],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(h
            .bots_agent_tool_execute(agent.id, 3, &turn, read())
            .is_err());
        s.transaction(|tx| {
            tx.execute(
                "UPDATE agent_profiles SET owner=?2 WHERE id=?1",
                params![agent.id.to_string(), Uuid::new_v4().to_string()],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
        assert!(h.bots_agent_tool_policy_get(agent.id).is_err());
        assert!(h
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: 3,
                    ..Default::default()
                }
            )
            .is_err());
    }
    #[test]
    fn agent_tool_wire_rejects_extra_authority_and_unknown_tools() {
        for value in [
            serde_json::json!({"tool":"run_command","command":"echo unsafe"}),
            serde_json::json!({"tool":"vault_search","vault":Uuid::new_v4(),"query":"hello","limit":5,"owner":"forged"}),
        ] {
            assert!(serde_json::from_value::<AgentToolCall>(value).is_err());
        }
        assert!(serde_json::from_value::<AgentToolPolicy>(
            serde_json::json!({"revision":0,"template":null,"readable_vaults":[],"shell":true})
        )
        .is_err());
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    use crate::bots::{AgentRuntimeKind, NewAgentProfile};
    #[tokio::test]
    async fn agent_tool_policies_survive_reopen_and_enforce_remote_identity() {
        let path = std::env::temp_dir().join(format!("agent-tools-{}.sqlite", Uuid::new_v4()));
        let store = LocalHubStore::open(&path).unwrap();
        let creds = store.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(creds.node_id, owner).unwrap();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(creds.node_id),
                capability_policy_ref: "none".into(),
                provider_account_ref: None,
                memory_namespace: "test".into(),
            })
            .unwrap();
        let turn = test_turn(&store, &agent);
        let vault = store.vault_create("Library").unwrap();
        store
            .vault_put(vault, Uuid::new_v4(), "a.md", "Evidence", "orchard")
            .unwrap();
        store.vault_grant(vault, creds.node_id, true).unwrap();
        let saved = store
            .connect(&creds.raw_key)
            .unwrap()
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    template: Some("researcher-v1".into()),
                    readable_vaults: vec![vault],
                    ..Default::default()
                },
            )
            .unwrap();
        drop(store);
        let store = LocalHubStore::open(&path).unwrap();
        store.vault_reopen_manual().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve(store.clone(), listener, std::future::pending()));
        let client = RemoteLocalHub::new(&url, creds.raw_key.clone()).unwrap();
        assert_eq!(
            client.bots_agent_tool_policy_get(agent.id).await.unwrap(),
            saved
        );
        assert!(client
            .bots_agent_tool_execute(
                agent.id,
                saved.revision,
                &turn,
                AgentToolCall::VaultSearch {
                    vault,
                    query: "orchard".into(),
                    limit: 1
                }
            )
            .await
            .is_ok());
        let stranger = RemoteLocalHub::new(&url, "invalid".into()).unwrap();
        assert!(stranger.bots_agent_tool_policy_get(agent.id).await.is_err());
        let next = client
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: saved.revision,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(client
            .bots_agent_tool_execute(
                agent.id,
                next.revision,
                &turn,
                AgentToolCall::VaultSearch {
                    vault,
                    query: "orchard".into(),
                    limit: 1
                }
            )
            .await
            .is_err());
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE agent_profiles SET archived=1 WHERE id=?1",
                    [agent.id.to_string()],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        assert!(client
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    revision: next.revision,
                    ..Default::default()
                }
            )
            .await
            .is_err());
        server.abort();
        let _ = server.await;
        drop(store);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod turn_tests {
    use super::*;
    use crate::bots::*;
    #[test]
    fn agent_tool_turn_fences_cancellation_membership_expiry_and_budget() {
        let store = LocalHubStore::in_memory().unwrap();
        let c = store.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(c.node_id, owner).unwrap();
        let h = store.connect(&c.raw_key).unwrap();
        let a = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(c.node_id),
                capability_policy_ref: "none".into(),
                provider_account_ref: None,
                memory_namespace: "test".into(),
            })
            .unwrap();
        let turn = test_turn(&store, &a);
        let v = store.vault_create("Library").unwrap();
        store.vault_set_available(v, true).unwrap();
        store.vault_grant(v, c.node_id, true).unwrap();
        store
            .vault_put(v, Uuid::new_v4(), "test.md", "Test", "evidence")
            .unwrap();
        h.bots_agent_tool_policy_set(
            a.id,
            AgentToolPolicy {
                readable_vaults: vec![v],
                ..Default::default()
            },
        )
        .unwrap();
        let call = || AgentToolCall::VaultSearch {
            vault: v,
            query: "evidence".into(),
            limit: 1,
        };
        let mut bad = turn.clone();
        bad.generation += 1;
        assert!(h.bots_agent_tool_execute(a.id, 1, &bad, call()).is_err());
        bad = turn.clone();
        bad.conversation = Uuid::new_v4();
        assert!(h.bots_agent_tool_execute(a.id, 1, &bad, call()).is_err());
        bad = turn.clone();
        bad.conversation_revision += 1;
        assert!(h.bots_agent_tool_execute(a.id, 1, &bad, call()).is_err());
        for status in ["cancelled", "done", "failed", "pending"] {
            store
                .transaction(|tx| {
                    tx.execute("UPDATE agent_deliveries SET status=?1", [status])
                        .unwrap();
                    Ok(())
                })
                .unwrap();
            assert!(h.bots_agent_tool_execute(a.id, 1, &turn, call()).is_err());
        }
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE agent_deliveries SET status='running',lease_deadline=0",
                    [],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        assert!(h.bots_agent_tool_execute(a.id, 1, &turn, call()).is_err());
        store.transaction(|tx|{tx.execute("UPDATE agent_deliveries SET lease_deadline=?1",[now()+600]).unwrap();tx.execute("UPDATE conversation_members SET allowed_actions='[]' WHERE principal_kind='agent'",[]).unwrap();Ok(())}).unwrap();
        assert!(h.bots_agent_tool_execute(a.id, 1, &turn, call()).is_err());
        store.transaction(|tx|{tx.execute("UPDATE conversation_members SET allowed_actions=?1 WHERE principal_kind='agent'",[encode(&vec![MemberAction::Read]).unwrap()]).unwrap();Ok(())}).unwrap();
        for _ in 0..8 {
            h.bots_agent_tool_execute(a.id, 1, &turn, call()).unwrap();
        }
        assert!(h.bots_agent_tool_execute(a.id, 1, &turn, call()).is_err());
        store
            .transaction(|tx| {
                assert_eq!(
                    tx.query_row("SELECT count(*) FROM bots_agent_tool_turns", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    8
                );
                Ok(())
            })
            .unwrap();
    }
}
