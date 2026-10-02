//! Separately granted research-result saving; it cannot overwrite any source or note.
use super::*;

pub(super) fn save_access(
    hub: &LocalHub,
    tx: &Transaction<'_>,
    node: &str,
    agent: Uuid,
    vault: Uuid,
) -> Result<()> {
    hub.vault_access(tx, node, vault, true)?;
    let allowed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM vaults v JOIN vault_readers r ON r.vault_id=v.id JOIN agent_profiles a ON a.preferred_host=r.node_id WHERE v.id=?1 AND a.id=?2 AND v.state='ready' AND NOT EXISTS(SELECT 1 FROM vault_sources s WHERE s.vault_id=v.id))",params![vault.to_string(),agent.to_string()],|r|r.get(0)).map_err(db_error)?;
    if !allowed {
        return Err(rejected(
            "choose a ready manual collection shared with the agent computer",
        ));
    }
    Ok(())
}
impl LocalHub {
    #[allow(clippy::too_many_arguments)]
    pub fn bots_agent_library_save(
        &self,
        agent: Uuid,
        revision: u32,
        turn: &AgentToolTurn,
        vault: Uuid,
        title: &str,
        findings: &str,
        receipts: &[Uuid],
    ) -> Result<Value> {
        check_text(title, 200)?;
        check_text(findings, 16384)?;
        let mut receipts = receipts.to_vec();
        receipts.sort();
        receipts.dedup();
        if receipts.is_empty() || receipts.len() > 8 {
            return Err(rejected(
                "save requires 1..8 observed source receipts from this attempt",
            ));
        }
        let fingerprint = digest(&encode(&(title, findings, &receipts))?);
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;turn_alive_check(tx,agent,turn)?;
            let hosted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND runtime_kind='local' AND preferred_host=?2)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
            if !hosted { return Err(rejected("save must come from the assigned agent host")); }
            let current=policy(tx,agent)?;
            if current.revision!=revision || !current.writable_vaults().contains(&vault) { return Err(rejected("collection save access changed or is not granted")); }
            save_access(self,tx,node,agent,vault)?;
            let mut sources=Vec::<Value>::new();
            for receipt in &receipts {
                let row:Option<(String,String,i64,Option<String>)>=tx.query_row("SELECT e.source,e.observation,e.observed_at,e.content_sha256 FROM bots_source_evidence e JOIN bots_agent_tool_receipts r ON r.id=e.receipt JOIN bots_agent_tool_turns t ON t.receipt=e.receipt WHERE e.receipt=?1 AND r.agent=?2 AND r.node=?3 AND r.policy_revision=?4 AND t.message=?5 AND t.conversation=?6 AND t.generation=?7 AND e.observation IS NOT NULL",params![receipt.to_string(),agent.to_string(),node,revision,turn.message.to_string(),turn.conversation.to_string(),turn.generation as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
                let(source,observation,time,hash)=row.ok_or_else(||rejected("source was not observed in this attempt"))?;
                let observed:source_evidence::Observation=decode(&observation)?;
                if observed.error.is_some() || observed.redirect_to.is_some() || observed.status.is_some_and(|s|!(200..=299).contains(&s)) || observed.content.trim().is_empty() { return Err(rejected("failed, redirected or empty sources cannot support saved findings")); }
                let source:source_evidence::Source=decode(&source)?;
                // Re-check current read grants: retaining a receipt is not durable access.
                match &source {
                    source_evidence::Source::Library { vault,.. } => {
                        if !current.readable_vaults.contains(vault) { return Err(rejected("source library read access revoked")); }
                        self.vault_access(tx,node,*vault,true)?;
                    },
                    source_evidence::Source::Web { url } => {
                        let parsed=url::Url::parse(url).map_err(|_|rejected("invalid recorded source"))?;
                        let host=parsed.host_str().ok_or_else(||rejected("invalid recorded source host"))?;
                        let host=if matches!(host,"127.0.0.1"|"localhost") {"localhost"} else {host};
                        if !host_allowed(current.web_hosts(),host) { return Err(rejected("source web access revoked")); }
                    }
                }
                let mut meta:Value=serde_json::to_value(observed).map_err(|_|rejected("invalid source result"))?;
                meta.as_object_mut().unwrap().remove("content");
                sources.push(json!({"receipt":receipt,"source":source,"observation":meta,"recorded_at":time,"content_sha256":hash}));
            }
            let existing:Option<(String,String,String,String)>=tx.query_row("SELECT d.id,d.path,d.title,s.payload FROM bots_agent_library_saves s JOIN vault_documents d ON d.id=s.document WHERE s.agent=?1 AND s.message=?2 AND s.generation=?3 AND s.vault=?4",params![agent.to_string(),turn.message.to_string(),turn.generation as i64,vault.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
            if let Some((id,path,title,payload))=existing {
                if payload!=fingerprint { return Err(rejected("this attempt already saved different findings to this collection")); }
                return Ok(json!({"document":id,"vault":vault,"path":path,"title":title,"already_saved":true}));
            }
            turn_check(tx,agent,turn)?;
            let id=Uuid::new_v4();let path=format!("agent-findings/{agent}/{}-{}.md",turn.message,turn.generation);
            let origin=json!({"agent":agent,"message":turn.message,"conversation":turn.conversation,"generation":turn.generation,"policy_revision":revision,"saved_at":now(),"sources":sources});
            let content=format!("# {title}\n\n## Agent findings\n\n{findings}\n\n## Source record\n\nSaved under your explicit collection permission. Findings are model-generated claims, not independently verified facts. Source records describe executor-reported retrieval; truncated sources are partial.\n\n```json\n{}\n```\n",serde_json::to_string_pretty(&origin).map_err(|_|rejected("invalid findings origin"))?);
            let document_revision=digest(&encode(&(id,&path,title,&content))?);
            tx.execute("INSERT INTO vault_documents(id,vault_id,path,revision,title,content,document_date) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id.to_string(),vault.to_string(),path,document_revision,title,content,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_library_saves(agent,message,generation,vault,document,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![agent.to_string(),turn.message.to_string(),turn.generation as i64,vault.to_string(),id.to_string(),fingerprint,now()]).map_err(db_error)?;
            let receipt=Uuid::new_v4();
            tx.execute("INSERT INTO bots_agent_tool_receipts(id,agent,node,tool,vault,policy_revision,created_at) VALUES(?1,?2,?3,'vault_save',?4,?5,?6)",params![receipt.to_string(),agent.to_string(),node,vault.to_string(),revision,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_tool_turns(receipt,message,conversation,agent,generation) VALUES(?1,?2,?3,?4,?5)",params![receipt.to_string(),turn.message.to_string(),turn.conversation.to_string(),agent.to_string(),turn.generation as i64]).map_err(db_error)?;
            Ok(json!({"document":id,"vault":vault,"path":path,"title":title,"already_saved":false}))
        })
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
        Uuid,
        AgentToolPolicy,
        AgentToolTurn,
        Uuid,
    ) {
        let s = LocalHubStore::in_memory().unwrap();
        let key = s.enroll_owner("Research host").unwrap();
        let owner = Uuid::new_v4();
        s.set_node_owner(key.node_id, owner).unwrap();
        let a = s
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(key.node_id),
                capability_policy_ref: "none".into(),
                provider_account_ref: None,
                memory_namespace: "save".into(),
            })
            .unwrap();
        let h = s.connect(&key.raw_key).unwrap();
        let v = s.vault_create("Research findings").unwrap();
        s.vault_set_available(v, true).unwrap();
        s.vault_grant(v, key.node_id, true).unwrap();
        let p = h
            .bots_agent_tool_policy_set(
                a.id,
                AgentToolPolicy {
                    writable_vaults: Some(vec![v]),
                    web_hosts: Some(vec!["example.org".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        let t = super::super::test_turn(&s, &a);
        let grant = h
            .bots_agent_web_authorize(a.id, p.revision, &t, "https://example.org/source")
            .unwrap();
        h.bots_agent_web_observe(
            a.id,
            p.revision,
            &t,
            grant.receipt,
            source_evidence::Observation {
                status: Some(200),
                content_type: Some("text/plain".into()),
                title: None,
                content: "Useful source".into(),
                truncated: true,
                error: None,
                redirect_to: None,
            },
        )
        .unwrap();
        (s, h, a.id, v, p, t, grant.receipt)
    }
    #[test]
    fn library_save_source_grounded_deduplication_preserves_human_edits() {
        let (s, h, a, v, p, t, r) = fixture();
        let saved = h
            .bots_agent_tool_execute(
                a,
                p.revision,
                &t,
                AgentToolCall::VaultSave {
                    vault: v,
                    title: "Findings".into(),
                    findings: "A qualified finding".into(),
                    source_receipts: vec![r],
                },
            )
            .unwrap();
        let id: Uuid = saved["document"].as_str().unwrap().parse().unwrap();
        let path = saved["path"].as_str().unwrap();
        let revision = s
            .transaction(|tx| {
                tx.query_row(
                    "SELECT revision FROM vault_documents WHERE id=?1",
                    [id.to_string()],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        let note = h.vault_read(v, id, &revision).unwrap();
        assert!(note.content.contains("https://example.org/source"));
        assert!(note.content.contains("\"truncated\": true"));
        assert!(note.content.contains("not independently verified"));
        s.vault_put(v, id, path, "Human edit", "Reviewed and corrected")
            .unwrap();
        let again = h
            .bots_agent_library_save(
                a,
                p.revision,
                &t,
                v,
                "Findings",
                "A qualified finding",
                &[r, r],
            )
            .unwrap();
        assert_eq!(again["document"], saved["document"]);
        let text = s
            .transaction(|tx| {
                tx.query_row(
                    "SELECT content FROM vault_documents WHERE id=?1",
                    [id.to_string()],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        assert_eq!(text, "Reviewed and corrected");
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Findings", "different output", &[r])
            .is_err());
    }
    #[test]
    fn library_save_does_not_infer_write_from_read_or_preserved_legacy_fields() {
        let (s, h, a, v, p, t, r) = fixture();
        let legacy: AgentToolPolicy = serde_json::from_value(
            json!({"revision":p.revision,"template":null,"readable_vaults":[v]}),
        )
        .unwrap();
        let kept = h.bots_agent_tool_policy_set(a, legacy).unwrap();
        assert_eq!(kept.writable_vaults(), &[v]);
        let revoked = h
            .bots_agent_tool_policy_set(
                a,
                AgentToolPolicy {
                    revision: kept.revision,
                    readable_vaults: vec![v],
                    writable_vaults: Some(vec![]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(h
            .bots_agent_library_save(a, revoked.revision, &t, v, "Title", "Result", &[r])
            .is_err());
        let count = s
            .transaction(|tx| {
                tx.query_row("SELECT count(*) FROM bots_agent_library_saves", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(db_error)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn library_save_rejects_unobserved_failed_and_foreign_attempt_sources() {
        let (_, h, a, v, p, t, r) = fixture();
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[])
            .is_err());
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[Uuid::new_v4()])
            .is_err());
        let grant = h
            .bots_agent_web_authorize(a, p.revision, &t, "https://example.org/pending")
            .unwrap();
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[grant.receipt])
            .is_err());
        h.bots_agent_web_observe(
            a,
            p.revision,
            &t,
            grant.receipt,
            source_evidence::Observation {
                status: Some(404),
                content_type: None,
                title: None,
                content: "not found".into(),
                truncated: false,
                error: None,
                redirect_to: None,
            },
        )
        .unwrap();
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[grant.receipt])
            .is_err());
        let mut stale = t.clone();
        stale.generation += 1;
        assert!(h
            .bots_agent_library_save(a, p.revision, &stale, v, "Title", "Result", &[r])
            .is_err());
    }
    #[test]
    fn library_save_revoked_access_rejects_retry_and_folder_grants() {
        let (s, h, a, v, p, t, r) = fixture();
        h.bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[r])
            .unwrap();
        let node = s
            .transaction(|tx| {
                tx.query_row(
                    "SELECT preferred_host FROM agent_profiles WHERE id=?1",
                    [a.to_string()],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        s.vault_grant(v, node.parse().unwrap(), false).unwrap();
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[r])
            .is_err());
        assert!(h
            .bots_agent_tool_policy_set(
                a,
                AgentToolPolicy {
                    revision: p.revision,
                    writable_vaults: Some(vec![v]),
                    ..Default::default()
                }
            )
            .is_err());
        s.vault_grant(v, node.parse().unwrap(), true).unwrap();
        s.transaction(|tx| {tx.execute("INSERT INTO vault_sources(vault_id,root,root_identity) VALUES(?1,'/tmp/source','test-root')",[v.to_string()]).map_err(db_error)?;Ok(())}).unwrap();
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[r])
            .is_err());
    }
    #[test]
    fn library_save_atomic_conflict_does_not_consume_a_receipt_or_overwrite() {
        let (s, h, a, v, p, t, r) = fixture();
        let path = format!("agent-findings/{a}/{}-{}.md", t.message, t.generation);
        s.vault_put(v, Uuid::new_v4(), &path, "Existing", "Keep this")
            .unwrap();
        assert!(h
            .bots_agent_library_save(a, p.revision, &t, v, "Title", "Result", &[r])
            .is_err());
        assert_eq!(s.bots_agent_tool_receipt_count("vault_save", None), 0);
        let count = s
            .transaction(|tx| {
                tx.query_row("SELECT count(*) FROM bots_agent_library_saves", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(db_error)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
