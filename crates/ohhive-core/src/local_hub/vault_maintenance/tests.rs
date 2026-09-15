use super::*;
fn setup() -> (LocalHubStore, Uuid, Uuid, String) {
    let s = LocalHubStore::in_memory().unwrap();
    let v = s.vault_create("notes").unwrap();
    s.vault_set_available(v, true).unwrap();
    let id = Uuid::new_v4();
    let r = s
        .vault_put(v, id, "note.md", "Title", "hello world")
        .unwrap();
    (s, v, id, r)
}
#[test]
fn disabled_due_cadence_and_unavailable_retry() {
    let (s, v, _, _) = setup();
    let mut p = MaintenancePolicy::default();
    s.vault_configure_maintenance(v, &p).unwrap();
    assert!(s.vault_maintenance_tick().unwrap().is_none());
    p.enabled = true;
    s.vault_configure_maintenance(v, &p).unwrap();
    let at = Utc::now().timestamp_millis();
    assert_eq!(
        s.maintenance_tick_at(at).unwrap().unwrap().outcome,
        "scanned"
    );
    assert!(s.maintenance_tick_at(at + 1).unwrap().is_none());
    s.vault_set_available(v, false).unwrap();
    assert_eq!(
        s.maintenance_tick_at(at + DAY).unwrap().unwrap().outcome,
        "unavailable"
    );
    assert_eq!(
        s.vault_maintenance_status(v).unwrap().unwrap().next_due_ms,
        at + DAY + 60_000
    );
}
#[test]
fn retention_keeps_overlay_and_protects_unique_history() {
    let (s, v, id, r) = setup();
    let c = s.enroll_owner("reader").unwrap();
    s.vault_grant(v, c.node_id, true).unwrap();
    let h = s.connect(&c.raw_key).unwrap();
    s.vault_archive(v, id, &r, "owner", "test").unwrap();
    let mut p = MaintenancePolicy::default();
    p.enabled = true;
    p.redundant_snapshot_days = Some(1);
    s.vault_configure_maintenance(v, &p).unwrap();
    let result = s
        .maintenance_tick_at(Utc::now().timestamp_millis() + 2 * DAY)
        .unwrap()
        .unwrap();
    assert_eq!(result.snapshots_expired, 1);
    assert_eq!(result.archive_bytes, 0);
    assert!(h.vault_read(v, id, &r).is_err());
    assert!(s.vault_archive_snapshot(v, id).is_err());
    assert!(!s.vault_archives(v, None, 10).unwrap()[0].snapshot_retained);
    s.vault_restore(v, id, &r, "owner", "restore").unwrap();
    assert!(h.vault_read(v, id, &r).is_ok());
    s.vault_archive(v, id, &r, "owner", "test").unwrap();
    s.vault_put(v, id, "note.md", "Changed", "different")
        .unwrap();
    s.vault_configure_maintenance(v, &p).unwrap();
    assert_eq!(
        s.maintenance_tick_at(Utc::now().timestamp_millis() + 2 * DAY)
            .unwrap()
            .unwrap()
            .snapshots_expired,
        0
    );
    assert!(s.vault_archive_snapshot(v, id).is_ok());
}
#[test]
fn quota_failure_rolls_back_and_policy_is_validated() {
    let (s, v, id, r) = setup();
    let mut p = MaintenancePolicy::default();
    p.archive_quota_bytes = 1024;
    s.vault_configure_maintenance(v, &p).unwrap();
    let r = s
        .vault_put(v, id, "note.md", "Title", &"x".repeat(2048))
        .unwrap_or(r);
    assert!(s.vault_archive(v, id, &r, "owner", "test").is_err());
    assert!(s.vault_archives(v, None, 10).unwrap().is_empty());
    assert!(s.vault_curation_history(v, 0, 10).unwrap().is_empty());
    p.interval_seconds = 0;
    assert!(s.vault_configure_maintenance(v, &p).is_err());
}
#[test]
fn persisted_claim_recovery_and_reopen() {
    let (s, v, _, _) = setup();
    let mut p = MaintenancePolicy::default();
    p.enabled = true;
    s.vault_configure_maintenance(v, &p).unwrap();
    let at = Utc::now().timestamp_millis();
    s.transaction(|tx| {
        tx.execute(
            "UPDATE vault_maintenance SET token='abandoned',lease_until_ms=?1",
            [at + 100],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    assert!(s.maintenance_tick_at(at).unwrap().is_none());
    assert!(s.maintenance_tick_at(at + 101).unwrap().is_some());
    let due = s.vault_maintenance_status(v).unwrap().unwrap().next_due_ms;
    let db = Arc::try_unwrap(s.db).unwrap().into_inner().unwrap();
    let s = LocalHubStore::from_connection(db).unwrap();
    assert_eq!(
        s.vault_maintenance_status(v).unwrap().unwrap().next_due_ms,
        due
    );
}
#[tokio::test]
async fn host_loop_runs_and_stops() {
    let (s, v, _, _) = setup();
    let mut p = MaintenancePolicy::default();
    p.enabled = true;
    s.vault_configure_maintenance(v, &p).unwrap();
    let (tx, rx) = tokio::sync::watch::channel(false);
    let runner = s.clone();
    let task = tokio::spawn(async move { runner.vault_maintenance_run(rx).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if s.vault_maintenance_status(v)
                .unwrap()
                .unwrap()
                .last_result
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tx.send(true).unwrap();
    task.await.unwrap().unwrap();
}

#[test]
fn quota_pressure_expires_only_eligible_copy_and_preserves_archive() {
    let (s, v, id, r) = setup();
    let r = s
        .vault_put(v, id, "note.md", "Title", &"a".repeat(450))
        .unwrap_or(r);
    s.vault_archive(v, id, &r, "owner", "test").unwrap();
    s.transaction(|tx| {
        tx.execute("UPDATE vault_archives SET archived_ms=0", [])
            .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    let mut p = MaintenancePolicy::default();
    p.archive_quota_bytes = 1024;
    p.redundant_snapshot_days = Some(1);
    s.vault_configure_maintenance(v, &p).unwrap();
    let other = Uuid::new_v4();
    let rev = s
        .vault_put(v, other, "other.md", "Other", &"b".repeat(450))
        .unwrap();
    s.vault_archive(v, other, &rev, "owner", "test").unwrap();
    assert!(
        !s.vault_archives(v, None, 10)
            .unwrap()
            .into_iter()
            .find(|a| a.document_id == id)
            .unwrap()
            .snapshot_retained
    );
    assert_eq!(
        s.vault_curation_history(v, 0, 10).unwrap()[1].action,
        "expire_snapshot"
    );
    s.vault_remove_document(v, id).unwrap();
    assert!(s.vault_restore(v, id, &r, "owner", "missing copy").is_err());
    assert_eq!(s.vault_archives(v, None, 10).unwrap().len(), 2);
}
#[test]
fn simultaneous_ticks_publish_once_and_cache_flags() {
    let (s, v, id, _) = setup();
    let mut p = MaintenancePolicy::default();
    p.enabled = true;
    s.vault_configure_maintenance(v, &p).unwrap();
    s.transaction(|tx| {
        tx.execute("UPDATE vault_observations SET changed_ms=0", [])
            .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    let at = Utc::now().timestamp_millis();
    let other = s.clone();
    let task = std::thread::spawn(move || other.maintenance_tick_at(at).unwrap());
    let a = s.maintenance_tick_at(at).unwrap();
    let b = task.join().unwrap();
    assert_eq!(usize::from(a.is_some()) + usize::from(b.is_some()), 1);
    assert_eq!(
        s.vault_maintenance_status(v)
            .unwrap()
            .unwrap()
            .last_result
            .unwrap()
            .stale_candidates,
        vec![id]
    );
}
#[test]
fn failed_scan_is_recorded_and_retry_is_bounded() {
    let (s, v, _, _) = setup();
    let mut p = MaintenancePolicy::default();
    p.enabled = true;
    s.vault_configure_maintenance(v, &p).unwrap();
    s.transaction(|tx| {
        tx.execute(
            "INSERT INTO vault_documents VALUES('bad-id',?1,'bad.md','r','Bad','bad')",
            [v.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    let at = Utc::now().timestamp_millis();
    let result = s.maintenance_tick_at(at).unwrap().unwrap();
    assert_eq!(result.outcome, "scan_failed");
    assert!(s.maintenance_tick_at(at + 1).unwrap().is_none());
    assert_eq!(
        s.vault_maintenance_status(v).unwrap().unwrap().next_due_ms,
        at + 60_000
    );
}
