//! One process-local store per path. Initialization is serialized, including failed attempts.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(super) struct StoreCache<T>(Mutex<Option<(PathBuf, Arc<T>)>>);

impl<T> StoreCache<T> {
    pub(super) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub(super) fn get(
        &self,
        path: &Path,
        allowed: impl FnOnce() -> Result<(), String>,
        open: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Result<Arc<T>, String> {
        let mut entry = self
            .0
            .lock()
            .map_err(|_| "Bots store cache unavailable".to_string())?;
        // Re-evaluate primary selection even on a hit. Never retain an accessible fallback.
        if let Err(error) = allowed() {
            *entry = None;
            return Err(error);
        }
        if let Some((cached_path, store)) = entry.as_ref() {
            if cached_path == path {
                return Ok(Arc::clone(store));
            }
        }
        *entry = None;
        let store = Arc::new(open(path)?);
        *entry = Some((path.to_owned(), Arc::clone(&store)));
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn concurrent_requests_open_once() {
        let cache = StoreCache::new();
        let opens = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        cache
                            .get(
                                Path::new("one"),
                                || Ok(()),
                                |_| {
                                    opens.fetch_add(1, Ordering::SeqCst);
                                    Ok(42)
                                },
                            )
                            .unwrap()
                    })
                })
                .collect();
            let stores: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            assert!(stores.iter().all(|store| Arc::ptr_eq(store, &stores[0])));
        });
        assert_eq!(opens.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn primary_selection_invalidates_cached_store() {
        let cache = StoreCache::new();
        let first = cache.get(Path::new("one"), || Ok(()), |_| Ok(1)).unwrap();
        assert!(cache
            .get(
                Path::new("one"),
                || Err("remote selected".into()),
                |_| Ok(2)
            )
            .is_err());
        let next = cache.get(Path::new("one"), || Ok(()), |_| Ok(3)).unwrap();
        assert!(!Arc::ptr_eq(&first, &next));
        assert_eq!(*next, 3);
    }

    #[test]
    fn path_change_and_failed_open_never_reuse_old_store() {
        let cache = StoreCache::new();
        cache.get(Path::new("one"), || Ok(()), |_| Ok(1)).unwrap();
        assert!(cache
            .get(Path::new("two"), || Ok(()), |_| Err("failed".into()))
            .is_err());
        let next = cache.get(Path::new("two"), || Ok(()), |_| Ok(2)).unwrap();
        assert_eq!(*next, 2);
        let original = cache.get(Path::new("one"), || Ok(()), |_| Ok(3)).unwrap();
        assert_eq!(*original, 3);
    }
}
