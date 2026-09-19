//! Owner-saved, resource-scoped agent tools. No prompt or opaque policy reference grants access.
use super::*;
use rusqlite::OptionalExtension;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolPolicy {
    pub revision: u32,
    /// Versioned template selection is descriptive; only concrete grants authorize tools.
    pub template: Option<String>,
    pub readable_vaults: Vec<Uuid>,
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
            )
        }) {
            return Err(rejected("unknown agent template"));
        }
        next.readable_vaults.sort();
        next.readable_vaults.dedup();
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            let previous=policy(tx,agent)?;
            if next.revision!=previous.revision {return Err(rejected("Tool access changed. Reload before saving."));}
            for vault in &next.readable_vaults {
                let exists: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM vaults WHERE id=?1)",[vault.to_string()],|r|r.get(0)).map_err(db_error)?;
                if !exists {return Err(rejected("library not found"));}
            }
            next.revision=next.revision.checked_add(1).ok_or_else(||rejected("policy revision exhausted"))?;
            tx.execute("INSERT INTO bots_agent_tool_policies(agent,policy) VALUES(?1,?2) ON CONFLICT(agent) DO UPDATE SET policy=excluded.policy",params![agent.to_string(),encode(&next)?]).map_err(db_error)?;
            tx.execute("UPDATE agent_profiles SET role_revision=role_revision+1 WHERE id=?1",[agent.to_string()]).map_err(db_error)?;
            Ok(next.clone())
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
            let vault=match &call {AgentToolCall::VaultSearch{vault,..}|AgentToolCall::VaultRead{vault,..}=>*vault};
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
            };
            let receipt=Uuid::new_v4();
            tx.execute("INSERT INTO bots_agent_tool_receipts(id,agent,node,tool,vault,policy_revision,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt.to_string(),agent.to_string(),node,tool,vault.to_string(),current.revision,now()]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_agent_tool_turns(receipt,message,conversation,agent,generation) VALUES(?1,?2,?3,?4,?5)",params![receipt.to_string(),turn.message.to_string(),turn.conversation.to_string(),agent.to_string(),turn.generation as i64]).map_err(db_error)?;
            Ok(serde_json::json!({"receipt":receipt,"result":result}))
        })
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
