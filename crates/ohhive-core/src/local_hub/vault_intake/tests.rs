use super::*;
fn item() -> IntakeItem {
    IntakeItem { source_id:Uuid::new_v4(), generation:1, content:IntakeContent::Knowledge {
        title:"Pooling measurement".into(), source_label:"Lab report".into(), project:Some("halo".into()),
        markdown:"# Test report\n\nUnicorn throughput reached 12 tokens per second.\nResults are preliminary.".into()
    }}
}
fn setup() -> (LocalHubStore, Uuid, LocalHub) {
    let s = LocalHubStore::in_memory().unwrap();
    let c = s.enroll_owner("reader").unwrap();
    let v = s.vault_create("Intake").unwrap();
    s.vault_grant(v, c.node_id, true).unwrap();
    s.vault_set_available(v, true).unwrap();
    let h = s.connect(&c.raw_key).unwrap();
    (s, v, h)
}
#[test]
fn files_source_preserves_original_and_reuses_granted_search() {
    let (s, v, h) = setup();
    let i = item();
    let r = s.vault_intake(v, &i).unwrap();
    let hit = h.vault_search(v, "unicorn", 5).unwrap();
    assert_eq!(hit.len(), 1);
    let d = h
        .vault_read(v, r.document_id, r.revision.as_ref().unwrap())
        .unwrap();
    assert!(d.path.starts_with("Intake/halo/research/"));
    assert!(d.content.contains("Source line 3: Unicorn throughput"));
    if let IntakeContent::Knowledge { markdown, .. } = &i.content {
        assert!(d.content.ends_with(markdown));
    }
    assert!(d.content.contains(&i.source_id.to_string()));
    let other = s.enroll_owner("ungranted").unwrap();
    let other_h = s.connect(&other.raw_key).unwrap();
    assert!(other_h.vault_search(v, "unicorn", 5).is_err());
    assert!(other_h
        .vault_read(v, r.document_id, r.revision.as_ref().unwrap())
        .is_err());
    s.vault_grant(v, other.node_id, true).unwrap();
    assert_eq!(other_h.vault_search(v, "unicorn", 5).unwrap().len(), 1);
    s.vault_grant(v, other.node_id, false).unwrap();
    assert!(other_h.vault_search(v, "unicorn", 5).is_err());
}
#[test]
fn retries_are_noops_and_new_generations_replace_fts_atomically() {
    let (s, v, h) = setup();
    let mut i = item();
    let r = s.vault_intake(v, &i).unwrap();
    s.db.lock().unwrap().execute_batch("CREATE TABLE audit(n INTEGER); CREATE TRIGGER audit_intake AFTER UPDATE ON vault_documents BEGIN INSERT INTO audit VALUES(1); END;").unwrap();
    assert!(s.vault_intake(v, &i).unwrap().unchanged);
    assert_eq!(
        s.db.lock()
            .unwrap()
            .query_row("SELECT count(*) FROM audit", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    if let IntakeContent::Knowledge {
        markdown, project, ..
    } = &mut i.content
    {
        *markdown = "# Decision\nNarwhal is the new baseline.".into();
        *project = Some("fleet".into());
    }
    assert!(s.vault_intake(v, &i).is_err()); // same generation, conflicting payload
    i.generation = 2;
    let next = s.vault_intake(v, &i).unwrap();
    assert_eq!(next.document_id, r.document_id);
    assert!(h.vault_search(v, "unicorn", 5).unwrap().is_empty());
    let d = h
        .vault_read(v, next.document_id, next.revision.as_ref().unwrap())
        .unwrap();
    assert!(d.path.starts_with("Intake/fleet/decisions/"));
    assert!(h
        .vault_read(v, r.document_id, r.revision.as_ref().unwrap())
        .is_err());
    i.generation = 1;
    assert!(s.vault_intake(v, &i).is_err());
}
#[test]
fn exclusion_removes_searchable_content_and_tombstone_survives_restart() {
    let (s, v, h) = setup();
    let i = item();
    let original = s.vault_intake(v, &i).unwrap();
    let excluded = IntakeItem {
        source_id: i.source_id,
        generation: 2,
        content: IntakeContent::Excluded {},
    };
    assert!(s.vault_intake(v, &excluded).unwrap().revision.is_none());
    assert!(h.vault_search(v, "unicorn", 5).unwrap().is_empty());
    assert!(h
        .vault_read(v, original.document_id, original.revision.as_ref().unwrap())
        .is_err());
    assert!(s.vault_intake(v, &i).is_err());
    assert!(s.vault_intake(v, &excluded).unwrap().unchanged);
    drop(h);
    let conn = Arc::try_unwrap(s.db).unwrap().into_inner().unwrap();
    let reopened = LocalHubStore::from_connection(conn).unwrap();
    assert!(reopened.vault_intake(v, &i).is_err());
    assert!(reopened.vault_intake(v, &excluded).unwrap().unchanged);
    // Excluded variant cannot accidentally carry a plaintext secret through deserialization.
    assert!(serde_json::from_str::<IntakeContent>(
        r#"{"disposition":"excluded","markdown":"secret"}"#
    )
    .is_err());
}
#[test]
fn manual_edits_are_never_overwritten_even_on_retry_or_exclusion() {
    let (s, v, h) = setup();
    let mut i = item();
    let r = s.vault_intake(v, &i).unwrap();
    let rev = s
        .vault_put(v, r.document_id, "my-note.md", "Human", "Human correction")
        .unwrap();
    assert!(s.vault_intake(v, &i).is_err());
    i.generation = 2;
    assert!(s.vault_intake(v, &i).is_err());
    i.content = IntakeContent::Excluded {};
    assert!(s.vault_intake(v, &i).is_err());
    assert_eq!(
        h.vault_read(v, r.document_id, &rev).unwrap().content,
        "Human correction"
    );
}
#[test]
fn rejects_folder_vaults_bad_paths_and_oversized_inputs_without_publication() {
    let (s, v, h) = setup();
    let mut i = item();
    if let IntakeContent::Knowledge { project, .. } = &mut i.content {
        *project = Some("../escape".into());
    }
    assert!(s.vault_intake(v, &i).is_err());
    if let IntakeContent::Knowledge {
        project, markdown, ..
    } = &mut i.content
    {
        *project = None;
        *markdown = "x".repeat(MAX_SOURCE + 1);
    }
    assert!(s.vault_intake(v, &i).is_err());
    assert!(h.vault_search(v, "unicorn", 5).unwrap().is_empty());
    s.db.lock()
        .unwrap()
        .execute(
            "INSERT INTO vault_sources(vault_id,root,root_identity) VALUES(?1,'x','x')",
            [v.to_string()],
        )
        .unwrap();
    assert!(s.vault_intake(v, &item()).is_err());
}
#[test]
fn source_identity_is_vault_scoped_and_summary_is_bounded_unicode() {
    let (s, v, h) = setup();
    let other = s.vault_create("Other").unwrap();
    let mut i = item();
    if let IntakeContent::Knowledge {
        markdown, project, ..
    } = &mut i.content
    {
        *project = None;
        *markdown = format!(
            "# Unclassified\n```\nsecret-looking-code\n```\n{}\nSecond\nThird\nFourth",
            "猫".repeat(1000)
        );
    }
    let p = prepare(&i, Uuid::new_v4()).unwrap().unwrap();
    let summary = p
        .content
        .split("## Source excerpts (not independently verified)\n")
        .nth(1)
        .unwrap()
        .split("## Original source")
        .next()
        .unwrap();
    assert!(!summary.contains("secret-looking-code"));
    assert!(!summary.contains("Fourth"));
    assert!(summary.len() < 2600);
    assert!(p.path.starts_with("Intake/inbox/notes/"));
    let a = s.vault_intake(v, &i).unwrap();
    let b = s.vault_intake(other, &i).unwrap();
    assert_ne!(a.document_id, b.document_id);
    assert!(h
        .vault_read(other, b.document_id, b.revision.as_ref().unwrap())
        .is_err());
}
#[test]
fn migrates_schema_three_preserving_existing_documents() {
    let mut conn = Connection::open_in_memory().unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute_batch(include_str!("../schema.sql")).unwrap();
    tx.execute_batch(include_str!("../vault_schema.sql"))
        .unwrap();
    tx.execute_batch(include_str!("../vault_folder_schema.sql"))
        .unwrap();
    tx.execute("INSERT INTO vaults(id,name) VALUES('old','Existing')", [])
        .unwrap();
    tx.execute(
        "INSERT INTO vault_documents VALUES('doc','old','a.md','r','Title','Preserved')",
        [],
    )
    .unwrap();
    tx.commit().unwrap();
    let s = LocalHubStore::from_connection(conn).unwrap();
    let db = s.db.lock().unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        // Room titles (v10) then bots_causation_schema.sql (v11, Track A slice 2, 2026-09-15)
        // and provider runtimes bumped current schema to 12 -- see local_hub/mod.rs's from_connection.
        12
    );
    assert_eq!(
        db.query_row(
            "SELECT content FROM vault_documents WHERE id='doc'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Preserved"
    );
}

#[test]
fn concurrent_duplicate_delivery_publishes_once_and_does_not_change_availability() {
    let (s, v, h) = setup();
    let i = item();
    s.vault_set_available(v, false).unwrap();
    let second = s.clone();
    let second_item = i.clone();
    let task = std::thread::spawn(move || second.vault_intake(v, &second_item).unwrap());
    let a = s.vault_intake(v, &i).unwrap();
    let b = task.join().unwrap();
    assert_ne!(a.unchanged, b.unchanged);
    assert_eq!(a.document_id, b.document_id);
    assert!(h.vault_search(v, "unicorn", 5).is_err());
    s.vault_set_available(v, true).unwrap();
    assert_eq!(h.vault_search(v, "unicorn", 5).unwrap().len(), 1);
}
#[test]
fn failed_publication_leaves_no_receipt_or_partial_index() {
    let (s, v, h) = setup();
    let i = item();
    // Force failure at the receipt write, after document/FTS insertion inside the transaction.
    s.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON vault_intake BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
    assert!(s.vault_intake(v, &i).is_err());
    assert!(h.vault_search(v, "unicorn", 5).unwrap().is_empty());
    s.db.lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_receipt")
        .unwrap();
    assert!(!s.vault_intake(v, &i).unwrap().unchanged);
    assert_eq!(h.vault_search(v, "unicorn", 5).unwrap().len(), 1);
}
