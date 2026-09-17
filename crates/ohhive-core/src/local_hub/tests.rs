use super::*;

#[test]
fn project_repository_defaults_are_snapshotted_and_explicit_locations_win() {
    use repository::ProjectRepository;
    let s = LocalHubStore::in_memory().unwrap();
    let p = s.create_project("Repository", "fixture").unwrap();
    let binding = ProjectRepository {
        repo_url: "https://github.com/example/fixture.git".into(),
        repo_ref: Some("main".into()),
    };
    s.set_project_repository(p, Some(&binding)).unwrap();
    assert_eq!(s.project_repository(p).unwrap(), Some(binding.clone()));
    let projects = s.repository_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, p);
    assert_eq!(projects[0].repository, Some(binding.clone()));
    for (key, caps) in [
        ("default", json!({"brain":"local"})),
        (
            "folder",
            json!({"brain":"local", "workspace_path":"/fixture"}),
        ),
        (
            "explicit",
            json!({"brain":"local", "repo_url":"https://github.com/other/repo"}),
        ),
    ] {
        let mut c = card(p, key);
        c.modality = "code".into();
        c.required_capabilities = caps;
        s.add_card(c).unwrap();
    }
    s.set_project_repository(p, None).unwrap();
    assert_eq!(s.project_repository(p).unwrap(), None);
    s.transaction(|tx| {
        for (key, expected, internet) in [
            ("default", Some(binding.repo_url.as_str()), true),
            ("folder", None, false),
            ("explicit", Some("https://github.com/other/repo"), true),
        ] {
            let raw: String = tx
                .query_row("SELECT data FROM cards WHERE key=?1", [key], |r| r.get(0))
                .unwrap();
            let c: ClaimedCard = decode(&raw).unwrap();
            assert_eq!(
                c.required_capabilities
                    .get("repo_url")
                    .and_then(Value::as_str),
                expected
            );
            assert_eq!(c.requires_internet, internet);
            if key == "default" {
                assert_eq!(c.required_capabilities["repo_ref"], "main");
            }
        }
        Ok(())
    })
    .unwrap();
    assert!(s
        .set_project_repository(Uuid::new_v4(), Some(&binding))
        .is_err());
    for url in [
        "https://token@github.com/o/r",
        "https://github.com/o/r?token=secret",
        "https://other.test/o/r",
        "https://github.com/../r",
    ] {
        assert!(s
            .set_project_repository(
                p,
                Some(&ProjectRepository {
                    repo_url: url.into(),
                    repo_ref: None
                })
            )
            .is_err());
    }
}

