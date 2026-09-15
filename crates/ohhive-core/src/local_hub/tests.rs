use super::*;
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
        capacity_path: std::env::temp_dir().join(format!("hive-worker-test-{}", uuid::Uuid::new_v4())),
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
    let held = crate::execution_capacity::try_acquire_at(&worker.capacity_path).unwrap().unwrap();
    assert!(!worker.tick().await.unwrap(), "busy local turn must prevent card claim");
    assert_eq!(s.inspect().unwrap()["cards"][0]["status"], "ready");
    drop(held);
    worker.tick().await.unwrap();
    assert!(crate::execution_capacity::try_acquire_at(&worker.capacity_path).unwrap().is_some());
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
        capacity_path: std::env::temp_dir().join(format!("hive-worker-test-{}", uuid::Uuid::new_v4())),
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
            Ok(BrainTurn::Text("local result".into()))
        }
    }
    let path = std::env::temp_dir().join(format!("hive-code-fixture-{}", Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    let c = card(p, "code-event");
    let spec = CodeSessionSpec {
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
    s.vault_archive(vault, doc, &revision, "host-owner", "archive transport fixture").unwrap();
    assert!(remote.vault_search(vault, "fixture", 10).await.unwrap().is_empty());
    assert!(remote.vault_read(vault, doc, &revision).await.is_err());
    s.vault_restore(vault, doc, &revision, "host-owner", "restore transport fixture").unwrap();
    assert_eq!(remote.vault_search(vault, "fixture", 10).await.unwrap().len(), 1);
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
                8
            );
            Ok(())
        })
        .unwrap();
}
