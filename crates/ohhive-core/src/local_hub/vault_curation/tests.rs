use super::*;
fn setup() -> (LocalHubStore, LocalHub, Uuid, Uuid, String) {
    let s = LocalHubStore::in_memory().unwrap();
    let c = s.enroll_owner("owner").unwrap();
    let v = s.vault_create("notes").unwrap();
    s.vault_grant(v, c.node_id, true).unwrap();
    s.vault_set_available(v, true).unwrap();
    let h = s.connect(&c.raw_key).unwrap();
    let id = Uuid::new_v4();
    let rev = s
        .vault_put(v, id, "note.md", "Plan", "alpha project")
        .unwrap();
    (s, h, v, id, rev)
}
#[test]
fn archive_hides_reads_and_search_restore_retains_grants() {
    let (s, h, v, id, rev) = setup();
    s.vault_archive(v, id, &rev, "owner", "superseded").unwrap();
    assert!(h.vault_read(v, id, &rev).is_err());
    assert!(h.vault_search(v, "alpha", 10).unwrap().is_empty());
    assert_eq!(
        s.vault_archive_snapshot(v, id).unwrap().content,
        "alpha project"
    );
    assert!(s.vault_archive(v, id, &rev, "owner", "retry").is_err());
    assert_eq!(s.vault_curation_history(v, 0, 100).unwrap().len(), 1);
    s.vault_restore(v, id, &rev, "owner", "still useful")
        .unwrap();
    assert_eq!(h.vault_search(v, "alpha", 10).unwrap().len(), 1);
    let events = s.vault_curation_history(v, 0, 1).unwrap();
    assert_eq!(events[0].action, "archive");
    assert_eq!(
        s.vault_curation_history(v, events[0].sequence, 1).unwrap()[0].action,
        "restore"
    );
    let stranger = s.enroll_owner("other device").unwrap();
    assert!(s
        .connect(&stranger.raw_key)
        .unwrap()
        .vault_read(v, id, &rev)
        .is_err());
}
#[test]
fn changed_source_stays_hidden_and_requires_current_revision() {
    let (s, h, v, id, rev) = setup();
    assert!(s.vault_archive(v, id, "stale", "owner", "reason").is_err());
    let other = s.vault_create("other").unwrap();
    assert!(s.vault_archive(other, id, &rev, "owner", "reason").is_err());
    s.vault_archive(v, id, &rev, "owner", "reason").unwrap();
    let new = s
        .vault_put(v, id, "renamed.md", "New", "beta project")
        .unwrap();
    assert!(h.vault_search(v, "beta", 10).unwrap().is_empty());
    assert!(s.vault_restore(v, id, &rev, "owner", "reason").is_err());
    assert_eq!(s.vault_archive_snapshot(v, id).unwrap().revision, rev);
    s.vault_restore(v, id, &new, "owner", "reviewed updated source")
        .unwrap();
    assert_eq!(h.vault_read(v, id, &new).unwrap().content, "beta project");
    let p = s.vault_provenance(v, id, 0, 100).unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(p[1].kind, "revised");
    assert_eq!(p[1].path, "renamed.md");
}
#[test]
fn missing_manual_snapshot_restores_and_conflict_rolls_back() {
    let (s, h, v, id, rev) = setup();
    s.vault_archive(v, id, &rev, "owner", "reason").unwrap();
    s.vault_remove_document(v, id).unwrap();
    let conflict = Uuid::new_v4();
    s.vault_put(v, conflict, "note.md", "New", "unrelated")
        .unwrap();
    assert!(s.vault_restore(v, id, &rev, "owner", "reason").is_err());
    assert!(s.vault_archive_snapshot(v, id).is_ok());
    let inventory = s.vault_archives(v, None, 1).unwrap();
    assert_eq!(inventory.len(), 1);
    assert!(s
        .vault_archives(v, Some(inventory[0].document_id), 1)
        .unwrap()
        .is_empty());
    assert_eq!(s.vault_curation_history(v, 0, 100).unwrap().len(), 1);
    s.vault_remove_document(v, conflict).unwrap();
    s.vault_restore(v, id, &rev, "owner", "reason").unwrap();
    assert!(h.vault_read(v, id, &rev).is_ok());
}
#[test]
fn detection_flags_without_mutating_and_unchanged_put_does_not_reset_age() {
    let (s, h, v, id, rev) = setup();
    let dup = Uuid::new_v4();
    s.vault_put(v, dup, "duplicate.md", "Copy", " ALPHA   project\n")
        .unwrap();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let words = (0..30)
        .map(|i| format!("word{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    s.vault_put(v, a, "a.md", "A", &words).unwrap();
    s.vault_put(v, b, "b.md", "B", &format!("{words} additional"))
        .unwrap();
    let initial = s.vault_curation_scan(v, 90).unwrap();
    let timestamp = initial
        .notes
        .iter()
        .find(|n| n.id == id)
        .unwrap()
        .changed_ms;
    s.vault_put(v, id, "note.md", "Plan", "alpha project")
        .unwrap();
    let report = s.curation_scan_at(v, 90, timestamp + 90 * DAY_MS).unwrap();
    assert!(report.stale.contains(&id));
    assert!(report
        .duplicates
        .iter()
        .any(|d| d.kind == "normalized_exact"));
    assert!(report.duplicates.iter().any(|d| d.kind == "near"));
    assert!(h.vault_read(v, id, &rev).is_ok());
    assert!(s.vault_curation_history(v, 0, 100).unwrap().is_empty());
    assert_eq!(s.vault_provenance(v, id, 0, 100).unwrap().len(), 1);
    assert!(s.vault_curation_scan(v, 0).is_err());
    assert!(s.vault_curation_history(v, 0, 101).is_err());
}
#[test]
fn long_notes_report_near_scan_limits_and_other_vault_is_excluded() {
    let (s, _, v, _, _) = setup();
    s.vault_put(v, Uuid::new_v4(), "long.md", "Long", &"word ".repeat(2049))
        .unwrap();
    let other = s.vault_create("other").unwrap();
    s.vault_put(other, Uuid::new_v4(), "copy.md", "Copy", "alpha project")
        .unwrap();
    let report = s.vault_curation_scan(v, 90).unwrap();
    assert!(!report.duplicate_scan_complete);
    assert!(report.duplicates.is_empty());
    assert_eq!(report.notes.len(), 2);
}
#[test]
fn folder_and_excluded_intake_deletions_cannot_be_resurrected() {
    let (s, _, v, id, rev) = setup();
    s.vault_archive(v, id, &rev, "owner", "reason").unwrap();
    s.vault_remove_document(v, id).unwrap();
    s.transaction(|tx|{
        tx.execute("INSERT INTO vault_sources(vault_id,root,root_identity) VALUES(?1,'/synthetic','synthetic')",[v.to_string()]).map_err(db_error)?;Ok(())
    }).unwrap();
    assert!(s.vault_restore(v, id, &rev, "owner", "reason").is_err());
    s.transaction(|tx| {
        tx.execute(
            "DELETE FROM vault_sources WHERE vault_id=?1",
            [v.to_string()],
        )
        .map_err(db_error)?;
        tx.execute(
            "INSERT INTO vault_intake VALUES(?1,?2,1,'fingerprint',?3,NULL)",
            params![v.to_string(), Uuid::new_v4().to_string(), id.to_string()],
        )
        .map_err(db_error)?;
        Ok(())
    })
    .unwrap();
    assert!(s.vault_restore(v, id, &rev, "owner", "reason").is_err());
    assert!(s.vault_archive_snapshot(v, id).is_ok());
}
#[test]
fn schema_four_upgrade_seeds_age_and_persists_archive() {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../schema.sql")).unwrap();
    db.execute_batch(include_str!("../vault_schema.sql"))
        .unwrap();
    db.execute_batch(include_str!("../vault_folder_schema.sql"))
        .unwrap();
    db.execute_batch(include_str!("../vault_intake_schema.sql"))
        .unwrap();
    let v = Uuid::new_v4();
    let id = Uuid::new_v4();
    db.execute(
        "INSERT INTO vaults(id,name) VALUES(?1,'old')",
        [v.to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO vault_documents VALUES(?1,?2,'old.md','rev','old','old text')",
        params![id.to_string(), v.to_string()],
    )
    .unwrap();
    let s = LocalHubStore::from_connection(db).unwrap();
    let scan = s.vault_curation_scan(v, 90).unwrap();
    assert!(scan.stale.is_empty());
    assert_eq!(
        s.vault_provenance(v, id, 0, 100).unwrap()[0].kind,
        "baseline"
    );
    s.vault_archive(v, id, "rev", "owner", "reason").unwrap();
    db = Arc::try_unwrap(s.db).unwrap().into_inner().unwrap();
    let reopened = LocalHubStore::from_connection(db).unwrap();
    assert_eq!(
        reopened.vault_archive_snapshot(v, id).unwrap().revision,
        "rev"
    );
    assert_eq!(reopened.vault_curation_history(v, 0, 100).unwrap().len(), 1);
}

#[test]
fn corpus_limits_fail_explicitly_and_pair_budget_is_reported() {
    let (s, _, v, _, _) = setup();
    s.transaction(|tx| {
        for n in 0..450 {
            let content = (0..21)
                .map(|i| format!("token{n}x{i}"))
                .collect::<Vec<_>>()
                .join(" ");
            tx.execute(
                "INSERT INTO vault_documents VALUES(?1,?2,?3,?4,'test',?5)",
                params![
                    Uuid::new_v4().to_string(),
                    v.to_string(),
                    format!("{n}.md"),
                    format!("r{n}"),
                    content
                ],
            )
            .map_err(db_error)?;
        }
        Ok(())
    })
    .unwrap();
    let report = s.vault_curation_scan(v, 90).unwrap();
    assert_eq!(report.near_pairs_examined, MAX_PAIRS);
    assert!(!report.duplicate_scan_complete);
    s.transaction(|tx| {
        for n in 450..10_000 {
            tx.execute(
                "INSERT INTO vault_documents VALUES(?1,?2,?3,'rev','test','text')",
                params![Uuid::new_v4().to_string(), v.to_string(), format!("{n}.md")],
            )
            .map_err(db_error)?;
        }
        Ok(())
    })
    .unwrap();
    assert!(s.vault_curation_scan(v, 90).is_err());
}

#[test]
fn folder_reconciliation_keeps_archive_overlay_without_touching_source() {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = Temp(std::env::temp_dir().join(format!("hive-curation-{}", Uuid::new_v4())));
    std::fs::create_dir(&root.0).unwrap();
    let file = root.0.join("source.md");
    std::fs::write(&file, "# Source\nalpha document").unwrap();
    let s = LocalHubStore::in_memory().unwrap();
    let v = s.vault_create("folder").unwrap();
    let c = s.enroll_owner("reader").unwrap();
    s.vault_grant(v, c.node_id, true).unwrap();
    let h = s.connect(&c.raw_key).unwrap();
    s.vault_attach_folder(v, &root.0).unwrap();
    s.vault_scan_folder(v).unwrap();
    let note = h.vault_search(v, "alpha", 10).unwrap().remove(0);
    s.vault_archive(v, note.id, &note.revision, "owner", "duplicate candidate")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "# Source\nalpha document"
    );
    std::fs::write(&file, "# Source\nbeta document").unwrap();
    s.vault_scan_folder(v).unwrap();
    assert!(h.vault_search(v, "beta", 10).unwrap().is_empty());
    let current = s.vault_curation_current(v, note.id).unwrap().unwrap();
    assert!(s
        .vault_restore(v, note.id, &note.revision, "owner", "old approval")
        .is_err());
    s.vault_restore(v, note.id, &current.revision, "owner", "reviewed change")
        .unwrap();
    assert_eq!(h.vault_search(v, "beta", 10).unwrap().len(), 1);
    s.vault_archive(v, note.id, &current.revision, "owner", "archive again")
        .unwrap();
    std::fs::remove_file(&file).unwrap();
    s.vault_scan_folder(v).unwrap();
    assert!(s
        .vault_restore(v, note.id, &current.revision, "owner", "missing source")
        .is_err());
    assert!(!file.exists());
    assert_eq!(
        s.vault_archive_snapshot(v, note.id).unwrap().content,
        "# Source\nbeta document"
    );
}
