//! Bounded, process-local reuse for runtime Keychain reads, never transaction snapshots.
use std::{collections::HashMap, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use zeroize::{Zeroize, Zeroizing};

type Revision = Option<(u64, u64)>;
type ReadResult = Result<Option<Vec<u8>>, String>;
type Entries = Mutex<HashMap<String, Arc<ReadEntry>>>;
const LIFETIME: Duration = Duration::from_secs(60);
const RETRY_DELAY: Duration = Duration::from_secs(3);
const MAX_ENTRIES: usize = 32;
const MAX_CREDENTIAL_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct ReadEntry {
    invalidated: AtomicBool,
    value: Mutex<Option<CachedRead>>,
}

struct CachedRead {
    revision: Option<Revision>,
    expires: Instant,
    result: Result<Option<Zeroizing<Vec<u8>>>, String>,
}

impl CachedRead {
    fn copy_result(&self) -> ReadResult {
        self.result.as_ref().map(|value| value.as_ref().map(|value| value.to_vec())).map_err(Clone::clone)
    }
}

pub(super) struct CredentialReadCache {
    entries: Arc<Entries>,
    enabled: bool,
}

impl CredentialReadCache {
    pub(super) fn new() -> Self {
        let entries = Arc::new(Mutex::new(HashMap::new()));
        let weak = Arc::downgrade(&entries);
        let enabled = std::thread::Builder::new().name("credential-expiry".into()).spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let Some(entries) = weak.upgrade() else { break; };
                expire(&entries, Instant::now());
            }
        }).is_ok();
        // If automatic zeroization cannot be scheduled, do not retain secrets.
        Self { entries, enabled }
    }

    pub(super) fn read(&self, service: &str, revision: impl FnOnce() -> Result<Revision, String>, load: impl FnOnce() -> ReadResult) -> ReadResult {
        if !self.enabled { return load(); }
        let entry = {
            let mut entries = self.entries.lock().unwrap_or_else(|error| error.into_inner());
            if !entries.contains_key(service) && entries.len() >= MAX_ENTRIES {
                drop(entries);
                return load();
            }
            entries.entry(service.into()).or_insert_with(|| Arc::new(ReadEntry::default())).clone()
        };
        // One loader per item; unrelated Providers never wait on its authorization.
        let mut cached = entry.value.lock().unwrap_or_else(|error| error.into_inner());
        if entry.invalidated.load(Ordering::SeqCst) {
            return Err("credential_read_stale: credential changed during lookup; retry".into());
        }
        // Probe after waiting for the previous loader: unlocking/changing the
        // item while waiting must not reuse an earlier metadata snapshot.
        let revision = revision().ok();
        if let Some(value) = cached.as_ref().filter(|value| value.revision == revision
            && value.expires > Instant::now() && (revision.is_some() || value.result.is_err())) {
            return value.copy_result();
        }
        *cached = None;
        let mut result = load();
        if entry.invalidated.load(Ordering::SeqCst) {
            if let Ok(Some(value)) = &mut result { value.zeroize(); }
            return Err("credential_read_stale: credential changed during lookup; retry".into());
        }
        if result.as_ref().ok().and_then(Option::as_ref).is_some_and(|value| value.len() > MAX_CREDENTIAL_BYTES) {
            return result;
        }
        // Unverifiable/locked metadata may trigger a fresh native read, but
        // cannot authorize retention or reuse of any returned secret.
        if revision.is_none() && result.is_ok() { return result; }
        let lifetime = if matches!(&result, Ok(Some(_))) { LIFETIME } else { RETRY_DELAY };
        let value = CachedRead {
            revision, expires: Instant::now() + lifetime,
            result: result.map(|value| value.map(Zeroizing::new)),
        };
        if entry.invalidated.load(Ordering::SeqCst) {
            return Err("credential_read_stale: credential changed during lookup; retry".into());
        }
        let result = value.copy_result();
        *cached = Some(value);
        result
    }

    pub(super) fn invalidate(&self, service: &str) {
        let entry = self.entries.lock().unwrap_or_else(|error| error.into_inner()).remove(service);
        if let Some(entry) = entry {
            entry.invalidated.store(true, Ordering::SeqCst);
            // A pending authorization must not block credential replacement.
            // Its loader drops the zeroizing result instead of publishing it.
            if let Ok(mut value) = entry.value.try_lock() { *value = None; }
        }
    }

    pub(super) fn retry(&self, service: &str) {
        let entry = self.entries.lock().unwrap_or_else(|error| error.into_inner()).get(service).cloned();
        if let Some(entry) = entry {
            if let Ok(mut value) = entry.value.try_lock() {
                if value.as_ref().is_some_and(|value| value.result.is_err()) { *value = None; }
            }
        }
    }
}

