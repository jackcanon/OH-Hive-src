//! One conservative inference slot shared by updated card workers and Bots on this OS account.
//! OS file locks survive neither process exit nor guard drop. Never unlink the lock file:
//! replacing its inode would let two processes each believe they own the slot.
use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

pub struct ExecutionPermit {
    _file: File,
}

impl Drop for ExecutionPermit {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

pub fn try_acquire() -> io::Result<Option<ExecutionPermit>> {
    let root = dirs::data_local_dir()
        .ok_or_else(|| io::Error::other("No local data directory"))?
        .join("OHHive")
        .join("execution");
    std::fs::create_dir_all(&root)?;
    try_acquire_at(&root.join("local-model.lock"))
}

pub(crate) fn try_acquire_at(path: &Path) -> io::Result<Option<ExecutionPermit>> {
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(io::Error::other("Execution lock must not be a symlink"));
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(ExecutionPermit { _file: file })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_probe() {
        if let Some(path) = std::env::var_os("HIVE_TEST_CAPACITY_PATH") {
            assert!(try_acquire_at(Path::new(&path)).unwrap().is_none());
        }
    }
    #[test]
    fn other_process_observes_busy_slot() {
        let path = std::env::temp_dir().join(format!("hive-process-slot-{}", uuid::Uuid::new_v4()));
        let held = try_acquire_at(&path).unwrap().unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "execution_capacity::tests::lock_probe"])
            .env("HIVE_TEST_CAPACITY_PATH", &path)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "child process must observe the held slot"
        );
        drop(held);
        assert!(try_acquire_at(&path).unwrap().is_some());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn independent_handles_exclude_and_release() {
        let path = std::env::temp_dir().join(format!("hive-slot-{}", uuid::Uuid::new_v4()));
        let first = try_acquire_at(&path).unwrap().unwrap();
        assert!(try_acquire_at(&path).unwrap().is_none());
        drop(first);
        assert!(try_acquire_at(&path).unwrap().is_some());
        std::fs::remove_file(path).unwrap();
    }
}
