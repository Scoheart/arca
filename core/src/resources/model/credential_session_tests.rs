use super::*;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}};

const A: &str = "com.scoheart.mux.model-provider.fixture-a";
const B: &str = "com.scoheart.mux.model-provider.fixture-b";
#[derive(Default)]
struct Memory {
    values: Mutex<BTreeMap<String, (Revision, Vec<u8>)>>,
    clock: AtomicU64,
    vault_reads: AtomicUsize,
    item_reads: AtomicUsize,
    vault_writes: AtomicUsize,
    deny: Mutex<Option<String>>,
    fail_write: AtomicBool,
    race: AtomicBool,
}
impl Memory {
    fn put(&self, service: &str, bytes: &[u8]) {
        let revision = (1, self.clock.fetch_add(1, Ordering::SeqCst) + 1);
        self.values.lock().unwrap().insert(service.into(), (revision, bytes.to_vec()));
    }
    fn bytes(&self, service: &str) -> Option<Vec<u8>> { self.values.lock().unwrap().get(service).map(|entry| entry.1.clone()) }
    fn sources() -> Vec<String> { vec![A.into(), B.into()] }
    fn fixture() -> Self { let backend = Self::default(); backend.put(A, b"fixture-alpha"); backend.put(B, b"fixture-beta"); backend }
}
impl Backend for Memory {
    fn revision(&self, service: &str) -> Result<Option<Revision>, String> {
        Ok(self.values.lock().unwrap().get(service).map(|entry| entry.0))
    }
    fn read(&self, service: &str, _: bool) -> ReadResult {
        if service == VAULT_SERVICE { self.vault_reads.fetch_add(1, Ordering::SeqCst); }
        else { self.item_reads.fetch_add(1, Ordering::SeqCst); }
        if self.deny.lock().unwrap().as_deref() == Some(service) { return Err("fixture-denied".into()); }
        let value = self.bytes(service);
        if service == A && self.race.swap(false, Ordering::SeqCst) { self.put(A, b"fixture-raced"); }
        Ok(value)
    }
    fn write_vault(&self, bytes: &[u8]) -> Result<(), String> {
        if self.fail_write.load(Ordering::SeqCst) { return Err("fixture-save-failed".into()); }
        self.vault_writes.fetch_add(1, Ordering::SeqCst);
        self.put(VAULT_SERVICE, bytes);
        Ok(())
    }
}

fn lifecycle_and_concurrency() {
    let backend = Arc::new(Memory::fixture());
    let mut first = Session::default();
    first.unlock(backend.as_ref(), &Memory::sources(), Instant::now(), false).unwrap();
    assert_eq!(backend.item_reads.load(Ordering::SeqCst), 2);
    // A new process reads one aggregate, with zero independent item reads.
    let reads = backend.item_reads.load(Ordering::SeqCst);
    let vault_reads = backend.vault_reads.load(Ordering::SeqCst);
    let session = Arc::new(Mutex::new(Session::default()));
    let handles = (0..12).map(|index| {
        let (session, backend) = (session.clone(), backend.clone());
        std::thread::spawn(move || {
            let mut session = session.lock().unwrap();
            session.unlock(backend.as_ref(), &Memory::sources(), Instant::now(), false).unwrap();
            session.read(backend.as_ref(), if index % 2 == 0 { A } else { B }, Instant::now()).unwrap().unwrap().len()
        })
    }).collect::<Vec<_>>();
    for handle in handles { assert!(handle.join().unwrap() > 0); }
    assert_eq!(backend.vault_reads.load(Ordering::SeqCst), vault_reads + 1);
    assert_eq!(backend.item_reads.load(Ordering::SeqCst), reads);
    let mut session = session.lock().unwrap();
    session.expire(Instant::now() + MAX_AGE + Duration::from_secs(60));
    assert!(session.read(backend.as_ref(), A, Instant::now()).is_err());
    session.unlock(backend.as_ref(), &Memory::sources(), Instant::now(), false).unwrap();
    assert_eq!(backend.item_reads.load(Ordering::SeqCst), reads);
    session.clear();
    assert!(session.remaining(Instant::now()).is_none());
    assert!(session.read(backend.as_ref(), B, Instant::now()).is_err());
    session.unlock(backend.as_ref(), &Memory::sources(), Instant::now(), false).unwrap();
    session.expires_wall = Some(SystemTime::now() - Duration::from_secs(1));
    assert!(session.read(backend.as_ref(), A, Instant::now()).is_err(), "sleep/wall expiry must lock even without uptime expiry");
    session.unlock(backend.as_ref(), &Memory::sources(), Instant::now(), false).unwrap();
    session.expires_wall = Some(SystemTime::now() + MAX_AGE + Duration::from_secs(60));
    assert!(session.read(backend.as_ref(), A, Instant::now()).is_err(), "clock rollback must not extend authorization");
}

