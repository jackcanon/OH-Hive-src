//! Atomic owner-scoped room creation. Receipt survives reconnect/retry and roster edits.
use super::*;
impl LocalHubStore {
    pub fn bots_rooms_create(
        &self,
        request_id: Uuid,
        mut draft: NewConversation,
        mut agents: Vec<AgentId>,
    ) -> Result<Conversation> {
        let title = draft.title.as_deref().unwrap_or("").trim();
        if request_id.is_nil()
            || title.is_empty()
            || title.len() > 200
            || agents.is_empty()
            || agents.len() > 16
            || draft.storage_scope != StorageScope::LocalOnly
            || draft.kind == ConversationKind::AgentDm
            || (draft.kind == ConversationKind::Project) != draft.project_id.is_some()
        {
            return Err(rejected("invalid room: choose a title and 1–16 agents"));
        }
        draft.title = Some(title.into());
        agents.sort_unstable();
        agents.dedup();
        if draft.coordinator.is_some_and(|id| !agents.contains(&id)) {
            return Err(rejected("coordinator must be a selected room member"));
        }
        let fingerprint = encode(&(
            draft.title.clone(),
            conversation_kind_to_str(draft.kind),
            draft.project_id,
            draft.coordinator,
            &agents,
        ))?;
        self.transaction(|tx| {
            let receipt: Option<(String, String)> = tx.query_row("SELECT payload,response FROM bots_room_create_receipts WHERE owner=?1 AND request_id=?2", params![draft.owner.to_string(),request_id.to_string()], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
            if let Some((original, response)) = receipt {
                if original != fingerprint { return Err(rejected("room request ID already used with different details")); }
                return decode(&response);
            }
            // Validate inside the write transaction: archive/owner changes cannot race insertion.
            for agent in &agents {
                let allowed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1 AND owner=?2 AND archived=0)", params![agent.to_string(),draft.owner.to_string()], |r| r.get(0)).map_err(db_error)?;
                if !allowed { return Err(rejected("room agent unavailable or belongs to another account")); }
            }
            let id = Uuid::new_v4(); let ts = now();
            tx.execute("INSERT INTO conversations(id,owner,kind,project_id,coordinator,storage_scope,policy_revision,created_at,title) VALUES(?1,?2,?3,?4,?5,'local_only',1,?6,?7)", params![id.to_string(),draft.owner.to_string(),conversation_kind_to_str(draft.kind),draft.project_id.map(|v|v.to_string()),draft.coordinator.map(|v|v.to_string()),ts,draft.title]).map_err(db_error)?;
            tx.execute("INSERT INTO conversation_members VALUES(?1,'user',?2,?3,?4,?4)", params![id.to_string(),draft.owner.to_string(),encode(&vec![MemberAction::Read,MemberAction::Post,MemberAction::Manage])?,ts]).map_err(db_error)?;
            for agent in &agents {
                tx.execute("INSERT INTO conversation_members VALUES(?1,'agent',?2,?3,?4,?4)", params![id.to_string(),agent.to_string(),encode(&vec![MemberAction::Read,MemberAction::Post])?,ts]).map_err(db_error)?;
            }
            let room = Conversation { id, owner:draft.owner, kind:draft.kind, project_id:draft.project_id, coordinator:draft.coordinator, storage_scope:StorageScope::LocalOnly, policy_revision:1, created_at:from_unix(ts)?, title:draft.title.clone() };
            tx.execute("INSERT INTO bots_room_create_receipts(owner,request_id,conversation_id,payload,response) VALUES(?1,?2,?3,?4,?5)", params![draft.owner.to_string(),request_id.to_string(),id.to_string(),fingerprint,encode(&room)?]).map_err(db_error)?;
            Ok(room)
        })
    }
}
impl LocalHub {
    pub fn bots_rooms_create(
        &self,
        request_id: Uuid,
        mut draft: NewConversation,
        agents: Vec<AgentId>,
    ) -> Result<Conversation> {
        draft.owner = self.bots_owner()?;
        self.store.bots_rooms_create(request_id, draft, agents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn agent(s: &LocalHubStore, owner: Uuid) -> AgentProfile {
        s.bots_agents_create(NewAgentProfile {
            owner,
            name: "Helper".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: Uuid::new_v4().to_string(),
        })
        .unwrap()
    }
    fn draft(owner: Uuid) -> NewConversation {
        NewConversation {
            title: Some(" Team ".into()),
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        }
    }
    #[test]
    fn retries_bind_original_details_and_do_not_reapply_roster() {
        let s = LocalHubStore::in_memory().unwrap();
        let owner = Uuid::new_v4();
        let a = agent(&s, owner);
        let b = agent(&s, owner);
        let key = Uuid::new_v4();
        let room = s
            .bots_rooms_create(key, draft(owner), vec![a.id, b.id])
            .unwrap();
        assert_eq!(
            s.bots_rooms_create(key, draft(owner), vec![b.id, a.id, a.id])
                .unwrap(),
            room
        );
        let mut changed = draft(owner);
        changed.title = Some("Other".into());
        assert!(s.bots_rooms_create(key, changed, vec![a.id, b.id]).is_err());
        s.bots_agents_archive(owner, a.id).unwrap();
        assert_eq!(
            s.bots_rooms_create(key, draft(owner), vec![a.id, b.id])
                .unwrap()
                .id,
            room.id
        );
        assert_eq!(
            s.bots_conversations_list(Principal::User(owner))
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn failed_members_and_injected_mid_write_error_roll_back_everything() {
        let s = LocalHubStore::in_memory().unwrap();
        let owner = Uuid::new_v4();
        let a = agent(&s, owner);
        let other = agent(&s, Uuid::new_v4());
        assert!(s
            .bots_rooms_create(Uuid::new_v4(), draft(owner), vec![a.id, other.id])
            .is_err());
        s.transaction(|tx| { tx.execute_batch("CREATE TRIGGER fail_room_member BEFORE INSERT ON conversation_members WHEN NEW.principal_kind='agent' BEGIN SELECT RAISE(ABORT,'test failure'); END;").map_err(db_error) }).unwrap();
        let key = Uuid::new_v4();
        assert!(s.bots_rooms_create(key, draft(owner), vec![a.id]).is_err());
        assert!(s
            .bots_conversations_list(Principal::User(owner))
            .unwrap()
            .is_empty());
        s.transaction(|tx| {
            tx.execute_batch("DROP TRIGGER fail_room_member")
                .map_err(db_error)
        })
        .unwrap();
        let room = s.bots_rooms_create(key, draft(owner), vec![a.id]).unwrap();
        assert_eq!(
            s.bots_room_agents(Principal::User(owner), room.id)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn simultaneous_retries_and_reopen_return_one_room() {
        let path = std::env::temp_dir().join(format!("room-receipt-{}.sqlite", Uuid::new_v4()));
        let s = LocalHubStore::open(&path).unwrap();
        let owner = Uuid::new_v4();
        let a = agent(&s, owner);
        let key = Uuid::new_v4();
        let s2 = LocalHubStore::open(&path).unwrap();
        let aid = a.id;
        let thread =
            std::thread::spawn(move || s2.bots_rooms_create(key, draft(owner), vec![aid]).unwrap());
        let room = s.bots_rooms_create(key, draft(owner), vec![a.id]).unwrap();
        assert_eq!(thread.join().unwrap().id, room.id);
        drop(s);
        let reopened = LocalHubStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .bots_rooms_create(key, draft(owner), vec![a.id])
                .unwrap()
                .id,
            room.id
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn remote_retries_bind_paired_owner_and_reject_revoked_credentials() {
        let s = LocalHubStore::in_memory().unwrap();
        let owner = Uuid::new_v4();
        let a = agent(&s, owner);
        let credentials = s.enroll_owner("room test").unwrap();
        s.set_node_owner(credentials.node_id, owner).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = RemoteLocalHub::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            credentials.raw_key,
        )
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve(s.clone(), listener, async {
            let _ = rx.await;
        }));
        let key = Uuid::new_v4();
        let forged = Uuid::new_v4();
        let room = client
            .bots_rooms_create(key, draft(forged), vec![a.id])
            .await
            .unwrap();
        assert_eq!(room.owner, owner);
        assert_eq!(
            client
                .bots_rooms_create(key, draft(forged), vec![a.id])
                .await
                .unwrap()
                .id,
            room.id
        );
        let foreign = agent(&s, forged);
        assert!(client
            .bots_rooms_create(Uuid::new_v4(), draft(forged), vec![foreign.id])
            .await
            .is_err());
        s.revoke(credentials.node_id).unwrap();
        assert!(client
            .bots_rooms_create(key, draft(owner), vec![a.id])
            .await
            .is_err());
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
    #[test]
    fn migration_from_twelve_preserves_existing_rooms() {
        let s = LocalHubStore::in_memory().unwrap();
        let owner = Uuid::new_v4();
        let a = agent(&s, owner);
        let room = s.bots_conversations_create(draft(owner)).unwrap();
        let db = Arc::try_unwrap(s.db).unwrap().into_inner().unwrap();
        crate::local_hub::rewind_to(&db, 12, "DROP TABLE private_coding_readiness; DROP TABLE private_preparation_recoveries; DROP TABLE private_run_retries; DROP TABLE private_run_stops; DROP TABLE private_runs; DROP TABLE private_preparations; ALTER TABLE agent_deliveries DROP COLUMN lease_deadline; DROP TABLE project_repositories; DROP TABLE bots_room_create_receipts;");
        let migrated = LocalHubStore::from_connection(db).unwrap();
        assert_eq!(
            migrated
                .bots_conversations_list(Principal::User(owner))
                .unwrap()[0]
                .id,
            room.id
        );
        migrated
            .bots_rooms_create(Uuid::new_v4(), draft(owner), vec![a.id])
            .unwrap();
    }
}
