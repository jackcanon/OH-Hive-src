//! Durable per-card repository checkout. Ownership receipt lives outside model workspace.
//! Unknown/partial clones are preserved and require operator recovery; never wipe on retry.
use super::{run_git, CodeSessionSpec, CoderError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

pub(crate) struct Prepared {
    pub root: PathBuf,
    // Hold throughout the entire session, not merely during preparation. OS releases on crash.
    _lock: Option<File>,
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Identity {
    version: u32,
    card: Uuid,
    repo: String,
    reference: Option<String>,
    branch: String,
    #[serde(default)]
    cache: Option<PathBuf>,
    #[serde(default)]
    base_commit: Option<String>,
}

/// The largest path the ordinary Win32 parser accepts, terminating NUL included.
const MAX_PATH: usize = 260;

/// `std::fs::canonicalize`, returning a path Git can actually be handed.
///
/// On Windows `canonicalize` returns the *verbatim* (extended-length) form, `\\?\C:\x`. Git for
/// Windows is MSYS2-based and refuses that form outright -- `fatal: could not create work tree
/// dir '\\?\C:\...': Invalid argument`, which is what made five of this module's tests fail on
/// `windows-latest` while passing everywhere else. Every canonical path here either becomes a
/// `git` argument or gets compared against a path that came from somewhere else, and both uses
/// need the one plain representation.
fn canonical(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    if !cfg!(windows) {
        return Ok(canonical);
    }
    match canonical.to_str().and_then(simplify) {
        Some(plain) => Ok(PathBuf::from(plain)),
        None => Ok(canonical),
    }
}

/// The string half of [`canonical`], split out and taking `&str` so it runs on every platform.
///
/// Windows path prefixes parse as prefixes only *on* Windows, so an implementation written
/// against `std::path::Prefix` could never be exercised by CI on macOS or Linux -- which is
/// precisely how the bug this guards against reached main and stayed red for five pushes. Here
/// the rule is ordinary string work and the tests below run wherever CI does.
///
/// Only two verbatim shapes are simplified, because only these two name something the ordinary
/// parser resolves identically: a verbatim disk path (`\\?\C:\x` -> `C:\x`) and a verbatim UNC
/// path (`\\?\UNC\server\share` -> `\\server\share`). Anything else -- a volume GUID, a device
/// path -- is left exactly as it was. Git's refusal is a better outcome than a path we quietly
/// rewrote into pointing somewhere else.
///
/// `None` means "nothing to change", so callers keep the path they already have.
///
/// Hand-rolled rather than taking the `dunce` crate: this is the whole of what we would use from
/// it, and the behaviour worth guarding is the four cases below.
fn simplify(path: &str) -> Option<String> {
    let rest = path.strip_prefix(r"\\?\")?;
    let plain = match rest.strip_prefix(r"UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => {
            let mut rest = rest.chars();
            if !rest.next()?.is_ascii_alphabetic()
                || rest.next() != Some(':')
                || rest.next() != Some('\\')
            {
                return None;
            }
            path[r"\\?\".len()..].to_string()
        }
    };
    // Past the Win32 limit the verbatim prefix is the only thing making the path openable at all.
    // Stripping it there would trade Git's clear refusal for a failure somewhere further away.
    (plain.len() < MAX_PATH).then_some(plain)
}

/// How long a lock that answers "held" is given to prove it really is.
///
/// Measured on a loaded 12-core mini, 2026-09-18: immediately after the owning `File` was
/// dropped, a fresh `try_lock` on the same path answered `WouldBlock` and then cleared on the
/// fourth attempt of a spin loop, 116 MICROseconds later, with `lsof` showing no holder at all.
/// `close` releasing a `flock` is not instantaneously visible to the next `flock` on a busy
/// machine, so a single instantaneous sample cannot tell "another session is working here" from
/// "the last one has just left". Roughly one full test run in three reported a stolen checkout
/// that nobody owned, and the same race is available to a real node preparing a card while a
/// previous one finishes.
///
/// This window is three orders of magnitude past what was measured and still cannot confuse the
/// two cases: a session lock is held for as long as the card runs -- minutes -- so a lock that
/// clears inside a quarter second was never a session.
const LOCK_SETTLE: Duration = Duration::from_millis(250);

/// Take `file`'s exclusive lock, waiting out [`LOCK_SETTLE`] before believing a refusal.
///
/// `busy` is returned only for a lock that is genuinely still held at the end of the window. A
/// lock CALL that fails is a different fact and is reported as the IO error it is -- the two used
/// to be collapsed into one message, which meant an operator could be told another session owned
/// their checkout when what actually happened was that the lock could not be taken at all.
async fn lock_exclusive(file: &File, path: &Path, busy: CoderError) -> Result<(), CoderError> {
    let deadline = std::time::Instant::now() + LOCK_SETTLE;
    loop {
        let error = match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if std::time::Instant::now() >= deadline {
            return Err(match error {
                std::fs::TryLockError::WouldBlock => busy,
                std::fs::TryLockError::Error(error) => io(path, error),
            });
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

fn recovery(path: &Path, detail: &str) -> CoderError {
    CoderError::WorkspaceRecovery(format!(
        "{}: {detail}; existing files have been preserved",
        path.display()
    ))
}
fn io(path: &Path, error: std::io::Error) -> CoderError {
    CoderError::Io(path.display().to_string(), error)
}
fn reject_symlink(path: &Path) -> Result<(), CoderError> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(recovery(path, "managed path is a symlink")),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(path, e)),
    }
}
pub(super) async fn prepare(
    data: &Path,
    card: Uuid,
    spec: &CodeSessionSpec,
) -> Result<Prepared, CoderError> {
    prepare_impl(data, card, spec, None).await
}

#[cfg(feature = "local-hub")]
pub(crate) async fn prepare_authenticated(
    data: &Path,
    card: Uuid,
    spec: &CodeSessionSpec,
    token: &str,
) -> Result<Prepared, CoderError> {
    if spec.workspace_path.is_some() {
        return Err(recovery(
            data,
            "authenticated preparation cannot import a folder",
        ));
    }
    prepare_impl(data, card, spec, Some(token)).await
}

async fn prepare_impl(
    data: &Path,
    card: Uuid,
    spec: &CodeSessionSpec,
    token: Option<&str>,
) -> Result<Prepared, CoderError> {
    if let Some(expected) = &spec.prepared_workspace_root {
        let destination = data.join("code-workspaces").join(card.to_string());
        let actual = canonical(&destination)
            .map_err(|_| recovery(&destination, "prepared checkout is missing"))?;
        if actual != Path::new(expected)
            || !data
                .join("code-workspace-state")
                .join(format!("{card}.json"))
                .exists()
            || spec.workspace_path.is_some()
        {
            return Err(recovery(&destination, "prepared checkout identity changed"));
        }
    }
    if let Some(path) = &spec.workspace_path {
        let root = PathBuf::from(path);
        let meta = tokio::fs::metadata(&root)
            .await
            .map_err(|e| CoderError::WorkspacePath(root.display().to_string(), e))?;
        if !meta.is_dir() {
            return Err(CoderError::WorkspaceNotADirectory(
                root.display().to_string(),
            ));
        }
        return Ok(Prepared {
            root: canonical(&root).map_err(|e| io(&root, e))?,
            _lock: None,
        });
    }
    let parent = data.join("code-workspaces");
    let state = data.join("code-workspace-state");
    for dir in [&parent, &state] {
        reject_symlink(dir)?;
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let dest = parent.join(card.to_string());
    let lock_path = state.join(format!("{card}.lock"));
    reject_symlink(&lock_path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| io(&lock_path, e))?;
    lock_exclusive(
        &lock,
        &lock_path,
        recovery(&dest, "another session owns this task checkout"),
    )
    .await?;
    reject_symlink(&dest)?;
    let receipt = state.join(format!("{card}.json"));
    reject_symlink(&receipt)?;
    let mut identity = Identity {
        version: 1,
        card,
        repo: spec.repo_url.clone().expect("validated repo spec"),
        reference: spec.repo_ref.clone(),
        branch: format!("hive/{card}"),
        cache: None,
        base_commit: None,
    };
    if receipt.exists() {
        let bytes = std::fs::read(&receipt).map_err(|e| io(&receipt, e))?;
        let saved: Identity = serde_json::from_slice(&bytes)
            .map_err(|_| recovery(&dest, "invalid ownership receipt"))?;
        if saved.version != 1
            || saved.card != identity.card
            || saved.repo != identity.repo
            || saved.reference != identity.reference
            || saved.branch != identity.branch
        {
            return Err(recovery(&dest, "repository or requested reference changed"));
        }
        reject_symlink(&dest.join(".git"))?;
        if let Some(cache) = &saved.cache {
            let expected = canonical(data.join("code-repositories"))
                .map_err(|e| io(data, e))?
                .join(repo_key(&identity.repo));
            if cache != &expected {
                return Err(recovery(&dest, "cache identity changed"));
            }
            reject_symlink(cache)?;
            reject_symlink(&cache.join(".git"))?;
            let common = run_git(
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                Some(&dest),
            )
            .await?;
            let actual = canonical(common.trim()).map_err(|e| io(&dest, e))?;
            let expected = canonical(cache.join(".git")).map_err(|e| io(cache, e))?;
            if actual != expected {
                return Err(recovery(&dest, "worktree belongs to another repository"));
            }
        } else if !dest.join(".git").is_dir() {
            return Err(recovery(
                &dest,
                "legacy checkout is missing its Git directory",
            ));
        }
        if saved.cache.is_none() {
            reject_symlink(&dest.join(".git"))?;
            let origin = run_git(&["config", "--get", "remote.origin.url"], Some(&dest)).await?;
            if origin.trim() != identity.repo {
                return Err(recovery(&dest, "checkout origin changed"));
            }
        }
        let head = run_git(&["symbolic-ref", "HEAD"], Some(&dest)).await?;
        if head.trim() != format!("refs/heads/{}", identity.branch) {
            return Err(recovery(&dest, "task branch changed or HEAD is detached"));
        }
        run_git(&["rev-parse", "--verify", "HEAD"], Some(&dest)).await?;
    } else {
        if dest.exists() {
            return Err(recovery(&dest,"checkout has no ownership receipt; inspect legacy or interrupted work before retrying"));
        }
        if let Some(token) = token {
            let absolute = canonical(&parent)
                .map_err(|e| io(&parent, e))?
                .join(card.to_string());
            super::github_git::clone_fresh(&identity.repo, token, &absolute)
                .await
                .map_err(|message| recovery(&dest, message))?;
            let base = resolve_base(&absolute, identity.reference.as_deref()).await?;
            run_git(
                &["checkout", "-b", &identity.branch, &base],
                Some(&absolute),
            )
            .await?;
            identity.base_commit = Some(base);
        } else {
            let (cache, base) = create_worktree(data, &dest, &identity).await?;
            identity.cache = Some(cache);
            identity.base_commit = Some(base);
        }
        // Publish the receipt only after preparation succeeds. A crash before this point leaves
        // a clone requiring inspection, never a clone we might silently discard or reinitialize.
        let staging = state.join(format!("{card}.{}.tmp", Uuid::new_v4()));
        use std::io::Write;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|e| io(&staging, e))?;
        file.write_all(&serde_json::to_vec(&identity).expect("identity serializes"))
            .map_err(|e| io(&staging, e))?;
        file.sync_all().map_err(|e| io(&staging, e))?;
        drop(file);
        std::fs::rename(&staging, &receipt).map_err(|e| io(&receipt, e))?;
    }
    Ok(Prepared {
        root: canonical(&dest).map_err(|e| io(&dest, e))?,
        _lock: Some(lock),
    })
}

fn repo_key(repo: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(repo.as_bytes()))
}
async fn create_worktree(
    data: &Path,
    dest: &Path,
    identity: &Identity,
) -> Result<(PathBuf, String), CoderError> {
    let repositories = data.join("code-repositories");
    reject_symlink(&repositories)?;
    std::fs::create_dir_all(&repositories).map_err(|e| io(&repositories, e))?;
    let repositories = canonical(&repositories).map_err(|e| io(&repositories, e))?;
    let key = repo_key(&identity.repo);
    let cache = repositories.join(&key);
    let lock_path = repositories.join(format!("{key}.lock"));
    reject_symlink(&lock_path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| io(&lock_path, e))?;
    lock_exclusive(
        &lock,
        &lock_path,
        recovery(
            dest,
            "repository cache is busy; retry after the other preparation finishes",
        ),
    )
    .await?;
    reject_symlink(&cache)?;
    let ready = repositories.join(format!("{key}.ready"));
    reject_symlink(&ready)?;
    if cache.exists() {
        if !ready.exists() {
            return Err(recovery(
                &cache,
                "interrupted cache creation requires inspection",
            ));
        }
        reject_symlink(&cache.join(".git"))?;
        let origin = run_git(&["config", "--get", "remote.origin.url"], Some(&cache)).await?;
        if origin.trim() != identity.repo {
            return Err(recovery(&cache, "cache origin changed"));
        }
        run_git(&["fetch", "origin"], Some(&cache)).await?;
        run_git(&["remote", "set-head", "origin", "--auto"], Some(&cache)).await?;
    } else {
        if ready.exists() {
            return Err(recovery(&cache, "recorded cache is missing"));
        }
        let path = cache
            .to_str()
            .ok_or_else(|| CoderError::NonUtf8Path(cache.clone()))?;
        run_git(
            &["clone", "--no-checkout", "--", &identity.repo, path],
            None,
        )
        .await?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ready)
            .map_err(|e| io(&ready, e))?;
        file.sync_all().map_err(|e| io(&ready, e))?;
    }
    let base = resolve_base(&cache, identity.reference.as_deref()).await?;
    let absolute = canonical(dest.parent().expect("task parent"))
        .map_err(|e| io(dest, e))?
        .join(dest.file_name().expect("card filename"));
    let path = absolute
        .to_str()
        .ok_or_else(|| CoderError::NonUtf8Path(absolute.clone()))?;
    run_git(
        &["worktree", "add", "-b", &identity.branch, "--", path, &base],
        Some(&cache),
    )
    .await?;
    Ok((cache, base))
}

async fn resolve_base(cache: &Path, reference: Option<&str>) -> Result<String, CoderError> {
    let requested = reference.unwrap_or("HEAD");
    let normalized = if requested == "HEAD" {
        "refs/remotes/origin/HEAD".to_owned()
    } else if let Some(branch) = requested.strip_prefix("refs/heads/") {
        format!("refs/remotes/origin/{branch}")
    } else {
        requested.to_owned()
    };
    let has_reference = reference.is_some();
    let reference = normalized.as_str();
    if reference.starts_with('-') {
        return Err(recovery(
            cache,
            "reference cannot begin with an option prefix",
        ));
    }
    let resolve = |r: &str| {
        vec![
            "rev-parse".to_owned(),
            "--verify".into(),
            "--end-of-options".into(),
            format!("{r}^{{commit}}"),
        ]
    };
    // Branch names refer to fetched remote branches, never another task's local branch.
    let remote = resolve(&format!("refs/remotes/origin/{reference}"));
    let base = if has_reference && !reference.starts_with("refs/") {
        match run_git(
            &remote.iter().map(String::as_str).collect::<Vec<_>>(),
            Some(cache),
        )
        .await
        {
            Ok(base) => base,
            Err(CoderError::GitFailed { .. }) => {
                let args = resolve(reference);
                run_git(
                    &args.iter().map(String::as_str).collect::<Vec<_>>(),
                    Some(cache),
                )
                .await?
            }
            Err(e) => return Err(e),
        }
    } else {
        let args = resolve(reference);
        run_git(
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            Some(cache),
        )
        .await?
    };
    let base = base.trim().to_owned();
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_file(name: &str) -> (PathBuf, File) {
        let path = std::env::temp_dir().join(format!("hive-lock-test-{}-{name}", Uuid::new_v4()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .unwrap();
        (path, file)
    }
    fn reopen(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap()
    }

    // The bug this guards: one instantaneous `try_lock` cannot tell a lock that is held from one
    // whose holder has just closed it, because the release is not immediately visible on a loaded
    // machine. Here the holder leaves well inside the window, which used to read as contention.
    #[tokio::test]
    async fn a_lock_whose_holder_leaves_inside_the_window_is_taken() {
        let (path, held) = lock_file("released");
        held.try_lock().unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            drop(held);
        });

        let waiting = reopen(&path);
        lock_exclusive(&waiting, &path, recovery(&path, "should not be reported"))
            .await
            .expect("a lock released inside the window is not contention");
        std::fs::remove_file(&path).ok();
    }

    // And the other half, which the window must not cost us: a holder that stays is still
    // reported, with the caller's own message, after waiting rather than before.
    #[tokio::test]
    async fn a_lock_still_held_at_the_end_of_the_window_is_reported_busy() {
        let (path, held) = lock_file("held");
        held.try_lock().unwrap();

        let waiting = reopen(&path);
        let started = std::time::Instant::now();
        let error = lock_exclusive(
            &waiting,
            &path,
            recovery(&path, "another session owns this task checkout"),
        )
        .await
        .expect_err("a lock that is genuinely held must still be refused");
        assert!(
            error.to_string().contains("another session owns"),
            "the caller's message must survive: {error}"
        );
        assert!(
            started.elapsed() >= LOCK_SETTLE,
            "refusal must come after the window, not before: {:?}",
            started.elapsed()
        );
        drop(held);
        std::fs::remove_file(&path).ok();
    }
    /// These run on every platform on purpose. The bug they guard against was invisible to macOS
    /// and Linux CI precisely because it lived in Windows-only path parsing, so the rule is
    /// expressed as string work that every runner can execute.
    #[test]
    fn a_verbatim_disk_path_loses_its_prefix_and_a_verbatim_unc_path_becomes_a_share() {
        assert_eq!(
            simplify(r"\\?\C:\Users\runneradmin\code-repositories\ea0e"),
            Some(r"C:\Users\runneradmin\code-repositories\ea0e".to_string()),
            "the exact shape Git for Windows refused in run 35452714646"
        );
        assert_eq!(
            simplify(r"\\?\UNC\server\share\repo"),
            Some(r"\\server\share\repo".to_string())
        );
    }

    #[test]
    fn a_path_with_nothing_to_strip_is_left_alone() {
        assert_eq!(simplify(r"C:\already\plain"), None);
        assert_eq!(simplify("/home/claude/plain"), None);
        assert_eq!(simplify(""), None);
    }

    #[test]
    fn a_verbatim_path_that_is_not_a_plain_drive_or_share_is_never_rewritten() {
        // A volume GUID and a device path have no plain equivalent -- simplifying either would
        // silently change which object the path names.
        assert_eq!(simplify(r"\\?\Volume{9f3a}\repo"), None);
        assert_eq!(simplify(r"\\?\PhysicalDrive0"), None);
        // Shapes that only look like a drive.
        assert_eq!(
            simplify(r"\\?\C:"),
            None,
            "verbatim paths are fully qualified"
        );
        assert_eq!(simplify(r"\\?\4:\repo"), None, "a drive letter is a letter");
    }

    #[test]
    fn a_path_only_the_verbatim_form_can_express_keeps_it() {
        let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
        assert_eq!(
            simplify(&long),
            None,
            "past MAX_PATH the prefix is load-bearing; Git's refusal beats a path Windows rejects"
        );
        let fits = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH - r"C:\".len() - 1));
        assert!(fits.len() > MAX_PATH, "the verbatim form is over the limit");
        assert!(
            simplify(&fits).is_some_and(|p| p.len() < MAX_PATH),
            "but the simplified form is under it, so it is simplified"
        );
    }

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=Workspace Test",
                "-c",
                "user.email=workspace@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn fixture() -> (PathBuf, CodeSessionSpec) {
        let dir = std::env::temp_dir().join(format!("hive-workspace-test-{}", Uuid::new_v4()));
        let source = dir.join("source");
        std::fs::create_dir_all(&source).unwrap();
        git(&source, &["init"]);
        std::fs::write(source.join("tracked.txt"), "base\n").unwrap();
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "base"]);
        let spec: CodeSessionSpec = serde_json::from_value(
            serde_json::json!({"task":"edit","repo_url":source.to_str().unwrap()}),
        )
        .unwrap();
        (dir, spec)
    }
    #[tokio::test]
    async fn retry_preserves_dirty_files_commits_and_task_branch_without_remote() {
        let (dir, spec) = fixture();
        let card = Uuid::new_v4();
        let data = dir.join("data");
        let first = prepare(&data, card, &spec).await.unwrap();
        std::fs::write(first.root.join("tracked.txt"), "committed\n").unwrap();
        git(&first.root, &["commit", "-am", "task checkpoint"]);
        std::fs::write(first.root.join("tracked.txt"), "dirty\n").unwrap();
        std::fs::write(first.root.join("untracked.txt"), "keep me").unwrap();
        assert!(
            prepare(&data, card, &spec).await.is_err(),
            "session lock must exclude a second owner"
        );
        drop(first);
        std::fs::rename(dir.join("source"), dir.join("offline-source")).unwrap();
        let second = prepare(&data, card, &spec).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(second.root.join("tracked.txt")).unwrap(),
            "dirty\n"
        );
        assert_eq!(
            std::fs::read_to_string(second.root.join("untracked.txt")).unwrap(),
            "keep me"
        );
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&second.root)
            .args(["log", "-1", "--format=%s"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "task checkpoint"
        );
        drop(second);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn mismatched_identity_partial_clone_and_changed_branch_preserve_work() {
        let (dir, spec) = fixture();
        let card = Uuid::new_v4();
        let data = dir.join("data");
        let prepared = prepare(&data, card, &spec).await.unwrap();
        let root = prepared.root.clone();
        drop(prepared);
        std::fs::write(root.join("precious"), "keep").unwrap();
        let mut changed = spec.clone();
        changed.repo_ref = Some("HEAD".into());
        assert!(prepare(&data, card, &changed).await.is_err());
        git(&root, &["checkout", "-b", "human-branch"]);
        assert!(prepare(&data, card, &spec).await.is_err());
        std::fs::remove_file(
            data.join("code-workspace-state")
                .join(format!("{card}.json")),
        )
        .unwrap();
        assert!(prepare(&data, card, &spec).await.is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("precious")).unwrap(),
            "keep"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn different_cards_are_isolated_and_imported_workspace_is_unchanged() {
        let (dir, spec) = fixture();
        let data = dir.join("data");
        let a = prepare(&data, Uuid::new_v4(), &spec).await.unwrap();
        let b = prepare(&data, Uuid::new_v4(), &spec).await.unwrap();
        assert_ne!(a.root, b.root);
        std::fs::write(a.root.join("only-a"), "a").unwrap();
        assert!(!b.root.join("only-a").exists());
        let mut imported = spec;
        imported.workspace_path = Some(dir.join("source").to_str().unwrap().into());
        let c = prepare(&data, Uuid::new_v4(), &imported).await.unwrap();
        assert_eq!(c.root, canonical(dir.join("source")).unwrap());
        drop((a, b, c));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn shared_cache_fetches_new_base_without_changing_existing_task() {
        let (dir, spec) = fixture();
        let data = dir.join("data");
        let first_card = Uuid::new_v4();
        let a = prepare(&data, first_card, &spec).await.unwrap();
        assert!(a.root.join(".git").is_file());
        std::fs::write(a.root.join("tracked.txt"), "private edit").unwrap();
        std::fs::write(dir.join("source/tracked.txt"), "upstream update").unwrap();
        git(&dir.join("source"), &["commit", "-am", "upstream"]);
        let b = prepare(&data, Uuid::new_v4(), &spec).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(a.root.join("tracked.txt")).unwrap(),
            "private edit"
        );
        assert_eq!(
            std::fs::read_to_string(b.root.join("tracked.txt")).unwrap(),
            "upstream update"
        );
        let common_a = run_git(&["rev-parse", "--git-common-dir"], Some(&a.root))
            .await
            .unwrap();
        let common_b = run_git(&["rev-parse", "--git-common-dir"], Some(&b.root))
            .await
            .unwrap();
        assert_eq!(common_a, common_b);
        let receipt: Identity = serde_json::from_slice(
            &std::fs::read(
                data.join("code-workspace-state")
                    .join(format!("{first_card}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(receipt.base_commit.as_ref().unwrap().len() >= 40);
        drop((a, b));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn legacy_managed_clone_still_resumes_without_migration() {
        let (dir, spec) = fixture();
        let data = dir.join("data");
        let card = Uuid::new_v4();
        let dest = data.join("code-workspaces").join(card.to_string());
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        run_git(
            &[
                "clone",
                "--",
                spec.repo_url.as_ref().unwrap(),
                dest.to_str().unwrap(),
            ],
            None,
        )
        .await
        .unwrap();
        let branch = format!("hive/{card}");
        git(&dest, &["checkout", "-b", &branch]);
        let state = data.join("code-workspace-state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join(format!("{card}.json")),serde_json::to_vec(&serde_json::json!({"version":1,"card":card,"repo":spec.repo_url,"reference":null,"branch":branch})).unwrap()).unwrap();
        std::fs::write(dest.join("legacy-edit"), "preserved").unwrap();
        let p = prepare(&data, card, &spec).await.unwrap();
        assert!(p.root.join(".git").is_dir());
        assert!(p.root.join("legacy-edit").exists());
        drop(p);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn named_refs_and_changed_remote_default_resolve_to_recorded_commit() {
        let (dir, mut spec) = fixture();
        let data = dir.join("data");
        let source = dir.join("source");
        git(&source, &["tag", "baseline"]);
        let old = prepare(&data, Uuid::new_v4(), &spec).await.unwrap();
        git(&source, &["checkout", "-b", "next"]);
        std::fs::write(source.join("tracked.txt"), "next branch").unwrap();
        git(&source, &["commit", "-am", "next"]);
        let current = prepare(&data, Uuid::new_v4(), &spec).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(current.root.join("tracked.txt")).unwrap(),
            "next branch"
        );
        spec.repo_ref = Some("refs/heads/next".into());
        let named = prepare(&data, Uuid::new_v4(), &spec).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(named.root.join("tracked.txt")).unwrap(),
            "next branch"
        );
        spec.repo_ref = Some("baseline".into());
        let pinned = prepare(&data, Uuid::new_v4(), &spec).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(pinned.root.join("tracked.txt")).unwrap(),
            "base\n"
        );
        drop((old, current, named, pinned));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn managed_symlink_is_rejected_without_touching_target() {
        let (dir, spec) = fixture();
        let data = dir.join("data");
        let card = Uuid::new_v4();
        std::fs::create_dir_all(data.join("code-workspaces")).unwrap();
        std::os::unix::fs::symlink(
            dir.join("source"),
            data.join("code-workspaces").join(card.to_string()),
        )
        .unwrap();
        assert!(prepare(&data, card, &spec).await.is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("source/tracked.txt")).unwrap(),
            "base\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
