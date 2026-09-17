//! Durable per-card repository checkout. Ownership receipt lives outside model workspace.
//! Unknown/partial clones are preserved and require operator recovery; never wipe on retry.
use super::{run_git, CodeSessionSpec, CoderError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(super) struct Prepared {
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
            root: tokio::fs::canonicalize(&root)
                .await
                .map_err(|e| io(&root, e))?,
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
    lock.try_lock()
        .map_err(|_| recovery(&dest, "another session owns this task checkout"))?;
    reject_symlink(&dest)?;
    let receipt = state.join(format!("{card}.json"));
    reject_symlink(&receipt)?;
    let identity = Identity {
        version: 1,
        card,
        repo: spec.repo_url.clone().expect("validated repo spec"),
        reference: spec.repo_ref.clone(),
        branch: format!("hive/{card}"),
    };
    if receipt.exists() {
        let bytes = std::fs::read(&receipt).map_err(|e| io(&receipt, e))?;
        let saved: Identity = serde_json::from_slice(&bytes)
            .map_err(|_| recovery(&dest, "invalid ownership receipt"))?;
        if saved != identity {
            return Err(recovery(&dest, "repository or requested reference changed"));
        }
        reject_symlink(&dest.join(".git"))?;
        if !dest.join(".git").is_dir() {
            return Err(recovery(
                &dest,
                "managed checkout is missing its Git directory",
            ));
        }
        let head_path = dest.join(".git/HEAD");
        reject_symlink(&head_path)?;
        let head = std::fs::read_to_string(&head_path).map_err(|e| io(&head_path, e))?;
        if head.trim() != format!("ref: refs/heads/{}", identity.branch) {
            return Err(recovery(&dest, "task branch changed or HEAD is detached"));
        }
        run_git(&["rev-parse", "--verify", "HEAD"], Some(&dest)).await?;
    } else {
        if dest.exists() {
            return Err(recovery(&dest,"checkout has no ownership receipt; inspect legacy or interrupted work before retrying"));
        }
        let dest_str = dest
            .to_str()
            .ok_or_else(|| CoderError::NonUtf8Path(dest.clone()))?;
        run_git(&["clone", "--", &identity.repo, dest_str], None).await?;
        let mut args = vec!["checkout", "-b", identity.branch.as_str()];
        if let Some(reference) = identity.reference.as_deref() {
            if reference.starts_with('-') {
                return Err(recovery(
                    &dest,
                    "reference cannot begin with an option prefix",
                ));
            }
            args.push(reference);
        }
        run_git(&args, Some(&dest)).await?;
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
        root: tokio::fs::canonicalize(&dest)
            .await
            .map_err(|e| io(&dest, e))?,
        _lock: Some(lock),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(c.root, std::fs::canonicalize(dir.join("source")).unwrap());
        drop((a, b, c));
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
