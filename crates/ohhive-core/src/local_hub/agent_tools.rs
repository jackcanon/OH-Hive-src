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
        call: AgentToolCall,
    ) -> Result<serde_json::Value> {
        self.with_node(|tx,node| {
            owner_check(tx,node,agent)?;
            let host: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND runtime_kind='local' AND preferred_host=?2)",params![agent.to_string(),node],|r|r.get(0)).map_err(db_error)?;
            if !host {return Err(rejected("tool calls must come from the assigned agent host"));}
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
            Ok(serde_json::json!({"receipt":receipt,"result":result}))
        })
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
        assert!(h.bots_agent_tool_execute(agent.id, 0, read()).is_err());
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
        let result = h.bots_agent_tool_execute(agent.id, 1, read()).unwrap();
        assert_eq!(result["result"]["content"], "alpha evidence");
        let hits = h
            .bots_agent_tool_execute(
                agent.id,
                1,
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
                AgentToolCall::VaultSearch {
                    vault: other,
                    query: "alpha".into(),
                    limit: 5
                }
            )
            .is_err());
        assert!(h.bots_agent_tool_execute(agent.id, 0, read()).is_err());
        assert!(h
            .bots_agent_tool_execute(
                agent.id,
                1,
                AgentToolCall::VaultRead {
                    vault: v,
                    document: doc,
                    revision: "stale".into()
                }
            )
            .is_err());
        s.vault_grant(v, c.node_id, false).unwrap();
        assert!(h.bots_agent_tool_execute(agent.id, 1, read()).is_err());
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
        assert!(h.bots_agent_tool_execute(agent.id, 1, read()).is_err());
        assert!(h
            .bots_agent_tool_execute(agent.id, revoked.revision, read())
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
        assert!(h.bots_agent_tool_execute(agent.id, 3, read()).is_err());
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
