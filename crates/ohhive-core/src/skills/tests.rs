use super::*;
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("hive-skills-{}", Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
const SOURCE:&str="---\nname: build-project\ndescription: Build and check this project.\n---\n\n1. Read the build instructions.\n2. Run the relevant checks.\n";
#[test]
fn parses_yaml_quotes_folded_scalars_crlf_and_preserves_body() {
    let s="\u{feff}---\r\nname: 'build: project'\r\ndescription: >-\r\n  Build the app\r\n  and run checks.\r\nallowed-tools: [shell]\r\n---\r\n# Procedure\r\nDo the work.\r\n";
    let p = parse_skill(s).unwrap();
    assert_eq!(p.name, "build: project");
    assert_eq!(p.description, "Build the app and run checks.");
    assert_eq!(p.procedure, "# Procedure\r\nDo the work.\r\n");
}
#[test]
fn rejects_malformed_missing_duplicate_and_oversized_frontmatter() {
    for s in [
        "# No header",
        "---\nname: x\n---\nbody",
        "---\nname: x\nname: y\ndescription: z\n---\nbody",
        "---\nname: x\ndescription: [bad]\n---\nbody",
        "---\nname: x\ndescription: y\n---\n ",
    ] {
        assert!(parse_skill(s).is_err());
    }
    assert_eq!(
        parse_skill(&format!(
            "---\n# {}\nname: x\ndescription: y\n---\nbody",
            "x".repeat(MAX_HEADER)
        ))
        .unwrap_err(),
        SkillError::Limit
    );
}
#[test]
fn create_inventory_read_and_usage_are_separate_operations() {
    let t = Temp::new();
    let s = SkillStore::open(&t.0).unwrap();
    assert!(s.list().unwrap().skills.is_empty());
    let saved = s.write_new("build-project", SOURCE).unwrap();
    assert_eq!(saved.last_used_unix_ms, None);
    let path = t.0.join(".hive/skills/build-project/SKILL.md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOURCE);
    assert!(s
        .read("build-project")
        .unwrap()
        .procedure
        .contains("Run the relevant checks"));
    assert_eq!(s.list().unwrap().skills[0].last_used_unix_ms, None);
    s.mark_used_at("build-project", &saved.revision, 100)
        .unwrap();
    s.mark_used_at("build-project", &saved.revision, 50)
        .unwrap();
    assert_eq!(s.list().unwrap().skills[0].last_used_unix_ms, Some(100));
    assert_eq!(std::fs::read_to_string(path).unwrap(), SOURCE);
    let reopened = SkillStore::open(&t.0).unwrap();
    assert_eq!(
        reopened.list().unwrap().skills[0].last_used_unix_ms,
        Some(100)
    );
}
#[test]
fn edits_invalidate_usage_and_stale_use_is_rejected() {
    let t = Temp::new();
    let s = SkillStore::open(&t.0).unwrap();
    let first = s.write_new("build-project", SOURCE).unwrap();
    s.mark_used_at("build-project", &first.revision, 100)
        .unwrap();
    std::fs::write(
        t.0.join(".hive/skills/build-project/SKILL.md"),
        SOURCE.replace("relevant", "new"),
    )
    .unwrap();
    assert_eq!(
        s.read("build-project").unwrap().summary.last_used_unix_ms,
        None
    );
    assert_eq!(
        s.mark_used_at("build-project", &first.revision, 101),
        Err(SkillError::Changed)
    );
    assert_eq!(
        s.write_new("build-project", SOURCE),
        Err(SkillError::Exists)
    );
    assert!(s
        .read("build-project")
        .unwrap()
        .procedure
        .contains("new checks"));
}
#[test]
fn unsafe_identifiers_and_malformed_files_do_not_hide_good_skills() {
    let t = Temp::new();
    let s = SkillStore::open(&t.0).unwrap();
    for id in [
        "../escape",
        "/absolute",
        "a/b",
        "a\\b",
        "",
        ".hidden",
        "x:bad",
        "UPPER",
    ] {
        assert_eq!(s.write_new(id, SOURCE), Err(SkillError::InvalidId));
    }
    s.write_new("good", SOURCE).unwrap();
    std::fs::create_dir(t.0.join(".hive/skills/broken")).unwrap();
    std::fs::write(t.0.join(".hive/skills/broken/SKILL.md"), "missing header").unwrap();
    let list = s.list().unwrap();
    assert_eq!(list.skills.len(), 1);
    assert_eq!(list.issues.len(), 1);
    assert_eq!(list.issues[0].id, "broken");
}
#[test]
fn exact_size_and_count_limits_are_enforced() {
    let header = "---\nname: x\ndescription: y\n---\n";
    let exact = format!("{header}{}", "x".repeat(MAX_SKILL_BYTES - header.len()));
    assert!(parse_skill(&exact).is_ok());
    assert_eq!(parse_skill(&(exact + "x")).unwrap_err(), SkillError::Limit);
    let t = Temp::new();
    let s = SkillStore::open(&t.0).unwrap();
    for n in 0..MAX_SKILLS {
        let p = t.0.join(format!(".hive/skills/skill-{n}"));
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("SKILL.md"), SOURCE).unwrap();
    }
    assert_eq!(s.list().unwrap().skills.len(), MAX_SKILLS);
    assert_eq!(s.write_new("too-many", SOURCE), Err(SkillError::Limit));
    assert!(!t.0.join(".hive/skills/too-many").exists());
}
#[test]
fn cooperative_lock_prevents_concurrent_access_without_stealing_stale_lock() {
    let t = Temp::new();
    let a = SkillStore::open(&t.0).unwrap();
    let b = SkillStore::open(&t.0).unwrap();
    let lock = a.lock().unwrap();
    assert!(matches!(b.list(), Err(SkillError::Busy)));
    assert_eq!(b.write_new("one", SOURCE), Err(SkillError::Busy));
    drop(lock);
    b.write_new("one", SOURCE).unwrap();
    assert!(!t.0.join(".hive/skills/.write.lock").exists());
    assert_eq!(
        std::fs::read_dir(t.0.join(".hive/skills/one"))
            .unwrap()
            .count(),
        1
    );
}
#[test]
fn corrupt_usage_is_reported_without_leaking_content() {
    let t = Temp::new();
    let s = SkillStore::open(&t.0).unwrap();
    s.write_new("one", SOURCE).unwrap();
    std::fs::write(
        t.0.join(".hive/skills/one/.last-used.json"),
        "SECRET invalid JSON",
    )
    .unwrap();
    let list = s.list().unwrap();
    assert_eq!(list.issues[0].error, SkillError::InvalidUsage);
    assert!(!list.issues[0].error.to_string().contains("SECRET"));
}
#[cfg(unix)]
#[test]
fn rejects_symlinks_at_every_level_and_hardlinked_content() {
    use std::os::unix::fs::symlink;
    let outside = Temp::new();
    std::fs::write(outside.0.join("SKILL.md"), SOURCE).unwrap();
    let t = Temp::new();
    let alias = t.0.join("alias");
    symlink(&outside.0, &alias).unwrap();
    assert!(matches!(
        SkillStore::open(alias),
        Err(SkillError::UnsafePath)
    ));
    symlink(&outside.0, t.0.join(".hive")).unwrap();
    assert!(matches!(
        SkillStore::open(&t.0),
        Err(SkillError::UnsafePath)
    ));
    std::fs::remove_file(t.0.join(".hive")).unwrap();
    std::fs::create_dir(t.0.join(".hive")).unwrap();
    symlink(&outside.0, t.0.join(".hive/skills")).unwrap();
    assert!(matches!(
        SkillStore::open(&t.0),
        Err(SkillError::UnsafePath)
    ));
    std::fs::remove_file(t.0.join(".hive/skills")).unwrap();
    let s = SkillStore::open(&t.0).unwrap();
    symlink(&outside.0, t.0.join(".hive/skills/escape")).unwrap();
    assert!(matches!(s.read("escape"), Err(SkillError::UnsafePath)));
    std::fs::create_dir(t.0.join(".hive/skills/file")).unwrap();
    symlink(
        outside.0.join("SKILL.md"),
        t.0.join(".hive/skills/file/SKILL.md"),
    )
    .unwrap();
    assert!(matches!(s.read("file"), Err(SkillError::UnsafePath)));
    std::fs::create_dir(t.0.join(".hive/skills/hard")).unwrap();
    std::fs::hard_link(
        outside.0.join("SKILL.md"),
        t.0.join(".hive/skills/hard/SKILL.md"),
    )
    .unwrap();
    assert!(matches!(s.read("hard"), Err(SkillError::UnsafePath)));
    assert_eq!(
        std::fs::read_to_string(outside.0.join("SKILL.md")).unwrap(),
        SOURCE
    );
}

#[test]
fn stale_sidecar_does_not_cause_partial_creation_and_public_usage_clock_works() {
    let t = Temp::new();
    let s = SkillStore::open(&t.0).unwrap();
    let p = t.0.join(".hive/skills/one");
    std::fs::create_dir(&p).unwrap();
    std::fs::write(p.join(".last-used.json"), "{}").unwrap();
    assert_eq!(s.write_new("one", SOURCE), Err(SkillError::InvalidUsage));
    assert!(!p.join("SKILL.md").exists());
    std::fs::remove_file(p.join(".last-used.json")).unwrap();
    let saved = s.write_new("one", SOURCE).unwrap();
    s.mark_used("one", &saved.revision).unwrap();
    assert!(s.read("one").unwrap().summary.last_used_unix_ms.unwrap() > 0);
}