#[test]
fn project_repository_migration_preserves_existing_projects() {
    let s = LocalHubStore::in_memory().unwrap();
    let p = s.create_project("Existing", "keep").unwrap();
    let db = Arc::try_unwrap(s.db).ok().unwrap().into_inner().unwrap();
    db.execute_batch("DROP TABLE project_repositories; PRAGMA user_version=13;")
        .unwrap();
    let s = LocalHubStore::from_connection(db).unwrap();
    assert_eq!(s.project_repository(p).unwrap(), None);
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: "https://github.com/example/repo".into(),
            repo_ref: None,
        }),
    )
    .unwrap();
}
fn caps() -> Capabilities {
    serde_json::from_value(json!({"hardware":{"cpu_model":"fixture","cpu_cores":4,"ram_bytes":16000000000u64,"gpu_vendor":"none","disk_free_bytes":1000000000},"modalities":["text","code"],"models":[],"allow_internet":false,"tools_level":"sandboxed_tools"})).unwrap()
}
fn card(project: Uuid, key: &str) -> ClaimedCard {
    ClaimedCard {
        id: Uuid::new_v4(),
        project_id: project,
        key: key.into(),
        title: key.into(),
        modality: "text".into(),
        inputs: "synthetic local-only task".into(),
        acceptance: "return an answer".into(),
        deps: vec![],
        requires_internet: false,
        required_capabilities: json!({"loop":"single"}),
    }
}
async fn fixture() -> (LocalHubStore, LocalHub, LocalHub, Uuid) {
    let store = LocalHubStore::in_memory().unwrap();
    let a = store.enroll_owner("a").unwrap();
    let b = store.enroll_owner("b").unwrap();
    let ha = store.connect(&a.raw_key).unwrap();
    let hb = store.connect(&b.raw_key).unwrap();
    ha.check_in(&caps(), None).await.unwrap();
    hb.check_in(&caps(), None).await.unwrap();
    let p = store.create_project("Fixture", "local only").unwrap();
    (store, ha, hb, p)
}
fn id(claim: Claim) -> Uuid {
    if let Claim::Leased { card, .. } = claim {
        card.id
    } else {
        panic!("expected lease")
    }
}
#[tokio::test]
async fn pairing_is_single_use_bounded_expiring_and_hash_only() {
    let store = LocalHubStore::in_memory().unwrap();
    let code = store.pairing_code().unwrap();
    let c = store.redeem_pairing(&code, "second").unwrap();
    assert_eq!(c.raw_key.len(), 56);
    assert!(store.redeem_pairing(&code, "third").is_err());
    store
        .transaction(|tx| {
            let hash: String = tx
                .query_row("SELECT hash FROM local_node_keys", [], |r| r.get(0))
                .unwrap();
            assert_eq!(hash, digest(&c.raw_key));
            assert_ne!(hash, c.raw_key);
            Ok(())
        })
        .unwrap();
    store.connect(&c.raw_key).unwrap();
    store.revoke(c.node_id).unwrap();
    assert!(matches!(store.connect(&c.raw_key), Err(HubError::BadKey)));
    let code = store.pairing_code().unwrap();
    for _ in 0..5 {
        assert!(store.redeem_pairing("not-a-code", "bad").is_err())
    }
    assert!(store.redeem_pairing(&code, "late").is_err());
    let code = store.pairing_code().unwrap();
    store
        .transaction(|tx| {
            tx.execute("UPDATE pairing SET expires=0", []).unwrap();
            Ok(())
        })
        .unwrap();
    assert!(store.redeem_pairing(&code, "late").is_err());
}
#[tokio::test]
async fn one_claim_and_lease_owner_required() {
    let (s, a, b, p) = fixture().await;
    let c = card(p, "one");
    s.add_card(c.clone()).unwrap();
    let (x, y) = tokio::join!(a.claim_card(), b.claim_card());
    let x = x.unwrap();
    let y = y.unwrap();
    let (owner, other) = if matches!(x, Claim::Leased { .. }) {
        assert!(matches!(y, Claim::NothingToDo));
        (&a, &b)
    } else {
        assert!(matches!(x, Claim::NothingToDo));
        assert!(matches!(y, Claim::Leased { .. }));
        (&b, &a)
    };
    assert!(other
        .complete_card(c.id, "stolen", None, Usage::default())
        .await
        .is_err());
    assert!(other.fail_card(c.id, "stolen").await.is_err());
    assert!(other
        .checkpoint(c.id, 1, &json!({}), Usage::default())
        .await
        .is_err());
    assert!(matches!(
        owner.claim_card().await.unwrap(),
        Claim::AlreadyLeased
    ));
    let done = owner
        .complete_card(c.id, "done", None, Usage::default())
        .await
        .unwrap();
    assert_eq!(done.earned_honey, 0.0);
    owner
        .complete_card(c.id, "done", None, Usage::default())
        .await
        .unwrap();
    assert!(owner
        .complete_card(c.id, "changed", None, Usage::default())
        .await
        .is_err());
    assert_eq!(s.inspect().unwrap()["outputs"].as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn dependencies_checkpoints_and_handoff() {
    let (s, a, b, p) = fixture().await;
    let first = card(p, "first");
    let mut second = card(p, "second");
    second.deps = vec!["first".into()];
    s.add_card(second.clone()).unwrap();
    s.add_card(first.clone()).unwrap();
    assert_eq!(id(a.claim_card().await.unwrap()), first.id);
    a.checkpoint(first.id, 2, &json!({"phase":"draft"}), Usage::default())
        .await
        .unwrap();
    a.release_card(first.id, "moving").await.unwrap();
    let claimed = b.claim_card().await.unwrap();
    match claimed {
        Claim::Leased {
            card, checkpoint, ..
        } => {
            assert_eq!(card.id, first.id);
            assert_eq!(checkpoint.unwrap().step, 2)
        }
        _ => panic!(),
    };
    assert!(a
        .complete_card(first.id, "stale", None, Usage::default())
        .await
        .is_err());
    b.complete_card(first.id, "dependency result", None, Usage::default())
        .await
        .unwrap();
    match a.claim_card().await.unwrap() {
        Claim::Leased {
            card, dep_outputs, ..
        } => {
            assert_eq!(card.id, second.id);
            assert_eq!(dep_outputs["first"]["content"], "dependency result")
        }
        _ => panic!(),
    }
}
#[tokio::test]
async fn expired_sessions_cannot_write_and_code_is_not_replayed() {
    let (s, a, b, p) = fixture().await;
    let mut c = card(p, "code");
    c.modality = "code".into();
    s.add_card(c.clone()).unwrap();
    id(a.claim_card().await.unwrap());
    s.transaction(|tx| {
        tx.execute("UPDATE leases SET expires=0", []).unwrap();
        Ok(())
    })
    .unwrap();
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    assert_eq!(s.inspect().unwrap()["cards"][0]["status"], "blocked");
    assert!(a
        .complete_card(c.id, "late", None, Usage::default())
        .await
        .is_err());
}
#[tokio::test]
async fn child_wait_completion_and_failure() {
    let (s, a, b, p) = fixture().await;
    let parent = card(p, "parent");
    s.add_card(parent.clone()).unwrap();
    id(a.claim_card().await.unwrap());
    let ch = a
        .spawn_child_card(
            parent.id,
            "child",
            "child",
            "text",
            "task",
            "done",
            json!({}),
        )
        .await
        .unwrap();
    let same = a
        .spawn_child_card(
            parent.id,
            "child",
            "child",
            "text",
            "task",
            "done",
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(ch.card_id, same.card_id);
    assert!(a.wait_on_child(parent.id, Uuid::new_v4()).await.is_err());
    a.checkpoint(parent.id, 1, &json!({"waiting":"child"}), Usage::default())
        .await
        .unwrap();
    a.wait_on_child(parent.id, ch.card_id).await.unwrap();
    assert_eq!(id(b.claim_card().await.unwrap()), ch.card_id);
    b.complete_card(ch.card_id, "child done", None, Usage::default())
        .await
        .unwrap();
    assert_eq!(id(a.claim_card().await.unwrap()), parent.id);
    a.fail_card(parent.id, "review required").await.unwrap();
    let parent2 = card(p, "parent2");
    s.add_card(parent2.clone()).unwrap();
    id(a.claim_card().await.unwrap());
    let ch = a
        .spawn_child_card(
            parent2.id,
            "child2",
            "child2",
            "text",
            "task",
            "done",
            json!({}),
        )
        .await
        .unwrap();
    a.wait_on_child(parent2.id, ch.card_id).await.unwrap();
    id(b.claim_card().await.unwrap());
    b.fail_card(ch.card_id, "test failure").await.unwrap();
    assert!(matches!(a.claim_card().await.unwrap(), Claim::NothingToDo));
}
#[tokio::test]
async fn capabilities_checkout_mcp_and_revocation() {
    let (s, a, _, p) = fixture().await;
    let mut c = card(p, "internet");
    c.requires_internet = true;
    s.add_card(c).unwrap();
    assert!(matches!(a.claim_card().await.unwrap(), Claim::NothingToDo));
    assert_eq!(a.check_out().await.unwrap(), "checked_out");
    assert!(matches!(a.claim_card().await.unwrap(), Claim::NotCheckedIn));
    let mut cp = caps();
    cp.allow_internet = true;
    a.check_in(&cp, None).await.unwrap();
    id(a.claim_card().await.unwrap());
    assert_eq!(a.check_out().await.unwrap(), "draining");
    a.heartbeat(None).await.unwrap();
    assert!(a.get_schedule().await.unwrap().is_none());
    let cfg = McpServerConfig {
        id: Uuid::new_v4(),
        name: "local fixture".into(),
        transport: "stdio".into(),
        command: "fixture".into(),
        args: vec![],
        env: Default::default(),
    };
    s.configure_mcp(&cfg, true).unwrap();
    assert_eq!(
        a.mcp_server_config(cfg.id).await.unwrap().command,
        "fixture"
    );
    s.configure_mcp(&cfg, false).unwrap();
    assert!(a.mcp_server_config(cfg.id).await.is_err());
    let creds = s.enroll_owner("revoked").unwrap();
    let h = s.connect(&creds.raw_key).unwrap();
    s.revoke(creds.node_id).unwrap();
    assert!(matches!(h.heartbeat(None).await, Err(HubError::BadKey)));
}
#[tokio::test]
async fn local_activity_and_cloud_card_gate() {
    let (s, a, _, p) = fixture().await;
    assert!(a.community_client().is_none());
    a.post_activity("started", "private content", json!({"path":"private"}))
        .await
        .unwrap();
    assert_eq!(s.inspect().unwrap()["activity_count"], 1);
    let mut c = card(p, "cloud");
    c.modality = "code".into();
    c.required_capabilities = json!({"brain":"nous"});
    assert!(s.add_card(c).is_err());
}
#[tokio::test]
async fn file_store_restart_retains_project_checkpoint_and_output() {
    let path = std::env::temp_dir().join(format!("hive-local-{}.sqlite", Uuid::new_v4()));
    let creds;
    let cid;
    {
        let s = LocalHubStore::open(&path).unwrap();
        creds = s.enroll_owner("a").unwrap();
        let a = s.connect(&creds.raw_key).unwrap();
        a.check_in(&caps(), None).await.unwrap();
        let p = s.create_project("persisted", "goal").unwrap();
        let c = card(p, "persisted-card");
        cid = c.id;
        s.add_card(c).unwrap();
        id(a.claim_card().await.unwrap());
        a.checkpoint(cid, 4, &json!({"saved":true}), Usage::default())
            .await
            .unwrap();
        a.release_card(cid, "restart").await.unwrap();
    }
    let s = LocalHubStore::open(&path).unwrap();
    let a = s.connect(&creds.raw_key).unwrap();
    match a.claim_card().await.unwrap() {
        Claim::Leased { checkpoint, .. } => assert_eq!(checkpoint.unwrap().state["saved"], true),
        _ => panic!(),
    };
    a.complete_card(cid, "persisted-output", None, Usage::default())
        .await
        .unwrap();
    drop(a);
    drop(s);
    let s = LocalHubStore::open(&path).unwrap();
    assert_eq!(
        s.inspect().unwrap()["outputs"][0]["content"],
        "persisted-output"
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[tokio::test]
async fn two_http_clients_pair_claim_and_complete_without_cloud() {
    let s = LocalHubStore::in_memory().unwrap();
    let p = s.create_project("HTTP fixture", "no cloud").unwrap();
    let c = card(p, "http");
    s.add_card(c.clone()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (st, rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(s.clone(), listener, async {
        let _ = rx.await;
    }));
    let code = s.pairing_code().unwrap();
    let creds = RemoteLocalHub::pair(&url, &code, "remote-one")
        .await
        .unwrap();
    assert!(RemoteLocalHub::pair(&url, &code, "replay").await.is_err());
    let a = RemoteLocalHub::new(&url, creds.raw_key.clone()).unwrap();
    let code = s.pairing_code().unwrap();
    let other = RemoteLocalHub::pair(&url, &code, "remote-two")
        .await
        .unwrap();
    let b = RemoteLocalHub::new(&url, other.raw_key).unwrap();
    a.check_in(&caps(), None).await.unwrap();
    b.check_in(&caps(), None).await.unwrap();
    assert!(a.community_client().is_none());
    assert_eq!(id(a.claim_card().await.unwrap()), c.id);
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    assert!(b
        .complete_card(c.id, "steal", None, Usage::default())
        .await
        .is_err());
    a.checkpoint(c.id, 1, &json!({"remote":true}), Usage::default())
        .await
        .unwrap();
    a.post_activity("test", "private", json!({})).await.unwrap();
    a.complete_card(c.id, "remote result", None, Usage::default())
        .await
        .unwrap();
    assert_eq!(
        s.inspect().unwrap()["outputs"][0]["content"],
        "remote result"
    );
    s.revoke(creds.node_id).unwrap();
    assert!(matches!(a.heartbeat(None).await, Err(HubError::BadKey)));
    st.send(()).unwrap();
    server.await.unwrap().unwrap();
}
#[tokio::test]
async fn normal_worker_runs_against_local_hub() {
    use crate::backend::mock::MockBackend;
    use crate::worker::Worker;
    let (s, a, _, p) = fixture().await;
    let c = card(p, "worker");
    s.add_card(c).unwrap();
    let cp = caps();
    let backend = MockBackend;
    let (_stop, rx) = tokio::sync::watch::channel(false);
    let worker = Worker {
        capacity_path: std::env::temp_dir()
            .join(format!("hive-worker-test-{}", uuid::Uuid::new_v4())),
        hub: &a,
        backend: &backend,
        caps: &cp,
        default_model: Some("mock".into()),
        stop: rx,
        events: None,
        #[cfg(feature = "sandbox")]
        data_dir: std::env::temp_dir(),
        #[cfg(feature = "sandbox")]
        sandbox: None,
    };
    let held = crate::execution_capacity::try_acquire_at(&worker.capacity_path)
        .unwrap()
        .unwrap();
    assert!(
        !worker.tick().await.unwrap(),
        "busy local turn must prevent card claim"
    );
    assert_eq!(s.inspect().unwrap()["cards"][0]["status"], "ready");
    drop(held);
    worker.tick().await.unwrap();
    assert!(
        crate::execution_capacity::try_acquire_at(&worker.capacity_path)
            .unwrap()
            .is_some()
    );
    std::fs::remove_file(&worker.capacity_path).unwrap();
    assert_eq!(s.inspect().unwrap()["cards"][0]["status"], "review");
    assert_eq!(s.inspect().unwrap()["outputs"].as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn heartbeat_is_not_blocked_by_a_long_running_card() {
    // Regression test for the 2026-09-14 `run_forever` heartbeat fix (ADR-029 review finding):
    // a single long-running card used to delay the node's heartbeat until that card's `tick()`
    // call returned. This drives one card through a `Backend` that sleeps well past several
    // heartbeat intervals and checks the heartbeat still fired repeatedly *during* that single
    // call, not just before/after it.
    use crate::backend::mock::MockBackend;
    use crate::backend::{Backend, BackendError, ChunkStream};
    use crate::hub::{Claim, Completion, Hub, HubError, McpServerConfig, SpawnedCard};
    use crate::worker::Worker;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Delegates every call straight through to `inner`, except `heartbeat`, which it also
    /// counts -- lets this test observe heartbeat cadence without reaching into the store's own
    /// tables.
    struct CountingHub<'a> {
        inner: &'a dyn Hub,
        heartbeats: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl<'a> Hub for CountingHub<'a> {
        async fn claim_card(&self) -> Result<Claim, HubError> {
            self.inner.claim_card().await
        }
        async fn complete_card(
            &self,
            card_id: Uuid,
            content: &str,
            model_id: Option<&str>,
            usage: crate::ledger::Usage,
        ) -> Result<Completion, HubError> {
            self.inner
                .complete_card(card_id, content, model_id, usage)
                .await
        }
        async fn checkpoint(
            &self,
            card_id: Uuid,
            step: u32,
            state: &serde_json::Value,
            usage: crate::ledger::Usage,
        ) -> Result<serde_json::Value, HubError> {
            self.inner.checkpoint(card_id, step, state, usage).await
        }
        async fn fail_card(
            &self,
            card_id: Uuid,
            reason: &str,
        ) -> Result<serde_json::Value, HubError> {
            self.inner.fail_card(card_id, reason).await
        }
        async fn release_card(
            &self,
            card_id: Uuid,
            reason: &str,
        ) -> Result<serde_json::Value, HubError> {
            self.inner.release_card(card_id, reason).await
        }
        async fn spawn_child_card(
            &self,
            parent_card_id: Uuid,
            key: &str,
            title: &str,
            modality: &str,
            inputs: &str,
            acceptance: &str,
            required_capabilities: serde_json::Value,
        ) -> Result<SpawnedCard, HubError> {
            self.inner
                .spawn_child_card(
                    parent_card_id,
                    key,
                    title,
                    modality,
                    inputs,
                    acceptance,
                    required_capabilities,
                )
                .await
        }
        async fn wait_on_child(
            &self,
            card_id: Uuid,
            child_card_id: Uuid,
        ) -> Result<serde_json::Value, HubError> {
            self.inner.wait_on_child(card_id, child_card_id).await
        }
        async fn mcp_server_config(&self, server_id: Uuid) -> Result<McpServerConfig, HubError> {
            self.inner.mcp_server_config(server_id).await
        }
        async fn check_in(
            &self,
            caps: &Capabilities,
            region: Option<&str>,
        ) -> Result<serde_json::Value, HubError> {
            self.inner.check_in(caps, region).await
        }
        async fn heartbeat(&self, prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
            self.heartbeats.fetch_add(1, Ordering::SeqCst);
            self.inner.heartbeat(prev_rtt_ms).await
        }
        async fn check_out(&self) -> Result<String, HubError> {
            self.inner.check_out().await
        }
        async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
            self.inner.get_schedule().await
        }
        async fn post_activity(
            &self,
            event_type: &str,
            body: &str,
            payload: serde_json::Value,
        ) -> Result<(), HubError> {
            self.inner.post_activity(event_type, body, payload).await
        }
    }

    /// A `Backend` that sleeps before responding, standing in for a slow local-model call --
    /// long enough that several heartbeat intervals should fit inside the one `tick()` it's
    /// part of.
    struct SlowBackend(Duration);

    #[async_trait::async_trait]
    impl Backend for SlowBackend {
        fn name(&self) -> &'static str {
            "slow-mock"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn capabilities(&self) -> Result<Capabilities, BackendError> {
            let inner = MockBackend;
            inner.capabilities().await
        }
        async fn run<'a>(
            &'a self,
            job: &'a crate::job::Job,
        ) -> Result<ChunkStream<'a>, BackendError> {
            tokio::time::sleep(self.0).await;
            // Returned stream may borrow its backend; it must outlive this call.
            static INNER: MockBackend = MockBackend;
            INNER.run(job).await
        }
    }

    let (s, a, _, p) = fixture().await;
    s.add_card(card(p, "slow")).unwrap();
    let cp = caps();
    let heartbeats = Arc::new(AtomicUsize::new(0));
    let hub = CountingHub {
        inner: &a,
        heartbeats: heartbeats.clone(),
    };
    let backend = SlowBackend(Duration::from_millis(280));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let worker = Worker {
        capacity_path: std::env::temp_dir()
            .join(format!("hive-worker-test-{}", uuid::Uuid::new_v4())),
        hub: &hub,
        backend: &backend,
        caps: &cp,
        default_model: Some("mock".into()),
        stop: stop_rx,
        events: None,
        #[cfg(feature = "sandbox")]
        data_dir: std::env::temp_dir(),
        #[cfg(feature = "sandbox")]
        sandbox: None,
    };

    // Stop shortly after the one slow card should have finished (short idle tail on purpose --
    // it caps how many *post-completion* heartbeats could pad the count either way).
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(320)).await;
        let _ = stop_tx.send(true);
    });
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        worker.run_forever(Duration::from_millis(20), 1),
    )
    .await;
    assert!(
        result.is_ok(),
        "run_forever did not return within 5s -- possible deadlock in the heartbeat/dispatch select loop"
    );
    result.unwrap().unwrap();

    assert_eq!(s.inspect().unwrap()["cards"][0]["status"], "review");
    // With a 20ms heartbeat interval and a single 280ms-long tick, the old (blocked-until-tick-
    // returns) behavior could land at most ~4 heartbeats in this whole ~320ms run (one at start,
    // one right after the slow tick finally returns, a couple more in the short idle tail).
    // Decoupled, well over that many should land purely from the ticker running independently
    // of the slow card.
    let n = heartbeats.load(Ordering::SeqCst);
    assert!(
        n >= 6,
        "expected several heartbeats to fire during the slow card's single tick, got {n}"
    );
}
#[cfg(feature = "sandbox")]
#[tokio::test]
async fn cloud_brain_fails_before_provider_and_code_receipts_stay_local() {
    use crate::coder::*;
    let (s, a, _, p) = fixture().await;
    let brain = CloudBrain::new(&a, "nous", None);
    assert!(brain
        .next_turn(&[BrainMessage::user("private prompt")], &[])
        .await
        .unwrap_err()
        .to_string()
        .contains("direct-to-provider"));
    struct Fixture;
    #[async_trait::async_trait]
    impl CodeBrain for Fixture {
        async fn next_turn(
            &self,
            _: &[BrainMessage],
            _: &[ToolSpec],
        ) -> std::result::Result<BrainTurn, CodeBrainError> {
            Ok(BrainTurn::text("local result".into()))
        }
    }
    let path = std::env::temp_dir().join(format!("hive-code-fixture-{}", Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    let c = card(p, "code-event");
    let spec = CodeSessionSpec {
        acceptance: Vec::new(),
        task: "synthetic".into(),
        workspace_path: Some(path.to_string_lossy().into()),
        repo_url: None,
        repo_ref: None,
        brain: "local".into(),
        model_id: None,
        max_turns: 1,
        vault_name: None,
        coordinator: false,
    };
    let result = run_session(
        &a,
        &path,
        c.id,
        &spec,
        &Fixture,
        chrono::Utc::now() + chrono::Duration::hours(1),
    )
    .await
    .unwrap();
    assert_eq!(result.final_text, "local result");
    assert!(s.inspect().unwrap()["activity_count"].as_i64().unwrap() >= 2);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn separate_sqlite_connections_claim_atomically() {
    let path = std::env::temp_dir().join(format!("hive-claim-race-{}.sqlite", Uuid::new_v4()));
    let s = LocalHubStore::open(&path).unwrap();
    let t = LocalHubStore::open(&path).unwrap();
    let ca = s.enroll_owner("one").unwrap();
    let cb = s.enroll_owner("two").unwrap();
    let a = s.connect(&ca.raw_key).unwrap();
    let b = t.connect(&cb.raw_key).unwrap();
    a.check_in(&caps(), None).await.unwrap();
    b.check_in(&caps(), None).await.unwrap();
    let p = s.create_project("race", "fixture").unwrap();
    s.add_card(card(p, "only-one")).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let barrier2 = barrier.clone();
    let x = std::thread::spawn(move || {
        barrier.wait();
        futures::executor::block_on(a.claim_card()).unwrap()
    });
    let y = std::thread::spawn(move || {
        barrier2.wait();
        futures::executor::block_on(b.claim_card()).unwrap()
    });
    let results = [x.join().unwrap(), y.join().unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|c| matches!(c, Claim::Leased { .. }))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|c| matches!(c, Claim::NothingToDo))
            .count(),
        1
    );
    drop(s);
    drop(t);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn isolated_tunnel_configuration_and_origin_checks() {
    let path = std::env::temp_dir().join(format!("hive-local-tunnel-{}.json", Uuid::new_v4()));
    tunnel::write_config(
        &path,
        "fixture-id",
        Path::new("/private/fixture.json"),
        "local.example.test",
        "127.0.0.1:8787".parse().unwrap(),
    )
    .unwrap();
    let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(v["ingress"][0]["service"], "http://127.0.0.1:8787");
    assert!(tunnel::write_config(
        &path,
        "other",
        Path::new("/fixture"),
        "other.test",
        "127.0.0.1:8787".parse().unwrap()
    )
    .is_err());
    std::fs::remove_file(path).unwrap();
    for base in [
        "http://example.com",
        "https://user:password@example.com",
        "https://example.com/path",
        "http://0.0.0.0:8787",
    ] {
        assert!(RemoteLocalHub::new(base, "fixture".into()).is_err());
    }
    assert!(RemoteLocalHub::new("http://127.0.0.1:8787", "fixture".into()).is_ok());
}

#[tokio::test]
async fn repository_cards_require_internet_and_target_node_is_honored() {
    let (s, a, b, p) = fixture().await;
    let mut c = card(p, "repo");
    c.modality = "code".into();
    c.required_capabilities =
        json!({"brain":"local","repo_url":"https://example.test/repository.git"});
    s.add_card(c).unwrap();
    assert!(matches!(a.claim_card().await.unwrap(), Claim::NothingToDo));
    let node = a.with_node(|_, node| Ok(node.to_owned())).unwrap();
    let mut c = card(p, "targeted");
    c.required_capabilities = json!({"target_node_id":node});
    s.add_card(c.clone()).unwrap();
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    assert_eq!(id(a.claim_card().await.unwrap()), c.id);
    let child = a
        .spawn_child_card(
            c.id,
            "repo-child",
            "Repository child",
            "code",
            "task",
            "done",
            json!({"brain":"local","repo_url":"https://example.test/repository.git"}),
        )
        .await
        .unwrap();
    assert!(child.requires_internet);
}

#[tokio::test]
async fn vault_http_grants_revocation_and_offline_errors() {
    let s = LocalHubStore::in_memory().unwrap();
    let vault = s.vault_create("HTTP notes").unwrap();
    let doc = Uuid::new_v4();
    let revision = s
        .vault_put(vault, doc, "note.md", "Synthetic", "searchable fixture")
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(s.clone(), listener, async {
        let _ = rx.await;
    }));
    let code = s.pairing_code().unwrap();
    let c = RemoteLocalHub::pair(&url, &code, "vault-reader")
        .await
        .unwrap();
    let remote = RemoteLocalHub::new(&url, c.raw_key).unwrap();
    assert!(remote.vault_list().await.unwrap().is_empty());
    assert!(remote.vault_read(vault, doc, &revision).await.is_err());
    s.vault_grant(vault, c.node_id, true).unwrap();
    assert_eq!(
        remote.vault_status(vault).await.unwrap().state,
        "unavailable"
    );
    assert!(matches!(
        remote.vault_search(vault, "fixture", 10).await,
        Err(HubError::Transport(_))
    ));
    s.vault_set_available(vault, true).unwrap();
    assert_eq!(
        remote.vault_search(vault, "fixture", 10).await.unwrap()[0].revision,
        revision
    );
    assert_eq!(
        remote
            .vault_read(vault, doc, &revision)
            .await
            .unwrap()
            .content,
        "searchable fixture"
    );
    s.vault_archive(
        vault,
        doc,
        &revision,
        "host-owner",
        "archive transport fixture",
    )
    .unwrap();
    assert!(remote
        .vault_search(vault, "fixture", 10)
        .await
        .unwrap()
        .is_empty());
    assert!(remote.vault_read(vault, doc, &revision).await.is_err());
    s.vault_restore(
        vault,
        doc,
        &revision,
        "host-owner",
        "restore transport fixture",
    )
    .unwrap();
    assert_eq!(
        remote
            .vault_search(vault, "fixture", 10)
            .await
            .unwrap()
            .len(),
        1
    );
    s.vault_grant(vault, c.node_id, false).unwrap();
    assert!(remote.vault_search(vault, "fixture", 10).await.is_err());
    s.revoke(c.node_id).unwrap();
    assert!(matches!(remote.vault_list().await, Err(HubError::BadKey)));
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
    assert!(matches!(
        remote.vault_list().await,
        Err(HubError::Transport(_))
    ));
}

/// Two devices share one verified owner; caller-supplied identities cannot cross accounts.
#[cfg(feature = "bots")]
#[tokio::test]
async fn bots_transport_two_clients_enforce_owner_binding() {
    use crate::bots::*;
    let store = LocalHubStore::in_memory().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(store.clone(), listener, async {
        let _ = rx.await;
    }));
    let ca = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "A")
        .await
        .unwrap();
    let cb = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "B")
        .await
        .unwrap();
    let a = RemoteLocalHub::new(&url, ca.raw_key.clone()).unwrap();
    let b = RemoteLocalHub::new(&url, cb.raw_key.clone()).unwrap();
    let owner = Uuid::new_v4();
    assert!(a.bots_agents_list().await.is_err());
    store.set_node_owner(ca.node_id, owner).unwrap();
    store.set_node_owner(cb.node_id, owner).unwrap();
    let agent = a
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Remote test agent".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: Some(ca.node_id),
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "transport-test".into(),
        })
        .await
        .unwrap();
    assert_eq!(b.bots_agents_list().await.unwrap()[0].id, agent.id);
    let updated = b
        .bots_agents_update(
            agent.id,
            AgentProfilePatch {
                name: Some("Renamed".into()),
                preferred_host: None,
                capability_policy_ref: None,
                memory_namespace: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    let c = a
        .bots_conversations_create(NewConversation {
            title: None,
            owner,
            kind: ConversationKind::AgentDm,
            project_id: None,
            coordinator: Some(agent.id),
            storage_scope: StorageScope::LocalOnly,
        })
        .await
        .unwrap();
    let actor = Principal::User(owner);
    assert_eq!(b.bots_conversations_list(actor).await.unwrap()[0].id, c.id);
    b.bots_conversations_join(actor, c.id).await.unwrap();
    let draft = NewMessage {
        thread_root: None,
        kind: MessageKind::Text,
        body: Some("hello over HTTP".into()),
        attachment_refs: vec![],
        task_ref: None,
        turn_ref: None,
        source_event_ref: None,
    };
    let m = a
        .bots_message_send(
            actor,
            c.id,
            "stable-request".into(),
            c.policy_revision,
            vec![agent.id],
            draft.clone(),
        )
        .await
        .unwrap();
    let retry = b
        .bots_message_send(
            actor,
            c.id,
            "stable-request".into(),
            c.policy_revision,
            vec![agent.id],
            draft,
        )
        .await
        .unwrap();
    assert_eq!(m.id, retry.id);
    let page = MessagePage {
        before: None,
        after: None,
        limit: 50,
    };
    let messages = b.bots_messages_list(actor, c.id, page).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].body.as_deref(), Some("hello over HTTP"));
    let seen = b
        .bots_conversation_mark_read(c.id, m.server_sequence)
        .await
        .unwrap();
    assert_eq!(seen.last_seen_sequence, m.server_sequence);
    let invalid = RemoteLocalHub::new(&url, "invalid".into()).unwrap();
    assert!(matches!(
        invalid.bots_agents_list().await,
        Err(HubError::BadKey)
    ));
    let foreign_owner = Uuid::new_v4();
    let foreign_credentials = store.enroll_owner("foreign account").unwrap();
    store
        .set_node_owner(foreign_credentials.node_id, foreign_owner)
        .unwrap();
    let foreign = RemoteLocalHub::new(&url, foreign_credentials.raw_key).unwrap();
    assert!(foreign.bots_agents_list().await.unwrap().is_empty());
    assert!(foreign.bots_conversations_list(actor).await.is_err());
    assert!(foreign.bots_messages_list(actor, c.id, page).await.is_err());
    assert!(foreign.bots_conversations_join(actor, c.id).await.is_err());
    assert!(foreign.bots_conversation_mark_read(c.id, 1).await.is_err());
    assert!(foreign.bots_agents_archive(agent.id).await.is_err());
    assert!(foreign
        .bots_agents_update(
            agent.id,
            AgentProfilePatch {
                name: Some("stolen".into()),
                preferred_host: None,
                capability_policy_ref: None,
                memory_namespace: None,
            }
        )
        .await
        .is_err());
    assert!(foreign
        .bots_message_send(
            actor,
            c.id,
            "spoof".into(),
            c.policy_revision,
            vec![agent.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("spoof".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            }
        )
        .await
        .is_err());
    assert!(foreign
        .bots_conversations_list(Principal::Agent(agent.id))
        .await
        .is_err());
    assert!(foreign
        .bots_conversations_create(NewConversation {
            title: None,
            owner,
            kind: ConversationKind::AgentDm,
            project_id: None,
            coordinator: Some(agent.id),
            storage_scope: StorageScope::LocalOnly
        })
        .await
        .is_err());
    let overridden = foreign
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Own account only".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "foreign".into(),
        })
        .await
        .unwrap();
    assert_eq!(overridden.owner, foreign_owner);
    let existing_session = store.connect(&cb.raw_key).unwrap();
    store.revoke(cb.node_id).unwrap();
    assert!(matches!(b.bots_agents_list().await, Err(HubError::BadKey)));
    assert!(matches!(
        b.bots_messages_list(actor, c.id, page).await,
        Err(HubError::BadKey)
    ));
    assert!(matches!(
        existing_session.bots_agents_list(),
        Err(HubError::BadKey)
    ));
    a.bots_agents_archive(agent.id).await.unwrap();
    assert!(a.bots_agents_list().await.unwrap().is_empty());
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[test]
fn version_seven_nodes_migrate_with_unconfirmed_owner() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    for schema in [
        include_str!("schema.sql"),
        include_str!("vault_schema.sql"),
        include_str!("vault_folder_schema.sql"),
        include_str!("vault_intake_schema.sql"),
        include_str!("vault_curation_schema.sql"),
        include_str!("vault_maintenance_schema.sql"),
        include_str!("bots_schema.sql"),
    ] {
        db.execute_batch(schema).unwrap();
    }
    let node = Uuid::new_v4();
    db.execute(
        "INSERT INTO nodes(id,name) VALUES(?1,'preserved')",
        [node.to_string()],
    )
    .unwrap();
    let store = LocalHubStore::from_connection(db).unwrap();
    store
        .transaction(|tx| {
            let row: (String, Option<String>) = tx
                .query_row(
                    "SELECT name,owner_member_id FROM nodes WHERE id=?1",
                    [node.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(row, ("preserved".into(), None));
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                // Includes repository defaults (v14).
                14
            );
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "bots")]
mod room_demo_tests {
    use super::*;
    use crate::bots::*;
    use crate::bots::{
        ConversationKind, LocalTurnOutcome, NewAgentProfile, NewConversation, StorageScope,
    };
    use std::sync::Arc;
    use uuid::Uuid;
    struct Reply;
    #[async_trait::async_trait]
    impl LocalBotsTurnRunner for Reply {
        async fn run_turn(
            &self,
            agent: &AgentProfile,
            _: LocalTurnRequest,
        ) -> Result<LocalTurnOutcome, LocalTurnError> {
            Ok(LocalTurnOutcome {
                reply_body: format!("{} says @everyone", agent.name),
                usage: None,
            })
        }
    }
    /// Sif's demo acceptance test, rescoped by Loki when the automation branch merged.
    ///
    /// It originally passed because `executor.rs` hardcoded an empty recipient list, so this
    /// property was a fact about the code. With agent-to-agent turns implemented it is a fact
    /// about *configuration*: `max_depth: 0` disables fan-out, and that is the mode the first
    /// release ships in. Keeping the test and making the setting explicit is the point -- the
    /// human-driven demo path stays a supported, tested configuration instead of becoming
    /// something that only used to work.
    #[tokio::test]
    async fn human_group_chat_replies_once_and_then_stays_quiet_with_fan_out_off() {
        for kind in [ConversationKind::Team, ConversationKind::Project] {
            let store = Arc::new(LocalHubStore::in_memory().unwrap());
            let owner = Uuid::new_v4();
            let host = Uuid::new_v4();
            let mut agents = Vec::new();
            for name in ["One", "Two", "Three"] {
                agents.push(
                    store
                        .bots_agents_create(NewAgentProfile {
                            owner,
                            name: name.into(),
                            runtime_kind: AgentRuntimeKind::Local,
                            preferred_host: Some(host),
                            capability_policy_ref: "default".into(),
                            provider_account_ref: None,
                            memory_namespace: name.into(),
                        })
                        .unwrap(),
                );
            }
            let room = store
                .bots_conversations_create(NewConversation {
                    title: Some("Demo".into()),
                    owner,
                    kind,
                    project_id: (kind == ConversationKind::Project)
                        .then(|| store.create_project("Demo", "Discuss the project").unwrap()),
                    coordinator: None,
                    storage_scope: StorageScope::LocalOnly,
                })
                .unwrap();
            for a in &agents {
                store
                    .bots_conversations_join(Principal::Agent(a.id), room.id)
                    .unwrap();
            }
            let mentions =
                crate::bots::resolve_mentions("@One @Two", &agents, Principal::User(owner));
            store
                .bots_message_send(
                    Principal::User(owner),
                    room.id,
                    "demo".into(),
                    1,
                    mentions.recipients,
                    NewMessage {
                        thread_root: None,
                        kind: MessageKind::Text,
                        body: Some("@One @Two".into()),
                        attachment_refs: vec![],
                        task_ref: None,
                        turn_ref: None,
                        source_event_ref: None,
                    },
                )
                .unwrap();
            // No with_budgets call: this is exactly what the CLI, the FFI bridge and the Tauri
            // shell construct, so this test pins the behavior that actually ships.
            let executor = DeliveryExecutor::new(store.clone(), Arc::new(Reply), host, owner);
            assert_eq!(executor.drain_once().await.delivered, 2);
            assert_eq!(executor.drain_once().await.delivered, 0);
            let messages = store
                .bots_messages_list(
                    Principal::User(owner),
                    room.id,
                    MessagePage {
                        before: None,
                        after: None,
                        limit: 20,
                    },
                )
                .unwrap();
            assert_eq!(messages.len(), 3);
            assert_eq!(
                messages
                    .iter()
                    .filter(|m| m.author == Principal::Agent(agents[2].id))
                    .count(),
                0
            );
            for a in &agents {
                assert!(store
                    .bots_deliveries_pending_for_agent(a.id, 20)
                    .unwrap()
                    .is_empty());
            }
            let count: i64 = store
                .transaction(|tx| {
                    tx.query_row("SELECT COUNT(*) FROM agent_deliveries", [], |r| r.get(0))
                        .map_err(crate::local_hub::db_error)
                })
                .unwrap();
            assert_eq!(
                count, 2,
                "with fan-out off, even @everyone in a reply creates nothing"
            );
        }
    }

    /// The same room and the same `@everyone` reply with automation on. The contrast with the
    /// test above is the whole of Track A: the cascade happens, and it is bounded.
    #[tokio::test]
    async fn the_same_everyone_reply_fans_out_but_stays_bounded_when_enabled() {
        let store = Arc::new(LocalHubStore::in_memory().unwrap());
        let owner = Uuid::new_v4();
        let host = Uuid::new_v4();
        let mut agents = Vec::new();
        for name in ["One", "Two", "Three"] {
            agents.push(
                store
                    .bots_agents_create(NewAgentProfile {
                        owner,
                        name: name.into(),
                        runtime_kind: AgentRuntimeKind::Local,
                        preferred_host: Some(host),
                        capability_policy_ref: "default".into(),
                        provider_account_ref: None,
                        memory_namespace: name.into(),
                    })
                    .unwrap(),
            );
        }
        let room = store
            .bots_conversations_create(NewConversation {
                title: Some("Demo".into()),
                owner,
                kind: ConversationKind::Team,
                project_id: None,
                coordinator: None,
                storage_scope: StorageScope::LocalOnly,
            })
            .unwrap();
        for a in &agents {
            store
                .bots_conversations_join(Principal::Agent(a.id), room.id)
                .unwrap();
        }
        let mentions = crate::bots::resolve_mentions("@One @Two", &agents, Principal::User(owner));
        let root = store
            .bots_message_send(
                Principal::User(owner),
                room.id,
                "demo".into(),
                1,
                mentions.recipients,
                NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some("@One @Two".into()),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .unwrap();

        // Every agent names two teammates explicitly, forever, so nothing but the budgets stops
        // this. Was `@everyone` until audit §3.9 correctly refused broadcast from an agent --
        // explicit names are also what llama3.1 actually produced in the live three-agent run.
        struct NameTwo;
        #[async_trait::async_trait]
        impl LocalBotsTurnRunner for NameTwo {
            async fn run_turn(
                &self,
                agent: &AgentProfile,
                _: LocalTurnRequest,
            ) -> Result<LocalTurnOutcome, LocalTurnError> {
                let others: Vec<&str> = ["One", "Two", "Three"]
                    .into_iter()
                    .filter(|n| !agent.name.eq_ignore_ascii_case(n))
                    .collect();
                Ok(LocalTurnOutcome {
                    reply_body: format!("@{} @{} thoughts?", others[0], others[1]),
                    usage: None,
                })
            }
        }
        let executor = DeliveryExecutor::new(store.clone(), Arc::new(NameTwo), host, owner)
            .with_budgets(HandoffBudgets {
                max_depth: 2,
                max_turns_per_root: 1000,
                ..HandoffBudgets::default()
            });
        for pass in 1..=40 {
            if executor.drain_once().await.delivered == 0 {
                break;
            }
            assert!(pass < 40, "the cascade is not terminating");
        }

        // Depth 0: the human's 2. Depth 1: each of those 2 replies wakes at most 2 (the fan-out
        // cap, not all 3 of @everyone) = 4. Depth 2: 4 replies x 2 = 8. Depth 3 is refused.
        assert_eq!(
            store.bots_turns_for_root(root.id).unwrap(),
            14,
            "2 + 4 + 8, bounded at depth 2"
        );
        for a in &agents {
            assert!(store
                .bots_deliveries_pending_for_agent(a.id, 50)
                .unwrap()
                .is_empty());
        }

        // Third agent does get drawn in here, unlike the fan-out-off case above.
        let messages = store
            .bots_messages_list(
                Principal::User(owner),
                room.id,
                MessagePage {
                    before: None,
                    after: None,
                    limit: 200,
                },
            )
            .unwrap();
        assert!(
            messages
                .iter()
                .any(|m| m.author == Principal::Agent(agents[2].id)),
            "@everyone in a reply must reach the agent the human never addressed"
        );
        assert!(
            messages.iter().any(|m| m.kind == MessageKind::System),
            "the suppressed fan-out and depth stop must be visible"
        );
    }
}

#[cfg(feature = "bots")]
#[test]
fn room_titles_migrate_v9_without_losing_conversations() {
    use crate::bots::*;
    let db = rusqlite::Connection::open_in_memory().unwrap();
    for sql in [
        include_str!("schema.sql"),
        include_str!("vault_schema.sql"),
        include_str!("vault_folder_schema.sql"),
        include_str!("vault_intake_schema.sql"),
        include_str!("vault_curation_schema.sql"),
        include_str!("vault_maintenance_schema.sql"),
        include_str!("bots_schema.sql"),
        include_str!("owner_schema.sql"),
        include_str!("enrollment_schema.sql"),
    ] {
        db.execute_batch(sql).unwrap();
    }
    let room = Uuid::new_v4();
    let owner = Uuid::new_v4();
    db.execute(
        "INSERT INTO conversations VALUES(?1,?2,'team',NULL,NULL,'local_only',1,1)",
        [room.to_string(), owner.to_string()],
    )
    .unwrap();
    db.execute("INSERT INTO conversation_members VALUES(?1,'user',?2,'[\"read\",\"post\",\"manage\"]',1,1)", [room.to_string(), owner.to_string()]).unwrap();
    let store = LocalHubStore::from_connection(db).unwrap();
    let rooms = store
        .bots_conversations_list(Principal::User(owner))
        .unwrap();
    assert_eq!(rooms.len(), 1);
    assert_eq!(rooms[0].id, room);
    assert_eq!(rooms[0].title, None);
    let created = store
        .bots_conversations_create(NewConversation {
            title: Some("Named".into()),
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        })
        .unwrap();
    assert_eq!(created.title.as_deref(), Some("Named"));
}

#[cfg(feature = "bots")]
#[test]
fn named_project_room_survives_database_reopen() {
    use crate::bots::*;
    let path = std::env::temp_dir().join(format!("hive-room-{}.sqlite3", Uuid::new_v4()));
    let owner = Uuid::new_v4();
    let store = LocalHubStore::open(&path).unwrap();
    let project = store
        .create_project("Real local project", "Demo conversation")
        .unwrap();
    let room = store
        .bots_conversations_create(NewConversation {
            title: Some("Project room".into()),
            owner,
            kind: ConversationKind::Project,
            project_id: Some(project),
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        })
        .unwrap();
    drop(store);
    let reopened = LocalHubStore::open(&path).unwrap();
    let rooms = reopened
        .bots_conversations_list(Principal::User(owner))
        .unwrap();
    assert_eq!(rooms[0].id, room.id);
    assert_eq!(rooms[0].project_id, Some(project));
    assert_eq!(rooms[0].title.as_deref(), Some("Project room"));
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "bots")]
#[test]
fn provider_runtime_migration_preserves_agent_references_and_enforces_foreign_keys() {
    use crate::bots::*;
    let db = rusqlite::Connection::open_in_memory().unwrap();
    for sql in [
        include_str!("schema.sql"),
        include_str!("vault_schema.sql"),
        include_str!("vault_folder_schema.sql"),
        include_str!("vault_intake_schema.sql"),
        include_str!("vault_curation_schema.sql"),
        include_str!("vault_maintenance_schema.sql"),
        include_str!("bots_schema.sql"),
        include_str!("owner_schema.sql"),
        include_str!("enrollment_schema.sql"),
        "ALTER TABLE conversations ADD COLUMN title TEXT; PRAGMA user_version=10;",
        include_str!("bots_causation_schema.sql"),
    ] {
        db.execute_batch(sql).unwrap();
    }
    let owner = Uuid::new_v4();
    let agent = Uuid::new_v4();
    let room = Uuid::new_v4();
    let message = Uuid::new_v4();
    db.execute("INSERT INTO agent_profiles VALUES(?1,?2,'Preserved',1,'local',NULL,'default',NULL,'memory',0,1,1)", [agent.to_string(), owner.to_string()]).unwrap();
    db.execute(
        "INSERT INTO conversations VALUES(?1,?2,'team',NULL,?3,'local_only',1,1,'Room')",
        [room.to_string(), owner.to_string(), agent.to_string()],
    )
    .unwrap();
    db.execute("INSERT INTO messages(id,conversation_id,author_kind,author_id,server_sequence,client_request_id,kind,created_at) VALUES(?1,?2,'user',?3,1,'original','text',1)", [message.to_string(), room.to_string(), owner.to_string()]).unwrap();
    db.execute("INSERT INTO agent_deliveries(message_id,recipient,status,lease_generation,updated_at,turn_depth) VALUES(?1,?2,'pending',0,1,0)", [message.to_string(), agent.to_string()]).unwrap();
    let store = LocalHubStore::from_connection(db).unwrap();
    assert_eq!(store.bots_agents_list(owner).unwrap()[0].name, "Preserved");
    assert_eq!(
        store.bots_deliveries_pending_for_agent(agent, 10).unwrap()[0]
            .key
            .message_id,
        message
    );
    for kind in [AgentRuntimeKind::AnthropicByok, AgentRuntimeKind::NousByok] {
        store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Provider".into(),
                runtime_kind: kind,
                preferred_host: None,
                capability_policy_ref: "default".into(),
                provider_account_ref: None,
                memory_namespace: "provider".into(),
            })
            .unwrap();
    }
    assert!(store
        .bots_conversations_create(NewConversation {
            title: None,
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: Some(Uuid::new_v4()),
            storage_scope: StorageScope::LocalOnly
        })
        .is_err());
    store
        .transaction(|tx| {
            assert!(!tx
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap());
            let version: i64 = tx
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(version, 14);
            Ok(())
        })
        .unwrap();
}

