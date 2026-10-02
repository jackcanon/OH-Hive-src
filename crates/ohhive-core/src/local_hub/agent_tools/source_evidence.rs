//! Durable executor reports; these are not independent verification of source truth.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Web {
        url: String,
    },
    Library {
        vault: Uuid,
        document: Uuid,
        revision: String,
    },
}
/// Executor-created only, never model tool arguments. Content is exactly the bounded text
/// returned to the model; its digest does not represent the full raw page or a truncated tail.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub title: Option<String>,
    pub content: String,
    pub truncated: bool,
    pub error: Option<String>,
    pub redirect_to: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub receipt: Uuid,
    pub source: Source,
    pub observation: Option<Observation>,
    pub observed_at: Option<i64>,
    pub content_sha256: Option<String>,
}
pub(super) fn pending_web(tx: &Transaction<'_>, receipt: Uuid, url: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO bots_source_evidence(receipt,source) VALUES(?1,?2)",
        params![
            receipt.to_string(),
            encode(&Source::Web { url: url.into() })?
        ],
    )
    .map_err(db_error)?;
    Ok(())
}
pub(super) fn record_library(
    tx: &Transaction<'_>,
    receipt: Uuid,
    vault: Uuid,
    document: Uuid,
    revision: &str,
    result: &Value,
) -> Result<()> {
    let observation = Observation {
        status: None,
        content_type: Some("text/plain".into()),
        title: result["title"].as_str().map(str::to_owned),
        content: result["content"]
            .as_str()
            .ok_or_else(|| rejected("missing source text"))?
            .into(),
        truncated: result["truncated"].as_bool().unwrap_or(false),
        error: None,
        redirect_to: None,
    };
    tx.execute("INSERT INTO bots_source_evidence(receipt,source,observation,observed_at,content_sha256) VALUES(?1,?2,?3,?4,?5)",
        params![receipt.to_string(), encode(&Source::Library { vault, document, revision: revision.into() })?, encode(&observation)?, now(), observed_digest(&observation)]).map_err(db_error)?;
    Ok(())
}
fn load(tx: &Transaction<'_>, receipt: Uuid) -> Result<Evidence> {
    let (source, observation, time, hash): (String,Option<String>,Option<i64>,Option<String>) = tx.query_row(
        "SELECT source,observation,observed_at,content_sha256 FROM bots_source_evidence WHERE receipt=?1", [receipt.to_string()],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(db_error)?;
    Ok(Evidence {
        receipt,
        source: decode(&source)?,
        observation: observation.map(|s| decode(&s)).transpose()?,
        observed_at: time,
        content_sha256: hash,
    })
}
fn observed_digest(o: &Observation) -> Option<String> {
    if o.error.is_some() || o.redirect_to.is_some() {
        None
    } else {
        Some(digest(&o.content))
    }
}
impl LocalHub {
    /// A completion report uses the original authorization, assigned host and live generation.
    /// It is not another tool call, so the eighth authorized fetch can still record its result.
    pub fn bots_agent_web_observe(
        &self,
        agent: Uuid,
        expected_revision: u32,
        turn: &AgentToolTurn,
        receipt: Uuid,
        observation: Observation,
    ) -> Result<Evidence> {
        if observation.content.len() > 32768
            || observation
                .status
                .is_some_and(|s| !(100..=599).contains(&s))
            || observation
                .content_type
                .as_ref()
                .is_some_and(|s| s.len() > 256)
            || observation.title.as_ref().is_some_and(|s| s.len() > 512)
            || observation
                .error
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 512)
            || observation
                .redirect_to
                .as_ref()
                .is_some_and(|s| s.len() > 2048)
            || (observation.error.is_some() && !observation.content.is_empty())
            || (observation.redirect_to.is_some()
                && (!observation.status.is_some_and(|s| (300..=399).contains(&s))
                    || !observation.content.is_empty()))
            || (observation.error.is_none() && observation.status.is_none())
        {
            return Err(rejected("invalid bounded source observation"));
        }
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            turn_alive_check(tx,agent,turn)?;
            let current=policy(tx,agent)?;
            if current.revision!=expected_revision { return Err(rejected("tool policy changed; reload access")); }
            let authorized: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM bots_agent_tool_receipts r JOIN bots_agent_tool_turns t ON t.receipt=r.id JOIN agent_profiles a ON a.id=r.agent WHERE r.id=?1 AND r.agent=?2 AND r.node=?3 AND r.tool='web_fetch' AND r.policy_revision=?4 AND t.message=?5 AND t.conversation=?6 AND t.generation=?7 AND a.runtime_kind='local' AND a.preferred_host=?3)",
                params![receipt.to_string(),agent.to_string(),node,expected_revision,turn.message.to_string(),turn.conversation.to_string(),turn.generation as i64],|r|r.get(0)).map_err(db_error)?;
            if !authorized { return Err(rejected("source observation does not match authorized host and turn")); }
            let existing=load(tx,receipt)?;
            if let Some(saved)=existing.observation.as_ref() {
                if saved!=&observation { return Err(rejected("source observation is immutable")); }
                return Ok(existing);
            }
            tx.execute("UPDATE bots_source_evidence SET observation=?2,observed_at=?3,content_sha256=?4 WHERE receipt=?1 AND observation IS NULL",
                params![receipt.to_string(),encode(&observation)?,now(),observed_digest(&observation)]).map_err(db_error)?;
            load(tx,receipt)
        })
    }
}

