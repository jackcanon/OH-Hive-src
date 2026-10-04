use super::*;
use crate::bots::*;
use std::future::IntoFuture;

fn fixture() -> (LocalHubStore, Uuid, Uuid, Uuid, Uuid) {
    fixture_with(LocalHubStore::in_memory().unwrap())
}
fn fixture_with(s: LocalHubStore) -> (LocalHubStore, Uuid, Uuid, Uuid, Uuid) {
    let owner = Uuid::new_v4();
    let agent = s
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "External collaborator".into(),
            runtime_kind: AgentRuntimeKind::ChatgptSubscription,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: Uuid::new_v4().to_string(),
        })
        .unwrap()
        .id;
    let project = s.create_project("Real project", "Shared goal").unwrap();
    let room = s
        .bots_rooms_create(
            Uuid::new_v4(),
            NewConversation {
                owner,
                title: Some("Project room".into()),
                kind: ConversationKind::Project,
                project_id: Some(project),
                coordinator: None,
                storage_scope: StorageScope::LocalOnly,
            },
            vec![agent],
        )
        .unwrap()
        .id;
    let other = s
        .bots_rooms_create(
            Uuid::new_v4(),
            NewConversation {
                owner,
                title: Some("Unshared project".into()),
                kind: ConversationKind::Project,
                project_id: Some(project),
                coordinator: None,
                storage_scope: StorageScope::LocalOnly,
            },
            vec![agent],
        )
        .unwrap()
        .id;
    (s, owner, agent, room, other)
}
fn post(room: Uuid) -> Value {
    json!({"room_id":room,"request_id":Uuid::new_v4(),"policy_revision":1,"body":"Research finding with supporting evidence"})
}