#[test]
fn initialization_and_writes_wait_for_competing_writer() {
    let path = std::env::temp_dir().join(format!("hive-busy-{}.sqlite", Uuid::new_v4()));
    let store = LocalHubStore::open(&path).unwrap();
    // Hold the actual SQLite writer lock longer than the old 250ms timeout.
    let mut blocker = rusqlite::Connection::open(&path).unwrap();
    let tx = blocker
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let other_path = path.clone();
    let (started, ready) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        started.send(()).unwrap();
        store.create_project("waited", "funded work").unwrap()
    });
    ready.recv().unwrap();
    let opener = std::thread::spawn(move || LocalHubStore::open(&other_path).unwrap());
    std::thread::sleep(std::time::Duration::from_millis(600));
    tx.commit().unwrap();
    writer.join().unwrap();
    drop(opener.join().unwrap());
    drop(blocker);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn simultaneous_first_open_serializes_migrations() {
    let path = std::env::temp_dir().join(format!("hive-first-open-{}.sqlite", Uuid::new_v4()));
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let store = LocalHubStore::open(&path).unwrap();
                store.create_project("first open", "fixture").unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let store = LocalHubStore::open(&path).unwrap();
    let db = store.db.lock().unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    drop(db);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[cfg(all(feature = "sandbox", unix))]
#[tokio::test]
async fn acceptance_gate_changes_real_card_status_and_keeps_receipt() {
    use crate::coder::*;
    struct Done;
    #[async_trait::async_trait]
    impl CodeBrain for Done {
        async fn next_turn(
            &self,
            _: &[BrainMessage],
            _: &[ToolSpec],
        ) -> std::result::Result<BrainTurn, CodeBrainError> {
            Ok(BrainTurn::Text(
                "The agent claims success".into(),
                crate::ledger::Usage {
                    tokens_in: 101,
                    tokens_out: 23,
                    compute_seconds: 0.25,
                },
                Some("resolved-test-model".into()),
            ))
        }
    }
    for (exit, status) in [(0, "review"), (1, "blocked")] {
        let (s, hub, _, p) = fixture().await;
        let path = std::env::temp_dir().join(format!("hive-gate-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        let mut c = card(p, "acceptance");
        c.required_capabilities = json!({"task":"verify","workspace_path":path,"acceptance":[{"name":"unit tests","command":"python3","args":["-c",format!("raise SystemExit({exit})")]}]});
        s.add_card(c.clone()).unwrap();
        let claimed = id(hub.claim_card().await.unwrap());
        assert_eq!(claimed, c.id);
        let outcome = crate::tools::run_code_session(
            &hub,
            &path,
            &c,
            &Done,
            chrono::Utc::now() + chrono::Duration::minutes(2),
        )
        .await
        .unwrap();
        let result = crate::worker::finish_code_session(&hub, c.id, &outcome)
            .await
            .unwrap();
        assert_eq!(result.is_some(), exit == 0);
        if exit == 0 {
            let recorded: String = s
                .transaction(|tx| {
                    tx.query_row(
                        "SELECT usage FROM card_outputs WHERE card_id=?1",
                        [c.id.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(db_error)
                })
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&recorded).unwrap()["tokens_out"],
                23
            );
        }

        if exit == 0 {
            let model: String = s
                .transaction(|tx| {
                    tx.query_row(
                        "SELECT model_id FROM card_outputs WHERE card_id=?1",
                        [c.id.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(db_error)
                })
                .unwrap();
            assert_eq!(model, "resolved-test-model");
        }
        assert_eq!(outcome.data.as_ref().unwrap()["usage"]["tokens_out"], 23);
        let snapshot = s.inspect().unwrap();
        assert_eq!(snapshot["cards"][0]["status"], status);
        let stored = if exit == 0 {
            &snapshot["outputs"][0]["content"]
        } else {
            &snapshot["cards"][0]["reason"]
        };
        assert_eq!(stored.as_str(), Some(outcome.summary.as_str()));
        assert!(outcome.summary.contains("unit tests"));
        assert!(outcome.summary.contains("exit_status"));
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(all(feature = "sandbox", feature = "llama-cpp"))]
#[tokio::test]
async fn coding_worker_resumes_with_children_and_blocks_duplicate_spawns_and_unverified_success() {
    use axum::{routing::post, Json, Router};
    use std::sync::{Arc, Mutex};
    for verified in [true, false] {
        let (store, a, b, p) = fixture().await;
        let dir = std::env::temp_dir().join(format!("hive-resume-{}", Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let mut parent = card(p, "parent");
        parent.modality = "code".into();
        parent.required_capabilities = json!({"task":"Review the two children", "workspace_path":dir,"coordinator":true,"max_turns":3});
        store.add_card(parent.clone()).unwrap();
        assert_eq!(id(a.claim_card().await.unwrap()), parent.id);
        let checks =
            json!([{"name":"test","command":"true","args":[],"expect_exit":0,"required":true}]);
        let required = json!({"task":"child","workspace_path":dir,"acceptance":checks});
        let mut children = Vec::new();
        for key in ["left", "right"] {
            let c = a
                .spawn_child_card(parent.id, key, key, "code", "child", "", required.clone())
                .await
                .unwrap();
            let same = a
                .spawn_child_card(parent.id, key, key, "code", "child", "", required.clone())
                .await
                .unwrap();
            assert_eq!(same.card_id, c.card_id);
            children.push(c.card_id);
        }
        a.wait_on_child(parent.id, children[0]).await.unwrap();
        for _ in 0..2 {
            let cid = id(b.claim_card().await.unwrap());
            let receipt =
                crate::coder::AcceptanceOutcome::Passed(vec![crate::coder::AcceptanceResult {
                    name: "test".into(),
                    command_line: "\"true\"".into(),
                    exit_status: Some(0),
                    passed: true,
                    required: true,
                    timed_out: false,
                    stdout_tail: String::new(),
                    stderr_tail: String::new(),
                    error: None,
                }])
                .receipt();
            let text = if verified {
                format!("CHILD_SOURCE_{cid}\n{receipt}")
            } else {
                "I claim all tests passed".into()
            };
            b.complete_card(cid, &text, Some("local-test"), Usage::default())
                .await
                .unwrap();
        }
        let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
        let capture = seen.clone();
        let app=Router::new().route("/v1/chat/completions",post(move |Json(body):Json<Value>| {
            let capture=capture.clone(); async move {
                let mut requests=capture.lock().unwrap();requests.push(body);let first=requests.len()==1;
                Json(if first {json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"duplicate","type":"function","function":{"name":"spawn_card","arguments":"{\"key\":\"replacement\",\"title\":\"bad\",\"modality\":\"text\",\"inputs\":\"duplicate\"}"}}]}}],"usage":{"prompt_tokens":1,"completion_tokens":1}})} else {json!({"choices":[{"finish_reason":"stop","message":{"content":"Reviewed both children"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}})})
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let backend = crate::backend::llama_cpp::LlamaCppBackend::new(format!("http://{addr}"));
        let cp = caps();
        let (_stop, rx) = tokio::sync::watch::channel(false);
        let worker = crate::worker::Worker {
            capacity_path: dir.join("capacity"),
            hub: &a,
            backend: &backend,
            caps: &cp,
            default_model: Some("test-model".into()),
            stop: rx,
            events: None,
            data_dir: dir.clone(),
            sandbox: None,
        };
        assert!(worker.tick().await.unwrap());
        let requests = seen.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let first = requests[0].to_string();
        for child in children {
            assert!(first.contains(&child.to_string()));
        }
        assert!(first.contains("untrusted evidence"));
        assert!(requests[1]
            .to_string()
            .contains("do not spawn replacements"));
        let snapshot = store.inspect().unwrap();
        assert_eq!(snapshot["cards"].as_array().unwrap().len(), 3);
        let parent_row = snapshot["cards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["card"]["id"] == parent.id.to_string())
            .unwrap();
        assert_eq!(
            parent_row["status"],
            if verified { "review" } else { "blocked" }
        );
        server.abort();
        drop(requests);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

// Exercise a genuinely failing host command, not a fabricated receipt or direct fail_card call.
#[cfg(all(feature = "sandbox", unix))]
#[tokio::test]
async fn team_failed_host_check_blocks_waiting_parent_and_releases_leases() {
    use crate::coder::*;
    struct ClaimsSuccess;
    #[async_trait::async_trait]
    impl CodeBrain for ClaimsSuccess {
        async fn next_turn(
            &self,
            _: &[BrainMessage],
            _: &[ToolSpec],
        ) -> std::result::Result<BrainTurn, CodeBrainError> {
            Ok(BrainTurn::Text(
                "Everything works; all tests passed".into(),
                Usage::default(),
                Some("fixture".into()),
            ))
        }
    }
    let (store, parent_hub, child_hub, project) = fixture().await;
    let dir = std::env::temp_dir().join(format!("hive-team-negative-{}", Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("answer.txt"), "incorrect").unwrap();
    let mut parent = card(project, "negative-parent");
    parent.modality = "code".into();
    parent.required_capabilities =
        json!({"task":"review", "workspace_path":dir, "coordinator":true});
    store.add_card(parent.clone()).unwrap();
    assert_eq!(id(parent_hub.claim_card().await.unwrap()), parent.id);
    let child = parent_hub.spawn_child_card(parent.id, "negative-child", "negative child", "code", "verify answer", "", json!({
        "task":"verify answer", "workspace_path":dir, "target_node_id":child_hub.node_id().unwrap(),
        "acceptance":[{"name":"answer is correct", "command":"python3", "args":["-c","from pathlib import Path; assert Path('answer.txt').read_text() == 'correct'"], "required":true}]
    })).await.unwrap();
    parent_hub
        .wait_on_child(parent.id, child.card_id)
        .await
        .unwrap();
    let Claim::Leased { card: claimed, .. } = child_hub.claim_card().await.unwrap() else {
        panic!("child not claimed")
    };
    assert_eq!(claimed.id, child.card_id);
    let outcome = crate::tools::run_code_session(
        &child_hub,
        &dir,
        &claimed,
        &ClaimsSuccess,
        Utc::now() + chrono::Duration::minutes(2),
    )
    .await
    .unwrap();
    assert!(!outcome.ok);
    assert!(outcome.summary.contains("exit_status"));
    assert!(
        crate::worker::finish_code_session(&child_hub, child.card_id, &outcome)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        parent_hub.claim_card().await.unwrap(),
        Claim::NothingToDo
    ));
    let snapshot = store.inspect().unwrap();
    for row in snapshot["cards"].as_array().unwrap() {
        assert_eq!(row["status"], "blocked");
    }
    assert!(snapshot["outputs"].as_array().unwrap().is_empty());
    let leases: i64 = store
        .transaction(|tx| {
            tx.query_row("SELECT count(*) FROM leases", [], |r| r.get(0))
                .map_err(db_error)
        })
        .unwrap();
    assert_eq!(leases, 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn team_offline_child_stays_targeted_until_original_node_returns() {
    let (store, a, b, project) = fixture().await;
    let parent = card(project, "offline-parent");
    store.add_card(parent.clone()).unwrap();
    assert_eq!(id(a.claim_card().await.unwrap()), parent.id);
    let child = a
        .spawn_child_card(
            parent.id,
            "offline-child",
            "offline child",
            "text",
            "synthetic",
            "",
            json!({"target_node_id":b.node_id().unwrap()}),
        )
        .await
        .unwrap();
    a.wait_on_child(parent.id, child.card_id).await.unwrap();
    assert_eq!(b.check_out().await.unwrap(), "checked_out");
    for _ in 0..3 {
        assert!(matches!(a.claim_card().await.unwrap(), Claim::NothingToDo));
    }
    let snapshot = store.inspect().unwrap();
    for row in snapshot["cards"].as_array().unwrap() {
        assert_eq!(
            row["status"],
            if row["card"]["id"] == parent.id.to_string() {
                "waiting_on_child"
            } else {
                "ready"
            }
        );
    }
    assert!(snapshot["outputs"].as_array().unwrap().is_empty());
    b.check_in(&caps(), None).await.unwrap();
    assert_eq!(id(b.claim_card().await.unwrap()), child.card_id);
    b.complete_card(
        child.card_id,
        "returned target finished",
        Some("fixture"),
        Usage::default(),
    )
    .await
    .unwrap();
    assert_eq!(id(a.claim_card().await.unwrap()), parent.id);
    a.complete_card(parent.id, "reviewed", Some("fixture"), Usage::default())
        .await
        .unwrap();
    let leases: i64 = store
        .transaction(|tx| {
            tx.query_row("SELECT count(*) FROM leases", [], |r| r.get(0))
                .map_err(db_error)
        })
        .unwrap();
    assert_eq!(leases, 0);
}
