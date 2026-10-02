//! Explicit owner action, not an agent tool or a consequence of a Library read grant.
use super::*;
use crate::bots::MemberAction;
use rusqlite::OptionalExtension;
use vault::{VaultDocument, VaultInfo};

fn owner(tx: &Transaction<'_>, node: &str) -> Result<String> {
    tx.query_row(
        "SELECT owner_member_id FROM nodes WHERE id=?1",
        [node],
        |r| r.get::<_, Option<String>>(0),
    )
    .map_err(db_error)?
    .ok_or_else(|| rejected("Private Fleet owner is required"))
}

type SourceReply = (
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
);

impl LocalHub {
    /// Only manual, ready collections this paired computer can read. No general remote admin.
    pub fn bots_library_collections(&self) -> Result<Vec<VaultInfo>> {
        self.with_node(|tx, node| {
            owner(tx, node)?;
            let mut q = tx.prepare("SELECT v.id,v.name,v.state FROM vaults v JOIN vault_readers r ON r.vault_id=v.id WHERE r.node_id=?1 AND v.state='ready' AND NOT EXISTS(SELECT 1 FROM vault_sources s WHERE s.vault_id=v.id) ORDER BY v.name,v.id LIMIT 1000").map_err(db_error)?;
            let rows = q.query_map([node], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?))).map_err(db_error)?;
            rows.map(|r| {
                let (id,name,state) = r.map_err(db_error)?;
                Ok(VaultInfo { id: id.parse().map_err(|_| rejected("invalid collection identity"))?, name, state })
            }).collect()
        })
    }

    /// Server reads the original reply. Caller cannot substitute content, author or provenance.
    /// Insert plus receipt is atomic; retries return the saved document without overwriting edits.
    pub fn bots_library_save(
        &self,
        message: Uuid,
        vault: Uuid,
        title: &str,
    ) -> Result<VaultDocument> {
        check_text(title, 200)?;
        self.with_node(|tx, node| {
            let owner = owner(tx, node)?;
            let allowed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM vaults v JOIN vault_readers r ON r.vault_id=v.id WHERE v.id=?1 AND r.node_id=?2 AND v.state='ready' AND NOT EXISTS(SELECT 1 FROM vault_sources s WHERE s.vault_id=v.id))", params![vault.to_string(),node], |r| r.get(0)).map_err(db_error)?;
            if !allowed { return Err(rejected("choose an available manual collection shared with this computer")); }
            let source: Option<SourceReply> = tx.query_row("SELECT m.conversation_id,m.body,a.name,a.id,m.created_at,m.turn_ref,m.source_event_ref FROM messages m JOIN conversations c ON c.id=m.conversation_id JOIN agent_profiles a ON a.id=m.author_id WHERE m.id=?1 AND c.owner=?2 AND a.owner=?2 AND m.author_kind='agent' AND m.kind='text'", params![message.to_string(),owner], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(db_error)?;
            let (conversation,body,name,agent,created,turn,event) = source.ok_or_else(|| rejected("saved agent reply not found for this owner"))?;
            let actions: Option<String> = tx.query_row("SELECT allowed_actions FROM conversation_members WHERE conversation_id=?1 AND principal_kind='user' AND principal_id=?2", params![conversation,owner], |r| r.get(0)).optional().map_err(db_error)?;
            let actions: Vec<MemberAction> = actions.map(|s| decode(&s)).transpose()?.unwrap_or_default();
            if !actions.contains(&MemberAction::Read) { return Err(rejected("conversation read access required")); }
            let revised: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM message_revisions WHERE original_id=?1)", [message.to_string()], |r| r.get(0)).map_err(db_error)?;
            if revised { return Err(rejected("edited or removed replies cannot be saved by this action")); }
            let existing: Option<(String,String,String,String,String)> = tx.query_row("SELECT d.id,d.path,d.revision,d.title,d.content FROM bots_library_saves s JOIN vault_documents d ON d.id=s.document WHERE s.message=?1 AND s.vault=?2 AND s.owner=?3",params![message.to_string(),vault.to_string(),owner],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db_error)?;
            if let Some((id,path,revision,title,content)) = existing {
                return Ok(VaultDocument { id:id.parse().map_err(|_| rejected("invalid saved document identity"))?,vault_id:vault,path,revision,title,content });
            }
            if body.trim().is_empty() || body.len() > 65536 { return Err(rejected("reply has no bounded text to save")); }
            let id = Uuid::new_v4();
            let path = format!("agent-replies/{message}.md");
            let evidence = agent_tools::source_evidence::reply_metadata(tx, message)?;
            let provenance = serde_json::json!({"message":message,"conversation":conversation,"agent":agent,"agent_name":name,"reply_created_at":created,"saved_at":now(),"turn":turn,"source_event":event,"body_sha256":digest(&body),"sources":evidence});
            let content = format!("# {title}\n\n## Saved agent reply\n\n{body}\n\n## Origin\n\nSaved by your explicit Library action. This is an agent reply, not independently verified research. Links and citations in the reply have not been verified by saving it.\n\n```json\n{}\n```\n",serde_json::to_string_pretty(&provenance).map_err(|_| rejected("cannot encode reply origin"))?);
            let revision = digest(&encode(&(id,&path,title,&content))?);
            tx.execute("INSERT INTO vault_documents(id,vault_id,path,revision,title,content,document_date) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id.to_string(),vault.to_string(),path,revision,title,content,created]).map_err(db_error)?;
            tx.execute("INSERT INTO bots_library_saves(message,vault,document,owner,node,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![message.to_string(),vault.to_string(),id.to_string(),owner,node,now()]).map_err(db_error)?;
            Ok(VaultDocument { id,vault_id:vault,path,revision,title:title.into(),content })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::*;

    fn fixture() -> (LocalHubStore, LocalHub, Uuid, Uuid, Uuid) {
        let store = LocalHubStore::in_memory().unwrap();
        let key = store.enroll_owner("Research host").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(key.node_id, owner).unwrap();
        let hub = store.connect(&key.raw_key).unwrap();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Researcher".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(key.node_id),
                capability_policy_ref: "no-tools".into(),
                provider_account_ref: None,
                memory_namespace: "research".into(),
            })
            .unwrap();
        let room = store
            .bots_conversations_create(NewConversation {
                title: Some("Research".into()),
                owner,
                kind: ConversationKind::AgentDm,
                project_id: None,
                coordinator: None,
                storage_scope: StorageScope::LocalOnly,
            })
            .unwrap();
        for actor in [Principal::User(owner), Principal::Agent(agent.id)] {
            store.bots_conversations_join(actor, room.id).unwrap();
        }
        let reply = store
            .bots_message_send(
                Principal::Agent(agent.id),
                room.id,
                "reply".into(),
                room.policy_revision,
                vec![],
                NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some("Source claim https://example.com, not verified.".into()),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: Some("turn-test".into()),
                    source_event_ref: None,
                },
            )
            .unwrap();
        let vault = store.vault_create("Research notes").unwrap();
        store.vault_set_available(vault, true).unwrap();
        store.vault_grant(vault, key.node_id, true).unwrap();
        (store, hub, reply.id, vault, key.node_id)
    }

    #[test]
    fn explicit_save_retains_origin_indexes_content_and_deduplicates_without_overwrite() {
        let (s, h, m, v, _) = fixture();
        let doc = h.bots_library_save(m, v, "Research result").unwrap();
        assert!(doc.content.contains(&m.to_string()));
        assert!(doc.content.contains("turn-test"));
        assert!(doc.content.contains("not independently verified research"));
        assert!(doc.content.contains("Source claim https://example.com"));
        assert_eq!(h.vault_search(v, "Source", 10).unwrap().len(), 1);
        assert_eq!(
            h.bots_library_save(m, v, "Changed title on retry")
                .unwrap()
                .id,
            doc.id
        );
        s.vault_put(v, doc.id, &doc.path, "Human edit", "Reviewed by a human")
            .unwrap();
        let retry = h.bots_library_save(m, v, "Retry").unwrap();
        assert_eq!(retry.title, "Human edit");
        assert_eq!(retry.content, "Reviewed by a human");
        assert_eq!(
            s.transaction(|tx| tx
                .query_row("SELECT count(*) FROM bots_library_saves", [], |r| r
                    .get::<_, i64>(0))
                .map_err(db_error))
                .unwrap(),
            1
        );
        // Removal cleans the receipt, so a later explicit save can make a fresh copy.
        s.vault_remove_document(v, doc.id).unwrap();
        assert_ne!(h.bots_library_save(m, v, "Fresh copy").unwrap().id, doc.id);
        s.vault_delete(v).unwrap();
    }

    #[test]
    fn revoked_collection_and_foreign_owner_are_rejected_even_on_retry() {
        let (s, h, m, v, node) = fixture();
        h.bots_library_save(m, v, "Result").unwrap();
        s.vault_grant(v, node, false).unwrap();
        assert!(h.bots_library_collections().unwrap().is_empty());
        assert!(h.bots_library_save(m, v, "Retry").is_err());
        s.vault_grant(v, node, true).unwrap();
        s.transaction(|tx| {
            tx.execute(
                "UPDATE conversations SET owner=?1",
                [Uuid::new_v4().to_string()],
            )
            .map_err(db_error)?;
            Ok(())
        })
        .unwrap();
        assert!(h.bots_library_save(m, v, "Foreign source").is_err());
    }

    #[test]
    fn folder_indexes_unavailable_collections_and_removed_replies_are_rejected() {
        let (s, h, m, v, _) = fixture();
        s.vault_set_available(v, false).unwrap();
        assert!(h.bots_library_save(m, v, "Unavailable").is_err());
        s.vault_set_available(v, true).unwrap();
        s.transaction(|tx| {tx.execute("INSERT INTO vault_sources(vault_id,root,root_identity) VALUES(?1,'fixture','fixture')",[v.to_string()]).map_err(db_error)?;Ok(())}).unwrap();
        assert!(h.bots_library_collections().unwrap().is_empty());
        assert!(h.bots_library_save(m, v, "Folder").is_err());
        let owner = h.bots_owner().unwrap();
        s.transaction(|tx| {tx.execute("DELETE FROM vault_sources WHERE vault_id=?1",[v.to_string()]).map_err(db_error)?;tx.execute("INSERT INTO message_revisions(id,original_id,revision_kind,new_body,author_kind,author_id,created_at) VALUES(?1,?2,'tombstone',NULL,'user',?3,0)",params![Uuid::new_v4().to_string(),m.to_string(),owner.to_string()]).map_err(db_error)?;Ok(())}).unwrap();
        assert!(h.bots_library_save(m, v, "Removed").is_err());
    }

    #[test]
    fn concurrent_saves_return_one_document_and_revoked_identity_cannot_retry() {
        let (s, h, m, v, node) = fixture();
        let other = h.clone();
        let first = std::thread::spawn(move || other.bots_library_save(m, v, "First").unwrap());
        let second = h.bots_library_save(m, v, "Second").unwrap();
        assert_eq!(first.join().unwrap().id, second.id);
        s.revoke(node).unwrap();
        assert!(matches!(
            h.bots_library_save(m, v, "Revoked"),
            Err(HubError::BadKey)
        ));
    }

    #[test]
    fn lost_conversation_membership_and_user_text_cannot_be_saved() {
        let (s, h, m, v, _) = fixture();
        h.bots_library_save(m, v, "Reply").unwrap();
        s.transaction(|tx| {
            tx.execute(
                "DELETE FROM conversation_members WHERE principal_kind='user'",
                [],
            )
            .map_err(db_error)?;
            Ok(())
        })
        .unwrap();
        assert!(h.bots_library_save(m, v, "Retry after removal").is_err());
        let (s, h, m, v, _) = fixture();
        s.transaction(|tx| {
            tx.execute(
                "UPDATE messages SET author_kind='user' WHERE id=?1",
                [m.to_string()],
            )
            .map_err(db_error)?;
            Ok(())
        })
        .unwrap();
        assert!(h.bots_library_save(m, v, "User message").is_err());
    }

    #[tokio::test]
    async fn remote_save_uses_selected_authority_and_retry_returns_same_note() {
        let (s, h, m, v, _) = fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve(s.clone(), listener, std::future::pending()));
        let key = RemoteLocalHub::pair(&url, &s.pairing_code().unwrap(), "Secondary")
            .await
            .unwrap();
        s.set_node_owner(key.node_id, h.bots_owner().unwrap())
            .unwrap();
        s.vault_grant(v, key.node_id, true).unwrap();
        let client = RemoteLocalHub::new(&url, key.raw_key).unwrap();
        assert_eq!(client.bots_library_collections().await.unwrap().len(), 1);
        let saved = client
            .bots_library_save(m, v, "Remote result")
            .await
            .unwrap();
        assert_eq!(
            client.bots_library_save(m, v, "Retry").await.unwrap().id,
            saved.id
        );
        assert_eq!(
            h.vault_read(v, saved.id, &saved.revision).unwrap().content,
            saved.content
        );
        s.vault_grant(v, key.node_id, false).unwrap();
        assert!(client.bots_library_save(m, v, "Revoked").await.is_err());
        server.abort();
    }

    #[test]
    fn insertion_conflict_rolls_back_note_and_receipt() {
        let (s, h, m, v, _) = fixture();
        s.vault_put(
            v,
            Uuid::new_v4(),
            &format!("agent-replies/{m}.md"),
            "Keep",
            "Original human note",
        )
        .unwrap();
        assert!(h.bots_library_save(m, v, "New").is_err());
        assert_eq!(
            s.transaction(|tx| tx
                .query_row("SELECT count(*) FROM bots_library_saves", [], |r| r
                    .get::<_, i64>(0))
                .map_err(db_error))
                .unwrap(),
            0
        );
        assert_eq!(h.vault_search(v, "Original", 10).unwrap().len(), 1);
    }
}