/// Bind evidence to a stored executor reply, not model-supplied source identifiers. Only
/// the exact originating agent, conversation and completed/current delivery generation match.
pub(crate) fn reply_metadata(tx: &Transaction<'_>, reply: Uuid) -> Result<Vec<Value>> {
    let mut q=tx.prepare("SELECT e.receipt,e.source,e.observation,e.observed_at,e.content_sha256 FROM bots_source_evidence e JOIN bots_agent_tool_turns t ON t.receipt=e.receipt JOIN messages m ON m.id=?1 JOIN agent_deliveries d ON d.message_id=t.message AND d.recipient=t.agent AND d.lease_generation=t.generation WHERE m.author_kind='agent' AND m.author_id=t.agent AND m.conversation_id=t.conversation AND m.client_request_id='delivery:' || t.message || ':' || t.agent ORDER BY e.receipt LIMIT 8").map_err(db_error)?;
    let rows = q
        .query_map([reply.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(db_error)?;
    rows.map(|r| {
        let(receipt,source,observation,time,hash)=r.map_err(db_error)?;
        let mut observation: Option<Value>=observation.map(|s|decode(&s)).transpose()?;
        // Preserve digest/status/truncation with the note; full bounded text stays in evidence.
        if let Some(Value::Object(o))=observation.as_mut() { o.remove("content"); }
        Ok(json!({"receipt":receipt,"source":decode::<Source>(&source)?,"observation":observation,"recorded_at":time,"content_sha256":hash,"verification":"executor report; not independent source verification"}))
    }).collect()
}

#[cfg(test)]
impl LocalHubStore {
    pub(crate) fn source_evidence_test_records(&self) -> Vec<Evidence> {
        self.transaction(|tx| {
            let mut q = tx
                .prepare("SELECT receipt FROM bots_source_evidence ORDER BY receipt")
                .map_err(db_error)?;
            let ids = q
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(db_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_error)?;
            ids.into_iter()
                .map(|s| load(tx, s.parse().unwrap()))
                .collect()
        })
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::{AgentRuntimeKind, NewAgentProfile};
    fn fixture() -> (
        LocalHubStore,
        LocalHub,
        Uuid,
        AgentToolPolicy,
        AgentToolTurn,
    ) {
        let store = LocalHubStore::in_memory().unwrap();
        let key = store.enroll_owner("Evidence host").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(key.node_id, owner).unwrap();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(key.node_id),
                capability_policy_ref: "none".into(),
                provider_account_ref: None,
                memory_namespace: "evidence-test".into(),
            })
            .unwrap();
        let hub = store.connect(&key.raw_key).unwrap();
        let policy = hub
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    web_hosts: Some(vec!["example.org".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        let turn = super::super::test_turn(&store, &agent);
        (store, hub, agent.id, policy, turn)
    }
    fn observation() -> Observation {
        Observation {
            status: Some(200),
            content_type: Some("text/plain".into()),
            title: None,
            content: "Observed evidence".into(),
            truncated: false,
            error: None,
            redirect_to: None,
        }
    }
    #[test]
    fn source_evidence_exact_origin_idempotent_immutable_and_eighth_call() {
        let (store, hub, agent, policy, turn) = fixture();
        for i in 0..8 {
            let url = format!("https://example.org/page/{i}");
            let grant = hub
                .bots_agent_web_authorize(agent, policy.revision, &turn, &url)
                .unwrap();
            store
                .transaction(|tx| {
                    let e = load(tx, grant.receipt)?;
                    assert!(e.observation.is_none());
                    assert!(e.content_sha256.is_none());
                    Ok(())
                })
                .unwrap();
            let e = hub
                .bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, observation())
                .unwrap();
            assert_eq!(e.source, Source::Web { url });
            assert_eq!(e.content_sha256, Some(digest("Observed evidence")));
            let repeated = hub
                .bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, observation())
                .unwrap();
            assert_eq!(e.observed_at, repeated.observed_at);
            let mut changed = observation();
            changed.content = "replacement".into();
            assert!(hub
                .bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, changed)
                .is_err());
        }
        assert!(hub
            .bots_agent_web_authorize(agent, policy.revision, &turn, "https://example.org/ninth")
            .is_err());
    }
    #[test]
    fn source_evidence_rejects_unknown_foreign_stale_revoked_and_bad_results() {
        let (store, hub, agent, policy, turn) = fixture();
        let grant = hub
            .bots_agent_web_authorize(agent, policy.revision, &turn, "https://example.org/a")
            .unwrap();
        assert!(hub
            .bots_agent_web_observe(agent, policy.revision, &turn, Uuid::new_v4(), observation())
            .is_err());
        let other = store.enroll_owner("Other host").unwrap();
        let owner = store
            .transaction(|tx| {
                tx.query_row(
                    "SELECT owner FROM agent_profiles WHERE id=?1",
                    [agent.to_string()],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        store
            .set_node_owner(other.node_id, owner.parse().unwrap())
            .unwrap();
        let other_hub = store.connect(&other.raw_key).unwrap();
        assert!(other_hub
            .bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, observation())
            .is_err());
        let mut stale = turn.clone();
        stale.generation += 1;
        assert!(hub
            .bots_agent_web_observe(agent, policy.revision, &stale, grant.receipt, observation())
            .is_err());
        let mut bad = observation();
        bad.content = "x".repeat(32769);
        assert!(hub
            .bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, bad)
            .is_err());
        hub.bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, observation())
            .unwrap();
        hub.bots_agent_tool_policy_set(
            agent,
            AgentToolPolicy {
                revision: policy.revision,
                web_hosts: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(hub
            .bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, observation())
            .is_err());
    }
    #[tokio::test]
    async fn source_evidence_remote_report_checks_assigned_node_and_deduplicates() {
        let store = LocalHubStore::in_memory().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve(store.clone(), listener, std::future::pending()));
        let key = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "Research machine")
            .await
            .unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(key.node_id, owner).unwrap();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(key.node_id),
                capability_policy_ref: "none".into(),
                provider_account_ref: None,
                memory_namespace: "remote-evidence".into(),
            })
            .unwrap();
        let client = RemoteLocalHub::new(&url, key.raw_key).unwrap();
        let p = client
            .bots_agent_tool_policy_set(
                agent.id,
                AgentToolPolicy {
                    web_hosts: Some(vec!["example.org".into()]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let turn = super::super::test_turn(&store, &agent);
        let grant = client
            .bots_agent_web_authorize(agent.id, p.revision, &turn, "https://example.org/source")
            .await
            .unwrap();
        let first = client
            .bots_agent_web_observe(agent.id, p.revision, &turn, grant.receipt, observation())
            .await
            .unwrap();
        let retry = client
            .bots_agent_web_observe(agent.id, p.revision, &turn, grant.receipt, observation())
            .await
            .unwrap();
        assert_eq!(first.observed_at, retry.observed_at);
        assert_eq!(first.content_sha256, Some(digest("Observed evidence")));
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE local_node_keys SET revoked=1 WHERE node_id=?1",
                    [key.node_id.to_string()],
                )
                .map_err(db_error)?;
                Ok(())
            })
            .unwrap();
        assert!(client
            .bots_agent_web_observe(agent.id, p.revision, &turn, grant.receipt, observation())
            .await
            .is_err());
        server.abort();
    }
    #[test]
    fn source_evidence_failure_redirect_http_error_and_truncation_remain_distinct() {
        let (_, hub, agent, policy, turn) = fixture();
        for o in [
            Observation {
                status: None,
                content: String::new(),
                error: Some("fetch failed: could not connect".into()),
                ..observation()
            },
            Observation {
                status: Some(302),
                content: String::new(),
                redirect_to: Some("https://unvisited.example/".into()),
                ..observation()
            },
            Observation {
                status: Some(404),
                ..observation()
            },
            Observation {
                truncated: true,
                content: "partial".into(),
                ..observation()
            },
        ] {
            let grant = hub
                .bots_agent_web_authorize(agent, policy.revision, &turn, "https://example.org/a")
                .unwrap();
            let saved = hub
                .bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, o.clone())
                .unwrap();
            assert_eq!(saved.observation, Some(o));
        }
    }
    #[test]
    fn source_evidence_saved_reply_includes_only_its_own_generation_metadata() {
        use crate::bots::{MessageKind, NewMessage, Principal};
        let (store, hub, agent, policy, turn) = fixture();
        let grant = hub
            .bots_agent_web_authorize(agent, policy.revision, &turn, "https://example.org/report")
            .unwrap();
        hub.bots_agent_web_observe(agent, policy.revision, &turn, grant.receipt, observation())
            .unwrap();
        let reply = store
            .bots_message_send(
                Principal::Agent(agent),
                turn.conversation,
                format!("delivery:{}:{}", turn.message, agent),
                turn.conversation_revision,
                vec![],
                NewMessage {
                    thread_root: Some(turn.message),
                    kind: MessageKind::Text,
                    body: Some("Source finding".into()),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .unwrap();
        let vault = store.vault_create("Findings").unwrap();
        store.vault_set_available(vault, true).unwrap();
        let node = store
            .transaction(|tx| {
                tx.query_row(
                    "SELECT preferred_host FROM agent_profiles WHERE id=?1",
                    [agent.to_string()],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        store
            .vault_grant(vault, node.parse().unwrap(), true)
            .unwrap();
        let saved = hub
            .bots_library_save(reply.id, vault, "Useful finding")
            .unwrap();
        assert!(saved.content.contains("https://example.org/report"));
        assert!(saved.content.contains(&digest("Observed evidence")));
        // The source text is retained in its record, not duplicated into the note's origin.
        assert!(!saved.content.contains("Observed evidence"));
        store.transaction(|tx| {
            assert_eq!(reply_metadata(tx,reply.id)?.len(),1);
            tx.execute("UPDATE agent_deliveries SET lease_generation=lease_generation+1 WHERE message_id=?1 AND recipient=?2",params![turn.message.to_string(),agent.to_string()]).map_err(db_error)?;
            assert!(reply_metadata(tx,reply.id)?.is_empty());Ok(())
        }).unwrap();
    }
    #[test]
    fn source_evidence_library_read_retains_revision_and_bounded_text() {
        let (store, hub, agent, policy, turn) = fixture();
        let vault = store.vault_create("Research").unwrap();
        let doc = Uuid::new_v4();
        let text = "é".repeat(20000);
        let revision = store
            .vault_put(vault, doc, "source.md", "Source", &text)
            .unwrap();
        store.vault_set_available(vault, true).unwrap();
        let node = store
            .transaction(|tx| {
                tx.query_row(
                    "SELECT preferred_host FROM agent_profiles WHERE id=?1",
                    [agent.to_string()],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        store
            .vault_grant(vault, node.parse().unwrap(), true)
            .unwrap();
        let policy = hub
            .bots_agent_tool_policy_set(
                agent,
                AgentToolPolicy {
                    revision: policy.revision,
                    readable_vaults: vec![vault],
                    ..Default::default()
                },
            )
            .unwrap();
        let result = hub
            .bots_agent_tool_execute(
                agent,
                policy.revision,
                &turn,
                AgentToolCall::VaultRead {
                    vault,
                    document: doc,
                    revision: revision.clone(),
                },
            )
            .unwrap();
        let receipt = result["receipt"].as_str().unwrap().parse().unwrap();
        let evidence = store.transaction(|tx| load(tx, receipt)).unwrap();
        assert_eq!(
            evidence.source,
            Source::Library {
                vault,
                document: doc,
                revision
            }
        );
        let o = evidence.observation.unwrap();
        assert!(o.truncated);
        assert_eq!(o.content.len(), 32768);
        assert_eq!(evidence.content_sha256, Some(digest(&o.content)));
    }
}