#[test]
fn grant_never_expands_membership_and_denies_non_project_rooms() {
    let (s, owner, agent, room, _) = fixture();
    assert!(s
        .project_connector_grant(Uuid::new_v4(), agent, vec![room], true, 60)
        .is_err());
    assert!(s
        .project_connector_grant(owner, agent, vec![], true, 60)
        .is_err());
    assert!(s
        .project_connector_grant(owner, agent, vec![room], true, MAX_TTL + 1)
        .is_err());
    let dm = s
        .bots_conversations_create(NewConversation {
            owner,
            title: None,
            kind: ConversationKind::AgentDm,
            project_id: None,
            coordinator: Some(agent),
            storage_scope: StorageScope::LocalOnly,
        })
        .unwrap();
    assert!(s
        .project_connector_grant(owner, agent, vec![dm.id], true, 60)
        .is_err());
    s.transaction(|tx| {
        tx.execute(
            "DELETE FROM conversation_members WHERE conversation_id=?1 AND principal_kind='agent'",
            [room.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    assert!(s
        .project_connector_grant(owner, agent, vec![room], true, 60)
        .is_err());
}
#[test]
fn updates_are_attributed_atomic_idempotent_and_do_not_run_agents() {
    let (s, owner, agent, room, other) = fixture();
    let grant = s
        .project_connector_grant(owner, agent, vec![room], true, 60)
        .unwrap();
    let args = post(room);
    let first = s
        .project_connector_call(&grant.bearer_token, "post_project_update", args.clone())
        .unwrap();
    assert_eq!(first["author_id"], agent.to_string());
    assert_eq!(
        s.project_connector_call(&grant.bearer_token, "post_project_update", args.clone())
            .unwrap(),
        first
    );
    let mut changed = args;
    changed["body"] = json!("Changed retry");
    assert!(s
        .project_connector_call(&grant.bearer_token, "post_project_update", changed)
        .is_err());
    assert!(s
        .project_connector_call(&grant.bearer_token, "post_project_update", post(other))
        .is_err());
    let mut stale = post(room);
    stale["policy_revision"] = json!(2);
    assert!(s
        .project_connector_call(&grant.bearer_token, "post_project_update", stale)
        .is_err());
    let messages = s
        .bots_messages_list(
            Principal::User(owner),
            room,
            MessagePage {
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].author, Principal::Agent(agent));
    s.transaction(|tx| {
        let n: i64 = tx
            .query_row("SELECT COUNT(*) FROM agent_deliveries", [], |r| r.get(0))
            .map_err(db_error)?;
        assert_eq!(n, 0);
        Ok(())
    })
    .unwrap();
    let updates = s
        .project_connector_call(
            &grant.bearer_token,
            "read_project_updates",
            json!({"room_id":room}),
        )
        .unwrap();
    assert_eq!(updates["updates"].as_array().unwrap().len(), 1);
    let next = updates["next_sequence"].clone();
    assert_eq!(
        s.project_connector_call(
            &grant.bearer_token,
            "read_project_updates",
            json!({"room_id":room,"after_sequence":next})
        )
        .unwrap()["updates"],
        json!([])
    );
}
#[test]
fn read_only_revocation_expiry_archive_and_membership_are_live_checks() {
    let (s, owner, agent, room, other) = fixture();
    let g = s
        .project_connector_grant(owner, agent, vec![room], false, 60)
        .unwrap();
    assert!(s
        .project_connector_call(&g.bearer_token, "post_project_update", post(room))
        .is_err());
    assert!(s
        .project_connector_call(
            &g.bearer_token,
            "read_project_updates",
            json!({"room_id":other})
        )
        .is_err());
    assert!(s
        .project_connector_call(
            &g.bearer_token,
            "read_project_updates",
            json!({"room_id":room,"actor":owner})
        )
        .is_err());
    assert!(s
        .project_connector_call(&g.bearer_token, "private_run_request", json!({}))
        .is_err());
    assert!(s
        .project_connector_revoke(Uuid::new_v4(), g.grant_id)
        .is_err());
    s.project_connector_revoke(owner, g.grant_id).unwrap();
    assert!(matches!(
        s.project_connector_call(&g.bearer_token, "list_project_rooms", json!({})),
        Err(HubError::BadKey)
    ));
    let g = s
        .project_connector_grant(owner, agent, vec![room], false, 60)
        .unwrap();
    s.transaction(|tx| {
        tx.execute(
            "UPDATE project_connector_grants SET expires_at=?1 WHERE id=?2",
            params![now() - 1, g.grant_id.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        s.project_connector_call(&g.bearer_token, "list_project_rooms", json!({})),
        Err(HubError::BadKey)
    ));
    let g = s
        .project_connector_grant(owner, agent, vec![room], false, 60)
        .unwrap();
    s.transaction(|tx| {
        tx.execute(
            "DELETE FROM conversation_members WHERE conversation_id=?1 AND principal_kind='agent'",
            [room.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    assert!(s
        .project_connector_call(
            &g.bearer_token,
            "read_project_updates",
            json!({"room_id":room})
        )
        .is_err());
    s.bots_agents_archive(owner, agent).unwrap();
    assert!(matches!(
        s.project_connector_call(&g.bearer_token, "list_project_rooms", json!({})),
        Err(HubError::BadKey)
    ));
}

#[tokio::test]
async fn http_protocol_auth_and_tool_permissions_use_real_transport() {
    let (s, owner, agent, room, _) = fixture();
    let g = s
        .project_connector_grant(owner, agent, vec![room], false, 60)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(axum::serve(listener, router(s.clone())).into_future());
    let client = reqwest::Client::new();
    let req = |payload: Value| {
        client
            .post(&url)
            .bearer_auth(&g.bearer_token)
            .header("accept", "application/json, text/event-stream")
            .json(&payload)
    };
    assert_eq!(
        client
            .post(&url)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}});
    assert_eq!(
        req(init.clone())
            .header("origin", "https://untrusted.example")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(init.clone())
            .header("mcp-protocol-version", "invalid")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        req(init)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["result"]["protocolVersion"],
        "2025-06-18"
    );
    let list = req(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 2);
    let deny=req(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"post_project_update","arguments":post(room)}})).send().await.unwrap().json::<Value>().await.unwrap();
    assert_eq!(deny["result"]["isError"], true);
    assert_eq!(
        client.get(&url).send().await.unwrap().status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        req(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    s.project_connector_revoke(owner, g.grant_id).unwrap();
    assert_eq!(
        req(json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    server.abort();
}

#[test]
fn token_hash_only_and_membership_history_boundary_are_preserved() {
    let (s, owner, agent, room, _) = fixture();
    let g = s
        .project_connector_grant(owner, agent, vec![room], true, 60)
        .unwrap();
    s.project_connector_call(&g.bearer_token, "post_project_update", post(room))
        .unwrap();
    s.transaction(|tx| {
        let hash: String = tx
            .query_row(
                "SELECT key_hash FROM project_connector_grants WHERE id=?1",
                [g.grant_id.to_string()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        assert_ne!(hash, g.bearer_token);
        tx.execute(
            "UPDATE messages SET created_at=?1 WHERE conversation_id=?2",
            params![now() - 10, room.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    let data = s
        .project_connector_call(
            &g.bearer_token,
            "read_project_updates",
            json!({"room_id":room}),
        )
        .unwrap();
    assert_eq!(data["updates"], json!([]));
}

#[test]
fn restart_preserves_grants_and_atomic_retry_receipts() {
    let directory = std::env::temp_dir().join(format!("den-connector-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("primary.sqlite");
    let (s, owner, agent, room, _) = fixture_with(LocalHubStore::open(&path).unwrap());
    let g = s
        .project_connector_grant(owner, agent, vec![room], true, 600)
        .unwrap();
    let args = post(room);
    let receipt = s
        .project_connector_call(&g.bearer_token, "post_project_update", args.clone())
        .unwrap();
    drop(s);
    let reopened = LocalHubStore::open(&path).unwrap();
    assert_eq!(
        reopened
            .project_connector_call(&g.bearer_token, "post_project_update", args)
            .unwrap(),
        receipt
    );
    assert_eq!(
        reopened
            .bots_messages_list(
                Principal::User(owner),
                room,
                MessagePage {
                    limit: 10,
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
        1
    );
    reopened
        .project_connector_revoke(owner, g.grant_id)
        .unwrap();
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}