fn failures_and_external_changes() {
    let backend = Memory::fixture();
    *backend.deny.lock().unwrap() = Some(VAULT_SERVICE.into());
    let mut session = Session::default();
    for _ in 0..8 { assert!(session.unlock(&backend, &Memory::sources(), Instant::now(), false).is_err()); }
    assert_eq!(backend.vault_reads.load(Ordering::SeqCst), 1);
    assert_eq!(backend.item_reads.load(Ordering::SeqCst), 0);
    *backend.deny.lock().unwrap() = None;
    session.unlock(&backend, &Memory::sources(), Instant::now(), true).unwrap();
    backend.put(A, b"fixture-rotated");
    let reads = backend.item_reads.load(Ordering::SeqCst);
    assert!(session.read(&backend, A, Instant::now()).unwrap_err().contains("refresh_required"));
    assert_eq!(backend.item_reads.load(Ordering::SeqCst), reads);
    backend.values.lock().unwrap().remove(B);
    assert_eq!(session.read(&backend, B, Instant::now()).unwrap(), None);
    session.unlock(&backend, &Memory::sources(), Instant::now(), true).unwrap();
    assert_eq!(session.read(&backend, A, Instant::now()).unwrap().unwrap(), b"fixture-rotated");
    let backend = Memory::fixture();
    *backend.deny.lock().unwrap() = Some(B.into());
    assert!(Session::default().unlock(&backend, &Memory::sources(), Instant::now(), false).is_err());
    assert!(backend.bytes(VAULT_SERVICE).is_none());
    assert!(backend.bytes(A).is_some() && backend.bytes(B).is_some());
    let backend = Memory::fixture();
    backend.race.store(true, Ordering::SeqCst);
    assert!(Session::default().unlock(&backend, &Memory::sources(), Instant::now(), false).unwrap_err().contains("refresh_required"));
    assert!(backend.bytes(VAULT_SERVICE).is_none());
}

fn writes_merge_and_safety() {
    let backend = Memory::fixture();
    let mut a = Session::default();
    let mut b = Session::default();
    a.unlock(&backend, &Memory::sources(), Instant::now(), false).unwrap();
    b.unlock(&backend, &Memory::sources(), Instant::now(), false).unwrap();
    backend.put(A, b"fixture-new-a");
    a.synchronize(&backend, A, Some(b"fixture-new-a"), Instant::now()).unwrap();
    backend.put(B, b"fixture-new-b");
    b.synchronize(&backend, B, Some(b"fixture-new-b"), Instant::now()).unwrap();
    // A third save from the process that previously merged a peer must not drop it.
    backend.put(B, b"fixture-newer-b");
    b.synchronize(&backend, B, Some(b"fixture-newer-b"), Instant::now()).unwrap();
    let document = Document::decode(&backend.bytes(VAULT_SERVICE).unwrap()).unwrap();
    assert_eq!(document.entries.get(A).unwrap().secret, b"fixture-new-a");
    assert_eq!(document.entries.get(B).unwrap().secret, b"fixture-newer-b");
    backend.fail_write.store(true, Ordering::SeqCst);
    backend.put(A, b"fixture-source-saved");
    assert!(a.synchronize(&backend, A, Some(b"fixture-source-saved"), Instant::now()).is_err());
    assert_eq!(a.read(&backend, A, Instant::now()).unwrap().unwrap(), b"fixture-source-saved");
    backend.fail_write.store(false, Ordering::SeqCst);
    backend.values.lock().unwrap().remove(A);
    a.synchronize(&backend, A, None, Instant::now()).unwrap();
    assert!(!Document::decode(&backend.bytes(VAULT_SERVICE).unwrap()).unwrap().entries.contains_key(A));
    backend.put(B, b"fixture-other-writer");
    assert!(a.synchronize(&backend, B, Some(b"fixture-unverified"), Instant::now()).is_err());
    assert!(a.read(&backend, B, Instant::now()).is_err());
}

fn malformed_and_scope() {
    let backend = Memory::fixture();
    let corrupt = b"{\"version\":99,\"entries\":{},\"private_fixture\":\"not-in-error\"}";
    backend.put(VAULT_SERVICE, corrupt);
    let error = Session::default().unlock(&backend, &Memory::sources(), Instant::now(), false).unwrap_err();
    assert!(error.contains("credential_vault_invalid") && !error.contains("not-in-error"));
    assert_eq!(backend.bytes(VAULT_SERVICE).unwrap(), corrupt);
    assert_eq!(backend.vault_writes.load(Ordering::SeqCst), 0);
    let backend = Memory::fixture();
    backend.put(A, &vec![b'x'; MAX_KEY_BYTES + 1]);
    assert!(Session::default().unlock(&backend, &Memory::sources(), Instant::now(), false).is_err());
    assert!(backend.bytes(VAULT_SERVICE).is_none());
    assert!(!managed("com.scoheart.mux.model-profile.__asset-operation-rollback__.operation.profile"));
    assert!(!managed("com.scoheart.mux.private-file-snapshot.operation.path"));
    assert!(!managed(VAULT_SERVICE));
}

#[test]
fn observation_never_waits_for_unlock_or_loads_a_vault() {
    let backend = Memory::fixture();
    let session = Mutex::new(Session::default());
    let guard = session.lock().unwrap();
    assert!(observe(&session, &backend, A, Instant::now()).unwrap_err().contains("busy"));
    drop(guard);
    assert!(observe(&session, &backend, A, Instant::now()).unwrap_err().contains("locked"));
    assert_eq!(backend.vault_reads.load(Ordering::SeqCst), 0);
    assert_eq!(backend.item_reads.load(Ordering::SeqCst), 0);
    assert_eq!(backend.vault_writes.load(Ordering::SeqCst), 0);

    session.lock().unwrap().unlock(&backend, &Memory::sources(), Instant::now(), false).unwrap();
    let reads = backend.vault_reads.load(Ordering::SeqCst);
    assert_eq!(observe(&session, &backend, A, Instant::now()).unwrap().unwrap(), b"fixture-alpha");
    backend.put(A, b"fixture-rotated");
    assert!(observe(&session, &backend, A, Instant::now()).unwrap_err().contains("refresh_required"));
    assert_eq!(backend.vault_reads.load(Ordering::SeqCst), reads);
    assert_eq!(backend.item_reads.load(Ordering::SeqCst), 2);
}

pub(super) fn run_checks() {
    lifecycle_and_concurrency();
    failures_and_external_changes();
    writes_merge_and_safety();
    malformed_and_scope();
}
#[test]
fn shared_authorization_session_regressions() { run_checks(); }