fn expire(entries: &Entries, now: Instant) {
    entries.lock().unwrap_or_else(|error| error.into_inner()).retain(|_, entry| {
        let Ok(mut value) = entry.value.try_lock() else { return true; };
        if value.as_ref().is_some_and(|value| value.expires <= now) { *value = None; }
        value.is_some() || Arc::strong_count(entry) > 1
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::{AtomicUsize, Ordering}, Barrier};

    fn cache() -> CredentialReadCache {
        CredentialReadCache { entries: Arc::new(Mutex::new(HashMap::new())), enabled: true }
    }

    #[test]
    fn concurrent_runtime_reads_share_one_lookup() {
        let cache = Arc::new(cache());
        let calls = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(Barrier::new(2));
        let handles = (0..8).map(|_| {
            let (cache, calls, ready) = (cache.clone(), calls.clone(), ready.clone());
            std::thread::spawn(move || cache.read("item", || Ok(Some((1, 1))), || {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 { ready.wait(); }
                Ok(Some(b"first".to_vec()))
            }).unwrap())
        }).collect::<Vec<_>>();
        ready.wait();
        for handle in handles { assert_eq!(handle.join().unwrap(), Some(b"first".to_vec())); }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn changed_revision_deletion_and_explicit_invalidation_discard_old_values() {
        let cache = cache();
        assert_eq!(cache.read("item", || Ok(Some((1, 1))), || Ok(Some(b"first".to_vec()))).unwrap(), Some(b"first".to_vec()));
        assert_eq!(cache.read("item", || Ok(Some((1, 2))), || Ok(Some(b"second".to_vec()))).unwrap(), Some(b"second".to_vec()));
        assert_eq!(cache.read("item", || Ok(None), || Ok(None)).unwrap(), None);
        cache.invalidate("item");
        assert_eq!(cache.read("item", || Ok(None), || Ok(Some(b"new".to_vec()))).unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn expired_credentials_are_dropped_without_another_read() {
        let cache = cache();
        cache.read("item", || Ok(Some((1, 1))), || Ok(Some(b"first".to_vec()))).unwrap();
        expire(&cache.entries, Instant::now() + LIFETIME);
        assert!(cache.entries.lock().unwrap().is_empty());
    }

    #[test]
    fn denied_reads_share_a_short_retry_delay() {
        let cache = cache();
        assert!(cache.read("item", || Ok(Some((1, 1))), || Err("denied".into())).is_err());
        assert_eq!(cache.read("item", || Ok(Some((1, 1))), || panic!("must not immediately prompt again")).unwrap_err(), "denied");
        expire(&cache.entries, Instant::now() + RETRY_DELAY);
        assert_eq!(cache.read("item", || Ok(Some((1, 1))), || Ok(Some(b"allowed".to_vec()))).unwrap(), Some(b"allowed".to_vec()));
    }

    #[test]
    fn an_explicit_retry_can_repeat_a_denied_lookup_immediately() {
        let cache = cache();
        assert!(cache.read("item", || Ok(Some((1, 1))), || Err("denied".into())).is_err());
        cache.retry("item");
        assert_eq!(cache.read("item", || Ok(Some((1, 1))), || Ok(Some(b"allowed".to_vec()))).unwrap(), Some(b"allowed".to_vec()));
    }

    #[test]
    fn invalidation_during_a_pending_lookup_never_publishes_the_old_value() {
        let cache = Arc::new(cache());
        let (started, ready) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let reader = cache.clone();
        let handle = std::thread::spawn(move || reader.read("item", || Ok(Some((1, 1))), || {
            started.send(()).unwrap();
            wait.recv().unwrap();
            Ok(Some(b"old".to_vec()))
        }));
        ready.recv().unwrap();
        cache.invalidate("item");
        release.send(()).unwrap();
        assert!(handle.join().unwrap().unwrap_err().contains("credential_read_stale"));
        assert_eq!(cache.read("item", || Ok(Some((1, 2))), || Ok(Some(b"new".to_vec()))).unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn unverifiable_metadata_never_reuses_a_previously_authorized_value() {
        let cache = cache();
        cache.read("item", || Ok(Some((1, 1))), || Ok(Some(b"old".to_vec()))).unwrap();
        assert!(cache.read("item", || Err("locked".into()), || Err("denied".into())).is_err());
        assert!(cache.read("item", || Err("locked".into()), || panic!("must share denial")).is_err());
        assert_eq!(cache.read("item", || Ok(Some((1, 1))), || Ok(Some(b"current".to_vec()))).unwrap(), Some(b"current".to_vec()));
    }
}
