//! Content-addressed blob store on local disk (ADR-007). Layout: `<dir>/ab/cd/<sha256>` + a
//! sidecar `<sha256>.mime`. Writes go to a temp file then rename, so a crash never leaves a
//! half-blob at a valid address. Replication between servers is the coordinator's job (later).

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct Store {
    dir: PathBuf,
}

pub fn valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        Ok(Self { dir: dir.to_path_buf() })
    }

    fn path_for(&self, hash: &str) -> PathBuf {
        self.dir.join(&hash[0..2]).join(&hash[2..4]).join(hash)
    }

    /// Store bytes; returns (hash, len). Idempotent — same bytes, same address, no rewrite.
    pub fn put(&self, data: &[u8], mime: &str) -> Result<(String, u64)> {
        let hash = hex::encode(Sha256::digest(data));
        let p = self.path_for(&hash);
        if p.exists() {
            return Ok((hash, data.len() as u64));
        }
        fs::create_dir_all(p.parent().unwrap())?;
        let tmp = p.with_extension("part");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &p)?;
        fs::write(p.with_extension("mime"), mime.as_bytes())?;
        Ok((hash, data.len() as u64))
    }

    pub fn get(&self, hash: &str) -> Result<Option<(Vec<u8>, String)>> {
        if !valid_hash(hash) {
            return Ok(None);
        }
        let p = self.path_for(hash);
        if !p.exists() {
            return Ok(None);
        }
        let data = fs::read(&p)?;
        Ok(Some((data, self.mime_of(hash))))
    }

    pub fn stat(&self, hash: &str) -> Result<Option<(u64, String)>> {
        if !valid_hash(hash) {
            return Ok(None);
        }
        let p = self.path_for(hash);
        match fs::metadata(&p) {
            Ok(m) => Ok(Some((m.len(), self.mime_of(hash)))),
            Err(_) => Ok(None),
        }
    }

    fn mime_of(&self, hash: &str) -> String {
        fs::read_to_string(self.path_for(hash).with_extension("mime")).unwrap_or_else(|_| "application/octet-stream".into())
    }

    /// Every (hash, bytes) on disk.
    pub fn list(&self) -> Result<Vec<(String, u64)>> {
        let mut out = vec![];
        for a in fs::read_dir(&self.dir)?.flatten() {
            if !a.path().is_dir() { continue; }
            for b in fs::read_dir(a.path())?.flatten() {
                if !b.path().is_dir() { continue; }
                for f in fs::read_dir(b.path())?.flatten() {
                    let name = f.file_name().to_string_lossy().to_string();
                    if valid_hash(&name) {
                        out.push((name, f.metadata().map(|m| m.len()).unwrap_or(0)));
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn count(&self) -> Result<usize> {
        Ok(self.list()?.len())
    }

    pub fn used_bytes(&self) -> Result<u64> {
        Ok(self.list()?.iter().map(|(_, b)| b).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_idempotent() {
        let dir = std::env::temp_dir().join(format!("hive-store-test-{}", std::process::id()));
        let s = Store::open(&dir).unwrap();
        let (h1, n) = s.put(b"hello hive", "text/plain").unwrap();
        assert_eq!(n, 10);
        assert!(valid_hash(&h1));
        let (h2, _) = s.put(b"hello hive", "text/plain").unwrap();
        assert_eq!(h1, h2);
        let (data, mime) = s.get(&h1).unwrap().unwrap();
        assert_eq!(data, b"hello hive");
        assert_eq!(mime, "text/plain");
        assert_eq!(s.count().unwrap(), 1);
        assert!(s.get("00").unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }
}
