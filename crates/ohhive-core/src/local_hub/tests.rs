use super::*;

/// Remove a test's temp directory, tolerating Windows' refusal to delete a file that still has an
/// open handle.
///
/// Unix unlinks an open file happily. Windows raises a sharing violation (`os error 32`) until the
/// last handle closes, and SQLite's does not always close in step with the value that owns it --
/// which is what failed two of these tests on `windows-latest` while they passed everywhere else.
/// This is teardown, not the thing under test: retry briefly, then complain rather than fail an
/// assertion that already passed.
fn remove_temp_dir(path: impl AsRef<std::path::Path>) {
    let path = path.as_ref();
    for _ in 0..50 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    eprintln!("left behind test directory {}", path.display());
}

/// An absolute path in the host's own terms.
///
/// `/tmp/x` is absolute on Unix and is *not* absolute on Windows -- it has no drive letter. A
/// hard-coded POSIX literal therefore made the host's correct rejection look like a test failure
/// on `windows-latest`, and quietly made every neighbouring `is_err()` assertion pass there for
/// the wrong reason.
fn absolute(name: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\{name}")
    } else {
        format!("/tmp/{name}")
    }
}

#[cfg(feature = "sandbox")]
#[tokio::test]
async fn private_preparation_recovers_completed_checkout_and_activates_only_its_target() {
    let (s, a, b, p) = fixture().await;
    let node = Uuid::parse_str(&a.with_node(|_, node| Ok(node.to_owned())).unwrap()).unwrap();
    let other = Uuid::parse_str(&b.with_node(|_, node| Ok(node.to_owned())).unwrap()).unwrap();
    let repo = "https://github.com/example/private.git";
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: repo.into(),
            repo_ref: None,
        }),
    )
    .unwrap();
    let request = private_code_tasks::PrivateCodeTaskRequest {
        request_id: Uuid::new_v4(),
        project_id: p,
        target_node_id: node,
        title: "Private fixture".into(),
        task: "Review readme".into(),
        model_id: None,
        max_turns: 2,
        acceptance: vec![],
    };
    s.stage_private_code_task(&request).unwrap();
    let data = std::env::temp_dir().join(format!("hive-private-prepare-{}", Uuid::new_v4()));
    assert!(s
        .prepare_private_code_task(request.request_id, other, &data, "")
        .await
        .is_err());
    assert!(!data.exists());
    // Simulate a crash after successful Git/receipt preparation but before queue activation.
    // This uses real local Git; no token or live GitHub download is involved.
    let root = data
        .join("code-workspaces")
        .join(request.request_id.to_string());
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "fixture Git failed");
        String::from_utf8(output.stdout).unwrap()
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.test",
        "commit",
        "--allow-empty",
        "-m",
        "fixture",
    ]);
    let base = git(&["rev-parse", "HEAD"]).trim().to_owned();
    let branch = format!("hive/{}", request.request_id);
    git(&["checkout", "-b", &branch]);
    git(&["remote", "add", "origin", repo]);
    std::fs::write(root.join("keep.txt"), "preserve unfinished work").unwrap();
    let state = data.join("code-workspace-state");
    std::fs::create_dir_all(&state).unwrap();
    let receipt = state.join(format!("{}.json", request.request_id));
    std::fs::write(&receipt, json!({"version":1,"card":request.request_id,"repo":repo,"reference":null,"branch":branch,"cache":null,"base_commit":base}).to_string()).unwrap();
    let prepared = s
        .prepare_private_code_task(request.request_id, node, &data, "")
        .await
        .unwrap();
    assert_eq!(prepared, std::fs::canonicalize(&root).unwrap());
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "preserve unfinished work"
    );
    assert_eq!(
        s.prepare_private_code_task(request.request_id, node, &data, "")
            .await
            .unwrap(),
        prepared
    );
    assert!(s
        .prepare_private_code_task(request.request_id, node, &data.join("wrong"), "")
        .await
        .is_err());
    assert!(!data.join("wrong").exists());
    std::fs::rename(&receipt, state.join("saved.json")).unwrap();
    assert!(s
        .prepare_private_code_task(request.request_id, node, &data, "")
        .await
        .is_err());
    std::fs::rename(state.join("saved.json"), &receipt).unwrap();
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    match a.claim_card().await.unwrap() {
        Claim::Leased { card, .. } => {
            assert_eq!(card.id, request.request_id);
            assert!(!card.requires_internet);
            assert_eq!(
                card.required_capabilities["prepared_workspace_root"],
                json!(prepared)
            );
        }
        _ => panic!("prepared task should be claimable offline"),
    }
    assert!(s
        .prepare_private_code_task(request.request_id, node, &data, "")
        .await
        .is_err());
    // Explicit recovery never steals a live lease or lets another node retry the task.
    assert!(s
        .retry_private_code_task(request.request_id, node, &data)
        .await
        .is_err());
    a.fail_card(request.request_id, "acceptance check failed")
        .await
        .unwrap();
    assert!(s
        .retry_private_code_task(request.request_id, other, &data)
        .await
        .is_err());
    std::fs::rename(&receipt, state.join("saved.json")).unwrap();
    assert!(s
        .retry_private_code_task(request.request_id, node, &data)
        .await
        .is_err());
    assert_eq!(
        s.private_code_task_statuses(p, node).unwrap()[0].status,
        "blocked"
    );
    std::fs::rename(state.join("saved.json"), &receipt).unwrap();
    let before = s
        .transaction(|tx| {
            tx.query_row(
                "SELECT data FROM cards WHERE id=?1",
                [request.request_id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .map_err(db_error)
        })
        .unwrap();
    s.retry_private_code_task(request.request_id, node, &data)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "preserve unfinished work"
    );
    assert_eq!(
        s.private_code_task_statuses(p, node).unwrap()[0].status,
        "ready"
    );
    assert!(s
        .retry_private_code_task(request.request_id, node, &data)
        .await
        .is_err());
    assert_eq!(id(a.claim_card().await.unwrap()), request.request_id);
    s.transaction(|tx| {
        tx.execute(
            "UPDATE leases SET expires=0 WHERE card_id=?1",
            [request.request_id.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    s.retry_private_code_task(request.request_id, node, &data)
        .await
        .unwrap();
    s.transaction(|tx| {
        let after: String = tx
            .query_row(
                "SELECT data FROM cards WHERE id=?1",
                [request.request_id.to_string()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        assert_eq!(before, after);
        let count: i64 = tx
            .query_row(
                "SELECT count(*) FROM activity WHERE kind='private_task_retry'",
                [],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        assert_eq!(count, 2);
        tx.execute(
            "UPDATE cards SET status='review' WHERE id=?1",
            [request.request_id.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    assert!(s
        .retry_private_code_task(request.request_id, node, &data)
        .await
        .is_err());
    remove_temp_dir(data);
}

#[cfg(feature = "sandbox")]
#[tokio::test]
async fn private_preparation_failure_keeps_job_blocked() {
    let (s, a, _, p) = fixture().await;
    let node = Uuid::parse_str(&a.with_node(|_, node| Ok(node.to_owned())).unwrap()).unwrap();
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: "https://github.com/example/private".into(),
            repo_ref: None,
        }),
    )
    .unwrap();
    let request = private_code_tasks::PrivateCodeTaskRequest {
        request_id: Uuid::new_v4(),
        project_id: p,
        target_node_id: node,
        title: "Failure fixture".into(),
        task: "Do not execute".into(),
        model_id: None,
        max_turns: 2,
        acceptance: vec![],
    };
    s.stage_private_code_task(&request).unwrap();
    let data = std::env::temp_dir().join(format!("hive-private-failure-{}", Uuid::new_v4()));
    assert!(s
        .prepare_private_code_task(request.request_id, node, &data, "")
        .await
        .is_err());
    let mut eligible = caps();
    eligible.allow_internet = true;
    a.check_in(&eligible, None).await.unwrap();
    assert!(matches!(a.claim_card().await.unwrap(), Claim::NothingToDo));
    let root = data
        .join("code-workspaces")
        .join(request.request_id.to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("partial"), "keep").unwrap();
    assert!(s
        .prepare_private_code_task(request.request_id, node, &data, "synthetic")
        .await
        .is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("partial")).unwrap(),
        "keep"
    );
    remove_temp_dir(data);
}

#[tokio::test]
async fn private_submission_is_frozen_idempotent_and_not_claimable_before_preparation() {
    let (s, a, _, p) = fixture().await;
    let node = Uuid::parse_str(&a.with_node(|_, node| Ok(node.to_owned())).unwrap()).unwrap();
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: "https://github.com/example/original.git".into(),
            repo_ref: Some("main".into()),
        }),
    )
    .unwrap();
    let request = private_code_tasks::PrivateCodeTaskRequest {
        request_id: Uuid::new_v4(),
        project_id: p,
        target_node_id: node,
        title: "Approved task".into(),
        task: "Add a readme".into(),
        model_id: None,
        max_turns: 4,
        acceptance: vec![],
    };
    let original = s.stage_private_code_task(&request).unwrap();
    let statuses = s.private_code_task_statuses(p, node).unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].id, request.request_id);
    assert_eq!(statuses[0].status, "blocked");
    assert!(statuses[0].workspace.is_none());
    assert!(s
        .private_code_task_statuses(p, Uuid::new_v4())
        .unwrap()
        .is_empty());
    assert!(s
        .private_code_task_statuses(Uuid::new_v4(), node)
        .unwrap()
        .is_empty());
    s.set_project_repository(p, None).unwrap();
    let retry = s.stage_private_code_task(&request).unwrap();
    assert_eq!(encode(&original).unwrap(), encode(&retry).unwrap());
    assert_eq!(
        retry.required_capabilities["repo_url"],
        "https://github.com/example/original.git"
    );
    let mut eligible = caps();
    eligible.allow_internet = true;
    a.check_in(&eligible, None).await.unwrap();
    assert!(matches!(a.claim_card().await.unwrap(), Claim::NothingToDo));
    s.transaction(|tx| {
        let count: i64 = tx
            .query_row("SELECT count(*) FROM cards", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let reason: String = tx
            .query_row("SELECT reason FROM cards", [], |r| r.get(0))
            .unwrap();
        assert_eq!(reason, "awaiting_repository_preparation");
        Ok(())
    })
    .unwrap();
    let mut changed = request.clone();
    changed.task = "Different task".into();
    assert!(s.stage_private_code_task(&changed).is_err());
    changed = request.clone();
    changed.target_node_id = Uuid::new_v4();
    assert!(s.stage_private_code_task(&changed).is_err());
    changed = request.clone();
    changed.request_id = Uuid::new_v4();
    assert!(s.stage_private_code_task(&changed).is_err()); // project disconnected
}

#[tokio::test]
async fn private_submission_rejects_unknown_targets_and_invalid_checks_without_rows() {
    let (s, a, _, p) = fixture().await;
    let node = Uuid::parse_str(&a.with_node(|_, node| Ok(node.to_owned())).unwrap()).unwrap();
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: "https://github.com/example/repo".into(),
            repo_ref: None,
        }),
    )
    .unwrap();
    let mut request = private_code_tasks::PrivateCodeTaskRequest {
        request_id: Uuid::new_v4(),
        project_id: p,
        target_node_id: Uuid::new_v4(),
        title: "Task".into(),
        task: "Approved instructions".into(),
        model_id: None,
        max_turns: 4,
        acceptance: vec![],
    };
    assert!(s.stage_private_code_task(&request).is_err());
    request.target_node_id = node;
    request.acceptance = vec![crate::acceptance::AcceptanceCheck {
        name: "Invalid".into(),
        command: "".into(),
        args: vec![],
        cwd: None,
        expect_exit: 0,
        required: true,
    }];
    assert!(s.stage_private_code_task(&request).is_err());
    request.acceptance.clear();
    s.revoke(node).unwrap();
    assert!(s.stage_private_code_task(&request).is_err());
    s.transaction(|tx| {
        assert_eq!(
            tx.query_row("SELECT count(*) FROM cards", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn child_repository_comes_from_parent_snapshot_and_retries_survive_project_changes() {
    let (s, a, _, p) = fixture().await;
    let mut c = card(p, "parent");
    c.modality = "code".into();
    c.required_capabilities = json!({"brain":"local", "repo_url":"https://github.com/example/original.git", "repo_ref":"stable"});
    s.add_card(c.clone()).unwrap();
    let mut eligible = caps();
    eligible.allow_internet = true;
    a.check_in(&eligible, None).await.unwrap();
    assert_eq!(id(a.claim_card().await.unwrap()), c.id);
    let child = a
        .spawn_child_card(
            c.id,
            "child",
            "Child",
            "code",
            "task",
            "done",
            json!({"brain":"local"}),
        )
        .await
        .unwrap();
    assert!(child.requires_internet);
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: "https://github.com/example/changed.git".into(),
            repo_ref: None,
        }),
    )
    .unwrap();
    let retry = a
        .spawn_child_card(
            c.id,
            "child",
            "Child",
            "code",
            "task",
            "done",
            json!({"brain":"local"}),
        )
        .await
        .unwrap();
    assert_eq!(child.card_id, retry.card_id);
    s.transaction(|tx| {
        let raw: String = tx
            .query_row(
                "SELECT data FROM cards WHERE id=?1",
                [child.card_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        let child: ClaimedCard = decode(&raw).unwrap();
        assert_eq!(
            child.required_capabilities["repo_url"],
            c.required_capabilities["repo_url"]
        );
        assert_eq!(child.required_capabilities["repo_ref"], "stable");
        Ok(())
    })
    .unwrap();
    let mut explicit = json!({"workspace_path":"/existing"});
    repository::inherit_parent_repository(&c, "code", &mut explicit).unwrap();
    assert!(explicit.get("repo_url").is_none());
    c.required_capabilities =
        json!({"workspace_path":"/parent", "repo_url":"https://github.com/example/ignored"});
    let mut child = json!({});
    repository::inherit_parent_repository(&c, "code", &mut child).unwrap();
    assert_eq!(child, json!({}));
}

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
fn migration_names_are_unique_and_in_order() {
    let names: Vec<&str> = super::MIGRATIONS.iter().map(|m| m.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names, sorted, "migration names must be unique and ordered");
}

#[test]
fn a_migration_this_build_never_heard_of_is_reported_not_erased() {
    let s = LocalHubStore::in_memory().unwrap();
    let db = Arc::try_unwrap(s.db).ok().unwrap().into_inner().unwrap();
    db.execute_batch(
        "INSERT INTO applied_migrations(name, applied_at) VALUES('9999-from-a-newer-build', 0)",
    )
    .unwrap();

    let err = match LocalHubStore::from_connection(db) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("opening must be refused"),
    };
    assert!(
        err.contains("9999-from-a-newer-build"),
        "the refusal must name the migration it does not know: {err}"
    );
}

#[test]
fn a_counter_era_database_from_a_newer_build_is_refused() {
    let s = LocalHubStore::in_memory().unwrap();
    let db = Arc::try_unwrap(s.db).ok().unwrap().into_inner().unwrap();
    // No applied_migrations table is what a database written by the counter-era ladder looks
    // like, and a counter past the end of this list is one a newer build moved.
    let ahead = super::MIGRATIONS.len() + 1;
    db.execute_batch(&format!(
        "DROP TABLE applied_migrations; PRAGMA user_version={ahead};"
    ))
    .unwrap();

    let err = match LocalHubStore::from_connection(db) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("opening must be refused"),
    };
    assert!(
        err.contains(&ahead.to_string()) && err.contains(&super::MIGRATIONS.len().to_string()),
        "the refusal must say how far ahead the database is: {err}"
    );
}

#[test]
fn project_repository_migration_preserves_existing_projects() {
    let s = LocalHubStore::in_memory().unwrap();
    let p = s.create_project("Existing", "keep").unwrap();
    let db = Arc::try_unwrap(s.db).ok().unwrap().into_inner().unwrap();
    super::rewind_to(&db, 13, "DROP TABLE private_coding_readiness; DROP TABLE private_preparation_recoveries; DROP TABLE private_run_retries; DROP TABLE private_run_stops; DROP TABLE private_runs; DROP TABLE private_preparations; ALTER TABLE agent_deliveries DROP COLUMN lease_deadline; DROP TABLE project_repositories;");
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
        prepared_workspace_root: None,
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
    remove_temp_dir(path);
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
                // Includes coding readiness (v21).
                crate::local_hub::MIGRATIONS.len() as i64
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
            request: LocalTurnRequest,
        ) -> Result<LocalTurnOutcome, LocalTurnError> {
            assert!(request
                .participants_note
                .contains("Cite sources for factual claims"));
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
            for agent in &agents {
                store
                    .bots_agent_bio_set(
                        owner,
                        agent.id,
                        agent.name.clone(),
                        AgentBio {
                            bio: "Research teammate".into(),
                            instructions: "Cite sources for factual claims".into(),
                            avatar: "sif".into(),
                            revision: 0,
                        },
                    )
                    .unwrap();
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
            assert_eq!(version, crate::local_hub::MIGRATIONS.len() as i64);
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
        remove_temp_dir(path);
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
        remove_temp_dir(dir);
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
    remove_temp_dir(dir);
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

#[tokio::test]
async fn scoped_worker_claims_only_selected_card() {
    let (s, a, _, p) = fixture().await;
    let first = card(p, "first");
    let selected = card(p, "selected");
    s.add_card(first.clone()).unwrap();
    s.add_card(selected.clone()).unwrap();
    let a = a.restricted_to_card(selected.id);
    assert_eq!(id(a.claim_card().await.unwrap()), selected.id);
    a.release_card(selected.id, "test finished").await.unwrap();
    let a = a.restricted_to_card(Uuid::new_v4());
    assert!(matches!(a.claim_card().await.unwrap(), Claim::NothingToDo));
    s.transaction(|tx| {
        let status: String = tx
            .query_row(
                "SELECT status FROM cards WHERE id=?1",
                [first.id.to_string()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        assert_eq!(status, "ready");
        Ok(())
    })
    .unwrap();
}

/// Track E item 4: a node may drain deliveries for agents it hosts, and for no others.
///
/// This is the load-bearing test for the whole remote-delivery surface, and the negative half is
/// the point. Both nodes here belong to the SAME verified owner, which is exactly the case owner
/// scoping cannot catch -- `bots_transport_two_clients_enforce_owner_binding` above already
/// covers cross-account, and would pass just as happily if host authorization did not exist at
/// all. Audit 3.6 named this hole; without the `preferred_host` check, node B could claim node
/// A's agent's delivery and post a reply in that agent's voice.
#[cfg(feature = "bots")]
#[tokio::test]
async fn bots_delivery_is_claimable_only_by_the_hosting_node() {
    use crate::bots::*;
    let store = LocalHubStore::in_memory().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(store.clone(), listener, async {
        let _ = rx.await;
    }));
    let ca = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "hostA")
        .await
        .unwrap();
    let cb = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "hostB")
        .await
        .unwrap();
    let a = RemoteLocalHub::new(&url, ca.raw_key).unwrap();
    let b = RemoteLocalHub::new(&url, cb.raw_key).unwrap();

    // One owner, two of their machines. This is the shape that makes the test meaningful.
    let owner = Uuid::new_v4();
    store.set_node_owner(ca.node_id, owner).unwrap();
    store.set_node_owner(cb.node_id, owner).unwrap();

    // An agent hosted by A only.
    let agent = a
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Hosted by A".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: Some(ca.node_id),
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "host-auth-test".into(),
        })
        .await
        .unwrap();

    // A conversation with that agent as a member, and a user message addressed to it -- which is
    // what creates the pending delivery the executor would drain.
    let conversation = a
        .bots_conversations_create(NewConversation {
            title: Some("host authorization".into()),
            owner,
            kind: ConversationKind::AgentDm,
            project_id: None,
            coordinator: None,
            storage_scope: StorageScope::LocalOnly,
        })
        .await
        .unwrap();
    a.bots_conversations_join(Principal::Agent(agent.id), conversation.id)
        .await
        .unwrap();
    let prompt = a
        .bots_message_send(
            Principal::User(owner),
            conversation.id,
            "host-auth-1".into(),
            conversation.policy_revision,
            vec![agent.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("who hosts you?".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .await
        .unwrap();
    let key = DeliveryKey {
        message_id: prompt.id,
        recipient: agent.id,
    };

    // THE NEGATIVE HALF. B is a fully paired, same-owner, authenticated node. It must still be
    // refused, because it does not host this agent.
    let stolen = b.bots_delivery_claim(key).await;
    assert!(
        stolen.is_err(),
        "a node that does not host an agent must not claim its delivery"
    );

    // And the refusal must not have consumed the delivery: the rightful host still gets it. A
    // guard that rejected B by burning the claim would look identical in the assertion above and
    // would silently break the real path.
    let claimed = a
        .bots_delivery_claim(key)
        .await
        .expect("the hosting node must still be able to claim after the refusal");

    // B cannot resolve it either, in either direction, even knowing the lease generation.
    assert!(
        b.bots_delivery_complete(key, claimed.lease_generation)
            .await
            .is_err(),
        "a non-host must not complete a delivery"
    );
    assert!(
        b.bots_delivery_fail(key, claimed.lease_generation, None)
            .await
            .is_err(),
        "a non-host must not fail a delivery"
    );

    // Nor speak as the agent. This is the Audit 3.6 case directly: minting an agent-authored
    // message from a machine that does not run that agent.
    assert!(
        b.bots_message_send_with_cause(
            Principal::Agent(agent.id),
            conversation.id,
            "host-auth-impersonate".into(),
            conversation.policy_revision,
            vec![],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("I am not who I say I am".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
            None,
            false,
        )
        .await
        .is_err(),
        "a non-host must not post as that agent"
    );

    // THE POSITIVE HALF, so the guard is not merely refusing everything: A does all of it.
    a.bots_message_send_with_cause(
        Principal::Agent(agent.id),
        conversation.id,
        "host-auth-reply".into(),
        conversation.policy_revision,
        vec![],
        NewMessage {
            thread_root: Some(prompt.id),
            kind: MessageKind::Text,
            body: Some("A hosts me.".into()),
            attachment_refs: vec![],
            task_ref: None,
            turn_ref: None,
            source_event_ref: None,
        },
        Some(DeliveryCause {
            cause_message_id: prompt.id,
            root_message_id: prompt.id,
            depth: 1,
        }),
        false,
    )
    .await
    .expect("the hosting node must be able to reply as its own agent");
    a.bots_delivery_complete(key, claimed.lease_generation)
        .await
        .expect("the hosting node must be able to complete its own delivery");
    assert!(
        a.bots_turns_for_root(prompt.id).await.unwrap() >= 1,
        "the reply must be attributed to the thread so loop budgets can count it"
    );

    // An agent no machine has claimed is not a free-for-all: `preferred_host: None` must fail
    // closed rather than match whoever asks.
    let unhosted = a
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Hosted by nobody".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "host-auth-unhosted".into(),
        })
        .await
        .unwrap();
    assert!(
        a.bots_delivery_claim(DeliveryKey {
            message_id: prompt.id,
            recipient: unhosted.id,
        })
        .await
        .is_err(),
        "an agent with no host must not be claimable by any node"
    );

    // ...but "no host" means something different for a BYOK agent, and conflating the two is
    // what broke every remote node's Claude and Nous deliveries in the field: the turn is spent
    // hub-side, so `ensure_provider_agents` deliberately leaves `preferred_host` unset and the
    // executor routes the delivery to whichever of the owner's machines has a cloud runner. A
    // guard that demanded an exact host match refused precisely the work the executor had just
    // decided was its own -- silently, once per poll. B is the node that is NOT pinned to
    // anything here, which is the whole point of claiming from it.
    let byok = a
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Hub-side provider".into(),
            runtime_kind: AgentRuntimeKind::AnthropicByok,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "host-auth-byok".into(),
        })
        .await
        .unwrap();
    a.bots_conversations_join(Principal::Agent(byok.id), conversation.id)
        .await
        .unwrap();
    let byok_prompt = a
        .bots_message_send(
            Principal::User(owner),
            conversation.id,
            "host-auth-byok-1".into(),
            conversation.policy_revision,
            vec![byok.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("who spends your key?".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .await
        .unwrap();
    b.bots_delivery_claim(DeliveryKey {
        message_id: byok_prompt.id,
        recipient: byok.id,
    })
    .await
    .expect("an unpinned BYOK agent must be claimable by any of the owner's nodes");

    // Owner scoping is not relaxed along with the pin: a different account's unpinned BYOK agent
    // is still refused, so "unpinned" widens the set of *the owner's* machines, not of machines.
    let stranger = Uuid::new_v4();
    let theirs = store
        .bots_agents_create(NewAgentProfile {
            owner: stranger,
            name: "Someone else's provider".into(),
            runtime_kind: AgentRuntimeKind::AnthropicByok,
            preferred_host: None,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: "host-auth-byok-stranger".into(),
        })
        .unwrap();
    assert!(
        b.bots_delivery_claim(DeliveryKey {
            message_id: byok_prompt.id,
            recipient: theirs.id,
        })
        .await
        .is_err(),
        "an unpinned BYOK agent of another account must still be refused"
    );

    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn private_remote_staging_requires_verified_same_owner_hosts_and_freezes_target() {
    let (s, a, b, p) = fixture().await;
    let node =
        |h: &LocalHub| Uuid::parse_str(&h.with_node(|_, n| Ok(n.to_owned())).unwrap()).unwrap();
    let an = node(&a);
    let bn = node(&b);
    let request = private_code_tasks::PrivateCodeTaskRequest {
        request_id: Uuid::new_v4(),
        project_id: p,
        target_node_id: bn,
        title: "Remote fixture".into(),
        task: "Write a marker".into(),
        model_id: Some("fixture".into()),
        max_turns: 2,
        acceptance: vec![],
    };
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: "https://github.com/example/fixture.git".into(),
            repo_ref: None,
        }),
    )
    .unwrap();
    assert!(a.private_execution_hosts().is_err());
    assert!(a.private_code_task_stage(&request).is_err());
    let owner = Uuid::new_v4();
    s.set_node_owner(an, owner).unwrap();
    s.set_node_owner(bn, owner).unwrap();
    // Merely binding an account is not signed enrollment.
    assert!(a.private_execution_hosts().is_err());
    s.transaction(|tx| {
        tx.execute(
            "UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust='fixture' WHERE id=1",
            params![Uuid::new_v4().to_string(), owner.to_string()],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",
            params![Uuid::new_v4().to_string(), an.to_string(), now()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(a.private_execution_hosts().unwrap().len(), 1);
    assert!(a.private_code_task_stage(&request).is_err());
    s.transaction(|tx| {
        tx.execute(
            "INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",
            params![Uuid::new_v4().to_string(), bn.to_string(), now()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(a.private_execution_hosts().unwrap().len(), 2);
    // Exercise the authenticated wire protocol with a distinct enrolled controller.
    let controller = s.enroll_owner("controller").unwrap();
    s.set_node_owner(controller.node_id, owner).unwrap();
    s.transaction(|tx| {
        tx.execute(
            "INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",
            params![
                Uuid::new_v4().to_string(),
                controller.node_id.to_string(),
                now()
            ],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server_store = s.clone();
    let server = tokio::spawn(async move {
        transport::serve(server_store, listener, async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    let remote = RemoteLocalHub::new(&url, controller.raw_key).unwrap();
    assert_eq!(remote.private_execution_hosts().await.unwrap().len(), 3);
    let card = remote.private_code_task_stage(&request).await.unwrap();
    assert_eq!(
        remote.private_code_task_stage(&request).await.unwrap().id,
        card.id
    );
    let invalid = RemoteLocalHub::new(&url, "not-a-key".into()).unwrap();
    assert!(invalid.private_execution_hosts().await.is_err());
    assert!(invalid.private_code_task_stage(&request).await.is_err());
    stop.send(()).unwrap();
    server.await.unwrap();
    assert_eq!(card.required_capabilities["target_node_id"], json!(bn));
    assert!(card
        .required_capabilities
        .get("prepared_workspace_root")
        .is_none());
    assert_eq!(a.private_code_task_stage(&request).unwrap().id, card.id);
    let mut altered = request.clone();
    altered.target_node_id = an;
    assert!(a.private_code_task_stage(&altered).is_err());
    altered = request.clone();
    altered.task = "Different work".into();
    assert!(a.private_code_task_stage(&altered).is_err());
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    s.transaction(|tx| {
        tx.execute(
            "UPDATE nodes SET owner_member_id=?2 WHERE id=?1",
            params![bn.to_string(), Uuid::new_v4().to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(a.private_execution_hosts().unwrap().len(), 2);
    assert!(a.private_code_task_stage(&request).is_err());
    assert!(b.private_execution_hosts().is_err());
    s.transaction(|tx| {
        tx.execute(
            "UPDATE nodes SET owner_member_id=?2 WHERE id=?1",
            params![bn.to_string(), owner.to_string()],
        )
        .unwrap();
        tx.execute(
            "UPDATE local_node_keys SET revoked=1 WHERE node_id=?1",
            [bn.to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(a.private_code_task_stage(&request).is_err());
    assert!(b.private_code_task_stage(&request).is_err());
    let mut injected = serde_json::to_value(&request).unwrap();
    injected["prepared_workspace_root"] = json!("/tmp/other-host");
    assert!(
        serde_json::from_value::<private_code_tasks::PrivateCodeTaskRequest>(injected).is_err()
    );
}

#[tokio::test]
async fn private_preparation_is_target_session_bound_and_never_starts_work() {
    let (s, a, b, p) = fixture().await;
    let node =
        |h: &LocalHub| Uuid::parse_str(&h.with_node(|_, n| Ok(n.to_owned())).unwrap()).unwrap();
    let owner = Uuid::new_v4();
    let an = node(&a);
    let bn = node(&b);
    let req = private_code_tasks::PrivateCodeTaskRequest {
        request_id: Uuid::new_v4(),
        project_id: p,
        target_node_id: bn,
        title: "Prepare remotely".into(),
        task: "Write a marker".into(),
        model_id: None,
        max_turns: 2,
        acceptance: vec![],
    };
    assert!(a.private_preparation_take().is_err());
    assert!(a.private_coding_pending().is_err());
    assert!(a.private_coding_tasks(p).is_err());
    for n in [an, bn] {
        s.set_node_owner(n, owner).unwrap();
    }
    s.transaction(|tx| {
        tx.execute(
            "UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust='fixture' WHERE id=1",
            params![Uuid::new_v4().to_string(), owner.to_string()],
        )
        .unwrap();
        for n in [an, bn] {
            tx.execute(
                "INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",
                params![Uuid::new_v4().to_string(), n.to_string(), now()],
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    s.set_project_repository(
        p,
        Some(&repository::ProjectRepository {
            repo_url: "https://github.com/example/preparation.git".into(),
            repo_ref: None,
        }),
    )
    .unwrap();
    a.private_code_task_stage(&req).unwrap();
    let overview = a.private_coding_tasks(p).unwrap();
    assert_eq!(overview.len(), 1);
    assert_eq!(overview[0].target_node_id, bn);
    assert!(overview[0].preparation_id.is_none());
    let op = Uuid::new_v4();
    assert_eq!(
        a.private_preparation_request(op, req.request_id)
            .unwrap()
            .state,
        "queued"
    );
    assert_eq!(
        a.private_preparation_request(op, req.request_id)
            .unwrap()
            .state,
        "queued"
    );
    assert!(a
        .private_preparation_request(Uuid::new_v4(), req.request_id)
        .is_err());
    assert!(a.private_preparation_request(op, Uuid::new_v4()).is_err());
    assert!(a.private_preparation_take().unwrap().is_none());
    assert!(b
        .private_preparation_complete(op, &absolute("checkout"))
        .is_err());
    assert!(!a.private_coding_pending().unwrap().preparation);
    assert!(b.private_coding_pending().unwrap().preparation);
    assert_eq!(
        a.private_coding_tasks(p).unwrap()[0].preparation_id,
        Some(op)
    );
    let work = b.private_preparation_take().unwrap().unwrap();
    assert_eq!(work.operation_id, op);
    assert_eq!(
        b.private_preparation_take().unwrap().unwrap().operation_id,
        op
    );
    assert_eq!(a.private_preparation_status(op).unwrap().state, "claimed");
    assert!(a
        .private_preparation_complete(op, &absolute("checkout"))
        .is_err());
    let mut different_session = b.clone();
    different_session.session = Uuid::new_v4();
    assert!(
        !different_session
            .private_coding_pending()
            .unwrap()
            .preparation
    );
    assert!(different_session.private_preparation_take().is_err());
    assert!(different_session
        .private_preparation_complete(op, &absolute("checkout"))
        .is_err());
    assert!(b.private_preparation_complete(op, "relative/path").is_err());
    assert_eq!(
        b.private_preparation_complete(op, &absolute("checkout"))
            .unwrap()
            .state,
        "prepared"
    );
    assert_eq!(
        b.private_preparation_complete(op, &absolute("checkout"))
            .unwrap()
            .state,
        "prepared"
    );
    assert!(b
        .private_preparation_complete(op, &absolute("different"))
        .is_err());
    assert_eq!(
        a.private_preparation_request(op, req.request_id)
            .unwrap()
            .state,
        "prepared"
    );
    assert!(b.private_preparation_take().unwrap().is_none());
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    assert_eq!(
        s.private_code_task_statuses(p, bn).unwrap()[0]
            .reason
            .as_deref(),
        Some("awaiting_private_run")
    );
    assert!(!b.private_coding_pending().unwrap().preparation);
    let run = Uuid::new_v4();
    assert_eq!(
        a.private_run_request(run, req.request_id).unwrap().state,
        "queued"
    );
    assert_eq!(b.private_coding_pending().unwrap().run, Some(run));
    assert!(a.private_coding_pending().unwrap().run.is_none());
    assert_eq!(
        a.private_coding_tasks(p).unwrap()[0]
            .run
            .as_ref()
            .unwrap()
            .operation_id,
        run
    );

    assert_eq!(
        a.private_run_request(run, req.request_id).unwrap().state,
        "queued"
    );
    assert!(a
        .private_run_request(Uuid::new_v4(), req.request_id)
        .is_err());
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    assert!(matches!(
        a.private_run_claim(run).await.unwrap(),
        Claim::NothingToDo
    ));
    assert_eq!(id(b.private_run_claim(run).await.unwrap()), req.request_id);
    assert!(matches!(
        b.private_run_claim(run).await.unwrap(),
        Claim::AlreadyLeased
    ));
    assert!(a.private_run_status(run).unwrap().lease_active);
    assert!(a.private_run_retry(run, Uuid::new_v4()).is_err());
    // Losing the worker/lease cannot create a second execution attempt.
    s.transaction(|tx| {
        tx.execute(
            "UPDATE leases SET expires=0 WHERE card_id=?1",
            [req.request_id.to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        b.private_run_claim(run).await.unwrap(),
        Claim::NothingToDo
    ));
    assert!(matches!(b.claim_card().await.unwrap(), Claim::NothingToDo));
    assert!(!a.private_run_status(run).unwrap().lease_active);
    assert_eq!(a.private_run_status(run).unwrap().state, "blocked");
    // Even if a repair restores card readiness, consumed authorization cannot run it again.
    s.transaction(|tx| {
        tx.execute(
            "UPDATE cards SET status='ready',reason=NULL WHERE id=?1",
            [req.request_id.to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        b.private_run_claim(run).await.unwrap(),
        Claim::NothingToDo
    ));
    assert_eq!(
        a.private_run_request(run, req.request_id).unwrap().state,
        "interrupted"
    );
    let retry = Uuid::new_v4();
    assert_eq!(a.private_run_retry(run, retry).unwrap().state, "queued");
    assert_eq!(a.private_run_retry(run, retry).unwrap().state, "queued");
    assert!(a.private_run_retry(run, Uuid::new_v4()).is_err());
    assert_eq!(a.private_run_status(run).unwrap().state, "superseded");
    assert_eq!(a.private_run_stop(run).unwrap().state, "superseded");
    assert!(!a.private_run_status(retry).unwrap().stop_requested);
    assert!(matches!(
        b.private_run_claim(retry).await.unwrap(),
        Claim::NothingToDo
    ));
    assert!(a.private_run_ready(retry).is_err());
    b.private_run_ready(retry).unwrap();
    // Old session cannot claim a new attempt, so its delayed output can never own the new lease.
    assert!(matches!(
        b.private_run_claim(retry).await.unwrap(),
        Claim::NothingToDo
    ));
    let mut fresh = b.clone();
    fresh.session = Uuid::new_v4();
    assert_eq!(
        id(fresh.private_run_claim(retry).await.unwrap()),
        req.request_id
    );
    assert!(b
        .complete_card(req.request_id, "stale", None, Usage::default())
        .await
        .is_err());
    fresh
        .complete_card(req.request_id, "retry succeeded", None, Usage::default())
        .await
        .unwrap();
    assert_eq!(a.private_run_status(retry).unwrap().state, "finished");
    assert!(!a.private_run_status(run).unwrap().lease_active);
    assert!(a.private_run_retry(retry, Uuid::new_v4()).is_err());
    let mut queued_runs = Vec::new();
    for _ in 0..2 {
        let mut next = req.clone();
        next.request_id = Uuid::new_v4();
        a.private_code_task_stage(&next).unwrap();
        let prep = Uuid::new_v4();
        a.private_preparation_request(prep, next.request_id)
            .unwrap();
        b.private_preparation_take().unwrap().unwrap();
        b.private_preparation_complete(prep, &absolute("fixture"))
            .unwrap();
        let next_run = Uuid::new_v4();
        a.private_run_request(next_run, next.request_id).unwrap();
        queued_runs.push(next_run);
    }
    assert_eq!(a.private_run_stop(queued_runs[0]).unwrap().state, "stopped");
    assert_eq!(a.private_run_stop(queued_runs[0]).unwrap().state, "stopped");
    assert!(matches!(
        b.private_run_claim(queued_runs[0]).await.unwrap(),
        Claim::NothingToDo
    ));
    assert!(b.private_run_work(queued_runs[0]).is_err());
    assert_eq!(
        a.private_run_status(queued_runs[1]).unwrap().state,
        "queued"
    );
    assert!(b.private_run_work(queued_runs[1]).is_ok());
    assert_eq!(
        b.private_coding_pending().unwrap().run,
        Some(queued_runs[1])
    );
    // Even valid credentials from a foreign owner cannot discover this fleet's tasks.
    let foreign = s.enroll_owner("foreign").unwrap();
    s.set_node_owner(foreign.node_id, Uuid::new_v4()).unwrap();
    assert!(s
        .connect(&foreign.raw_key)
        .unwrap()
        .private_coding_tasks(p)
        .is_err());
    s.transaction(|tx| {
        tx.execute(
            "UPDATE local_node_keys SET revoked=1 WHERE node_id=?1",
            [bn.to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(a.private_coding_tasks(p).unwrap().is_empty());
    assert!(b.private_coding_pending().is_err());
    assert!(a.private_preparation_status(op).is_err());
    assert!(b
        .private_preparation_complete(op, &absolute("checkout"))
        .is_err());
}

/// A claim was fenced but never expired, which answered "can a stale holder resolve this" and
/// left "what if nobody holds it" unasked. A worker killed mid-turn -- Ctrl-C, crash, sleep,
/// pkill -- left `status='running'` forever, and because the executor counts running rows
/// against `max_active_turns_per_agent`, one orphan silenced that agent permanently. It looked
/// exactly like a healthy worker draining an empty queue, which is how it survived a whole
/// evening (2026-09-17: Jotunheim, four queued messages, one zombie row; clearing it by hand
/// produced `delivered=4` on the very next pass).
#[test]
fn an_abandoned_delivery_is_requeued_and_its_dead_holder_is_fenced_out() {
    use crate::bots::*;
    let store = LocalHubStore::in_memory().unwrap();
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let (agent, _conversation, prompt) = lease_fixture(&store, owner, host, "lease-reap");
    let key = DeliveryKey {
        message_id: prompt,
        recipient: agent,
    };

    let claimed = store.bots_delivery_claim(key).unwrap();

    // Still running, so it must NOT be reaped. This half matters more than the other: reaping a
    // live turn duplicates work and can put two replies under one message.
    assert!(
        store
            .bots_deliveries_pending_for_agent(agent, 200)
            .unwrap()
            .is_empty(),
        "a live turn must not be requeued out from under its worker"
    );
    assert!(
        store.bots_delivery_claim(key).is_err(),
        "and it must not be claimable by anyone else while it runs"
    );

    // The worker dies here. Nothing calls complete or fail; the row just sits. Age the lease
    // rather than sleeping out a ten-minute constant.
    age_lease(&store, key);

    let pending = store.bots_deliveries_pending_for_agent(agent, 200).unwrap();
    assert_eq!(
        pending.len(),
        1,
        "an expired lease must return the delivery to the queue"
    );
    assert_eq!(pending[0].key.message_id, prompt);
    assert!(
        pending[0].lease_generation > claimed.lease_generation,
        "the generation must move, or the dead holder could still resolve it"
    );

    // THE FENCING HALF. The original worker was not necessarily dead -- it may have been slow
    // past the lease, or partitioned, and it can come back holding the old generation. It must
    // not resolve a delivery someone else now owns.
    assert!(
        store
            .bots_delivery_complete(key, claimed.lease_generation)
            .is_err(),
        "the fenced-out holder must not be able to complete the delivery"
    );
    assert!(
        store
            .bots_delivery_fail(key, claimed.lease_generation, None)
            .is_err(),
        "nor fail it"
    );

    // And the requeued delivery is genuinely claimable again -- a reap that left it unclaimable
    // would trade a silent stall for a quieter one.
    let reclaimed = store
        .bots_delivery_claim(key)
        .expect("requeued work must be claimable");
    assert!(reclaimed.lease_generation > claimed.lease_generation);
    store
        .bots_delivery_complete(key, reclaimed.lease_generation)
        .expect("the new holder resolves it normally");
}

/// Rows written before the lease column existed read NULL, which is "no deadline recorded", not
/// "deadline long past". Reaping the whole in-flight backlog of a live fleet the moment the
/// migration lands would not be a recovery, it would be an outage.
#[test]
fn deliveries_with_no_recorded_lease_are_left_alone() {
    use crate::bots::*;
    let store = LocalHubStore::in_memory().unwrap();
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let (agent, _conversation, prompt) = lease_fixture(&store, owner, host, "lease-null");
    let key = DeliveryKey {
        message_id: prompt,
        recipient: agent,
    };
    let claimed = store.bots_delivery_claim(key).unwrap();

    // Exactly the shape a pre-migration row has: running, with no deadline.
    store
        .transaction(|tx| {
            tx.execute(
                "UPDATE agent_deliveries SET lease_deadline=NULL \
                 WHERE message_id=?1 AND recipient=?2",
                params![prompt.to_string(), agent.to_string()],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();

    assert!(
        store
            .bots_deliveries_pending_for_agent(agent, 200)
            .unwrap()
            .is_empty(),
        "a running delivery with no recorded deadline must not be reaped"
    );
    store
        .bots_delivery_complete(key, claimed.lease_generation)
        .expect("its original holder still owns it and can finish normally");
}

/// Push a claimed delivery's lease into the past. Beats sleeping past `DELIVERY_LEASE_SECS`, and
/// beats making the constant injectable purely so a test can shrink it -- the production value is
/// what these tests should be exercising.
fn age_lease(store: &LocalHubStore, key: crate::bots::DeliveryKey) {
    store
        .transaction(|tx| {
            tx.execute(
                "UPDATE agent_deliveries SET lease_deadline=1 \
                 WHERE message_id=?1 AND recipient=?2",
                params![key.message_id.to_string(), key.recipient.to_string()],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
}

/// One local agent, a room it belongs to, and a message addressed to it -- i.e. one pending
/// delivery, which is the starting state every lease test needs.
fn lease_fixture(
    store: &LocalHubStore,
    owner: Uuid,
    host: Uuid,
    tag: &str,
) -> (
    crate::bots::AgentId,
    crate::bots::ConversationId,
    crate::bots::MessageId,
) {
    use crate::bots::*;
    let agent = store
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Leaseholder".into(),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: Some(host),
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: tag.into(),
        })
        .unwrap();
    let conversation = store
        .bots_conversations_create(NewConversation {
            title: Some("lease".into()),
            owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: Some(agent.id),
            storage_scope: StorageScope::LocalOnly,
        })
        .unwrap();
    store
        .bots_conversations_join(Principal::Agent(agent.id), conversation.id)
        .unwrap();
    let prompt = store
        .bots_message_send(
            Principal::User(owner),
            conversation.id,
            format!("{tag}-prompt"),
            conversation.policy_revision,
            vec![agent.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("please reply".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .unwrap();
    (agent.id, conversation.id, prompt.id)
}

#[cfg(all(feature = "sandbox", feature = "llama-cpp"))]
#[tokio::test]
async fn remote_preparation_reconciles_host_checkout_and_survives_authority_reopen() {
    remote_preparation_scenario(false).await;
}
#[cfg(all(feature = "sandbox", feature = "llama-cpp"))]
#[tokio::test]
async fn remote_worker_preflights_and_obeys_task_specific_stop() {
    remote_preparation_scenario(true).await;
}
#[cfg(all(feature = "sandbox", feature = "llama-cpp"))]
async fn remote_preparation_scenario(stop_worker: bool) {
    let data = std::env::temp_dir().join(format!("hive-remote-prep-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&data).unwrap();
    let db = data.join("authority.sqlite3");
    let store = LocalHubStore::open(&db).unwrap();
    let controller = store.enroll_owner("Midgaard fixture").unwrap();
    let target = store.enroll_owner("Overgaard fixture").unwrap();
    let owner = Uuid::new_v4();
    for n in [controller.node_id, target.node_id] {
        store.set_node_owner(n, owner).unwrap();
    }
    store.transaction(|tx| {
        tx.execute("UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust='fixture' WHERE id=1",params![Uuid::new_v4().to_string(),owner.to_string()]).unwrap();
        for n in [controller.node_id,target.node_id] { tx.execute("INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",params![Uuid::new_v4().to_string(),n.to_string(),now()]).unwrap(); }
        Ok(())
    }).unwrap();
    let project = store
        .create_project("Remote fixture", "prepare only")
        .unwrap();
    let repo = "https://github.com/example/remote-prep.git";
    store
        .set_project_repository(
            project,
            Some(&repository::ProjectRepository {
                repo_url: repo.into(),
                repo_ref: None,
            }),
        )
        .unwrap();
    let task = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(store.clone(), listener, async {
        let _ = stopped.await;
    }));
    let coordinator = RemoteLocalHub::new(&url, controller.raw_key.clone()).unwrap();
    let mut worker = RemoteLocalHub::new(&url, target.raw_key.clone()).unwrap();
    coordinator
        .private_code_task_stage(&private_code_tasks::PrivateCodeTaskRequest {
            request_id: task,
            project_id: project,
            target_node_id: target.node_id,
            title: "Remote checkout".into(),
            task: "Write a marker".into(),
            model_id: if stop_worker {
                Some("fixture".into())
            } else {
                None
            },
            max_turns: 2,
            acceptance: vec![],
        })
        .await
        .unwrap();
    coordinator
        .private_preparation_request(operation, task)
        .await
        .unwrap();
    assert!(coordinator
        .private_preparation_take()
        .await
        .unwrap()
        .is_none());
    assert!(worker.private_coding_pending().await.unwrap().preparation);
    assert!(
        !coordinator
            .private_coding_pending()
            .await
            .unwrap()
            .preparation
    );
    let work = worker.private_preparation_take().await.unwrap().unwrap();
    assert_eq!(work.operation_id, operation);
    let recovery = Uuid::new_v4();
    assert_eq!(
        coordinator
            .private_preparation_recover(recovery, operation)
            .await
            .unwrap()
            .state,
        "queued"
    );
    assert_eq!(
        coordinator
            .private_preparation_recover(recovery, operation)
            .await
            .unwrap()
            .state,
        "queued"
    );
    assert!(coordinator
        .private_preparation_recover(Uuid::new_v4(), operation)
        .await
        .is_err());
    assert!(worker.private_preparation_take().await.is_err());
    assert!(worker
        .private_preparation_complete(operation, &absolute("stale"))
        .await
        .is_err());
    worker = RemoteLocalHub::new(&url, target.raw_key.clone()).unwrap();
    assert_eq!(
        worker
            .private_preparation_take()
            .await
            .unwrap()
            .unwrap()
            .operation_id,
        operation
    );
    // Replaying recovery after the replacement has claimed must not reset its ownership.
    assert_eq!(
        coordinator
            .private_preparation_recover(recovery, operation)
            .await
            .unwrap()
            .state,
        "claimed"
    );
    // A completed local checkout with a durable host receipt models a crash before ACK.
    // Real Git validation, no network Git fetch and no model inference.
    let worker_data = data.join("execution-host");
    let root = worker_data.join("code-workspaces").join(task.to_string());
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(out.status.success(), "fixture git failed");
        String::from_utf8(out.stdout).unwrap()
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.test",
        "commit",
        "--allow-empty",
        "-m",
        "fixture",
    ]);
    let base = git(&["rev-parse", "HEAD"]).trim().to_owned();
    let branch = format!("hive/{task}");
    git(&["checkout", "-b", &branch]);
    git(&["remote", "add", "origin", repo]);
    std::fs::write(root.join("keep.txt"), "preserve me").unwrap();
    let state = worker_data.join("code-workspace-state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join(format!("{task}.json")),json!({"version":1,"card":task,"repo":repo,"reference":null,"branch":branch,"cache":null,"base_commit":base}).to_string()).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(state.join(format!("{task}.lock")))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(worker
        .prepare_next_private_checkout(&worker_data, "")
        .await
        .is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "preserve me"
    );
    drop(lock);
    let result = worker
        .prepare_next_private_checkout(&worker_data, "unused-fixture-token")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.state, "prepared");
    assert_eq!(
        coordinator
            .private_preparation_recover(recovery, operation)
            .await
            .unwrap()
            .state,
        "prepared"
    );
    assert!(coordinator
        .private_preparation_recover(Uuid::new_v4(), operation)
        .await
        .is_err());
    assert_eq!(
        coordinator
            .private_preparation_status(operation)
            .await
            .unwrap()
            .state,
        "prepared"
    );
    let path = root.canonicalize().unwrap();
    assert_eq!(
        worker
            .private_preparation_complete(operation, path.to_str().unwrap())
            .await
            .unwrap()
            .state,
        "prepared"
    );
    assert!(worker
        .prepare_next_private_checkout(&worker_data, "")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "preserve me"
    );
    assert!(!data.join("code-workspaces").exists());
    let run = Uuid::new_v4();
    coordinator.private_run_request(run, task).await.unwrap();
    assert!(coordinator.private_run_work(run).await.is_err());
    if stop_worker {
        use axum::{
            routing::{get, post},
            Json, Router,
        };
        let started = Arc::new(tokio::sync::Notify::new());
        let signal = started.clone();
        let model = Router::new()
            .route(
                "/v1/models",
                get(|| async { Json(json!({"data":[{"id":"fixture"}]})) }),
            )
            .route("/api/tags", get(|| async { Json(json!({"models":[]})) }))
            .route(
                "/api/show",
                post(|| async { Json(json!({"capabilities":["tools","completion"]})) }),
            )
            .route(
                "/v1/chat/completions",
                post(move || {
                    let signal = signal.clone();
                    async move {
                        signal.notify_one();
                        std::future::pending::<Json<Value>>().await
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let mock = tokio::spawn(async move {
            axum::serve(listener, model).await.unwrap();
        });
        let backend = crate::backend::llama_cpp::LlamaCppBackend::new(&endpoint);
        worker
            .refresh_private_coding_readiness(&backend, true, true, true)
            .await
            .unwrap();
        let hosts = coordinator.private_coding_hosts().await.unwrap();
        let host = hosts
            .iter()
            .find(|h| h.host.node_id == target.node_id)
            .unwrap();
        assert!(host.fresh);
        assert_eq!(
            host.report.as_ref().unwrap().models[0].supports_tools,
            Some(true)
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), started.notified())
                .await
                .is_err()
        );
        let (_local_stop, rx) = tokio::sync::watch::channel(false);
        assert!(worker
            .execute_private_run(run, &backend, false, &worker_data, rx.clone())
            .await
            .is_err());
        assert_eq!(
            coordinator.private_run_status(run).await.unwrap().state,
            "queued"
        );
        let target_worker = worker.clone();
        let host_data = worker_data.clone();
        let execution = tokio::spawn(async move {
            target_worker
                .execute_private_run(run, &backend, true, &host_data, rx)
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(15), started.notified())
            .await
            .expect("worker never contacted mock model");
        assert_eq!(
            coordinator.private_run_stop(run).await.unwrap().state,
            "stopping"
        );
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), execution)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(outcome.state, "stopped");
        assert!(!outcome.lease_active);
        assert_eq!(
            coordinator.private_run_stop(run).await.unwrap().state,
            "stopped"
        );
        assert!(worker.private_run_work(run).await.is_err());
        let retry = Uuid::new_v4();
        coordinator.private_run_retry(run, retry).await.unwrap();
        assert_eq!(
            coordinator.private_run_status(run).await.unwrap().state,
            "superseded"
        );
        let backend = crate::backend::llama_cpp::LlamaCppBackend::new(&endpoint);
        let (_local_stop, rx) = tokio::sync::watch::channel(false);
        let retry_worker = worker.clone();
        let retry_data = worker_data.clone();
        let second = tokio::spawn(async move {
            retry_worker
                .execute_private_run(retry, &backend, true, &retry_data, rx)
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(15), started.notified())
            .await
            .expect("retry never reached mock model");
        assert_eq!(
            coordinator.private_run_stop(run).await.unwrap().state,
            "superseded"
        );
        assert!(
            !coordinator
                .private_run_status(retry)
                .await
                .unwrap()
                .stop_requested
        );
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "preserve me"
        );
        coordinator.private_run_stop(retry).await.unwrap();
        let retried = tokio::time::timeout(std::time::Duration::from_secs(10), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(retried.state, "stopped");
        mock.abort();
    } else {
        worker.check_in(&caps(), None).await.unwrap();
        assert!(matches!(
            worker.claim_card().await.unwrap(),
            Claim::NothingToDo
        ));
        let scoped = worker.clone().for_private_run(run);
        assert_eq!(id(scoped.claim_card().await.unwrap()), task);
        assert!(
            coordinator
                .private_run_status(run)
                .await
                .unwrap()
                .lease_active
        );
        assert!(matches!(
            scoped.claim_card().await.unwrap(),
            Claim::AlreadyLeased
        ));
        scoped
            .complete_card(
                task,
                "fixture complete without inference",
                None,
                Usage::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            coordinator.private_run_status(run).await.unwrap().state,
            "finished"
        );
        assert_eq!(
            coordinator
                .private_run_request(run, task)
                .await
                .unwrap()
                .state,
            "finished"
        );
        assert!(matches!(
            scoped.claim_card().await.unwrap(),
            Claim::NothingToDo
        ));
    }
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
    drop(store);
    let reopened = LocalHubStore::open(&db).unwrap();
    let coordinator = reopened.connect(&controller.raw_key).unwrap();
    assert_eq!(
        coordinator
            .private_preparation_status(operation)
            .unwrap()
            .state,
        "prepared"
    );
    let tasks = reopened
        .private_code_task_statuses(project, target.node_id)
        .unwrap();
    assert_eq!(
        tasks[0].status,
        if stop_worker { "blocked" } else { "review" }
    );
    if !stop_worker {
        assert_eq!(tasks[0].reason.as_deref(), None);
    }
    reopened
        .transaction(|tx| {
            let raw: String = tx
                .query_row(
                    "SELECT card FROM private_preparations WHERE id=?1",
                    [operation.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(!raw.contains("unused-fixture-token"));
            Ok(())
        })
        .unwrap();
    drop(reopened);
    remove_temp_dir(data);
}

#[test]
fn private_preparation_migrates_v15_without_repeating_bots_migration() {
    let s = LocalHubStore::in_memory().unwrap();
    let p = s
        .create_project("Keep history", "primary stays here")
        .unwrap();
    let db = Arc::try_unwrap(s.db).ok().unwrap().into_inner().unwrap();
    super::rewind_to(&db, 15, "DROP TABLE private_coding_readiness; DROP TABLE private_preparation_recoveries; DROP TABLE private_run_retries; DROP TABLE private_run_stops; DROP TABLE private_runs; DROP TABLE private_preparations;");
    let upgraded = LocalHubStore::from_connection(db).unwrap();
    upgraded
        .transaction(|tx| {
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                crate::local_hub::MIGRATIONS.len() as i64
            );
            assert_eq!(
                tx.query_row(
                    "SELECT title FROM projects WHERE id=?1",
                    [p.to_string()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "Keep history"
            );
            assert!(tx
                .prepare("SELECT lease_deadline FROM agent_deliveries")
                .is_ok());
            assert_eq!(
                tx.query_row("SELECT count(*) FROM private_preparations", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn retry_schema_upgrade_preserves_run_stop_receipts_and_foreign_keys() {
    let s = LocalHubStore::in_memory().unwrap();
    let project = s
        .create_project("Migration fixture", "keep receipts")
        .unwrap();
    let node = s.enroll_owner("target").unwrap().node_id;
    let card = card(project, "existing");
    s.add_card(card.clone()).unwrap();
    let run = Uuid::new_v4();
    s.transaction(|tx| {
        tx.execute(
            "INSERT INTO private_runs VALUES(?1,?2,?3,'queued',NULL,1)",
            params![run.to_string(), card.id.to_string(), node.to_string()],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO private_run_stops VALUES(?1,?2,2)",
            params![run.to_string(), node.to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    let db = Arc::try_unwrap(s.db).ok().unwrap().into_inner().unwrap();
    super::rewind_to(&db, 18, "DROP TABLE private_coding_readiness; DROP TABLE private_preparation_recoveries; DROP TABLE private_run_retries;");
    let migrated = LocalHubStore::from_connection(db).unwrap();
    migrated
        .transaction(|tx| {
            assert_eq!(
                tx.query_row("SELECT operation_id FROM private_run_stops", [], |r| r
                    .get::<_, String>(
                    0
                ))
                .unwrap(),
                run.to_string()
            );
            assert_eq!(
                tx.query_row("SELECT card_id FROM private_runs", [], |r| r
                    .get::<_, String>(0))
                    .unwrap(),
                card.id.to_string()
            );
            assert!(!tx
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap());
            assert!(tx
                .execute(
                    "INSERT INTO private_run_stops VALUES('invalid',?1,3)",
                    [node.to_string()]
                )
                .is_err());
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn coding_readiness_is_self_bound_expiring_and_owner_scoped() {
    use super::private_readiness::{CodingModel, CodingReadiness};
    let (s, a, b, _) = fixture().await;
    let node =
        |h: &LocalHub| Uuid::parse_str(&h.with_node(|_, n| Ok(n.to_owned())).unwrap()).unwrap();
    let an = node(&a);
    let bn = node(&b);
    let owner = Uuid::new_v4();
    let report = CodingReadiness {
        worker_enabled: true,
        coding_enabled: true,
        git_connected: true,
        models: vec![CodingModel {
            id: "fixture".into(),
            supports_tools: None,
        }],
    };
    assert!(a.private_coding_advertise(&report).is_err());
    for n in [an, bn] {
        s.set_node_owner(n, owner).unwrap();
    }
    s.transaction(|tx| {
        tx.execute(
            "UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust='fixture' WHERE id=1",
            params![Uuid::new_v4().to_string(), owner.to_string()],
        )
        .unwrap();
        for n in [an, bn] {
            tx.execute(
                "INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",
                params![Uuid::new_v4().to_string(), n.to_string(), now()],
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert!(a
        .private_coding_hosts()
        .unwrap()
        .iter()
        .all(|h| !h.fresh && h.report.is_none()));
    b.private_coding_advertise(&report).unwrap();
    let hosts = a.private_coding_hosts().unwrap();
    let advertised = hosts.iter().find(|h| h.host.node_id == bn).unwrap();
    assert!(advertised.fresh);
    assert_eq!(
        advertised.report.as_ref().unwrap().models[0].supports_tools,
        None
    );
    assert!(!hosts.iter().find(|h| h.host.node_id == an).unwrap().fresh);
    let mut invalid = report.clone();
    invalid.models.push(invalid.models[0].clone());
    assert!(b.private_coding_advertise(&invalid).is_err());
    let mut raw = serde_json::to_value(&report).unwrap();
    raw["node_id"] = json!(an);
    assert!(serde_json::from_value::<CodingReadiness>(raw).is_err());
    for stamp in [now() - 45, now() + 60] {
        s.transaction(|tx| {
            tx.execute(
                "UPDATE private_coding_readiness SET observed_at=?1",
                [stamp],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
        assert!(a.private_coding_hosts().unwrap().iter().all(|h| !h.fresh));
    }
    b.private_coding_advertise(&CodingReadiness {
        worker_enabled: false,
        coding_enabled: false,
        git_connected: false,
        models: vec![],
    })
    .unwrap();
    let host = a
        .private_coding_hosts()
        .unwrap()
        .into_iter()
        .find(|h| h.host.node_id == bn)
        .unwrap();
    assert!(host.fresh);
    assert!(!host.report.unwrap().worker_enabled);
    s.transaction(|tx| {
        tx.execute(
            "UPDATE nodes SET owner_member_id=?2 WHERE id=?1",
            params![bn.to_string(), Uuid::new_v4().to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(a.private_coding_hosts().unwrap().len(), 1);
    assert!(b.private_coding_advertise(&report).is_err());
    s.transaction(|tx| {
        tx.execute(
            "UPDATE local_node_keys SET revoked=1 WHERE node_id=?1",
            [an.to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(a.private_coding_hosts().is_err());
}

/// `0ff03190`: a process asking a vault "which machine am I" must get back a row that vault
/// actually has. The CLI used to answer from the Hive *account* node id when the vault was its
/// own, so `agent-register` on the hub machine wrote a `preferred_host` with no matching `nodes`
/// row -- every message to that agent then drew a "pinned to a computer this vault does not know"
/// notice, which pairing could not clear because there was nothing wrong with the pairing.
///
/// Sets `HIVE_VAULT_SELF_KEY` so the mint-and-persist branch never runs: minting here would write
/// the developer's real `node.env`. No other test in this binary reads that variable.
#[test]
fn self_node_id_is_a_row_this_vault_has() {
    let store = LocalHubStore::in_memory().unwrap();
    let credentials = store.enroll_owner("this machine").unwrap();
    std::env::set_var("HIVE_VAULT_SELF_KEY", &credentials.raw_key);
    let resolved = store.self_node_id().unwrap();
    std::env::remove_var("HIVE_VAULT_SELF_KEY");

    assert_eq!(resolved, credentials.node_id);
    let known: bool = store
        .transaction(|tx| {
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=?1)",
                [resolved.to_string()],
                |r| r.get(0),
            )
            .map_err(db_error)
        })
        .unwrap();
    assert!(
        known,
        "self_node_id returned an id the vault has no node row for"
    );
}
