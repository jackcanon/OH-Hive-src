use super::*;

fn setup() -> (LocalHubStore, Uuid) {
    let s = LocalHubStore::in_memory().unwrap();
    let v = s.vault_create("Folder intake fixture").unwrap();
    (s, v)
}

fn temp_root() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("hive-intake-folder-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn write(root: &std::path::Path, relative: &str, content: &str) {
    let full = root.join(relative);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(full, content).unwrap();
}

fn intake_generation(s: &LocalHubStore, vault: Uuid, relative_path: &str) -> i64 {
    let source_id = source_id_for(vault, relative_path);
    s.transaction(|tx| {
        tx.query_row(
            "SELECT generation FROM vault_intake WHERE vault_id=?1 AND source_id=?2",
            params![vault.to_string(), source_id.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)
    })
    .unwrap()
}

#[test]
fn list_candidates_finds_nested_markdown_skips_hidden_and_non_markdown() {
    let root = temp_root();
    write(&root, "top.md", "# Top level\n\nhello");
    write(&root, "sub/nested.md", "# Nested\n\nworld");
    write(&root, "notes.txt", "not markdown");
    write(&root, ".hidden.md", "# Should not appear");
    write(&root, ".git/config.md", "# Should not appear either");

    let (s, _v) = setup();
    let candidates = s.vault_intake_list_candidates(&root).unwrap();

    let paths: Vec<&str> = candidates
        .iter()
        .map(|c| c.relative_path.as_str())
        .collect();
    assert_eq!(paths, vec!["sub/nested.md", "top.md"]);
    assert_eq!(candidates[0].title, "Nested");
    assert_eq!(candidates[1].title, "Top level");

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn approve_new_source_starts_at_generation_one_and_is_searchable() {
    let root = temp_root();
    write(
        &root,
        "decision.md",
        "# Decision\n\nWe chose the extractive-only approach.",
    );

    let (s, v) = setup();
    let owner = s.enroll_owner("reader").unwrap();
    s.vault_grant(v, owner.node_id, true).unwrap();
    s.vault_set_available(v, true).unwrap();
    let h = s.connect(&owner.raw_key).unwrap();

    let receipt = s
        .vault_intake_approve_file(v, &root, "decision.md", Some("halo"))
        .unwrap();
    assert!(!receipt.unchanged);

    let hits = h.vault_search(v, "extractive", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(intake_generation(&s, v, "decision.md"), 1);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn approving_unchanged_file_again_is_a_noop_at_the_same_generation() {
    let root = temp_root();
    write(
        &root,
        "notes/idea.md",
        "# Research\n\nFirst pass numbers look promising.",
    );

    let (s, v) = setup();
    let first = s
        .vault_intake_approve_file(v, &root, "notes/idea.md", None)
        .unwrap();
    assert!(!first.unchanged);

    let second = s
        .vault_intake_approve_file(v, &root, "notes/idea.md", None)
        .unwrap();
    assert!(second.unchanged);
    assert_eq!(first.document_id, second.document_id);
    assert_eq!(first.revision, second.revision);
    assert_eq!(intake_generation(&s, v, "notes/idea.md"), 1);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn approving_after_edit_bumps_generation_and_replaces_content() {
    let root = temp_root();
    write(&root, "log.md", "# Research\n\nDraft one.");

    let (s, v) = setup();
    let owner = s.enroll_owner("reader").unwrap();
    s.vault_grant(v, owner.node_id, true).unwrap();
    s.vault_set_available(v, true).unwrap();
    let h = s.connect(&owner.raw_key).unwrap();

    let first = s
        .vault_intake_approve_file(v, &root, "log.md", None)
        .unwrap();
    assert_eq!(intake_generation(&s, v, "log.md"), 1);

    write(
        &root,
        "log.md",
        "# Research\n\nRevised numbers after rerun.",
    );
    let second = s
        .vault_intake_approve_file(v, &root, "log.md", None)
        .unwrap();
    assert!(!second.unchanged);
    // Same document id (source identity is stable), new revision (content actually changed).
    assert_eq!(first.document_id, second.document_id);
    assert_ne!(first.revision, second.revision);
    assert_eq!(intake_generation(&s, v, "log.md"), 2);

    let hits = h.vault_search(v, "revised", 5).unwrap();
    assert_eq!(hits.len(), 1);
    let stale = h.vault_search(v, "draft one", 5).unwrap();
    assert_eq!(stale.len(), 0);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn symlinked_root_is_rejected_outright() {
    if cfg!(not(unix)) {
        return;
    }
    let real = temp_root();
    write(&real, "a.md", "# A\n\ntext");
    let link = std::env::temp_dir().join(format!("hive-intake-folder-link-{}", Uuid::new_v4()));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let (s, _v) = setup();
    assert!(s.vault_intake_list_candidates(&link).is_err());

    std::fs::remove_file(&link).ok();
    std::fs::remove_dir_all(&real).unwrap();
}

#[test]
fn oversized_file_is_excluded_from_listing_and_rejected_on_approve() {
    let root = temp_root();
    let huge = "x".repeat((MAX_FILE + 1) as usize);
    write(&root, "big.md", &format!("# Big\n\n{huge}"));
    write(&root, "small.md", "# Small\n\nfine");

    let (s, v) = setup();
    let candidates = s.vault_intake_list_candidates(&root).unwrap();
    let paths: Vec<&str> = candidates
        .iter()
        .map(|c| c.relative_path.as_str())
        .collect();
    assert_eq!(paths, vec!["small.md"]);

    assert!(s
        .vault_intake_approve_file(v, &root, "big.md", None)
        .is_err());

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn traversal_and_non_markdown_paths_are_rejected() {
    let root = temp_root();
    write(&root, "ok.md", "# Ok\n\nfine");

    let (s, v) = setup();
    assert!(s
        .vault_intake_approve_file(v, &root, "../ok.md", None)
        .is_err());
    assert!(s
        .vault_intake_approve_file(v, &root, "ok.txt", None)
        .is_err());

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn folder_attached_vault_rejects_intake_the_same_way_vault_intake_itself_does() {
    let root = temp_root();
    write(&root, "note.md", "# Notes\n\ntext");

    let attach_root = temp_root();
    write(
        &attach_root,
        "existing.md",
        "# Existing\n\nlive external state",
    );

    let s = LocalHubStore::in_memory().unwrap();
    let v = s.vault_create("Attached").unwrap();
    s.vault_attach_folder(v, &attach_root).unwrap();

    assert!(s
        .vault_intake_approve_file(v, &root, "note.md", None)
        .is_err());

    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&attach_root).unwrap();
}
