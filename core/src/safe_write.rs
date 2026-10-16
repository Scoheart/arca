//! Crash-safe writes for Arca-owned state and reviewed Agent leaves.
//!
//! The guarantees cover process crashes, cooperating Arca processes, ordinary
//! concurrent edits, and parent/symlink replacement detected at an anchored
//! identity boundary. Unknown namespace state is preserved and reported as
//! `recovery_required`. This is not a sandbox against a hostile process with
//! the same OS user privileges: such a process can continuously rename private
//! directories between the final identity check and a kernel `*at` operation,
//! or hide an entire WAL tree under an undiscoverable name. Preventing that
//! requires an OS trust boundary rather than a filesystem protocol.

// Durable .mux-* transaction claim names remain a versioned recovery protocol.
// Arca keeps them so an interrupted old write is never orphaned or misidentified.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);
const SETTINGS_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const SETTINGS_LOCK_POLL: Duration = Duration::from_millis(25);
const TRANSACTION_WRITE_RECORD_VERSION: u32 = 2;
const TRANSACTION_MUTATION_INTENT_VERSION: u32 = 4;

pub(crate) struct SettingsLock {
    lock_path: PathBuf,
    _lock_file: Rc<fs::File>,
    _thread_bound: PhantomData<Rc<()>>,
}

/// A shared filesystem lock used by read-only workspace projections.
///
/// This guard is deliberately separate from [`SettingsLock`]: a projection
/// must coordinate with cooperating writers, but it must not make an empty
/// Arca home observable merely by reading it.
pub(crate) struct SettingsReadLock {
    _ownership: SettingsReadLockOwnership,
    _thread_bound: PhantomData<Rc<()>>,
}

enum SettingsReadLockOwnership {
    Reentrant { _lock: SettingsLock },
    Shared { _file: fs::File },
}

pub(crate) enum TrySettingsReadLock {
    Missing,
    Acquired(SettingsReadLock),
    Contended,
}

struct HeldSettingsLock {
    depth: usize,
    lock_file: Rc<fs::File>,
}

thread_local! {
    /// A full asset commit holds the settings filesystem lock across several
    /// nested settings mutations. Track ownership per thread so those nested
    /// calls are reentrant without letting another thread in this process bypass
    /// the cross-process lock.
    static HELD_SETTINGS_LOCKS: RefCell<BTreeMap<PathBuf, HeldSettingsLock>> =
        const { RefCell::new(BTreeMap::new()) };
    static ACTIVE_TRANSACTION_WRITES: RefCell<Option<Rc<RefCell<ActiveTransactionWrites>>>> =
        const { RefCell::new(None) };
    // Native document edits are independent of central asset recovery. Their
    // durable claims must never become a global MCP/Model/Skill blocker.
    static FILE_EDIT_CLAIMS: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    #[cfg(test)]
    static TEST_GLOBAL_MUTATION_ROOT: PathBuf = {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join(format!(
            "arca-safe-write-journal-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    };
}

#[cfg(test)]
type MutationClaimHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
    static BEFORE_MUTATION_CLAIM_HOOK: RefCell<Option<MutationClaimHook>> =
        const { RefCell::new(None) };
}

#[cfg(test)]
fn set_before_mutation_claim_hook(hook: impl FnOnce(&Path) + 'static) {
    BEFORE_MUTATION_CLAIM_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
fn run_before_mutation_claim_hook(path: &Path) {
    BEFORE_MUTATION_CLAIM_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook(path);
        }
    });
}

#[cfg(not(test))]
fn run_before_mutation_claim_hook(_path: &Path) {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum TransactionPathState {
    Missing,
    PrivateMissing,
    File {
        bytes: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
        identity: PathIdentity,
    },
    PrivateFile {
        content_hash: String,
        mode: Option<u32>,
        identity: PathIdentity,
    },
    Symlink {
        target: PathBuf,
        identity: PathIdentity,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableTransactionWrite {
    version: u32,
    sequence: u64,
    path: PathBuf,
    state: TransactionPathState,
}

#[derive(Debug)]
struct ActiveTransactionWrites {
    directory: PathBuf,
    tracked_paths: BTreeSet<PathBuf>,
    private_paths: BTreeSet<PathBuf>,
    parent_snapshots: BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: BTreeMap<PathBuf, ReviewedTransactionPathState>,
    states: BTreeMap<PathBuf, TransactionPathState>,
    next_sequence: u64,
    next_intent_sequence: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum TransactionPathPrivacy {
    #[default]
    Ordinary,
    Private,
}

fn transaction_path_privacy(
    path: &Path,
    private_paths: &BTreeSet<PathBuf>,
) -> TransactionPathPrivacy {
    if private_paths.contains(path) {
        TransactionPathPrivacy::Private
    } else {
        TransactionPathPrivacy::Ordinary
    }
}

#[derive(Debug, Clone)]
enum ReviewedTransactionPathState {
    Anchored(AnchoredPathState),
    PrivateFile {
        content_hash: String,
        mode: Option<u32>,
        identity: PathIdentity,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum MutationIntentOperation {
    Create,
    Replace,
    Rewrite,
    Remove,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum MutationIntentPhase {
    Prepared,
    Armed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableMutationIntent {
    version: u32,
    sequence: u64,
    record_id: String,
    path: PathBuf,
    #[serde(default)]
    privacy: TransactionPathPrivacy,
    parent: ParentDirectorySnapshot,
    mutation_parent_identity: PathIdentity,
    journal_identity: PathIdentity,
    guard_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    temp_name: Option<String>,
    operation: MutationIntentOperation,
    phase: MutationIntentPhase,
    expected_fingerprint: String,
    desired_semantic_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    desired_fingerprint: Option<String>,
}

struct MutationIntent {
    record_path: PathBuf,
    record: DurableMutationIntent,
    record_identity: PathIdentity,
    _journal_lock: SettingsLock,
}

#[cfg(unix)]
struct PrivateJournalDirectory {
    path: PathBuf,
    directory: fs::File,
    identity: PathIdentity,
}

/// Stable identity for the directory tree containing a transaction target.
///
/// The nearest directory that existed when the operation was reviewed is the
/// anchor. Any missing descendants are opened (and, for forward writes,
/// created) one component at a time below that descriptor. This prevents a
/// renamed parent plus replacement symlink from redirecting a write or a
/// rollback outside the reviewed tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParentDirectorySnapshot {
    parent_path: PathBuf,
    anchor_path: PathBuf,
    relative_parent: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inode: Option<u64>,
    canonical_anchor: PathBuf,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PathIdentity {
    device: Option<u64>,
    inode: Option<u64>,
}

impl PathIdentity {
    pub(crate) const fn unknown() -> Self {
        Self {
            device: None,
            inode: None,
        }
    }

    pub(crate) fn is_exact(self) -> bool {
        self.device.is_some() && self.inode.is_some()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AnchoredPathState {
    Missing,
    File {
        bytes: Zeroizing<Vec<u8>>,
        mode: Option<u32>,
        identity: PathIdentity,
    },
    Symlink {
        target: PathBuf,
        identity: PathIdentity,
    },
    Directory {
        identity: PathIdentity,
    },
    Other {
        identity: PathIdentity,
    },
}

impl std::fmt::Debug for AnchoredPathState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => formatter.write_str("Missing"),
            Self::File { mode, identity, .. } => formatter
                .debug_struct("File")
                .field("bytes", &"<redacted>")
                .field("mode", mode)
                .field("identity", identity)
                .finish(),
            Self::Symlink { target, identity } => formatter
                .debug_struct("Symlink")
                .field("target", target)
                .field("identity", identity)
                .finish(),
            Self::Directory { identity } => formatter
                .debug_struct("Directory")
                .field("identity", identity)
                .finish(),
            Self::Other { identity } => formatter
                .debug_struct("Other")
                .field("identity", identity)
                .finish(),
        }
    }
}

fn private_transaction_content_hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn valid_private_transaction_content_hash(content_hash: &str) -> bool {
    content_hash.len() == 64
        && content_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A private target already has an encrypted rollback snapshot owned by the
/// active asset transaction. Callers must not make a second plaintext backup.
pub(crate) fn has_private_transaction_snapshot(path: &Path) -> bool {
    ACTIVE_TRANSACTION_WRITES.with(|slot| {
        slot.borrow().as_ref().is_some_and(|active| active.borrow().private_paths.contains(path))
    })
}

pub(crate) fn transaction_private_file_state(
    bytes: &[u8],
    mode: Option<u32>,
    identity: PathIdentity,
) -> Result<TransactionPathState, String> {
    transaction_private_file_state_from_hash(
        private_transaction_content_hash(bytes),
        mode,
        identity,
    )
}

fn transaction_private_file_state_from_hash(
    content_hash: String,
    mode: Option<u32>,
    identity: PathIdentity,
) -> Result<TransactionPathState, String> {
    if !valid_private_transaction_content_hash(&content_hash)
        || mode.is_none()
        || !identity.is_exact()
    {
        return Err(
            "asset_target_unsafe: private transaction file state requires an exact hash, mode, and identity"
                .into(),
        );
    }
    Ok(TransactionPathState::PrivateFile {
        content_hash,
        mode,
        identity,
    })
}

fn validate_transaction_state_parity(
    path: &Path,
    state: &TransactionPathState,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    match (transaction_path_privacy(path, private_paths), state) {
        (TransactionPathPrivacy::Private, TransactionPathState::PrivateMissing)
        | (TransactionPathPrivacy::Ordinary, TransactionPathState::Missing)
        | (TransactionPathPrivacy::Ordinary, TransactionPathState::File { .. })
        | (TransactionPathPrivacy::Ordinary, TransactionPathState::Symlink { .. }) => Ok(()),
        (
            TransactionPathPrivacy::Private,
            TransactionPathState::PrivateFile {
                content_hash,
                mode,
                identity,
            },
        ) if valid_private_transaction_content_hash(content_hash)
            && mode.is_some()
            && identity.is_exact() =>
        {
            Ok(())
        }
        _ => Err(format!(
            "transaction private/ordinary parity mismatch for {}",
            path.display()
        )),
    }
}

fn reviewed_state_from_transaction_state(
    path: &Path,
    state: &TransactionPathState,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<ReviewedTransactionPathState, String> {
    if transaction_path_privacy(path, private_paths) == TransactionPathPrivacy::Private
        && matches!(state, TransactionPathState::Missing)
    {
        return Ok(ReviewedTransactionPathState::Anchored(
            AnchoredPathState::Missing,
        ));
    }
    validate_transaction_state_parity(path, state, private_paths)?;
    Ok(match state {
        TransactionPathState::Missing | TransactionPathState::PrivateMissing => {
            ReviewedTransactionPathState::Anchored(AnchoredPathState::Missing)
        }
        TransactionPathState::File {
            bytes,
            mode,
            identity,
        } => ReviewedTransactionPathState::Anchored(AnchoredPathState::File {
            bytes: Zeroizing::new(bytes.clone()),
            mode: *mode,
            identity: *identity,
        }),
        TransactionPathState::PrivateFile {
            content_hash,
            mode,
            identity,
        } => ReviewedTransactionPathState::PrivateFile {
            content_hash: content_hash.clone(),
            mode: *mode,
            identity: *identity,
        },
        TransactionPathState::Symlink { target, identity } => {
            ReviewedTransactionPathState::Anchored(AnchoredPathState::Symlink {
                target: target.clone(),
                identity: *identity,
            })
        }
    })
}

fn validate_reviewed_transaction_state_parity(
    path: &Path,
    state: &ReviewedTransactionPathState,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    match (transaction_path_privacy(path, private_paths), state) {
        (TransactionPathPrivacy::Ordinary, ReviewedTransactionPathState::Anchored(_))
        | (
            TransactionPathPrivacy::Private,
            ReviewedTransactionPathState::Anchored(AnchoredPathState::Missing),
        ) => Ok(()),
        (
            TransactionPathPrivacy::Private,
            ReviewedTransactionPathState::PrivateFile {
                content_hash,
                mode,
                identity,
            },
        ) if valid_private_transaction_content_hash(content_hash)
            && mode.is_some()
            && identity.is_exact() =>
        {
            Ok(())
        }
        _ => Err(format!(
            "transaction private/ordinary parity mismatch for {}",
            path.display()
        )),
    }
}

fn private_transaction_state_matches_anchored(
    content_hash: &str,
    mode: Option<u32>,
    identity: PathIdentity,
    actual: &AnchoredPathState,
) -> bool {
    if !valid_private_transaction_content_hash(content_hash)
        || mode.is_none()
        || !identity.is_exact()
    {
        return false;
    }
    let AnchoredPathState::File {
        bytes,
        mode: actual_mode,
        identity: actual_identity,
    } = actual
    else {
        return false;
    };
    actual_identity.is_exact()
        && *actual_identity == identity
        && *actual_mode == mode
        && private_transaction_content_hash(bytes) == content_hash
}

/// Thread-bound ownership evidence for a central asset transaction.
///
/// Every successful safe write records the exact post-state before control
/// returns to the transaction coordinator. The record is also synced under the
/// durable rollback directory. A crash between publishing a target and syncing
/// its record therefore fails closed during recovery instead of guessing that
/// the current bytes belong to Arca.
pub(crate) struct TransactionWriteTracker {
    active: Rc<RefCell<ActiveTransactionWrites>>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl TransactionWriteTracker {
    pub(crate) fn states(&self) -> BTreeMap<PathBuf, TransactionPathState> {
        self.active.borrow().states.clone()
    }
}

impl Drop for TransactionWriteTracker {
    fn drop(&mut self) {
        ACTIVE_TRANSACTION_WRITES.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|active| Rc::ptr_eq(active, &self.active))
            {
                slot.take();
            }
        });
    }
}

impl Drop for SettingsLock {
    fn drop(&mut self) {
        let released = HELD_SETTINGS_LOCKS.with(|held| {
            let mut held = held.borrow_mut();
            let entry = held.get_mut(&self.lock_path)?;
            entry.depth -= 1;
            if entry.depth == 0 {
                held.remove(&self.lock_path)
            } else {
                None
            }
        });
        if let Some(released) = released {
            let _ = FileExt::unlock(released.lock_file.as_ref());
        }
    }
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

/// Coordinate cooperating writers with an OS-backed advisory lock next to the
/// shared settings file. The lock file is intentionally persistent: the kernel
/// releases ownership if a process crashes, so no stale directory can brick all
/// future commits.
pub(crate) fn acquire_settings_lock(path: &Path) -> Result<SettingsLock, String> {
    let lock_path = append_suffix(path, ".lockfile");
    if let Some(reentered) = reenter_settings_lock(&lock_path) {
        return Ok(reentered);
    }

    #[cfg(not(unix))]
    {
        let parent = lock_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let lock_file = open_settings_lock_file(&lock_path)?;
    let started = Instant::now();
    loop {
        match lock_file.try_lock_exclusive() {
            Ok(()) => {
                let lock_file = Rc::new(lock_file);
                HELD_SETTINGS_LOCKS.with(|held| {
                    held.borrow_mut().insert(
                        lock_path.clone(),
                        HeldSettingsLock {
                            depth: 1,
                            lock_file: lock_file.clone(),
                        },
                    );
                });
                return Ok(SettingsLock {
                    _lock_file: lock_file,
                    lock_path,
                    _thread_bound: PhantomData,
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if started.elapsed() >= SETTINGS_LOCK_TIMEOUT {
                    return Err(format!(
                        "refusing to modify {}: timed out waiting for the settings lock",
                        path.display()
                    ));
                }
                thread::sleep(SETTINGS_LOCK_POLL);
            }
            Err(error) => {
                return Err(format!(
                    "failed to acquire settings lock for {}: {}",
                    path.display(),
                    error
                ));
            }
        }
    }
}

fn reenter_settings_lock(lock_path: &Path) -> Option<SettingsLock> {
    let reentered = HELD_SETTINGS_LOCKS.with(|held| {
        let mut held = held.borrow_mut();
        let entry = held.get_mut(lock_path)?;
        entry.depth += 1;
        Some(entry.lock_file.clone())
    });
    reentered.map(|lock_file| SettingsLock {
        lock_path: lock_path.to_path_buf(),
        _lock_file: lock_file,
        _thread_bound: PhantomData,
    })
}

/// Acquire a shared settings lock only when Arca storage has already been
/// initialized. An existing settings document is enough to initialize the
/// persistent lock file; when neither file exists this returns `None` without
/// creating the parent directory.
///
/// Callers that receive `None` must recheck after completing their read. A
/// writer creates the lock file before publishing settings, so appearance of
/// either path means the read should be retried under the shared lock.
#[cfg(test)]
pub(crate) fn acquire_settings_read_lock_if_initialized(
    path: &Path,
) -> Result<Option<SettingsReadLock>, String> {
    let started = Instant::now();
    loop {
        match try_acquire_settings_read_lock_if_initialized(path)? {
            TrySettingsReadLock::Missing => return Ok(None),
            TrySettingsReadLock::Acquired(lock) => return Ok(Some(lock)),
            TrySettingsReadLock::Contended if started.elapsed() < SETTINGS_LOCK_TIMEOUT => {
                thread::sleep(
                    SETTINGS_LOCK_POLL.min(SETTINGS_LOCK_TIMEOUT.saturating_sub(started.elapsed())),
                );
            }
            TrySettingsReadLock::Contended => {
                return Err(format!(
                    "refusing to read {}: timed out waiting for the settings lock",
                    path.display()
                ));
            }
        }
    }
}

/// Attempt the shared settings lock once without waiting. Workspace snapshots
/// use this together with the Skills try-lock so they never wait for one domain
/// while retaining the other domain's shared lock.
pub(crate) fn try_acquire_settings_read_lock_if_initialized(
    path: &Path,
) -> Result<TrySettingsReadLock, String> {
    let lock_path = append_suffix(path, ".lockfile");
    if let Some(lock) = reenter_settings_lock(&lock_path) {
        return Ok(TrySettingsReadLock::Acquired(SettingsReadLock {
            _ownership: SettingsReadLockOwnership::Reentrant { _lock: lock },
            _thread_bound: PhantomData,
        }));
    }

    if !settings_lock_is_initialized(path)? {
        return Ok(TrySettingsReadLock::Missing);
    }

    // `open_settings_lock_file` may create only the lock file. Reaching this
    // point proves either it or settings.json already existed, so a read of an
    // empty Arca home remains side-effect free.
    let lock_file = open_settings_lock_file(&lock_path)?;
    match FileExt::try_lock_shared(&lock_file) {
        Ok(()) => Ok(TrySettingsReadLock::Acquired(SettingsReadLock {
            _ownership: SettingsReadLockOwnership::Shared { _file: lock_file },
            _thread_bound: PhantomData,
        })),
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(TrySettingsReadLock::Contended),
        Err(error) => Err(format!(
            "failed to acquire shared settings lock for {}: {}",
            path.display(),
            error
        )),
    }
}

pub(crate) fn settings_lock_is_initialized(path: &Path) -> Result<bool, String> {
    let lock_path = append_suffix(path, ".lockfile");
    let lock_exists = match fs::symlink_metadata(&lock_path) {
        Ok(_) => true,
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "failed to inspect settings lock {}: {error}",
                lock_path.display()
            ));
        }
    };
    if lock_exists {
        return Ok(true);
    }
    let settings_exists = match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "failed to inspect settings path {}: {error}",
                path.display()
            ));
        }
    };
    Ok(settings_exists)
}

#[cfg(unix)]
fn open_settings_lock_file(path: &Path) -> Result<fs::File, String> {
    use rustix::fs::{openat, Mode, OFlags};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_directory = open_directory_tree_once(parent, true)?
        .ok_or_else(|| "settings lock parent was not created".to_string())?;
    let parent_metadata = parent_directory.metadata().map_err(|error| {
        format!(
            "failed to inspect settings lock parent {}: {error}",
            parent.display()
        )
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| format!("settings lock path has no file name: {}", path.display()))?;
    let file = openat(
        &parent_directory,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from(0o600),
    )
    .map(fs::File::from)
    .map_err(|error| format!("failed to open settings lock {}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| {
        format!(
            "failed to inspect settings lock {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(format!(
            "refusing unsafe settings lock file: {}",
            path.display()
        ));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("failed to secure settings lock {}: {error}", path.display()))?;
    let current_parent = open_directory_tree_once(parent, false)?.ok_or_else(|| {
        format!(
            "settings lock parent disappeared while opening: {}",
            parent.display()
        )
    })?;
    let current_metadata = current_parent.metadata().map_err(|error| {
        format!(
            "failed to recheck settings lock parent {}: {error}",
            parent.display()
        )
    })?;
    if parent_metadata.dev() != current_metadata.dev()
        || parent_metadata.ino() != current_metadata.ino()
    {
        return Err(format!(
            "refusing a settings lock whose parent changed: {}",
            path.display()
        ));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_settings_lock_file(path: &Path) -> Result<fs::File, String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(format!(
                "refusing unsafe settings lock file: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .map_err(|error| format!("failed to open settings lock {}: {error}", path.display()))
}

#[cfg(test)]
pub(crate) fn begin_transaction_write_tracking(
    directory: &Path,
    tracked_paths: &[PathBuf],
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
) -> Result<TransactionWriteTracker, String> {
    let reviewed_states = tracked_paths
        .iter()
        .map(|path| {
            let parent = parent_snapshots.get(path).ok_or_else(|| {
                "transaction parent snapshots do not cover every tracked target".to_string()
            })?;
            Ok((path.clone(), read_path_state_anchored(path, parent)?))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    begin_transaction_write_tracking_with_states(
        directory,
        tracked_paths,
        parent_snapshots,
        &reviewed_states,
    )
}

#[cfg(test)]
pub(crate) fn begin_transaction_write_tracking_with_states(
    directory: &Path,
    tracked_paths: &[PathBuf],
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: &BTreeMap<PathBuf, AnchoredPathState>,
) -> Result<TransactionWriteTracker, String> {
    let reviewed_states = reviewed_states
        .iter()
        .map(|(path, state)| {
            (
                path.clone(),
                ReviewedTransactionPathState::Anchored(state.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    begin_transaction_write_tracking_with_reviewed_states(
        directory,
        tracked_paths,
        &BTreeSet::new(),
        parent_snapshots,
        &reviewed_states,
    )
}

pub(crate) fn begin_transaction_write_tracking_with_private_states(
    directory: &Path,
    tracked_paths: &[PathBuf],
    private_paths: &BTreeSet<PathBuf>,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Result<TransactionWriteTracker, String> {
    let reviewed_states = reviewed_states
        .iter()
        .map(|(path, state)| {
            reviewed_state_from_transaction_state(path, state, private_paths)
                .map(|state| (path.clone(), state))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    begin_transaction_write_tracking_with_reviewed_states(
        directory,
        tracked_paths,
        private_paths,
        parent_snapshots,
        &reviewed_states,
    )
}

fn begin_transaction_write_tracking_with_reviewed_states(
    directory: &Path,
    tracked_paths: &[PathBuf],
    private_paths: &BTreeSet<PathBuf>,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: &BTreeMap<PathBuf, ReviewedTransactionPathState>,
) -> Result<TransactionWriteTracker, String> {
    let already_active = ACTIVE_TRANSACTION_WRITES.with(|slot| slot.borrow().is_some());
    if already_active {
        return Err("a transaction write tracker is already active on this thread".into());
    }
    let tracked_paths = tracked_paths.iter().cloned().collect::<BTreeSet<_>>();
    validate_transaction_tracker_inputs(
        &tracked_paths,
        private_paths,
        parent_snapshots,
        reviewed_states,
    )?;
    match fs::create_dir(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            return Err(format!(
                "transaction write evidence directory already exists: {}",
                directory.display()
            ));
        }
        Err(error) => {
            return Err(format!(
                "failed to create transaction write evidence directory {}: {error}",
                directory.display()
            ));
        }
    }
    set_private_directory(directory)?;
    sync_parent(directory)?;
    let active = Rc::new(RefCell::new(ActiveTransactionWrites {
        directory: directory.to_path_buf(),
        tracked_paths,
        private_paths: private_paths.clone(),
        parent_snapshots: parent_snapshots.clone(),
        reviewed_states: reviewed_states.clone(),
        states: BTreeMap::new(),
        next_sequence: 0,
        next_intent_sequence: 0,
    }));
    ACTIVE_TRANSACTION_WRITES.with(|slot| {
        *slot.borrow_mut() = Some(active.clone());
    });
    Ok(TransactionWriteTracker {
        active,
        _thread_bound: PhantomData,
    })
}

#[cfg(test)]
pub(crate) fn resume_transaction_write_tracking(
    directory: &Path,
    tracked_paths: &[PathBuf],
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: &BTreeMap<PathBuf, AnchoredPathState>,
    states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Result<TransactionWriteTracker, String> {
    let reviewed_states = reviewed_states
        .iter()
        .map(|(path, state)| {
            (
                path.clone(),
                ReviewedTransactionPathState::Anchored(state.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    resume_transaction_write_tracking_with_reviewed_states(
        directory,
        tracked_paths,
        &BTreeSet::new(),
        parent_snapshots,
        &reviewed_states,
        states,
    )
}

pub(crate) fn resume_transaction_write_tracking_with_private_states(
    directory: &Path,
    tracked_paths: &[PathBuf],
    private_paths: &BTreeSet<PathBuf>,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: &BTreeMap<PathBuf, TransactionPathState>,
    states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Result<TransactionWriteTracker, String> {
    let reviewed_states = reviewed_states
        .iter()
        .map(|(path, state)| {
            reviewed_state_from_transaction_state(path, state, private_paths)
                .map(|state| (path.clone(), state))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    resume_transaction_write_tracking_with_reviewed_states(
        directory,
        tracked_paths,
        private_paths,
        parent_snapshots,
        &reviewed_states,
        states,
    )
}

fn resume_transaction_write_tracking_with_reviewed_states(
    directory: &Path,
    tracked_paths: &[PathBuf],
    private_paths: &BTreeSet<PathBuf>,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: &BTreeMap<PathBuf, ReviewedTransactionPathState>,
    states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Result<TransactionWriteTracker, String> {
    let already_active = ACTIVE_TRANSACTION_WRITES.with(|slot| slot.borrow().is_some());
    if already_active {
        return Err("a transaction write tracker is already active on this thread".into());
    }
    let tracked_paths = tracked_paths.iter().cloned().collect::<BTreeSet<_>>();
    validate_transaction_tracker_inputs(
        &tracked_paths,
        private_paths,
        parent_snapshots,
        reviewed_states,
    )
    .map_err(|error| format!("recovery_required: {error}"))?;
    if states.keys().any(|path| !tracked_paths.contains(path)) {
        return Err(
            "recovery_required: transaction recovery state does not cover the reviewed targets"
                .to_string(),
        );
    }
    for (path, state) in states {
        validate_transaction_state_parity(path, state, private_paths)
            .map_err(|error| format!("recovery_required: {error}"))?;
        #[cfg(unix)]
        if !transaction_path_state_has_exact_identity(state) {
            return Err(format!(
                "recovery_required: transaction recovery state has no exact identity: {}",
                path.display()
            ));
        }
    }
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::create_dir(directory).map_err(|error| {
                format!(
                    "recovery_required: failed to create transaction write evidence {}: {error}",
                    directory.display()
                )
            })?;
            set_private_directory(directory)?;
            sync_parent(directory)?;
            fs::symlink_metadata(directory).map_err(|error| {
                format!(
                    "recovery_required: failed to inspect transaction write evidence {}: {error}",
                    directory.display()
                )
            })?
        }
        Err(error) => {
            return Err(format!(
                "recovery_required: failed to inspect transaction write evidence {}: {error}",
                directory.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "recovery_required: transaction write evidence is not a directory: {}",
            directory.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(format!(
                "recovery_required: transaction write evidence is not private: {}",
                directory.display()
            ));
        }
    }
    let mut next_sequence = 0;
    for entry in fs::read_dir(directory).map_err(|error| format!("recovery_required: {error}"))? {
        let path = entry
            .map_err(|error| format!("recovery_required: {error}"))?
            .path();
        if !is_journal_pending_path(&path) {
            next_sequence += 1;
        }
    }
    let active = Rc::new(RefCell::new(ActiveTransactionWrites {
        directory: directory.to_path_buf(),
        tracked_paths,
        private_paths: private_paths.clone(),
        parent_snapshots: parent_snapshots.clone(),
        reviewed_states: reviewed_states.clone(),
        states: states.clone(),
        next_sequence,
        next_intent_sequence: 0,
    }));
    ACTIVE_TRANSACTION_WRITES.with(|slot| {
        *slot.borrow_mut() = Some(active.clone());
    });
    Ok(TransactionWriteTracker {
        active,
        _thread_bound: PhantomData,
    })
}

fn validate_transaction_tracker_inputs(
    tracked_paths: &BTreeSet<PathBuf>,
    private_paths: &BTreeSet<PathBuf>,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    reviewed_states: &BTreeMap<PathBuf, ReviewedTransactionPathState>,
) -> Result<(), String> {
    if tracked_paths.len() != parent_snapshots.len()
        || tracked_paths.len() != reviewed_states.len()
        || !private_paths.is_subset(tracked_paths)
        || tracked_paths
            .iter()
            .any(|path| !parent_snapshots.contains_key(path) || !reviewed_states.contains_key(path))
    {
        return Err(
            "transaction parent snapshots, private paths, and reviewed states do not cover every tracked target"
                .to_string(),
        );
    }
    for (path, state) in reviewed_states {
        validate_reviewed_transaction_state_parity(path, state, private_paths)?;
    }
    Ok(())
}

pub(crate) fn ensure_no_transaction_mutation_intents(directory: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let Some(journal) = private_journal_directory(directory, false)? else {
            return Ok(());
        };
        for name in journal.names()? {
            // Pending record publications are not authoritative, but they must
            // still be private regular files inside the verified journal.
            if is_journal_pending_path(Path::new(&name)) {
                journal.read_private(&name)?;
                continue;
            }
            return Err(format!(
                "recovery_required: a durable mutation intent is still active: {}",
                directory.join(name).display()
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("recovery_required: {error}")),
        };
        for entry in entries {
            let path = entry
                .map_err(|error| format!("recovery_required: {error}"))?
                .path();
            if !is_journal_pending_path(&path) {
                return Err(format!(
                    "recovery_required: a durable mutation intent is still active: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn load_transaction_write_states(
    directory: &Path,
) -> Result<BTreeMap<PathBuf, TransactionPathState>, String> {
    load_transaction_write_states_with_private_paths(directory, &BTreeSet::new())
}

pub(crate) fn load_transaction_write_states_with_private_paths(
    directory: &Path,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, TransactionPathState>, String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(format!(
                "recovery_required: failed to read transaction write evidence {}: {error}",
                directory.display()
            ));
        }
    };
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| format!("recovery_required: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for path in paths.iter().filter(|path| is_journal_pending_path(path)) {
        let metadata =
            fs::symlink_metadata(path).map_err(|error| format!("recovery_required: {error}"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "recovery_required: invalid pending transaction write evidence {}",
                path.display()
            ));
        }
    }
    paths.retain(|path| !is_journal_pending_path(path));
    paths.sort();
    let mut states = BTreeMap::new();
    for (expected_sequence, path) in paths.into_iter().enumerate() {
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| format!("recovery_required: {error}"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "recovery_required: invalid transaction write evidence {}",
                path.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(format!(
                    "recovery_required: transaction write evidence is not private: {}",
                    path.display()
                ));
            }
        }
        let bytes = fs::read(&path).map_err(|error| format!("recovery_required: {error}"))?;
        let record: DurableTransactionWrite = serde_json::from_slice(&bytes).map_err(|_| {
            format!(
                "recovery_required: malformed transaction write evidence {}",
                path.display()
            )
        })?;
        if record.version != TRANSACTION_WRITE_RECORD_VERSION
            || record.sequence != expected_sequence as u64
            || path.file_name().and_then(|name| name.to_str())
                != Some(format!("{expected_sequence:020}.json").as_str())
        {
            return Err(format!(
                "recovery_required: inconsistent transaction write evidence {}",
                path.display()
            ));
        }
        validate_transaction_state_parity(&record.path, &record.state, private_paths)
            .map_err(|error| format!("recovery_required: {error}"))?;
        #[cfg(unix)]
        if !transaction_path_state_has_exact_identity(&record.state) {
            return Err(format!(
                "recovery_required: transaction write evidence has no exact identity: {}",
                path.display()
            ));
        }
        states.insert(record.path, record.state);
    }
    Ok(states)
}

fn transaction_path_state_has_exact_identity(state: &TransactionPathState) -> bool {
    match state {
        TransactionPathState::Missing | TransactionPathState::PrivateMissing => true,
        TransactionPathState::File { identity, .. }
        | TransactionPathState::PrivateFile { identity, .. }
        | TransactionPathState::Symlink { identity, .. } => identity.is_exact(),
    }
}

/// Reconcile durable leaf-claim intents before the outer asset transaction
/// decides whether to verify or roll back. Unknown entries are never replaced
/// or deleted; ambiguous namespace states remain recovery-required.
pub(crate) fn recover_transaction_mutation_intents(
    directory: &Path,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
) -> Result<(), String> {
    recover_transaction_mutation_intents_with_private_paths(
        directory,
        parent_snapshots,
        &BTreeSet::new(),
    )
}

pub(crate) fn recover_transaction_mutation_intents_with_private_paths(
    directory: &Path,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    if !private_paths
        .iter()
        .all(|path| parent_snapshots.contains_key(path))
    {
        return Err(
            "recovery_required: private mutation paths are not covered by reviewed parents".into(),
        );
    }
    let _journal_lock = acquire_settings_lock(&global_mutation_lock_subject())?;
    let transaction_evidence_directory = if parent_snapshots.is_empty() {
        None
    } else {
        Some(
            directory
                .parent()
                .ok_or_else(|| {
                    "recovery_required: mutation intent directory has no rollback parent"
                        .to_string()
                })?
                .join("post"),
        )
    };
    let mut raw_records = Vec::new();
    #[cfg(unix)]
    {
        let Some(journal) = private_journal_directory(directory, false)? else {
            return Ok(());
        };
        for name in journal.names()? {
            let path = directory.join(&name);
            let (bytes, record_identity) = journal.read_private(&name)?;
            if is_journal_pending_path(Path::new(&name)) {
                continue;
            }
            raw_records.push((path, bytes, record_identity, journal.identity));
        }
    }
    #[cfg(not(unix))]
    {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("recovery_required: {error}")),
        };
        for entry in entries {
            let entry = entry.map_err(|error| format!("recovery_required: {error}"))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("recovery_required: {error}"))?;
            if is_journal_pending_path(&path) {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err("recovery_required: invalid pending mutation intent entry".into());
                }
                continue;
            }
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("recovery_required: invalid mutation intent entry".into());
            }
            let bytes = fs::read(&path).map_err(|error| format!("recovery_required: {error}"))?;
            raw_records.push((
                path,
                bytes,
                PathIdentity::unknown(),
                PathIdentity::unknown(),
            ));
        }
    }
    let mut records = Vec::new();
    for (path, bytes, record_identity, observed_journal_identity) in raw_records {
        let record: DurableMutationIntent = serde_json::from_slice(&bytes)
            .map_err(|_| "recovery_required: malformed mutation intent".to_string())?;
        let reviewed_privacy = transaction_path_privacy(&record.path, private_paths);
        if record.privacy != reviewed_privacy {
            return Err(format!(
                "recovery_required: mutation intent privacy does not match reviewed policy for {}",
                record.path.display()
            ));
        }
        let expected_name = format!("{:020}-{}.json", record.sequence, record.record_id);
        #[cfg(unix)]
        let journal_identity_matches = record.journal_identity.is_exact()
            && record.journal_identity == observed_journal_identity
            && record_identity.is_exact();
        #[cfg(not(unix))]
        let journal_identity_matches = true;
        if record.version != TRANSACTION_MUTATION_INTENT_VERSION
            || path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
            || uuid::Uuid::parse_str(&record.record_id).is_err()
            || !record.mutation_parent_identity.is_exact()
            || !journal_identity_matches
            || !valid_mutation_entry_name(&record.guard_name)
            || record
                .temp_name
                .as_deref()
                .is_some_and(|name| !valid_mutation_entry_name(name))
            || !match record.operation {
                MutationIntentOperation::Create | MutationIntentOperation::Replace => {
                    record.temp_name.is_some()
                }
                MutationIntentOperation::Rewrite | MutationIntentOperation::Remove => {
                    record.temp_name.is_none()
                }
            }
            || !match record.phase {
                MutationIntentPhase::Prepared => {
                    record.operation != MutationIntentOperation::Remove
                        && record.desired_fingerprint.is_none()
                }
                MutationIntentPhase::Armed => record.desired_fingerprint.is_some(),
            }
        {
            return Err("recovery_required: inconsistent mutation intent".into());
        }
        records.push((record.sequence, path, record_identity, record));
    }
    records.sort_by_key(|(sequence, _, _, _)| *sequence);
    for (_, record_path, record_identity, record) in records.into_iter().rev() {
        recover_one_mutation_intent(
            &record,
            &record_path,
            record_identity,
            parent_snapshots,
            transaction_evidence_directory.as_deref(),
            private_paths,
        )?;
    }
    Ok(())
}

fn recover_one_mutation_intent(
    record: &DurableMutationIntent,
    record_path: &Path,
    record_identity: PathIdentity,
    parent_snapshots: &BTreeMap<PathBuf, ParentDirectorySnapshot>,
    transaction_evidence_directory: Option<&Path>,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        use rustix::fs::{unlinkat, AtFlags};

        if parent_snapshots.is_empty() {
            if target_parent(&record.path) != record.parent.parent_path {
                return Err(
                    "recovery_required: global mutation intent parent is inconsistent".into(),
                );
            }
        } else {
            let reviewed = parent_snapshots.get(&record.path).ok_or_else(|| {
                "recovery_required: mutation intent has no reviewed rollback target".to_string()
            })?;
            if reviewed != &record.parent
                || target_parent(&record.path) != record.parent.parent_path
            {
                return Err(
                    "recovery_required: mutation intent parent does not match rollback".into(),
                );
            }
        }
        let parent_snapshot = &record.parent;
        let parent = open_parent_anchored(parent_snapshot, false)?
            .ok_or_else(|| "recovery_required: mutation parent disappeared".to_string())?;
        let observed_parent_identity = open_parent_identity(&parent)?;
        if !record.mutation_parent_identity.is_exact()
            || observed_parent_identity != record.mutation_parent_identity
        {
            return Err(
                "recovery_required: mutation parent identity changed; all entries were preserved"
                    .into(),
            );
        }
        let name = target_name(&record.path)?;
        let guard_name = std::ffi::OsStr::new(&record.guard_name);
        let guard_path = sibling_path(parent_snapshot, guard_name);
        let mut target = read_path_state_from_parent(&record.path, &parent)?;
        let guard = read_path_state_from_parent(&guard_path, &parent)?;
        let target_fingerprint = fingerprint_anchored_path_state(&target, parent_snapshot)?;
        let guard_fingerprint = fingerprint_anchored_path_state(&guard, parent_snapshot)?;
        let target_is_expected = target_fingerprint == record.expected_fingerprint;
        let target_is_desired = record
            .desired_fingerprint
            .as_deref()
            .is_some_and(|desired| target_fingerprint == desired);
        let target_is_semantically_desired = semantic_path_state_fingerprint(&target)?
            == record.desired_semantic_fingerprint
            || legacy_semantic_path_state_fingerprint(&target)?
                == record.desired_semantic_fingerprint;
        let guard_is_missing = matches!(guard, AnchoredPathState::Missing);
        let guard_is_expected = guard_fingerprint == record.expected_fingerprint;
        let missing_fingerprint =
            fingerprint_anchored_path_state(&AnchoredPathState::Missing, parent_snapshot)?;

        if record.phase == MutationIntentPhase::Prepared {
            if guard_is_missing && target_is_expected {
                let temp = mutation_temp_state(record, parent_snapshot, &parent)?;
                if matches!(temp, AnchoredPathState::Missing) {
                    retire_mutation_intent_record(record_path, record, record_identity)?;
                    return Ok(());
                }
                return Err(
                    "recovery_required: a prepared mutation temp has no exact ownership evidence; it was preserved"
                        .into(),
                );
            }
            return Err(
                "recovery_required: prepared mutation no longer matches its reviewed namespace; all unknown entries were preserved"
                    .into(),
            );
        }

        let desired_fingerprint = record
            .desired_fingerprint
            .as_deref()
            .expect("armed mutation intents have an exact desired fingerprint");
        match record.operation {
            MutationIntentOperation::Create
                if record.expected_fingerprint != missing_fingerprint
                    || desired_fingerprint == missing_fingerprint =>
            {
                return Err("recovery_required: invalid create mutation intent".into());
            }
            MutationIntentOperation::Remove if desired_fingerprint != missing_fingerprint => {
                return Err("recovery_required: invalid remove mutation intent".into());
            }
            MutationIntentOperation::Replace
                if record.expected_fingerprint == missing_fingerprint
                    || desired_fingerprint == missing_fingerprint =>
            {
                return Err("recovery_required: invalid replace mutation intent".into());
            }
            MutationIntentOperation::Rewrite
                if record.expected_fingerprint == missing_fingerprint
                    || desired_fingerprint == missing_fingerprint =>
            {
                return Err("recovery_required: invalid rewrite mutation intent".into());
            }
            _ => {}
        }

        if record.operation == MutationIntentOperation::Rewrite {
            if !guard_is_missing {
                return Err(
                    "recovery_required: in-place rewrite has an unexpected claim guard".into(),
                );
            }
            if target_is_expected {
                retire_mutation_intent_record(record_path, record, record_identity)?;
                return Ok(());
            }
            if target_is_desired || target_is_semantically_desired {
                persist_current_transaction_state(
                    record,
                    &parent,
                    transaction_evidence_directory,
                    private_paths,
                )?;
                retire_mutation_intent_record(record_path, record, record_identity)?;
                return Ok(());
            }
            return Err(
                "recovery_required: in-place rewrite target is neither the reviewed nor desired semantic state"
                    .into(),
            );
        }

        if record.operation == MutationIntentOperation::Create {
            if !guard_is_missing {
                return Err(
                    "recovery_required: create mutation intent has an unexpected claim guard"
                        .into(),
                );
            }
            if target_is_expected || target_is_desired || target_is_semantically_desired {
                cleanup_intent_temp_if_owned(record, parent_snapshot, &parent)?;
                retire_mutation_intent_record(record_path, record, record_identity)?;
                return Ok(());
            }
            return Err(
                "recovery_required: create mutation target changed; the unknown entry was preserved"
                    .into(),
            );
        }

        if guard_is_missing && target_is_expected {
            persist_reconciled_transaction_state(
                record,
                parent_snapshot,
                &parent,
                transaction_evidence_directory,
                private_paths,
            )?;
            cleanup_intent_temp_if_owned(record, parent_snapshot, &parent)?;
            retire_mutation_intent_record(record_path, record, record_identity)?;
            return Ok(());
        }
        if guard_is_missing && (target_is_desired || target_is_semantically_desired) {
            // The verified guard was already removed after publication. The
            // outer transaction write evidence and rollback snapshot own the
            // remaining desired leaf.
            cleanup_intent_temp_if_owned(record, parent_snapshot, &parent)?;
            retire_mutation_intent_record(record_path, record, record_identity)?;
            return Ok(());
        }
        if guard_is_expected && matches!(target, AnchoredPathState::Missing) {
            rename_entry_noreplace(&parent, guard_name, name)
                .map_err(|error| format!("recovery_required: {error}"))?;
            parent
                .directory
                .sync_all()
                .map_err(|error| error.to_string())?;
            persist_reconciled_transaction_state(
                record,
                parent_snapshot,
                &parent,
                transaction_evidence_directory,
                private_paths,
            )?;
            cleanup_intent_temp_if_owned(record, parent_snapshot, &parent)?;
            retire_mutation_intent_record(record_path, record, record_identity)?;
            return Ok(());
        }
        if guard_is_expected && (target_is_desired || target_is_semantically_desired) {
            // The live target already has the reviewed desired meaning. This
            // commonly happens when a watching Agent recreates the file while
            // an older Arca release is publishing it. Keep the live target and
            // retire only Arca-owned claim evidence; restoring the guard would
            // incorrectly undo a successfully converged configuration.
            verify_mutation_parent_is_current(record)?;
            unlinkat(&parent.directory, guard_name, AtFlags::empty())
                .map_err(|error| format!("recovery_required: {error}"))?;
            parent
                .directory
                .sync_all()
                .map_err(|error| error.to_string())?;
            persist_current_transaction_state(
                record,
                &parent,
                transaction_evidence_directory,
                private_paths,
            )?;
            cleanup_intent_temp_if_owned(record, parent_snapshot, &parent)?;
            retire_mutation_intent_record(record_path, record, record_identity)?;
            return Ok(());
        }
        if !guard_is_expected && matches!(target, AnchoredPathState::Missing) {
            // The claim captured an unreviewed replacement. Put that exact
            // entry back without interpreting or deleting it, then let the
            // outer rollback fail closed against its reviewed snapshot.
            rename_entry_noreplace(&parent, guard_name, name)
                .map_err(|error| format!("recovery_required: {error}"))?;
            parent
                .directory
                .sync_all()
                .map_err(|error| error.to_string())?;
            cleanup_intent_temp_if_owned(record, parent_snapshot, &parent)?;
            retire_mutation_intent_record(record_path, record, record_identity)?;
            return Ok(());
        }

        // Refresh once before reporting so an error never claims that a state
        // observed before the final anchored operation is still current.
        target = read_path_state_from_parent(&record.path, &parent)?;
        let _ = target;
        Err("recovery_required: mutation claim coexists with an unknown target; both were preserved"
            .into())
    }

    #[cfg(not(unix))]
    {
        let _ = (record, record_path, parent_snapshots);
        Err("recovery_required: durable mutation claims require anchored Unix operations".into())
    }
}

#[cfg(unix)]
fn mutation_temp_state(
    record: &DurableMutationIntent,
    parent_snapshot: &ParentDirectorySnapshot,
    parent: &OpenParent,
) -> Result<AnchoredPathState, String> {
    let Some(temp_name) = record.temp_name.as_deref() else {
        return Ok(AnchoredPathState::Missing);
    };
    read_path_state_from_parent(
        &sibling_path(parent_snapshot, std::ffi::OsStr::new(temp_name)),
        parent,
    )
}

#[cfg(unix)]
fn persist_reconciled_transaction_state(
    record: &DurableMutationIntent,
    parent_snapshot: &ParentDirectorySnapshot,
    parent: &OpenParent,
    transaction_evidence_directory: Option<&Path>,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let Some(directory) = transaction_evidence_directory else {
        return Ok(());
    };
    let state = read_path_state_from_parent(&record.path, parent)?;
    if fingerprint_anchored_path_state(&state, parent_snapshot)? != record.expected_fingerprint {
        return Err(
            "recovery_required: restored mutation target no longer matches its expected state"
                .into(),
        );
    }
    let state =
        transaction_state_from_anchored_with_private_paths(&record.path, state, private_paths)?;
    let active = ACTIVE_TRANSACTION_WRITES.with(|slot| slot.borrow().clone());
    if let Some(active) = active {
        let active_state = active.borrow();
        if active_state.directory != directory || active_state.private_paths != *private_paths {
            return Err(
                "recovery_required: active transaction evidence or private paths do not match the mutation intent"
                    .into(),
            );
        }
        drop(active_state);
        return record_transaction_path_state(&record.path, state);
    }
    append_transaction_write_state(directory, &record.path, state, private_paths)
}

#[cfg(unix)]
fn persist_current_transaction_state(
    record: &DurableMutationIntent,
    parent: &OpenParent,
    transaction_evidence_directory: Option<&Path>,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let Some(directory) = transaction_evidence_directory else {
        return Ok(());
    };
    let state = transaction_state_from_anchored_with_private_paths(
        &record.path,
        read_path_state_from_parent(&record.path, parent)?,
        private_paths,
    )?;
    let active = ACTIVE_TRANSACTION_WRITES.with(|slot| slot.borrow().clone());
    if let Some(active) = active {
        let active_state = active.borrow();
        if active_state.directory != directory || active_state.private_paths != *private_paths {
            return Err(
                "recovery_required: active transaction evidence or private paths do not match the rewrite intent"
                    .into(),
            );
        }
        drop(active_state);
        return record_transaction_path_state(&record.path, state);
    }
    append_transaction_write_state(directory, &record.path, state, private_paths)
}

fn transaction_state_from_anchored_with_private_paths(
    path: &Path,
    state: AnchoredPathState,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<TransactionPathState, String> {
    if !private_paths.contains(path) {
        return transaction_state_from_anchored(state);
    }
    match state {
        AnchoredPathState::Missing => Ok(TransactionPathState::PrivateMissing),
        AnchoredPathState::File {
            bytes,
            mode,
            identity,
        } => transaction_private_file_state(&bytes, mode, identity)
            .map_err(|error| format!("recovery_required: {error}")),
        AnchoredPathState::Symlink { .. }
        | AnchoredPathState::Directory { .. }
        | AnchoredPathState::Other { .. } => Err(format!(
            "recovery_required: private transaction target is not a regular file: {}",
            path.display()
        )),
    }
}

fn transaction_state_from_anchored(
    state: AnchoredPathState,
) -> Result<TransactionPathState, String> {
    match state {
        AnchoredPathState::Missing => Ok(TransactionPathState::Missing),
        AnchoredPathState::File {
            bytes,
            mode,
            identity,
        } => Ok(TransactionPathState::File {
            bytes: bytes.to_vec(),
            mode,
            identity,
        }),
        AnchoredPathState::Symlink { target, identity } => {
            Ok(TransactionPathState::Symlink { target, identity })
        }
        AnchoredPathState::Directory { .. } | AnchoredPathState::Other { .. } => {
            Err("recovery_required: a mutation intent restored an unsupported target type".into())
        }
    }
}

fn append_transaction_write_state(
    directory: &Path,
    path: &Path,
    state: TransactionPathState,
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| {
        format!(
            "recovery_required: failed to inspect transaction evidence {}: {error}",
            directory.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "recovery_required: transaction evidence is not a real directory: {}",
            directory.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(format!(
                "recovery_required: transaction evidence is not private: {}",
                directory.display()
            ));
        }
    }
    validate_transaction_state_parity(path, &state, private_paths)
        .map_err(|error| format!("recovery_required: {error}"))?;
    let existing = load_transaction_write_states_with_private_paths(directory, private_paths)?;
    let sequence = existing_journal_record_count(directory)?;
    let record = DurableTransactionWrite {
        version: TRANSACTION_WRITE_RECORD_VERSION,
        sequence,
        path: path.to_path_buf(),
        state,
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    let record_path = directory.join(format!("{sequence:020}.json"));
    write_private_new_file(&record_path, &bytes)?;
    sync_parent(&record_path)?;
    drop(existing);
    Ok(())
}

fn existing_journal_record_count(directory: &Path) -> Result<u64, String> {
    let mut count = 0_u64;
    for entry in fs::read_dir(directory).map_err(|error| format!("recovery_required: {error}"))? {
        let path = entry
            .map_err(|error| format!("recovery_required: {error}"))?
            .path();
        if !is_journal_pending_path(&path) {
            count = count
                .checked_add(1)
                .ok_or_else(|| "recovery_required: transaction evidence overflow".to_string())?;
        }
    }
    Ok(count)
}

fn valid_mutation_entry_name(name: &str) -> bool {
    let path = Path::new(name);
    !name.is_empty()
        && path.file_name().and_then(|value| value.to_str()) == Some(name)
        && path.components().count() == 1
}

#[cfg(unix)]
fn cleanup_active_mutation_temp(
    intent: Option<&MutationIntent>,
    parent_snapshot: &ParentDirectorySnapshot,
    parent: &OpenParent,
) -> Result<(), String> {
    let intent = intent.ok_or_else(|| {
        "recovery_required: an active mutation lost its durable intent".to_string()
    })?;
    cleanup_intent_temp_if_owned(&intent.record, parent_snapshot, parent)
}

#[cfg(unix)]
fn cleanup_intent_temp_if_owned(
    record: &DurableMutationIntent,
    parent_snapshot: &ParentDirectorySnapshot,
    parent: &OpenParent,
) -> Result<(), String> {
    use rustix::fs::{unlinkat, AtFlags};

    let Some(temp_name) = record.temp_name.as_deref() else {
        return Ok(());
    };
    let temp_name = std::ffi::OsStr::new(temp_name);
    let temp_path = sibling_path(parent_snapshot, temp_name);
    let state = read_path_state_from_parent(&temp_path, parent)?;
    if matches!(state, AnchoredPathState::Missing) {
        return Ok(());
    }
    let owned = match record.phase {
        MutationIntentPhase::Prepared => {
            return Err(
                "recovery_required: a prepared mutation temp has no exact ownership evidence; it was preserved"
                    .into(),
            );
        }
        MutationIntentPhase::Armed => {
            let expected = record.desired_fingerprint.as_deref().ok_or_else(|| {
                "recovery_required: armed mutation has no desired fingerprint".to_string()
            })?;
            fingerprint_anchored_path_state(&state, parent_snapshot)? == expected
        }
    };
    if !owned {
        return Err("recovery_required: mutation temp no longer belongs to Arca".into());
    }
    // An open directory descriptor keeps naming a directory after it has been
    // displaced. Re-open the reviewed path before deleting ownership evidence
    // so a successful write can never be stranded in that displaced tree.
    verify_mutation_parent_is_current(record)?;
    unlinkat(&parent.directory, temp_name, AtFlags::empty())
        .map_err(|error| format!("recovery_required: {error}"))?;
    parent
        .directory
        .sync_all()
        .map_err(|error| error.to_string())
}

fn retire_mutation_intent_record(
    path: &Path,
    record: &DurableMutationIntent,
    record_identity: PathIdentity,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        verify_mutation_parent_is_current(record)?;
        let directory_path = path.parent().ok_or_else(|| {
            "recovery_required: mutation intent record has no journal parent".to_string()
        })?;
        let journal = private_journal_directory(directory_path, false)?
            .ok_or_else(|| "recovery_required: mutation intent journal disappeared".to_string())?;
        if journal.identity != record.journal_identity {
            return Err("recovery_required: mutation intent journal identity changed".into());
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                "recovery_required: mutation intent has an invalid record name".to_string()
            })?;
        journal.remove(name, record_identity)
    }

    #[cfg(not(unix))]
    {
        let _ = (record, record_identity);
        fs::remove_file(path).map_err(|error| format!("recovery_required: {error}"))?;
        sync_parent(path)
    }
}

fn record_transaction_symlink_state(
    path: &Path,
    target: &Path,
    identity: PathIdentity,
) -> Result<(), String> {
    record_transaction_path_state(
        path,
        TransactionPathState::Symlink {
            target: target.to_path_buf(),
            identity,
        },
    )
}

pub(crate) fn record_transaction_removal(path: &Path) -> Result<(), String> {
    let private = ACTIVE_TRANSACTION_WRITES.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|active| active.borrow().private_paths.contains(path))
    });
    record_transaction_path_state(
        path,
        if private {
            TransactionPathState::PrivateMissing
        } else {
            TransactionPathState::Missing
        },
    )
}

fn record_transaction_file(
    path: &Path,
    bytes: &[u8],
    mode: Option<u32>,
    identity: PathIdentity,
) -> Result<(), String> {
    let private = ACTIVE_TRANSACTION_WRITES.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|active| active.borrow().private_paths.contains(path))
    });
    let state = if private {
        transaction_private_file_state(bytes, mode, identity)?
    } else {
        TransactionPathState::File {
            bytes: bytes.to_vec(),
            mode,
            identity,
        }
    };
    record_transaction_path_state(path, state)
}

fn record_transaction_path_state(path: &Path, state: TransactionPathState) -> Result<(), String> {
    let active = ACTIVE_TRANSACTION_WRITES.with(|slot| slot.borrow().clone());
    let Some(active) = active else {
        return Ok(());
    };
    let mut active = active.borrow_mut();
    if !active.tracked_paths.contains(path) {
        return Err(format!(
            "asset_target_unsafe: transaction attempted to record an unreviewed write: {}",
            path.display()
        ));
    }
    validate_transaction_state_parity(path, &state, &active.private_paths)
        .map_err(|error| format!("asset_target_unsafe: {error}"))?;
    #[cfg(unix)]
    if !transaction_path_state_has_exact_identity(&state) {
        return Err(format!(
            "asset_target_unsafe: transaction write state has no exact identity: {}",
            path.display()
        ));
    }
    // Keep the in-memory ownership evidence first. If syncing the durable
    // evidence fails, the current process can still roll back safely; a crash
    // in that interval will conservatively require manual recovery.
    active.states.insert(path.to_path_buf(), state.clone());
    let sequence = active.next_sequence;
    active.next_sequence += 1;
    let record = DurableTransactionWrite {
        version: TRANSACTION_WRITE_RECORD_VERSION,
        sequence,
        path: path.to_path_buf(),
        state,
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    let record_path = active.directory.join(format!("{sequence:020}.json"));
    write_private_new_file(&record_path, &bytes)?;
    sync_parent(&record_path)
}

fn write_private_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let final_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("record");
    let temp = parent.join(format!(
        ".{final_name}.mux-pending-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temp)
            .map_err(|error| format!("failed to create {}: {error}", temp.display()))?;
        file.write_all(bytes)
            .map_err(|error| format!("failed to write {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync {}: {error}", temp.display()))?;
        publish_path_noreplace(&temp, path)?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(crate) fn ensure_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_private_directory_all_durable(parent)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!(
                    "asset_target_unsafe: managed icon is not a regular file: {}",
                    path.display()
                ));
            }
            #[cfg(unix)]
            let mut file = {
                use rustix::fs::{open, Mode, OFlags};
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                let file = open(
                    path,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map(fs::File::from)
                .map_err(|error| format!("failed to open {} safely: {error}", path.display()))?;
                let metadata = file
                    .metadata()
                    .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
                if !metadata.is_file() || metadata.nlink() != 1 {
                    return Err(format!(
                        "asset_target_unsafe: managed icon has multiple links: {}",
                        path.display()
                    ));
                }
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|error| format!("failed to secure {}: {error}", path.display()))?;
                file
            };
            #[cfg(not(unix))]
            let mut file = OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
            let mut existing = Vec::new();
            file.read_to_end(&mut existing)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            if existing != bytes {
                return Err("managed icon hash collision".into());
            }
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => write_private_new_file(path, bytes),
        Err(error) => Err(format!("failed to inspect {}: {error}", path.display())),
    }
}

#[cfg(not(unix))]
fn write_private_replace_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "asset_target_unsafe: mutation intent is not a regular file: {}",
            path.display()
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let final_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("record");
    let temp = parent.join(format!(
        ".{final_name}.mux-pending-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temp)
            .map_err(|error| format!("failed to create {}: {error}", temp.display()))?;
        file.write_all(bytes)
            .map_err(|error| format!("failed to write {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync {}: {error}", temp.display()))?;
        fs::rename(&temp, path).map_err(|error| {
            format!("failed to arm mutation intent {}: {error}", path.display())
        })?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn is_journal_pending_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') && name.contains(".mux-pending-"))
}

#[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
pub(crate) fn publish_path_noreplace(source: &Path, destination: &Path) -> Result<(), String> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};

    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE)
        .map_err(|error| format!("failed to publish {}: {error}", destination.display()))
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
pub(crate) fn publish_path_noreplace(source: &Path, destination: &Path) -> Result<(), String> {
    fs::hard_link(source, destination)
        .map_err(|error| format!("failed to publish {}: {error}", destination.display()))?;
    fs::remove_file(source).map_err(|error| error.to_string())
}

fn set_private_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(unix)]
fn open_directory_tree_once(path: &Path, create: bool) -> Result<Option<fs::File>, String> {
    use rustix::fs::{mkdirat, openat, Mode, OFlags, CWD};
    use rustix::io::Errno;
    use std::path::Component;

    if path.as_os_str().is_empty() {
        return Err("asset_target_unsafe: durable directory path is empty".into());
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let start = if path.is_absolute() {
        Path::new("/")
    } else {
        Path::new(".")
    };
    let mut directory =
        fs::File::from(openat(CWD, start, flags, Mode::empty()).map_err(|error| {
            format!("asset_target_unsafe: failed to open journal root: {error}")
        })?);
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir | Component::Prefix(_) => {
                return Err(format!(
                    "asset_target_unsafe: durable directory contains an unsafe component: {}",
                    path.display()
                ));
            }
        };
        let child = match openat(&directory, name, flags, Mode::empty()) {
            Ok(child) => child,
            Err(Errno::NOENT) if !create => return Ok(None),
            Err(Errno::NOENT) => {
                match mkdirat(&directory, name, Mode::from(0o700)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(error) => {
                        return Err(format!(
                            "asset_target_unsafe: failed to create durable directory {}: {error}",
                            path.display()
                        ));
                    }
                }
                directory
                    .sync_all()
                    .map_err(|error| format!("failed to sync a durable journal parent: {error}"))?;
                openat(&directory, name, flags, Mode::empty()).map_err(|error| {
                    format!(
                        "asset_target_unsafe: durable directory component is unsafe for {}: {error}",
                        path.display()
                    )
                })?
            }
            Err(error) => {
                return Err(format!(
                    "asset_target_unsafe: durable directory component is unsafe for {}: {error}",
                    path.display()
                ));
            }
        };
        directory = fs::File::from(child);
    }
    Ok(Some(directory))
}

#[cfg(unix)]
fn private_journal_directory(
    path: &Path,
    create: bool,
) -> Result<Option<PrivateJournalDirectory>, String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let Some(first) = open_directory_tree_once(path, create)? else {
        return Ok(None);
    };
    let first_metadata = first
        .metadata()
        .map_err(|error| format!("failed to inspect journal {}: {error}", path.display()))?;
    if !first_metadata.is_dir() || first_metadata.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "asset_target_unsafe: durable journal is not a private real directory: {}",
            path.display()
        ));
    }
    let first_identity = PathIdentity {
        device: Some(first_metadata.dev()),
        inode: Some(first_metadata.ino()),
    };
    let second = open_directory_tree_once(path, false)?.ok_or_else(|| {
        format!(
            "asset_target_unsafe: durable journal disappeared while opening: {}",
            path.display()
        )
    })?;
    let second_metadata = second
        .metadata()
        .map_err(|error| format!("failed to recheck journal {}: {error}", path.display()))?;
    let second_identity = PathIdentity {
        device: Some(second_metadata.dev()),
        inode: Some(second_metadata.ino()),
    };
    if first_identity != second_identity
        || !second_metadata.is_dir()
        || second_metadata.permissions().mode() & 0o077 != 0
    {
        return Err(format!(
            "asset_target_unsafe: durable journal changed while opening: {}",
            path.display()
        ));
    }
    Ok(Some(PrivateJournalDirectory {
        path: path.to_path_buf(),
        directory: second,
        identity: second_identity,
    }))
}

#[cfg(unix)]
impl PrivateJournalDirectory {
    fn ensure_current(&self) -> Result<(), String> {
        let current = private_journal_directory(&self.path, false)?.ok_or_else(|| {
            format!(
                "recovery_required: durable journal disappeared: {}",
                self.path.display()
            )
        })?;
        if current.identity != self.identity {
            return Err(format!(
                "recovery_required: durable journal identity changed: {}",
                self.path.display()
            ));
        }
        Ok(())
    }

    fn names(&self) -> Result<Vec<String>, String> {
        use rustix::fs::Dir;

        self.ensure_current()?;
        let mut names = Vec::new();
        let entries = Dir::read_from(&self.directory)
            .map_err(|error| format!("recovery_required: failed to read journal: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("recovery_required: {error}"))?;
            let raw = entry.file_name().to_bytes();
            if raw == b"." || raw == b".." {
                continue;
            }
            let name = std::str::from_utf8(raw)
                .map_err(|_| "recovery_required: journal entry name is not UTF-8".to_string())?;
            if !valid_mutation_entry_name(name) {
                return Err("recovery_required: invalid journal entry name".into());
            }
            names.push(name.to_string());
        }
        names.sort();
        self.ensure_current()?;
        Ok(names)
    }

    fn read_private(&self, name: &str) -> Result<(Vec<u8>, PathIdentity), String> {
        use rustix::fs::{fstat, openat, statat, AtFlags, FileType, Mode, OFlags};
        use std::io::Read;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        if !valid_mutation_entry_name(name) {
            return Err("recovery_required: invalid journal entry name".into());
        }
        self.ensure_current()?;
        let before = statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
            format!("recovery_required: failed to inspect journal record: {error}")
        })?;
        if FileType::from_raw_mode(before.st_mode as _) != FileType::RegularFile
            || before.st_mode & 0o077 != 0
            || before.st_nlink != 1
        {
            return Err("recovery_required: journal record is not a private regular file".into());
        }
        let mut file = fs::File::from(
            openat(
                &self.directory,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| {
                format!("recovery_required: failed to open journal record: {error}")
            })?,
        );
        let metadata = file.metadata().map_err(|error| {
            format!("recovery_required: failed to inspect journal record: {error}")
        })?;
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.nlink() != 1
            || before.st_dev as u64 != metadata.dev()
            || before.st_ino as u64 != metadata.ino()
        {
            return Err("recovery_required: journal record changed while opening".into());
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                format!("recovery_required: failed to read journal record: {error}")
            })?;
        if bytes.len() > 1024 * 1024 {
            return Err("recovery_required: mutation intent exceeds its size limit".into());
        }
        let after = fstat(&file).map_err(|error| {
            format!("recovery_required: failed to recheck journal record: {error}")
        })?;
        if after.st_dev != before.st_dev || after.st_ino != before.st_ino {
            return Err("recovery_required: journal record changed while reading".into());
        }
        self.ensure_current()?;
        Ok((
            bytes,
            PathIdentity {
                device: Some(metadata.dev()),
                inode: Some(metadata.ino()),
            },
        ))
    }

    fn write_new(&self, name: &str, bytes: &[u8]) -> Result<PathIdentity, String> {
        use rustix::fs::{openat, unlinkat, AtFlags, Mode, OFlags};
        use std::os::unix::fs::MetadataExt;

        if !valid_mutation_entry_name(name) {
            return Err("asset_target_unsafe: invalid mutation record name".into());
        }
        let temp = format!(
            ".{name}.mux-pending-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        );
        let result = (|| {
            let mut file = fs::File::from(
                openat(
                    &self.directory,
                    temp.as_str(),
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from(0o600),
                )
                .map_err(|error| format!("failed to create mutation record: {error}"))?,
            );
            file.write_all(bytes)
                .map_err(|error| format!("failed to write mutation record: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("failed to sync mutation record: {error}"))?;
            let metadata = file
                .metadata()
                .map_err(|error| format!("failed to inspect mutation record: {error}"))?;
            let identity = PathIdentity {
                device: Some(metadata.dev()),
                inode: Some(metadata.ino()),
            };
            self.directory
                .sync_all()
                .map_err(|error| format!("failed to sync mutation journal: {error}"))?;
            self.ensure_current()?;
            rename_entry_noreplace(
                &OpenParent {
                    directory: self
                        .directory
                        .try_clone()
                        .map_err(|error| error.to_string())?,
                },
                std::ffi::OsStr::new(&temp),
                std::ffi::OsStr::new(name),
            )?;
            self.directory
                .sync_all()
                .map_err(|error| format!("failed to sync mutation journal: {error}"))?;
            self.ensure_current()?;
            Ok(identity)
        })();
        if result.is_err() {
            let _ = unlinkat(&self.directory, temp.as_str(), AtFlags::empty());
            let _ = self.directory.sync_all();
        }
        result
    }

    fn replace(
        &self,
        name: &str,
        expected: PathIdentity,
        bytes: &[u8],
    ) -> Result<PathIdentity, String> {
        use rustix::fs::{openat, renameat, statat, unlinkat, AtFlags, Mode, OFlags};
        use std::os::unix::fs::MetadataExt;

        if !expected.is_exact() || !valid_mutation_entry_name(name) {
            return Err("recovery_required: mutation record lacks exact ownership".into());
        }
        let current =
            statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
                format!("recovery_required: failed to inspect mutation record: {error}")
            })?;
        if (PathIdentity {
            device: Some(current.st_dev as u64),
            inode: Some(current.st_ino as u64),
        }) != expected
        {
            return Err("recovery_required: mutation record changed before update".into());
        }
        let temp = format!(
            ".{name}.mux-pending-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        );
        let result = (|| {
            let mut file = fs::File::from(
                openat(
                    &self.directory,
                    temp.as_str(),
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from(0o600),
                )
                .map_err(|error| format!("failed to create armed mutation record: {error}"))?,
            );
            file.write_all(bytes)
                .map_err(|error| format!("failed to write armed mutation record: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("failed to sync armed mutation record: {error}"))?;
            let metadata = file
                .metadata()
                .map_err(|error| format!("failed to inspect armed mutation record: {error}"))?;
            let identity = PathIdentity {
                device: Some(metadata.dev()),
                inode: Some(metadata.ino()),
            };
            self.ensure_current()?;
            let rechecked =
                statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
                    format!("recovery_required: failed to recheck mutation record: {error}")
                })?;
            if (PathIdentity {
                device: Some(rechecked.st_dev as u64),
                inode: Some(rechecked.st_ino as u64),
            }) != expected
            {
                return Err("recovery_required: mutation record changed before publication".into());
            }
            renameat(&self.directory, temp.as_str(), &self.directory, name)
                .map_err(|error| format!("failed to arm mutation intent: {error}"))?;
            self.directory
                .sync_all()
                .map_err(|error| format!("failed to sync armed mutation journal: {error}"))?;
            self.ensure_current()?;
            Ok(identity)
        })();
        if result.is_err() {
            let _ = unlinkat(&self.directory, temp.as_str(), AtFlags::empty());
            let _ = self.directory.sync_all();
        }
        result
    }

    fn remove(&self, name: &str, expected: PathIdentity) -> Result<(), String> {
        use rustix::fs::{statat, unlinkat, AtFlags};

        if !expected.is_exact() || !valid_mutation_entry_name(name) {
            return Err("recovery_required: mutation record lacks exact ownership".into());
        }
        self.ensure_current()?;
        let current =
            statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
                format!("recovery_required: failed to inspect mutation record: {error}")
            })?;
        if (PathIdentity {
            device: Some(current.st_dev as u64),
            inode: Some(current.st_ino as u64),
        }) != expected
        {
            return Err("recovery_required: mutation record changed before retirement".into());
        }
        unlinkat(&self.directory, name, AtFlags::empty()).map_err(|error| {
            format!("recovery_required: failed to retire mutation intent: {error}")
        })?;
        self.directory.sync_all().map_err(|error| {
            format!("recovery_required: failed to sync mutation journal: {error}")
        })?;
        self.ensure_current()
    }
}

#[cfg(unix)]
fn create_private_directory_all_durable(path: &Path) -> Result<(), String> {
    private_journal_directory(path, true)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_directory_all_durable(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "asset_target_unsafe: durable directory is not a real directory: {}",
                    path.display()
                ));
            }
            return Ok(());
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "failed to inspect durable directory {}: {error}",
                path.display()
            ));
        }
    }
    let parent = path.parent().ok_or_else(|| {
        format!(
            "asset_target_unsafe: durable directory has no parent: {}",
            path.display()
        )
    })?;
    create_private_directory_all_durable(parent)?;
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "asset_target_unsafe: durable directory was replaced: {}",
                    path.display()
                ));
            }
        }
        Err(error) => {
            return Err(format!(
                "failed to create durable directory {}: {error}",
                path.display()
            ));
        }
    }
    set_private_directory(path)?;
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            format!(
                "failed to sync durable directory {}: {error}",
                path.display()
            )
        })?;
    sync_parent(path)
}

fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("failed to sync {}: {error}", parent.display()))
}

#[cfg(any(test, not(unix)))]
fn file_mode(path: &Path) -> Result<Option<u32>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(Some(metadata.permissions().mode()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Ok(None)
    }
}

#[cfg(not(unix))]
fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to read {}: {}", path.display(), error)),
    }
}

fn resolve_destination(path: &Path) -> Result<PathBuf, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::canonicalize(path)
            .map_err(|error| format!("failed to resolve {}: {}", path.display(), error)),
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(format!("failed to inspect {}: {}", path.display(), error)),
    }
}

fn target_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn target_name(path: &Path) -> Result<&std::ffi::OsStr, String> {
    path.file_name().ok_or_else(|| {
        format!(
            "asset_target_unsafe: transaction target has no file name: {}",
            path.display()
        )
    })
}

/// Capture the nearest existing parent directory without following the final
/// parent component. Missing descendants are retained as a relative path below
/// that stable anchor.
pub(crate) fn capture_parent_directory(path: &Path) -> Result<ParentDirectorySnapshot, String> {
    target_name(path)?;
    let parent_path = target_parent(path).to_path_buf();
    let mut cursor = parent_path.clone();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!(
                    "asset_target_unsafe: parent is not a real directory: {}",
                    cursor.display()
                ));
            }
            Ok(metadata) => {
                let mut relative_parent = PathBuf::new();
                for component in missing.iter().rev() {
                    relative_parent.push(component);
                }
                let canonical_anchor = fs::canonicalize(&cursor).map_err(|error| {
                    format!(
                        "asset_target_unsafe: failed to resolve parent anchor {}: {error}",
                        cursor.display()
                    )
                })?;
                #[cfg(unix)]
                let (device, inode) = {
                    use std::os::unix::fs::MetadataExt;
                    (Some(metadata.dev()), Some(metadata.ino()))
                };
                #[cfg(not(unix))]
                let (device, inode) = (None, None);
                let snapshot = ParentDirectorySnapshot {
                    parent_path,
                    anchor_path: cursor,
                    relative_parent,
                    device,
                    inode,
                    canonical_anchor,
                };
                // Open and compare the anchor after inspection. A path swap in
                // the inspection/open interval must not become reviewed state.
                verify_parent_anchor(&snapshot)?;
                return Ok(snapshot);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let name = cursor.file_name().ok_or_else(|| {
                    format!(
                        "asset_target_unsafe: no existing parent anchor for {}",
                        path.display()
                    )
                })?;
                missing.push(name.to_os_string());
                cursor = cursor
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf();
            }
            Err(error) => {
                return Err(format!(
                    "asset_target_unsafe: failed to inspect parent {}: {error}",
                    cursor.display()
                ));
            }
        }
    }
}

fn transaction_parent_snapshot(path: &Path) -> Option<ParentDirectorySnapshot> {
    ACTIVE_TRANSACTION_WRITES.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|active| active.borrow().parent_snapshots.get(path).cloned())
    })
}

fn transaction_expected_state(path: &Path) -> Result<Option<AnchoredPathState>, String> {
    let active = ACTIVE_TRANSACTION_WRITES.with(|slot| slot.borrow().clone());
    let Some(active) = active else {
        return Ok(None);
    };
    let active = active.borrow();
    if !active.tracked_paths.contains(path) {
        return Err(format!(
            "asset_target_unsafe: transaction attempted an unreviewed write: {}",
            path.display()
        ));
    }
    let state = if let Some(state) = active.states.get(path) {
        reviewed_state_from_transaction_state(path, state, &active.private_paths)?
    } else {
        active.reviewed_states.get(path).cloned().ok_or_else(|| {
            format!(
                "asset_target_unsafe: transaction has no reviewed state for {}",
                path.display()
            )
        })?
    };
    let parent = active.parent_snapshots.get(path).cloned().ok_or_else(|| {
        format!(
            "asset_target_unsafe: transaction has no reviewed parent for {}",
            path.display()
        )
    })?;
    drop(active);
    let state = match state {
        ReviewedTransactionPathState::Anchored(state) => state,
        ReviewedTransactionPathState::PrivateFile {
            content_hash,
            mode,
            identity,
        } => {
            let current = read_path_state_anchored(path, &parent)?;
            if !private_transaction_state_matches_anchored(&content_hash, mode, identity, &current)
            {
                return Err(format!(
                    "asset_operation_stale: private transaction target changed before hash/mode/identity verification: {}",
                    path.display()
                ));
            }
            current
        }
    };
    Ok(Some(state))
}

fn require_transaction_expected_state(
    path: &Path,
    current: &AnchoredPathState,
) -> Result<(), String> {
    let Some(expected) = transaction_expected_state(path)? else {
        return Ok(());
    };
    if !anchored_states_match(&expected, current) {
        return Err(format!(
            "asset_operation_stale: reviewed target changed before write: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn anchored_states_match(
    expected: &AnchoredPathState,
    actual: &AnchoredPathState,
) -> bool {
    fn identity_matches(expected: PathIdentity, actual: PathIdentity) -> bool {
        (expected.device.is_none() || expected.device == actual.device)
            && (expected.inode.is_none() || expected.inode == actual.inode)
    }

    match (expected, actual) {
        (AnchoredPathState::Missing, AnchoredPathState::Missing) => true,
        (
            AnchoredPathState::File {
                bytes: expected_bytes,
                mode: expected_mode,
                identity: expected_identity,
            },
            AnchoredPathState::File {
                bytes,
                mode,
                identity,
            },
        ) => {
            expected_bytes == bytes
                && expected_mode == mode
                && identity_matches(*expected_identity, *identity)
        }
        (
            AnchoredPathState::Symlink {
                target: expected_target,
                identity: expected_identity,
            },
            AnchoredPathState::Symlink { target, identity },
        ) => expected_target == target && identity_matches(*expected_identity, *identity),
        (
            AnchoredPathState::Directory {
                identity: expected_identity,
            },
            AnchoredPathState::Directory { identity },
        )
        | (
            AnchoredPathState::Other {
                identity: expected_identity,
            },
            AnchoredPathState::Other { identity },
        ) => identity_matches(*expected_identity, *identity),
        _ => false,
    }
}

fn reviewed_parent_snapshot(
    transaction_path: &Path,
    destination: &Path,
) -> Result<ParentDirectorySnapshot, String> {
    if let Some(snapshot) = transaction_parent_snapshot(transaction_path) {
        if transaction_path != destination {
            return Err(format!(
                "asset_target_unsafe: a reviewed configuration target became a symlink: {}",
                transaction_path.display()
            ));
        }
        return Ok(snapshot);
    }
    capture_parent_directory(destination)
}

#[cfg(unix)]
fn verify_parent_anchor(snapshot: &ParentDirectorySnapshot) -> Result<(), String> {
    use rustix::fs::{openat, Mode, OFlags, CWD};
    use std::os::unix::fs::MetadataExt;

    let directory = fs::File::from(
        openat(
            CWD,
            &snapshot.anchor_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            format!(
                "asset_target_unsafe: failed to open parent anchor {}: {error}",
                snapshot.anchor_path.display()
            )
        })?,
    );
    let metadata = directory.metadata().map_err(|error| {
        format!(
            "asset_target_unsafe: failed to inspect parent anchor {}: {error}",
            snapshot.anchor_path.display()
        )
    })?;
    if !metadata.is_dir()
        || snapshot.device != Some(metadata.dev())
        || snapshot.inode != Some(metadata.ino())
    {
        return Err(format!(
            "asset_target_unsafe: parent directory identity changed: {}",
            snapshot.anchor_path.display()
        ));
    }
    let canonical = fs::canonicalize(&snapshot.anchor_path).map_err(|error| {
        format!(
            "asset_target_unsafe: failed to resolve parent anchor {}: {error}",
            snapshot.anchor_path.display()
        )
    })?;
    if canonical != snapshot.canonical_anchor {
        return Err(format!(
            "asset_target_unsafe: parent directory path changed: {}",
            snapshot.anchor_path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_parent_anchor(snapshot: &ParentDirectorySnapshot) -> Result<(), String> {
    let metadata = fs::symlink_metadata(&snapshot.anchor_path).map_err(|error| {
        format!(
            "asset_target_unsafe: failed to inspect parent anchor {}: {error}",
            snapshot.anchor_path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "asset_target_unsafe: parent is not a real directory: {}",
            snapshot.anchor_path.display()
        ));
    }
    let canonical = fs::canonicalize(&snapshot.anchor_path).map_err(|error| error.to_string())?;
    if canonical != snapshot.canonical_anchor {
        return Err(format!(
            "asset_target_unsafe: parent directory path changed: {}",
            snapshot.anchor_path.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
struct OpenParent {
    directory: fs::File,
}

#[cfg(unix)]
fn open_parent_identity(parent: &OpenParent) -> Result<PathIdentity, String> {
    use std::os::unix::fs::MetadataExt;

    let metadata = parent.directory.metadata().map_err(|error| {
        format!("asset_target_unsafe: failed to inspect mutation parent: {error}")
    })?;
    if !metadata.is_dir() {
        return Err("asset_target_unsafe: mutation parent is not a directory".into());
    }
    Ok(PathIdentity {
        device: Some(metadata.dev()),
        inode: Some(metadata.ino()),
    })
}

#[cfg(unix)]
fn verify_mutation_parent_is_current(record: &DurableMutationIntent) -> Result<(), String> {
    let current = open_parent_anchored(&record.parent, false)
        .map_err(|error| {
            format!(
                "recovery_required: canonical mutation parent cannot be re-opened; the durable claim was preserved ({error})"
            )
        })?
        .ok_or_else(|| {
            "recovery_required: mutation parent disappeared before claim cleanup".to_string()
        })?;
    let identity = open_parent_identity(&current).map_err(|error| {
        format!(
            "recovery_required: canonical mutation parent cannot be inspected; the durable claim was preserved ({error})"
        )
    })?;
    if !record.mutation_parent_identity.is_exact() || identity != record.mutation_parent_identity {
        return Err(
            "recovery_required: canonical mutation parent changed; the durable claim was preserved"
                .into(),
        );
    }
    Ok(())
}

#[cfg(unix)]
fn open_parent_anchored(
    snapshot: &ParentDirectorySnapshot,
    create: bool,
) -> Result<Option<OpenParent>, String> {
    use rustix::fs::{mkdirat, openat, Mode, OFlags, CWD};
    use rustix::io::Errno;
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;

    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = fs::File::from(
        openat(CWD, &snapshot.anchor_path, flags, Mode::empty()).map_err(|error| {
            format!(
                "asset_target_unsafe: failed to open parent anchor {}: {error}",
                snapshot.anchor_path.display()
            )
        })?,
    );
    let metadata = directory.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_dir()
        || snapshot.device != Some(metadata.dev())
        || snapshot.inode != Some(metadata.ino())
    {
        return Err(format!(
            "asset_target_unsafe: parent directory identity changed: {}",
            snapshot.anchor_path.display()
        ));
    }

    for component in snapshot.relative_parent.components() {
        let Component::Normal(name) = component else {
            return Err(format!(
                "asset_target_unsafe: invalid relative parent for {}",
                snapshot.parent_path.display()
            ));
        };
        let child = match openat(&directory, name, flags, Mode::empty()) {
            Ok(child) => child,
            Err(Errno::NOENT) if !create => return Ok(None),
            Err(Errno::NOENT) => {
                match mkdirat(&directory, name, Mode::from(0o700)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(error) => {
                        return Err(format!(
                            "asset_target_unsafe: failed to create parent {}: {error}",
                            snapshot.parent_path.display()
                        ));
                    }
                }
                directory.sync_all().map_err(|error| error.to_string())?;
                openat(&directory, name, flags, Mode::empty()).map_err(|error| {
                    format!(
                        "asset_target_unsafe: parent component is unsafe for {}: {error}",
                        snapshot.parent_path.display()
                    )
                })?
            }
            Err(error) => {
                return Err(format!(
                    "asset_target_unsafe: parent component is unsafe for {}: {error}",
                    snapshot.parent_path.display()
                ));
            }
        };
        directory = fs::File::from(child);
    }
    Ok(Some(OpenParent { directory }))
}

#[cfg(unix)]
pub(crate) fn read_path_state_anchored(
    path: &Path,
    snapshot: &ParentDirectorySnapshot,
) -> Result<AnchoredPathState, String> {
    if target_parent(path) != snapshot.parent_path {
        return Err(format!(
            "asset_target_unsafe: parent snapshot does not match {}",
            path.display()
        ));
    }
    let Some(parent) = open_parent_anchored(snapshot, false)? else {
        return Ok(AnchoredPathState::Missing);
    };
    read_path_state_from_parent(path, &parent)
}

#[cfg(unix)]
pub(crate) fn read_path_state_anchored_limited(
    path: &Path, snapshot: &ParentDirectorySnapshot, limit: u64,
) -> Result<AnchoredPathState, String> {
    if target_parent(path) != snapshot.parent_path { return Err("file parent changed".into()); }
    let Some(parent) = open_parent_anchored(snapshot, false)? else { return Ok(AnchoredPathState::Missing); };
    read_path_state_from_parent_limited(path, &parent, Some(limit))
}

#[cfg(not(unix))]
pub(crate) fn read_path_state_anchored_limited(
    _path: &Path, _snapshot: &ParentDirectorySnapshot, _limit: u64,
) -> Result<AnchoredPathState, String> { Err("native file editing is unavailable on this platform".into()) }

/// Run the existing durable leaf-write protocol in one document's local scope.
/// Reads do not call this and therefore never create Arca or Agent directories.
pub(crate) fn with_file_edit_claims<T>(directory: &Path, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    recover_transaction_mutation_intents(directory, &BTreeMap::new())?;
    let occupied = FILE_EDIT_CLAIMS.with(|slot| slot.borrow().is_some());
    if occupied { return Err("nested native file edit".into()); }
    struct Scope;
    impl Drop for Scope { fn drop(&mut self) { FILE_EDIT_CLAIMS.with(|slot| *slot.borrow_mut() = None); } }
    FILE_EDIT_CLAIMS.with(|slot| *slot.borrow_mut() = Some(directory.to_path_buf()));
    let _scope = Scope;
    operation()
}

#[cfg(unix)]
fn read_path_state_from_parent(
    path: &Path,
    parent: &OpenParent,
) -> Result<AnchoredPathState, String> {
    read_path_state_from_parent_limited(path, parent, None)
}

#[cfg(unix)]
fn read_path_state_from_parent_limited(
    path: &Path, parent: &OpenParent, limit: Option<u64>,
) -> Result<AnchoredPathState, String> {
    use rustix::fs::{openat, readlinkat, statat, AtFlags, FileType, Mode, OFlags};
    use rustix::io::Errno;
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let name = target_name(path)?;
    let stat = match statat(&parent.directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(Errno::NOENT) => return Ok(AnchoredPathState::Missing),
        Err(error) => {
            return Err(format!(
                "failed to inspect anchored target {}: {error}",
                path.display()
            ));
        }
    };
    match FileType::from_raw_mode(stat.st_mode as _) {
        FileType::RegularFile => {
            let file = fs::File::from(
                openat(
                    &parent.directory,
                    name,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?,
            );
            let metadata = file
                .metadata()
                .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
            if !metadata.is_file() {
                return Err(format!(
                    "asset_target_unsafe: target type changed while reading {}",
                    path.display()
                ));
            }
            let mode = Some(metadata.permissions().mode());
            let mut bytes = Vec::new();
            if limit.is_some_and(|limit| metadata.len() > limit) { return Err("file exceeds editor size limit".into()); }
            file.take(limit.map(|limit| limit + 1).unwrap_or(u64::MAX)).read_to_end(&mut bytes)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            if limit.is_some_and(|limit| bytes.len() as u64 > limit) { return Err("file exceeds editor size limit".into()); }
            Ok(AnchoredPathState::File {
                bytes: Zeroizing::new(bytes),
                mode,
                identity: PathIdentity {
                    device: Some(metadata.dev()),
                    inode: Some(metadata.ino()),
                },
            })
        }
        FileType::Symlink => {
            let target = readlinkat(&parent.directory, name, Vec::new())
                .map_err(|error| format!("failed to read symlink {}: {error}", path.display()))?
                .into_bytes();
            let confirmed =
                statat(&parent.directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
                    format!("failed to recheck symlink {}: {error}", path.display())
                })?;
            if FileType::from_raw_mode(confirmed.st_mode as _) != FileType::Symlink
                || confirmed.st_dev != stat.st_dev
                || confirmed.st_ino != stat.st_ino
            {
                return Err(format!(
                    "asset_target_unsafe: symlink changed while reading {}",
                    path.display()
                ));
            }
            Ok(AnchoredPathState::Symlink {
                target: PathBuf::from(std::ffi::OsString::from_vec(target)),
                identity: PathIdentity {
                    device: Some(confirmed.st_dev as u64),
                    inode: Some(confirmed.st_ino as u64),
                },
            })
        }
        FileType::Directory => Ok(AnchoredPathState::Directory {
            identity: PathIdentity {
                device: Some(stat.st_dev as u64),
                inode: Some(stat.st_ino as u64),
            },
        }),
        _ => Ok(AnchoredPathState::Other {
            identity: PathIdentity {
                device: Some(stat.st_dev as u64),
                inode: Some(stat.st_ino as u64),
            },
        }),
    }
}

#[cfg(not(unix))]
pub(crate) fn read_path_state_anchored(
    path: &Path,
    snapshot: &ParentDirectorySnapshot,
) -> Result<AnchoredPathState, String> {
    if target_parent(path) != snapshot.parent_path {
        return Err(format!(
            "asset_target_unsafe: parent snapshot does not match {}",
            path.display()
        ));
    }
    verify_parent_anchor(snapshot)?;
    let mut current = snapshot.anchor_path.clone();
    for component in snapshot.relative_parent.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!(
                    "asset_target_unsafe: parent component is unsafe: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(AnchoredPathState::Missing);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::read_link(path)
            .map(|target| AnchoredPathState::Symlink {
                target,
                identity: PathIdentity::unknown(),
            })
            .map_err(|error| error.to_string()),
        Ok(metadata) if metadata.is_file() => fs::read(path)
            .map(|bytes| AnchoredPathState::File {
                bytes,
                mode: None,
                identity: PathIdentity::unknown(),
            })
            .map_err(|error| error.to_string()),
        Ok(metadata) if metadata.is_dir() => Ok(AnchoredPathState::Directory {
            identity: PathIdentity::unknown(),
        }),
        Ok(_) => Ok(AnchoredPathState::Other {
            identity: PathIdentity::unknown(),
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(AnchoredPathState::Missing),
        Err(error) => Err(error.to_string()),
    }
}

/// Canonical review fingerprint for one leaf and its anchored parent. File
/// contents are hashed before serialization so secret-bearing Agent configs
/// never enter persisted operation plans.
pub(crate) fn fingerprint_anchored_path_state(
    state: &AnchoredPathState,
    parent: &ParentDirectorySnapshot,
) -> Result<String, String> {
    let leaf = match state {
        AnchoredPathState::Missing => serde_json::json!({ "kind": "missing" }),
        AnchoredPathState::File {
            bytes,
            mode,
            identity,
        } => serde_json::json!({
            "kind": "file",
            "content_hash": hex::encode(Sha256::digest(bytes)),
            "mode": mode,
            "identity": identity,
        }),
        AnchoredPathState::Symlink { target, identity } => serde_json::json!({
            "kind": "symlink",
            "target_hash": hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes())),
            "identity": identity,
        }),
        AnchoredPathState::Directory { identity } => serde_json::json!({
            "kind": "directory",
            "identity": identity,
        }),
        AnchoredPathState::Other { identity } => serde_json::json!({
            "kind": "other",
            "identity": identity,
        }),
    };
    let parent = serde_json::to_vec(parent).map_err(|error| error.to_string())?;
    let parent_hash = hex::encode(Sha256::digest(parent));
    let bytes = serde_json::to_vec(&(leaf, parent_hash)).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn zeroize_json_value_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(value) => value.zeroize(),
        serde_json::Value::Array(values) => values.iter_mut().for_each(zeroize_json_value_strings),
        serde_json::Value::Object(values) => {
            values.values_mut().for_each(zeroize_json_value_strings)
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn semantic_path_state_fingerprint(state: &AnchoredPathState) -> Result<String, String> {
    let value = match state {
        AnchoredPathState::Missing => serde_json::json!({ "kind": "missing" }),
        AnchoredPathState::File { bytes, .. } => {
            let mut parsed = serde_json::from_slice::<serde_json::Value>(bytes).ok();
            let canonical = Zeroizing::new(
                parsed
                    .as_ref()
                    .and_then(|value| serde_json::to_vec(value).ok())
                    .unwrap_or_else(|| bytes.to_vec()),
            );
            let content_hash = hex::encode(Sha256::digest(canonical.as_slice()));
            if let Some(value) = parsed.as_mut() {
                zeroize_json_value_strings(value);
            }
            serde_json::json!({
                "kind": "file",
                "content_hash": content_hash,
            })
        }
        AnchoredPathState::Symlink { target, .. } => serde_json::json!({
            "kind": "symlink",
            "target_hash": hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes())),
        }),
        AnchoredPathState::Directory { .. } => serde_json::json!({ "kind": "directory" }),
        AnchoredPathState::Other { .. } => serde_json::json!({ "kind": "other" }),
    };
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn legacy_semantic_path_state_fingerprint(state: &AnchoredPathState) -> Result<String, String> {
    let value = match state {
        AnchoredPathState::Missing => serde_json::json!({ "kind": "missing" }),
        AnchoredPathState::File { bytes, .. } => serde_json::json!({
            "kind": "file",
            "content_hash": hex::encode(Sha256::digest(bytes)),
        }),
        AnchoredPathState::Symlink { target, .. } => serde_json::json!({
            "kind": "symlink",
            "target_hash": hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes())),
        }),
        AnchoredPathState::Directory { .. } => serde_json::json!({ "kind": "directory" }),
        AnchoredPathState::Other { .. } => serde_json::json!({ "kind": "other" }),
    };
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

struct MutationIntentRequest<'a> {
    path: &'a Path,
    parent: &'a ParentDirectorySnapshot,
    mutation_parent_identity: PathIdentity,
    guard_name: &'a std::ffi::OsStr,
    temp_name: Option<&'a std::ffi::OsStr>,
    operation: MutationIntentOperation,
    expected: &'a AnchoredPathState,
    desired: &'a AnchoredPathState,
}

fn begin_mutation_intent(
    request: MutationIntentRequest<'_>,
) -> Result<Option<MutationIntent>, String> {
    let exact_desired = request.desired;
    create_mutation_intent_record(request, MutationIntentPhase::Armed, Some(exact_desired))
        .map(Some)
}

fn prepare_mutation_intent(request: MutationIntentRequest<'_>) -> Result<MutationIntent, String> {
    debug_assert!(request.temp_name.is_some());
    create_mutation_intent_record(request, MutationIntentPhase::Prepared, None)
}

fn create_mutation_intent_record(
    request: MutationIntentRequest<'_>,
    phase: MutationIntentPhase,
    exact_desired: Option<&AnchoredPathState>,
) -> Result<MutationIntent, String> {
    let MutationIntentRequest {
        path,
        parent,
        mutation_parent_identity,
        guard_name,
        temp_name,
        operation,
        expected,
        desired: desired_semantic_state,
    } = request;
    let journal_lock = acquire_settings_lock(&global_mutation_lock_subject())?;
    let active = ACTIVE_TRANSACTION_WRITES.with(|slot| slot.borrow().clone());
    let (claims, sequence, privacy) = if let Some(active) = active {
        ensure_no_transaction_mutation_intents(&global_mutation_intent_dir())?;
        let mut active = active.borrow_mut();
        if !active.tracked_paths.contains(path) {
            return Err(format!(
                "asset_target_unsafe: transaction attempted an unreviewed mutation: {}",
                path.display()
            ));
        }
        let sequence = active.next_intent_sequence;
        active.next_intent_sequence += 1;
        let claims = active
            .directory
            .parent()
            .ok_or_else(|| "asset_target_unsafe: transaction evidence has no parent".to_string())?
            .join("claims");
        (
            claims,
            sequence,
            transaction_path_privacy(path, &active.private_paths),
        )
    } else {
        let sequence = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        (
            FILE_EDIT_CLAIMS.with(|slot| slot.borrow().clone()).unwrap_or_else(global_mutation_intent_dir),
            sequence,
            TransactionPathPrivacy::Ordinary,
        )
    };
    ensure_no_transaction_mutation_intents(&claims)?;
    let guard_name = guard_name
        .to_str()
        .ok_or_else(|| "asset_target_unsafe: mutation guard name is not UTF-8".to_string())?
        .to_string();
    let temp_name = temp_name
        .map(|name| {
            name.to_str()
                .map(str::to_string)
                .ok_or_else(|| "asset_target_unsafe: mutation temp name is not UTF-8".to_string())
        })
        .transpose()?;
    create_private_directory_all_durable(&claims)?;
    #[cfg(unix)]
    let journal = private_journal_directory(&claims, false)?
        .ok_or_else(|| "asset_target_unsafe: mutation intent journal disappeared".to_string())?;
    #[cfg(unix)]
    let journal_identity = journal.identity;
    #[cfg(not(unix))]
    let journal_identity = PathIdentity::unknown();
    let record_id = uuid::Uuid::new_v4().to_string();
    let record = DurableMutationIntent {
        version: TRANSACTION_MUTATION_INTENT_VERSION,
        sequence,
        record_id: record_id.clone(),
        path: path.to_path_buf(),
        privacy,
        parent: parent.clone(),
        mutation_parent_identity,
        journal_identity,
        guard_name: guard_name.clone(),
        temp_name: temp_name.clone(),
        operation,
        phase,
        expected_fingerprint: fingerprint_anchored_path_state(expected, parent)?,
        desired_semantic_fingerprint: semantic_path_state_fingerprint(desired_semantic_state)?,
        desired_fingerprint: exact_desired
            .map(|desired| fingerprint_anchored_path_state(desired, parent))
            .transpose()?,
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    let record_name = format!("{sequence:020}-{record_id}.json");
    let record_path = claims.join(&record_name);
    #[cfg(unix)]
    let record_identity = journal.write_new(&record_name, &bytes)?;
    #[cfg(not(unix))]
    let record_identity = {
        write_private_new_file(&record_path, &bytes)?;
        PathIdentity::unknown()
    };
    Ok(MutationIntent {
        record_path,
        record,
        record_identity,
        _journal_lock: journal_lock,
    })
}

fn arm_mutation_intent(
    intent: &mut MutationIntent,
    desired: &AnchoredPathState,
) -> Result<(), String> {
    intent.record.phase = MutationIntentPhase::Armed;
    intent.record.desired_semantic_fingerprint = semantic_path_state_fingerprint(desired)?;
    intent.record.desired_fingerprint = Some(fingerprint_anchored_path_state(
        desired,
        &intent.record.parent,
    )?);
    let bytes = serde_json::to_vec(&intent.record).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        let directory_path = intent.record_path.parent().ok_or_else(|| {
            "recovery_required: mutation intent record has no journal parent".to_string()
        })?;
        let journal = private_journal_directory(directory_path, false)?
            .ok_or_else(|| "recovery_required: mutation intent journal disappeared".to_string())?;
        if journal.identity != intent.record.journal_identity {
            return Err("recovery_required: mutation intent journal identity changed".into());
        }
        let name = intent
            .record_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "recovery_required: invalid mutation intent record name".to_string())?;
        intent.record_identity = journal.replace(name, intent.record_identity, &bytes)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        write_private_replace_file(&intent.record_path, &bytes)
    }
}

fn global_mutation_intent_dir() -> PathBuf {
    #[cfg(test)]
    {
        TEST_GLOBAL_MUTATION_ROOT.with(|root| root.join("claims"))
    }
    #[cfg(not(test))]
    {
        crate::paths::arca_dir().join("staging/safe-write/claims")
    }
}

fn global_mutation_lock_subject() -> PathBuf {
    #[cfg(test)]
    {
        TEST_GLOBAL_MUTATION_ROOT.with(|root| root.clone())
    }
    #[cfg(not(test))]
    {
        crate::paths::arca_dir().join("staging/safe-write")
    }
}

pub(crate) fn recover_global_mutation_intents() -> Result<(), String> {
    let _journal_lock = acquire_settings_lock(&global_mutation_lock_subject())?;
    let directory = global_mutation_intent_dir();
    recover_transaction_mutation_intents(&directory, &BTreeMap::new())?;
    ensure_no_transaction_mutation_intents(&directory)
}

pub(crate) fn pending_global_mutation_error() -> Option<String> {
    ensure_no_transaction_mutation_intents(&global_mutation_intent_dir())
        .err()
        .map(|error| format!("unresolved durable safe-write claim: {error}"))
}

#[cfg(all(test, unix))]
pub(crate) struct TestGlobalMutationClaim {
    journal: PrivateJournalDirectory,
    name: String,
    identity: PathIdentity,
}

#[cfg(all(test, unix))]
impl Drop for TestGlobalMutationClaim {
    fn drop(&mut self) {
        let _ = self.journal.remove(&self.name, self.identity);
    }
}

#[cfg(all(test, unix))]
pub(crate) fn install_test_global_mutation_claim() -> Result<TestGlobalMutationClaim, String> {
    let journal = private_journal_directory(&global_mutation_intent_dir(), true)?
        .ok_or_else(|| "test mutation journal was not created".to_string())?;
    let name = format!("{}.json", uuid::Uuid::new_v4());
    // Deliberately malformed: recovery must fail closed and retain the record.
    let identity = journal.write_new(&name, b"{}")?;
    Ok(TestGlobalMutationClaim {
        journal,
        name,
        identity,
    })
}

fn complete_mutation_intent(intent: Option<&MutationIntent>) -> Result<(), String> {
    let Some(intent) = intent else {
        return Ok(());
    };
    retire_mutation_intent_record(&intent.record_path, &intent.record, intent.record_identity)
}

fn recover_failed_mutation_intent(
    intent: Option<&MutationIntent>,
    mutation_error: String,
) -> Result<(), String> {
    let Some(intent) = intent else {
        return Err(mutation_error);
    };
    let directory = intent.record_path.parent().ok_or_else(|| {
        format!(
            "recovery_required: mutation failed ({mutation_error}); intent has no journal parent"
        )
    })?;
    let (parent_snapshots, private_paths) = ACTIVE_TRANSACTION_WRITES.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|active| {
                let active = active.borrow();
                (
                    active.parent_snapshots.clone(),
                    active.private_paths.clone(),
                )
            })
            .unwrap_or_default()
    });
    match recover_transaction_mutation_intents_with_private_paths(
        directory,
        &parent_snapshots,
        &private_paths,
    ) {
        Ok(()) => Err(mutation_error),
        Err(recovery) => Err(format!(
            "recovery_required: mutation failed ({mutation_error}); immediate claim recovery failed: {recovery}"
        )),
    }
}

fn file_state(state: AnchoredPathState, path: &Path) -> Result<Option<FileBytes>, String> {
    match state {
        AnchoredPathState::Missing => Ok(None),
        AnchoredPathState::File { bytes, mode, .. } => Ok(Some(FileBytes { bytes, mode })),
        AnchoredPathState::Symlink { .. } => Err(format!(
            "refusing to treat symlink {} as a regular transaction file",
            path.display()
        )),
        AnchoredPathState::Directory { .. } => Err(format!(
            "refusing to treat directory {} as a transaction file",
            path.display()
        )),
        AnchoredPathState::Other { .. } => Err(format!(
            "unsupported transaction target type: {}",
            path.display()
        )),
    }
}

fn symlink_state(state: AnchoredPathState, path: &Path) -> Result<Option<PathBuf>, String> {
    match state {
        AnchoredPathState::Missing => Ok(None),
        AnchoredPathState::Symlink { target, .. } => Ok(Some(target)),
        _ => Err(format!(
            "refusing to treat non-symlink {} as a transaction link",
            path.display()
        )),
    }
}

#[cfg(unix)]
fn temp_entry_name(path: &Path, purpose: &str) -> std::ffi::OsString {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    std::ffi::OsString::from(format!(
        ".{file_name}.mux-{purpose}-{}-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(all(
    unix,
    any(target_vendor = "apple", target_os = "linux", target_os = "android")
))]
fn rename_entry_noreplace(
    parent: &OpenParent,
    old_name: &std::ffi::OsStr,
    new_name: &std::ffi::OsStr,
) -> Result<(), String> {
    use rustix::fs::{renameat_with, RenameFlags};
    renameat_with(
        &parent.directory,
        old_name,
        &parent.directory,
        new_name,
        RenameFlags::NOREPLACE,
    )
    .map_err(|error| format!("exclusive anchored rename failed: {error}"))
}

#[cfg(all(
    unix,
    not(any(target_vendor = "apple", target_os = "linux", target_os = "android"))
))]
fn rename_entry_noreplace(
    _parent: &OpenParent,
    _old_name: &std::ffi::OsStr,
    _new_name: &std::ffi::OsStr,
) -> Result<(), String> {
    Err("asset_target_unsafe: exclusive anchored rename is unsupported on this platform".into())
}

#[cfg(unix)]
fn sibling_path(parent: &ParentDirectorySnapshot, name: &std::ffi::OsStr) -> PathBuf {
    parent.parent_path.join(name)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn rewrite_regular_file_in_place(
    path: &Path,
    parent_snapshot: &ParentDirectorySnapshot,
    parent: &OpenParent,
    authoritative: &AnchoredPathState,
    expected: (&[u8], Option<u32>),
    content: &[u8],
    mode: Option<u32>,
    record_path: &Path,
) -> Result<(), String> {
    use rustix::fs::{fchmod, openat, Mode, OFlags, RawMode};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let expected_identity = match authoritative {
        AnchoredPathState::File { identity, .. } if identity.is_exact() => *identity,
        _ => {
            return Err(format!(
                "asset_target_unsafe: in-place rewrite requires an exact regular-file identity: {}",
                path.display()
            ));
        }
    };
    let desired = AnchoredPathState::File {
        bytes: Zeroizing::new(content.to_vec()),
        mode,
        identity: expected_identity,
    };
    let guard = temp_entry_name(path, "rewrite-guard");
    let intent = begin_mutation_intent(MutationIntentRequest {
        path,
        parent: parent_snapshot,
        mutation_parent_identity: open_parent_identity(parent)?,
        guard_name: &guard,
        temp_name: None,
        operation: MutationIntentOperation::Rewrite,
        expected: authoritative,
        desired: &desired,
    })?;
    let result = (|| {
        let name = target_name(path)?;
        let mut file = fs::File::from(
            openat(
                &parent.directory,
                name,
                OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| {
                format!(
                    "failed to open {} for in-place edit: {error}",
                    path.display()
                )
            })?,
        );
        let metadata = file
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        let opened_identity = PathIdentity {
            device: Some(metadata.dev()),
            inode: Some(metadata.ino()),
        };
        if opened_identity != expected_identity {
            return Err(format!(
                "asset_operation_stale: {} changed before the in-place edit",
                path.display()
            ));
        }
        let mut opened_bytes = Zeroizing::new(Vec::new());
        file.read_to_end(&mut opened_bytes).map_err(|error| {
            format!("failed to read {} before editing: {error}", path.display())
        })?;
        if opened_bytes.as_slice() != expected.0
            || expected
                .1
                .is_some_and(|expected_mode| metadata.permissions().mode() != expected_mode)
        {
            return Err(format!(
                "asset_operation_stale: {} changed before the in-place edit",
                path.display()
            ));
        }

        run_before_mutation_claim_hook(path);
        let current = read_path_state_from_parent(path, parent)?;
        if !anchored_states_match(authoritative, &current) {
            return Err(format!(
                "asset_operation_stale: {} changed at the in-place CAS boundary",
                path.display()
            ));
        }

        file.seek(SeekFrom::Start(0))
            .map_err(|error| format!("failed to seek {}: {error}", path.display()))?;
        file.set_len(0)
            .map_err(|error| format!("failed to truncate {}: {error}", path.display()))?;
        file.write_all(content)
            .map_err(|error| format!("failed to edit {}: {error}", path.display()))?;
        if let Some(mode) = mode {
            fchmod(
                &file,
                Mode::from_raw_mode(RawMode::try_from(mode & 0o7777).unwrap_or(0o600)),
            )
            .map_err(|error| {
                format!(
                    "failed to preserve permissions for {}: {error}",
                    path.display()
                )
            })?;
        }
        file.sync_all()
            .map_err(|error| format!("failed to sync {}: {error}", path.display()))?;

        let published = read_path_state_from_parent(path, parent)?;
        if semantic_path_state_fingerprint(&published)?
            != semantic_path_state_fingerprint(&desired)?
        {
            return Err(format!(
                "target_convergence_failed: {} changed while Arca was editing it",
                path.display()
            ));
        }
        verify_mutation_parent_is_current(
            &intent
                .as_ref()
                .expect("rewrite mutation intent remains active")
                .record,
        )?;
        let AnchoredPathState::File {
            bytes,
            mode,
            identity,
        } = published
        else {
            return Err(format!(
                "target_convergence_failed: {} is no longer a regular file",
                path.display()
            ));
        };
        record_transaction_file(record_path, &bytes, mode, identity)?;
        complete_mutation_intent(intent.as_ref())
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => recover_failed_mutation_intent(intent.as_ref(), error),
    }
}

#[cfg(unix)]
fn write_regular_file_anchored(
    path: &Path,
    parent_snapshot: &ParentDirectorySnapshot,
    expected: Option<(&[u8], Option<u32>)>,
    content: &[u8],
    mode: Option<u32>,
    record_path: &Path,
    purpose: &str,
    preserve_inode: bool,
    required_identity: Option<PathIdentity>,
) -> Result<(), String> {
    use rustix::fs::{fchmod, linkat, openat, unlinkat, AtFlags, Mode, OFlags, RawMode};
    use std::os::unix::fs::PermissionsExt;

    if target_parent(path) != parent_snapshot.parent_path {
        return Err(format!(
            "asset_target_unsafe: parent snapshot does not match {}",
            path.display()
        ));
    }
    let parent = open_parent_anchored(parent_snapshot, true)?
        .expect("create=true always opens the reviewed parent");
    let current = read_path_state_from_parent(path, &parent)?;
    if !file_bytes_match(file_state(current.clone(), path)?.as_ref(), expected) {
        if purpose == "rollback" {
            return Err(format!(
                "refusing to restore {}: file changed before rollback",
                path.display()
            ));
        }
        return Err(format!(
            "refusing to modify {}: file changed while Arca was preparing the update",
            path.display()
        ));
    }
    if required_identity.is_some_and(|identity| {
        !identity.is_exact()
            || !matches!(
                &current,
                AnchoredPathState::File {
                    identity: current_identity,
                    ..
                } if current_identity.is_exact() && *current_identity == identity
            )
    }) {
        return Err(format!(
            "refusing to modify {}: hash/mode/identity changed before rollback",
            path.display()
        ));
    }
    let authoritative = transaction_expected_state(record_path)?.unwrap_or(current.clone());
    if !anchored_states_match(&authoritative, &current) {
        return Err(format!(
            "asset_operation_stale: reviewed target changed before write preparation: {}",
            path.display()
        ));
    }

    if expected.is_some() && preserve_inode {
        return rewrite_regular_file_in_place(
            path,
            parent_snapshot,
            &parent,
            &authoritative,
            expected.expect("checked above"),
            content,
            mode,
            record_path,
        );
    }

    let name = target_name(path)?;
    let temp = temp_entry_name(path, purpose);
    let guard = if expected.is_none() {
        temp_entry_name(path, "create-claim")
    } else {
        temp_entry_name(path, "claim")
    };
    let operation = if expected.is_none() {
        MutationIntentOperation::Create
    } else {
        MutationIntentOperation::Replace
    };
    let planned_desired = AnchoredPathState::File {
        bytes: Zeroizing::new(content.to_vec()),
        mode,
        identity: PathIdentity::unknown(),
    };
    let mut intent = Some(prepare_mutation_intent(MutationIntentRequest {
        path,
        parent: parent_snapshot,
        mutation_parent_identity: open_parent_identity(&parent)?,
        guard_name: &guard,
        temp_name: Some(&temp),
        operation,
        expected: &authoritative,
        desired: &planned_desired,
    })?);
    let result = (|| {
        let mut file = fs::File::from(
            openat(
                &parent.directory,
                &temp,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from(if mode.is_some() { 0o600 } else { 0o666 }),
            )
            .map_err(|error| {
                format!(
                    "failed to create temporary file for {}: {error}",
                    path.display()
                )
            })?,
        );
        if let Some(mode) = mode {
            fchmod(
                &file,
                Mode::from_raw_mode(RawMode::try_from(mode & 0o7777).unwrap_or(0o600)),
            )
            .map_err(|error| {
                format!("failed to set permissions for {}: {error}", path.display())
            })?;
        }
        file.write_all(content)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync {}: {error}", path.display()))?;
        let written_metadata = file
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        let written_mode = Some(written_metadata.permissions().mode());
        use std::os::unix::fs::MetadataExt;
        let written_identity = PathIdentity {
            device: Some(written_metadata.dev()),
            inode: Some(written_metadata.ino()),
        };
        let desired = AnchoredPathState::File {
            bytes: Zeroizing::new(content.to_vec()),
            mode: written_mode,
            identity: written_identity,
        };
        parent.directory.sync_all().map_err(|error| {
            format!(
                "failed to sync prepared target parent {}: {error}",
                path.display()
            )
        })?;
        arm_mutation_intent(
            intent
                .as_mut()
                .expect("prepared mutation intent remains active"),
            &desired,
        )?;

        let current = read_path_state_from_parent(path, &parent)?;
        if !file_bytes_match(file_state(current.clone(), path)?.as_ref(), expected) {
            if purpose == "rollback" {
                return Err(format!(
                    "refusing to restore {}: file changed during rollback",
                    path.display()
                ));
            }
            return Err(format!(
                "refusing to modify {}: file changed while Arca was preparing the update",
                path.display()
            ));
        }
        if !anchored_states_match(&authoritative, &current) {
            return Err(format!(
                "asset_operation_stale: reviewed target changed before claim: {}",
                path.display()
            ));
        }
        if expected.is_none() {
            // Persist ownership of the prepared inode before publishing it.
            // Recovery can then distinguish it from a path created by another
            // process on either side of the atomic link operation.
            run_before_mutation_claim_hook(path);
            record_transaction_file(record_path, content, written_mode, written_identity)?;
            linkat(
                &parent.directory,
                &temp,
                &parent.directory,
                name,
                AtFlags::empty(),
            )
            .map_err(|error| {
                format!(
                    "refusing to create {} because the target changed: {error}",
                    path.display()
                )
            })?;
            parent
                .directory
                .sync_all()
                .map_err(|error| format!("failed to sync parent of {}: {error}", path.display()))?;
            let published = read_path_state_from_parent(path, &parent)?;
            if !anchored_states_match(&desired, &published) {
                return Err(format!(
                    "recovery_required: {} changed immediately after Arca published it",
                    path.display()
                ));
            }
            verify_mutation_parent_is_current(
                &intent
                    .as_ref()
                    .expect("prepared mutation intent remains active")
                    .record,
            )?;
            unlinkat(&parent.directory, &temp, AtFlags::empty())
                .map_err(|error| format!("failed to remove Arca temp entry: {error}"))?;
            parent
                .directory
                .sync_all()
                .map_err(|error| format!("failed to sync retired Arca temp entry: {error}"))?;
            complete_mutation_intent(intent.as_ref())?;
        } else {
            run_before_mutation_claim_hook(path);
            let claim = rename_entry_noreplace(&parent, name, &guard);
            parent
                .directory
                .sync_all()
                .map_err(|error| format!("failed to sync claimed target: {error}"))?;
            let guard_path = sibling_path(parent_snapshot, &guard);
            let target_after_claim = read_path_state_from_parent(path, &parent)?;
            let guard_after_claim = read_path_state_from_parent(&guard_path, &parent)?;
            if let Err(claim_error) = claim {
                if !(matches!(target_after_claim, AnchoredPathState::Missing)
                    && anchored_states_match(&authoritative, &guard_after_claim))
                {
                    if matches!(guard_after_claim, AnchoredPathState::Missing)
                        && anchored_states_match(&authoritative, &target_after_claim)
                    {
                        cleanup_active_mutation_temp(intent.as_ref(), parent_snapshot, &parent)?;
                        complete_mutation_intent(intent.as_ref())?;
                        return Err(format!(
                            "refusing to claim {} for replacement: {}",
                            path.display(),
                            claim_error
                        ));
                    }
                    return Err(format!(
                        "recovery_required: exclusive claim of {} had an ambiguous result",
                        path.display()
                    ));
                }
            }
            if !anchored_states_match(&authoritative, &guard_after_claim) {
                if rename_entry_noreplace(&parent, &guard, name).is_ok() {
                    parent
                        .directory
                        .sync_all()
                        .map_err(|error| error.to_string())?;
                    cleanup_active_mutation_temp(intent.as_ref(), parent_snapshot, &parent)?;
                    complete_mutation_intent(intent.as_ref())?;
                    return Err(format!(
                        "asset_operation_stale: {} changed at the claim boundary",
                        path.display()
                    ));
                }
                return Err(format!(
                    "recovery_required: an unknown entry was safely retained while claiming {}",
                    path.display()
                ));
            }
            if let Err(error) = rename_entry_noreplace(&parent, &temp, name) {
                if rename_entry_noreplace(&parent, &guard, name).is_ok() {
                    parent
                        .directory
                        .sync_all()
                        .map_err(|sync| sync.to_string())?;
                    cleanup_active_mutation_temp(intent.as_ref(), parent_snapshot, &parent)?;
                    complete_mutation_intent(intent.as_ref())?;
                    return Err(format!(
                        "refusing to publish {} because the target reappeared: {error}",
                        path.display()
                    ));
                }
                return Err(format!(
                    "recovery_required: {} reappeared after its reviewed entry was retained",
                    path.display()
                ));
            }
            parent
                .directory
                .sync_all()
                .map_err(|error| format!("failed to sync published target: {error}"))?;
            record_transaction_file(record_path, content, written_mode, written_identity)?;
            let published = read_path_state_from_parent(path, &parent)?;
            if !anchored_states_match(&desired, &published) {
                return Err(format!(
                    "recovery_required: {} changed before Arca retired its claim guard",
                    path.display()
                ));
            }
            verify_mutation_parent_is_current(
                &intent
                    .as_ref()
                    .expect("prepared mutation intent remains active")
                    .record,
            )?;
            unlinkat(&parent.directory, &guard, AtFlags::empty())
                .map_err(|error| format!("failed to remove verified claim guard: {error}"))?;
            parent
                .directory
                .sync_all()
                .map_err(|error| format!("failed to sync retired claim guard: {error}"))?;
            complete_mutation_intent(intent.as_ref())?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => recover_failed_mutation_intent(intent.as_ref(), error),
    }
}

#[cfg(unix)]
fn remove_regular_file_anchored(
    path: &Path,
    parent_snapshot: &ParentDirectorySnapshot,
    expected: (&[u8], Option<u32>),
    required_identity: Option<PathIdentity>,
) -> Result<(), String> {
    use rustix::fs::{unlinkat, AtFlags};

    let Some(parent) = open_parent_anchored(parent_snapshot, false)? else {
        return Err(format!(
            "refusing to remove {} during rollback: parent changed",
            path.display()
        ));
    };
    let current = read_path_state_from_parent(path, &parent)?;
    if !file_bytes_match(file_state(current.clone(), path)?.as_ref(), Some(expected)) {
        return Err(format!(
            "refusing to remove {} during rollback: file changed",
            path.display()
        ));
    }
    if required_identity.is_some_and(|identity| {
        !identity.is_exact()
            || !matches!(
                &current,
                AnchoredPathState::File {
                    identity: current_identity,
                    ..
                } if current_identity.is_exact() && *current_identity == identity
            )
    }) {
        return Err(format!(
            "refusing to remove {} during rollback: hash/mode/identity changed",
            path.display()
        ));
    }
    let authoritative = transaction_expected_state(path)?.unwrap_or(current.clone());
    if !anchored_states_match(&authoritative, &current) {
        return Err(format!(
            "asset_operation_stale: reviewed target changed before removal claim: {}",
            path.display()
        ));
    }
    let name = target_name(path)?;
    let guard = temp_entry_name(path, "remove-claim");
    let intent = begin_mutation_intent(MutationIntentRequest {
        path,
        parent: parent_snapshot,
        mutation_parent_identity: open_parent_identity(&parent)?,
        guard_name: &guard,
        temp_name: None,
        operation: MutationIntentOperation::Remove,
        expected: &authoritative,
        desired: &AnchoredPathState::Missing,
    })?;
    run_before_mutation_claim_hook(path);
    let claim = rename_entry_noreplace(&parent, name, &guard);
    parent
        .directory
        .sync_all()
        .map_err(|error| error.to_string())?;
    let guard_path = sibling_path(parent_snapshot, &guard);
    let target_after_claim = read_path_state_from_parent(path, &parent)?;
    let guard_after_claim = read_path_state_from_parent(&guard_path, &parent)?;
    if let Err(claim_error) = claim {
        if !(matches!(target_after_claim, AnchoredPathState::Missing)
            && anchored_states_match(&authoritative, &guard_after_claim))
        {
            if matches!(guard_after_claim, AnchoredPathState::Missing)
                && anchored_states_match(&authoritative, &target_after_claim)
            {
                complete_mutation_intent(intent.as_ref())?;
                return Err(format!(
                    "refusing to claim {} for removal: {}",
                    path.display(),
                    claim_error
                ));
            }
            return Err(format!(
                "recovery_required: exclusive removal claim of {} had an ambiguous result",
                path.display()
            ));
        }
    }
    if !anchored_states_match(&authoritative, &guard_after_claim) {
        if rename_entry_noreplace(&parent, &guard, name).is_ok() {
            parent
                .directory
                .sync_all()
                .map_err(|error| error.to_string())?;
            complete_mutation_intent(intent.as_ref())?;
            return Err(format!(
                "asset_operation_stale: {} changed at the removal claim boundary",
                path.display()
            ));
        }
        return Err(format!(
            "recovery_required: an unknown entry was retained while removing {}",
            path.display()
        ));
    }
    if !matches!(target_after_claim, AnchoredPathState::Missing) {
        return Err(format!(
            "recovery_required: {} reappeared while its reviewed entry was retained",
            path.display()
        ));
    }
    record_transaction_removal(path)?;
    verify_mutation_parent_is_current(
        &intent
            .as_ref()
            .expect("removal mutation intent remains active")
            .record,
    )?;
    unlinkat(&parent.directory, &guard, AtFlags::empty())
        .map_err(|error| format!("failed to remove verified removal guard: {error}"))?;
    parent
        .directory
        .sync_all()
        .map_err(|error| format!("failed to sync parent of {}: {error}", path.display()))?;
    complete_mutation_intent(intent.as_ref())
}

#[cfg(unix)]
fn remove_symlink_anchored(
    path: &Path,
    parent_snapshot: &ParentDirectorySnapshot,
    expected: &Path,
) -> Result<(), String> {
    use rustix::fs::{unlinkat, AtFlags};

    let Some(parent) = open_parent_anchored(parent_snapshot, false)? else {
        return Err(format!(
            "refusing to remove symlink {} during rollback: parent changed",
            path.display()
        ));
    };
    let current = read_path_state_from_parent(path, &parent)?;
    if symlink_state(current.clone(), path)?.as_deref() != Some(expected) {
        return Err(format!(
            "refusing to remove symlink {} during rollback: link changed",
            path.display()
        ));
    }
    let authoritative = transaction_expected_state(path)?.unwrap_or(current.clone());
    if !anchored_states_match(&authoritative, &current) {
        return Err(format!(
            "asset_operation_stale: reviewed symlink changed before removal claim: {}",
            path.display()
        ));
    }
    let name = target_name(path)?;
    let guard = temp_entry_name(path, "remove-link-claim");
    let intent = begin_mutation_intent(MutationIntentRequest {
        path,
        parent: parent_snapshot,
        mutation_parent_identity: open_parent_identity(&parent)?,
        guard_name: &guard,
        temp_name: None,
        operation: MutationIntentOperation::Remove,
        expected: &authoritative,
        desired: &AnchoredPathState::Missing,
    })?;
    run_before_mutation_claim_hook(path);
    let claim = rename_entry_noreplace(&parent, name, &guard);
    parent
        .directory
        .sync_all()
        .map_err(|error| format!("failed to sync symlink removal claim: {error}"))?;
    let guard_path = sibling_path(parent_snapshot, &guard);
    let target_after_claim = read_path_state_from_parent(path, &parent)?;
    let guard_after_claim = read_path_state_from_parent(&guard_path, &parent)?;
    if let Err(claim_error) = claim {
        if !(matches!(target_after_claim, AnchoredPathState::Missing)
            && anchored_states_match(&authoritative, &guard_after_claim))
        {
            if matches!(guard_after_claim, AnchoredPathState::Missing)
                && anchored_states_match(&authoritative, &target_after_claim)
            {
                complete_mutation_intent(intent.as_ref())?;
                return Err(format!(
                    "refusing to claim symlink {} for removal: {}",
                    path.display(),
                    claim_error
                ));
            }
            return Err(format!(
                "recovery_required: exclusive symlink removal claim of {} had an ambiguous result",
                path.display()
            ));
        }
    }
    if !anchored_states_match(&authoritative, &guard_after_claim) {
        if rename_entry_noreplace(&parent, &guard, name).is_ok() {
            parent
                .directory
                .sync_all()
                .map_err(|error| error.to_string())?;
            complete_mutation_intent(intent.as_ref())?;
            return Err(format!(
                "asset_operation_stale: symlink {} changed at the removal claim boundary",
                path.display()
            ));
        }
        return Err(format!(
            "recovery_required: an unknown symlink was retained while removing {}",
            path.display()
        ));
    }
    if !matches!(target_after_claim, AnchoredPathState::Missing) {
        return Err(format!(
            "recovery_required: {} reappeared while its reviewed symlink was retained",
            path.display()
        ));
    }
    record_transaction_removal(path)?;
    verify_mutation_parent_is_current(
        &intent
            .as_ref()
            .expect("symlink removal intent remains active")
            .record,
    )?;
    unlinkat(&parent.directory, &guard, AtFlags::empty())
        .map_err(|error| format!("failed to remove verified symlink guard: {error}"))?;
    parent
        .directory
        .sync_all()
        .map_err(|error| format!("failed to sync parent of {}: {error}", path.display()))?;
    complete_mutation_intent(intent.as_ref())
}

#[cfg(unix)]
fn write_symlink_anchored(
    path: &Path,
    parent_snapshot: &ParentDirectorySnapshot,
    expected: Option<&Path>,
    target: &Path,
) -> Result<(), String> {
    use rustix::fs::{symlinkat, unlinkat, AtFlags};

    let parent = open_parent_anchored(parent_snapshot, true)?
        .expect("create=true always opens the reviewed parent");
    let current = read_path_state_from_parent(path, &parent)?;
    if symlink_state(current.clone(), path)?.as_deref() != expected {
        return Err(format!(
            "refusing to restore symlink {}: link changed before rollback",
            path.display()
        ));
    }
    let authoritative = transaction_expected_state(path)?.unwrap_or(current.clone());
    if !anchored_states_match(&authoritative, &current) {
        return Err(format!(
            "asset_operation_stale: reviewed symlink changed before write: {}",
            path.display()
        ));
    }
    let name = target_name(path)?;
    let temp = temp_entry_name(path, "rollback-link");
    let guard = if expected.is_none() {
        temp_entry_name(path, "create-link-claim")
    } else {
        temp_entry_name(path, "link-claim")
    };
    let operation = if expected.is_none() {
        MutationIntentOperation::Create
    } else {
        MutationIntentOperation::Replace
    };
    let planned_desired = AnchoredPathState::Symlink {
        target: target.to_path_buf(),
        identity: PathIdentity::unknown(),
    };
    let mut intent = Some(prepare_mutation_intent(MutationIntentRequest {
        path,
        parent: parent_snapshot,
        mutation_parent_identity: open_parent_identity(&parent)?,
        guard_name: &guard,
        temp_name: Some(&temp),
        operation,
        expected: &authoritative,
        desired: &planned_desired,
    })?);
    let result = (|| {
        symlinkat(target, &parent.directory, &temp).map_err(|error| error.to_string())?;
        parent
            .directory
            .sync_all()
            .map_err(|error| format!("failed to sync prepared symlink: {error}"))?;
        let temp_path = sibling_path(parent_snapshot, &temp);
        let desired = read_path_state_from_parent(&temp_path, &parent)?;
        let desired_identity = match &desired {
            AnchoredPathState::Symlink {
                target: actual,
                identity,
            } if actual == target => *identity,
            _ => {
                return Err(
                    "asset_target_unsafe: prepared symlink changed before publication".into(),
                )
            }
        };
        arm_mutation_intent(
            intent
                .as_mut()
                .expect("prepared symlink intent remains active"),
            &desired,
        )?;
        if expected.is_none() {
            run_before_mutation_claim_hook(path);
            record_transaction_symlink_state(path, target, desired_identity)?;
            rename_entry_noreplace(&parent, &temp, name).map_err(|error| {
                format!(
                    "refusing to create symlink {} because the target changed: {error}",
                    path.display()
                )
            })?;
            parent
                .directory
                .sync_all()
                .map_err(|error| format!("failed to sync published symlink: {error}"))?;
            let published = read_path_state_from_parent(path, &parent)?;
            if !anchored_states_match(&desired, &published) {
                return Err(format!(
                    "recovery_required: symlink {} changed immediately after publication",
                    path.display()
                ));
            }
            complete_mutation_intent(intent.as_ref())?;
            return Ok(());
        }

        run_before_mutation_claim_hook(path);
        let claim = rename_entry_noreplace(&parent, name, &guard);
        parent
            .directory
            .sync_all()
            .map_err(|error| format!("failed to sync symlink claim: {error}"))?;
        let guard_path = sibling_path(parent_snapshot, &guard);
        let target_after_claim = read_path_state_from_parent(path, &parent)?;
        let guard_after_claim = read_path_state_from_parent(&guard_path, &parent)?;
        if let Err(claim_error) = claim {
            if !(matches!(target_after_claim, AnchoredPathState::Missing)
                && anchored_states_match(&authoritative, &guard_after_claim))
            {
                if matches!(guard_after_claim, AnchoredPathState::Missing)
                    && anchored_states_match(&authoritative, &target_after_claim)
                {
                    cleanup_active_mutation_temp(intent.as_ref(), parent_snapshot, &parent)?;
                    complete_mutation_intent(intent.as_ref())?;
                    return Err(format!(
                        "refusing to claim symlink {} for replacement: {}",
                        path.display(),
                        claim_error
                    ));
                }
                return Err(format!(
                    "recovery_required: exclusive symlink claim of {} had an ambiguous result",
                    path.display()
                ));
            }
        }
        if !anchored_states_match(&authoritative, &guard_after_claim) {
            if rename_entry_noreplace(&parent, &guard, name).is_ok() {
                parent
                    .directory
                    .sync_all()
                    .map_err(|error| error.to_string())?;
                cleanup_active_mutation_temp(intent.as_ref(), parent_snapshot, &parent)?;
                complete_mutation_intent(intent.as_ref())?;
                return Err(format!(
                    "asset_operation_stale: symlink {} changed at the claim boundary",
                    path.display()
                ));
            }
            return Err(format!(
                "recovery_required: an unknown symlink was retained while claiming {}",
                path.display()
            ));
        }
        if let Err(error) = rename_entry_noreplace(&parent, &temp, name) {
            if rename_entry_noreplace(&parent, &guard, name).is_ok() {
                parent
                    .directory
                    .sync_all()
                    .map_err(|sync| sync.to_string())?;
                cleanup_active_mutation_temp(intent.as_ref(), parent_snapshot, &parent)?;
                complete_mutation_intent(intent.as_ref())?;
                return Err(format!(
                    "refusing to publish symlink {} because the target reappeared: {error}",
                    path.display()
                ));
            }
            return Err(format!(
                "recovery_required: {} reappeared after its reviewed symlink was retained",
                path.display()
            ));
        }
        parent
            .directory
            .sync_all()
            .map_err(|error| format!("failed to sync published symlink: {error}"))?;
        record_transaction_symlink_state(path, target, desired_identity)?;
        let published = read_path_state_from_parent(path, &parent)?;
        if !anchored_states_match(&desired, &published) {
            return Err(format!(
                "recovery_required: symlink {} changed before Arca retired its claim guard",
                path.display()
            ));
        }
        verify_mutation_parent_is_current(
            &intent
                .as_ref()
                .expect("prepared symlink intent remains active")
                .record,
        )?;
        unlinkat(&parent.directory, &guard, AtFlags::empty())
            .map_err(|error| format!("failed to remove verified symlink claim: {error}"))?;
        parent
            .directory
            .sync_all()
            .map_err(|error| format!("failed to sync retired symlink claim: {error}"))?;
        complete_mutation_intent(intent.as_ref())?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => recover_failed_mutation_intent(intent.as_ref(), error),
    }
}

/// Atomically replace a text file only if it still contains the text that was
/// parsed by the caller. This avoids truncated files and refuses to overwrite a
/// concurrent edit made by the Agent or the user.
pub(crate) fn write_if_unchanged(
    path: &Path,
    expected: Option<&str>,
    content: &str,
) -> Result<(), String> {
    write_if_unchanged_impl(path, expected, content, false)
}

/// Settings may contain environment variables, request headers, and disabled
/// entry snapshots. Keep that Arca-owned store private while retaining all other
/// optimistic-concurrency and atomic-replace guarantees.
pub(crate) fn write_private_if_unchanged(
    path: &Path,
    expected: Option<&str>,
    content: &str,
) -> Result<(), String> {
    write_if_unchanged_impl(path, expected, content, true)
}

/// Remove a file only when its current contents still match what the caller
/// prepared against. Used to roll back a newly-created file in a multi-file
/// model configuration transaction.
pub(crate) fn remove_if_unchanged(path: &Path, expected: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        let parent = reviewed_parent_snapshot(path, path)?;
        let current_state = read_path_state_anchored(path, &parent)?;
        require_transaction_expected_state(path, &current_state)?;
        let current = file_state(current_state, path)?;
        let Some(current) = current else {
            return Err(format!(
                "refusing to remove {} during rollback: file changed after Arca wrote it",
                path.display()
            ));
        };
        if current.bytes.as_slice() != expected.as_bytes() {
            return Err(format!(
                "refusing to remove {} during rollback: file changed after Arca wrote it",
                path.display()
            ));
        }
        remove_regular_file_anchored(path, &parent, (expected.as_bytes(), current.mode), None)
    }

    #[cfg(not(unix))]
    {
        if read_optional(path)?.as_deref() != Some(expected) {
            return Err(format!(
                "refusing to remove {} during rollback: file changed after Arca wrote it",
                path.display()
            ));
        }
        fs::remove_file(path)
            .map_err(|error| format!("failed to remove {}: {}", path.display(), error))?;
        record_transaction_removal(path)
    }
}

#[derive(PartialEq, Eq)]
struct FileBytes {
    bytes: Zeroizing<Vec<u8>>,
    mode: Option<u32>,
}

impl std::fmt::Debug for FileBytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileBytes")
            .field("bytes", &"<redacted>")
            .field("mode", &self.mode)
            .finish()
    }
}

#[cfg(not(unix))]
fn read_optional_bytes(path: &Path) -> Result<Option<FileBytes>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refusing to treat symlink {} as a regular transaction file",
            path.display()
        )),
        Ok(metadata) if metadata.is_dir() => Err(format!(
            "refusing to treat directory {} as a transaction file",
            path.display()
        )),
        Ok(metadata) if metadata.is_file() => {
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                Some(metadata.permissions().mode())
            };
            #[cfg(not(unix))]
            let mode = None;
            fs::read(path)
                .map(|bytes| {
                    Some(FileBytes {
                        bytes: Zeroizing::new(bytes),
                        mode,
                    })
                })
                .map_err(|error| format!("failed to read {}: {error}", path.display()))
        }
        Ok(_) => Err(format!(
            "unsupported transaction target type: {}",
            path.display()
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to inspect {}: {error}", path.display())),
    }
}

/// Atomically restore a regular file only while its bytes still match the state
/// captured immediately before rollback. This deliberately does not follow
/// symlinks or replace directories.
#[cfg(test)]
pub(crate) fn write_bytes_if_unchanged(
    path: &Path,
    expected: Option<(&[u8], Option<u32>)>,
    content: &[u8],
    mode: Option<u32>,
) -> Result<(), String> {
    let parent = capture_parent_directory(path)?;
    write_bytes_if_unchanged_in_parent(path, &parent, expected, content, mode)
}

pub(crate) fn write_bytes_if_unchanged_in_parent(
    path: &Path,
    parent: &ParentDirectorySnapshot,
    expected: Option<(&[u8], Option<u32>)>,
    content: &[u8],
    mode: Option<u32>,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        write_regular_file_anchored(
            path, parent, expected, content, mode, path, "rollback", true, None,
        )
    }

    #[cfg(not(unix))]
    {
        if !file_bytes_match(read_optional_bytes(path)?.as_ref(), expected) {
            return Err(format!(
                "refusing to restore {}: file changed before rollback",
                path.display()
            ));
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config");
        let temp = parent.join(format!(
            ".{}.mux-rollback-{}-{}-{}.tmp",
            file_name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|error| format!("failed to create {}: {error}", temp.display()))?;
            #[cfg(unix)]
            if let Some(mode) = mode {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(mode))
                    .map_err(|error| error.to_string())?;
            }
            #[cfg(not(unix))]
            let _ = mode;
            file.write_all(content).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            if !file_bytes_match(read_optional_bytes(path)?.as_ref(), expected) {
                return Err(format!(
                    "refusing to restore {}: file changed during rollback",
                    path.display()
                ));
            }
            if expected.is_none() {
                // A hard link publishes a fully-written same-filesystem temp without
                // replacing a path that appeared after the final CAS check.
                fs::hard_link(&temp, path).map_err(|error| {
                    format!(
                        "refusing to create {} during rollback: {error}",
                        path.display()
                    )
                })?;
                fs::remove_file(&temp).map_err(|error| error.to_string())
            } else {
                fs::rename(&temp, path).map_err(|error| {
                    format!("failed to atomically restore {}: {error}", path.display())
                })
            }
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

/// Remove a rollback-created regular file only if its bytes still match the
/// state captured after the failed operation.
pub(crate) fn remove_bytes_if_unchanged_in_parent(
    path: &Path,
    parent: &ParentDirectorySnapshot,
    expected: &[u8],
    expected_mode: Option<u32>,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        remove_regular_file_anchored(path, parent, (expected, expected_mode), None)
    }

    #[cfg(not(unix))]
    {
        let _ = parent;
        let expected = Some((expected, expected_mode));
        if !file_bytes_match(read_optional_bytes(path)?.as_ref(), expected) {
            return Err(format!(
                "refusing to remove {} during rollback: file changed",
                path.display()
            ));
        }
        // Recheck immediately before unlinking. Arca writers are serialized by the
        // settings lock; this second comparison protects against ordinary external
        // edits during rollback preparation.
        if !file_bytes_match(read_optional_bytes(path)?.as_ref(), expected) {
            return Err(format!(
                "refusing to remove {} during rollback: file changed",
                path.display()
            ));
        }
        fs::remove_file(path)
            .map_err(|error| format!("failed to remove {}: {error}", path.display()))
    }
}

pub(crate) fn write_bytes_if_hash_unchanged_in_parent(
    path: &Path,
    parent: &ParentDirectorySnapshot,
    expected: Option<(&str, Option<u32>, PathIdentity)>,
    content: &[u8],
    mode: Option<u32>,
) -> Result<(), String> {
    let Some((expected_hash, expected_mode, expected_identity)) = expected else {
        return write_bytes_if_unchanged_in_parent(path, parent, None, content, mode);
    };
    let mut current = private_hash_guarded_file(
        path,
        parent,
        expected_hash,
        expected_mode,
        expected_identity,
    )?;
    #[cfg(unix)]
    let result = {
        write_regular_file_anchored(
            path,
            parent,
            Some((&current.bytes, current.mode)),
            content,
            mode,
            path,
            "rollback",
            true,
            Some(expected_identity),
        )
    };
    #[cfg(not(unix))]
    let result = {
        let _ = (content, mode);
        Err(format!(
            "refusing to restore {}: exact hash/mode/identity rollback is unsupported",
            path.display()
        ))
    };
    current.bytes.zeroize();
    result
}

pub(crate) fn remove_bytes_if_hash_unchanged_in_parent(
    path: &Path,
    parent: &ParentDirectorySnapshot,
    expected_hash: &str,
    expected_mode: Option<u32>,
    expected_identity: PathIdentity,
) -> Result<(), String> {
    let mut current = private_hash_guarded_file(
        path,
        parent,
        expected_hash,
        expected_mode,
        expected_identity,
    )?;
    #[cfg(unix)]
    let result = {
        remove_regular_file_anchored(
            path,
            parent,
            (&current.bytes, current.mode),
            Some(expected_identity),
        )
    };
    #[cfg(not(unix))]
    let result = {
        Err(format!(
            "refusing to remove {}: exact hash/mode/identity rollback is unsupported",
            path.display()
        ))
    };
    current.bytes.zeroize();
    result
}

fn private_hash_guarded_file(
    path: &Path,
    parent: &ParentDirectorySnapshot,
    expected_hash: &str,
    expected_mode: Option<u32>,
    expected_identity: PathIdentity,
) -> Result<FileBytes, String> {
    transaction_private_file_state_from_hash(
        expected_hash.to_string(),
        expected_mode,
        expected_identity,
    )
    .map_err(|_| {
        format!(
            "refusing to access {} during rollback: invalid hash/mode/identity evidence",
            path.display()
        )
    })?;
    let mut current = read_path_state_anchored(path, parent)?;
    if !private_transaction_state_matches_anchored(
        expected_hash,
        expected_mode,
        expected_identity,
        &current,
    ) {
        if let AnchoredPathState::File { bytes, .. } = &mut current {
            bytes.zeroize();
        }
        return Err(format!(
            "refusing to access {} during rollback: hash/mode/identity changed",
            path.display()
        ));
    }
    let AnchoredPathState::File { bytes, mode, .. } = current else {
        unreachable!("private state matcher only accepts regular files");
    };
    Ok(FileBytes { bytes, mode })
}

fn file_bytes_match(actual: Option<&FileBytes>, expected: Option<(&[u8], Option<u32>)>) -> bool {
    match (actual, expected) {
        (None, None) => true,
        (Some(actual), Some((bytes, mode))) => {
            actual.bytes.as_slice() == bytes && actual.mode == mode
        }
        _ => false,
    }
}

#[cfg(not(unix))]
fn read_optional_symlink(path: &Path) -> Result<Option<PathBuf>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::read_link(path)
            .map(Some)
            .map_err(|error| format!("failed to read symlink {}: {error}", path.display())),
        Ok(_) => Err(format!(
            "refusing to treat non-symlink {} as a transaction link",
            path.display()
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to inspect {}: {error}", path.display())),
    }
}

pub(crate) fn remove_symlink_if_unchanged_in_parent(
    path: &Path,
    parent: &ParentDirectorySnapshot,
    expected: &Path,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        remove_symlink_anchored(path, parent, expected)
    }

    #[cfg(not(unix))]
    {
        let _ = parent;
        if read_optional_symlink(path)?.as_deref() != Some(expected) {
            return Err(format!(
                "refusing to remove symlink {} during rollback: link changed",
                path.display()
            ));
        }
        if read_optional_symlink(path)?.as_deref() != Some(expected) {
            return Err(format!(
                "refusing to remove symlink {} during rollback: link changed",
                path.display()
            ));
        }
        fs::remove_file(path)
            .map_err(|error| format!("failed to remove symlink {}: {error}", path.display()))
    }
}

pub(crate) fn write_symlink_if_unchanged_in_parent(
    path: &Path,
    parent_snapshot: &ParentDirectorySnapshot,
    expected: Option<&Path>,
    target: &Path,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        write_symlink_anchored(path, parent_snapshot, expected, target)
    }

    #[cfg(not(unix))]
    {
        let _ = parent_snapshot;
        if read_optional_symlink(path)?.as_deref() != expected {
            return Err(format!(
                "refusing to restore symlink {}: link changed before rollback",
                path.display()
            ));
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        if expected.is_none() {
            #[cfg(unix)]
            return std::os::unix::fs::symlink(target, path)
                .map_err(|error| format!("failed to restore symlink {}: {error}", path.display()));
            #[cfg(windows)]
            return std::os::windows::fs::symlink_dir(target, path)
                .map_err(|error| format!("failed to restore symlink {}: {error}", path.display()));
        }

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("link");
        let temp = parent.join(format!(
            ".{}.mux-rollback-link-{}-{}-{}.tmp",
            file_name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &temp).map_err(|error| error.to_string())?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(target, &temp).map_err(|error| error.to_string())?;
        let result = (|| {
            if read_optional_symlink(path)?.as_deref() != expected {
                return Err(format!(
                    "refusing to restore symlink {}: link changed during rollback",
                    path.display()
                ));
            }
            fs::rename(&temp, path)
                .map_err(|error| format!("failed to restore symlink {}: {error}", path.display()))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

/// Create a forward-transaction symlink only below the parent directory that
/// was captured with the reviewed operation. The target must still be missing;
/// a path-based parent lookup is never used for a tracked write.
pub(crate) fn create_transaction_symlink_if_missing(
    path: &Path,
    target: &Path,
) -> Result<(), String> {
    let parent = transaction_parent_snapshot(path).ok_or_else(|| {
        format!(
            "asset_target_unsafe: no reviewed parent snapshot for {}",
            path.display()
        )
    })?;
    let current = read_path_state_anchored(path, &parent)?;
    require_transaction_expected_state(path, &current)?;
    if current != AnchoredPathState::Missing {
        return Err(format!(
            "asset_operation_stale: symlink destination changed before write: {}",
            path.display()
        ));
    }
    write_symlink_if_unchanged_in_parent(path, &parent, None, target)
}

/// Apply one reviewed forward-transaction symlink state under the same durable
/// intent and exact-inode evidence protocol used by file writes. Callers must
/// not supplement this with a later path-based ownership record: publication,
/// evidence, and claim retirement are one indivisible protocol here.
pub(crate) fn set_transaction_symlink(
    path: &Path,
    desired_target: Option<&Path>,
) -> Result<bool, String> {
    let parent = transaction_parent_snapshot(path).ok_or_else(|| {
        format!(
            "asset_target_unsafe: no reviewed parent snapshot for {}",
            path.display()
        )
    })?;
    let current = read_path_state_anchored(path, &parent)?;
    require_transaction_expected_state(path, &current)?;
    match (current, desired_target) {
        (AnchoredPathState::Missing, None) => Ok(false),
        (AnchoredPathState::Missing, Some(target)) => {
            write_symlink_if_unchanged_in_parent(path, &parent, None, target)?;
            Ok(true)
        }
        (AnchoredPathState::Symlink { target, .. }, Some(desired)) if target == desired => {
            Ok(false)
        }
        (AnchoredPathState::Symlink { target, .. }, Some(desired)) => {
            write_symlink_if_unchanged_in_parent(path, &parent, Some(&target), desired)?;
            Ok(true)
        }
        (AnchoredPathState::Symlink { target, .. }, None) => {
            remove_symlink_if_unchanged_in_parent(path, &parent, &target)?;
            Ok(true)
        }
        (AnchoredPathState::File { .. }, _)
        | (AnchoredPathState::Directory { .. }, _)
        | (AnchoredPathState::Other { .. }, _) => Err(format!(
            "asset_operation_stale: refusing to replace a non-symlink Skill target: {}",
            path.display()
        )),
    }
}

fn write_if_unchanged_impl(
    path: &Path,
    expected: Option<&str>,
    content: &str,
    private: bool,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let destination = resolve_destination(path)?;
        let parent_snapshot = reviewed_parent_snapshot(path, &destination)?;
        let current_state = read_path_state_anchored(&destination, &parent_snapshot)?;
        require_transaction_expected_state(path, &current_state)?;
        let current = file_state(current_state, &destination)?;
        let current_text = current
            .as_ref()
            .map(|file| std::str::from_utf8(&file.bytes))
            .transpose()
            .map_err(|error| format!("failed to read {} as UTF-8: {error}", path.display()))?;
        if current_text != expected {
            return Err(format!(
                "refusing to modify {}: file changed while Arca was preparing the update",
                path.display()
            ));
        }
        let expected_bytes = expected.map(|value| {
            (
                value.as_bytes(),
                current.as_ref().and_then(|file| file.mode),
            )
        });
        let mode = if private {
            Some(fs::Permissions::from_mode(0o600).mode())
        } else {
            current.as_ref().and_then(|file| file.mode)
        };
        write_regular_file_anchored(
            &destination,
            &parent_snapshot,
            expected_bytes,
            content.as_bytes(),
            mode,
            &destination,
            "write",
            !private,
            None,
        )?;
        if resolve_destination(path)? != destination {
            return Err(format!(
                "refusing to modify {}: symlink target changed while Arca was preparing the update",
                path.display()
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        if read_optional(path)?.as_deref() != expected {
            return Err(format!(
                "refusing to modify {}: file changed while Arca was preparing the update",
                path.display()
            ));
        }

        let destination = resolve_destination(path)?;
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let permissions = fs::metadata(&destination)
            .ok()
            .map(|metadata| metadata.permissions());
        let _ = private;
        let file_name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config");
        let temp = parent.join(format!(
            ".{}.mux-{}-{}-{}.tmp",
            file_name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));

        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|error| format!("failed to create {}: {}", temp.display(), error))?;
            if let Some(permissions) = permissions {
                file.set_permissions(permissions)
                    .map_err(|error| error.to_string())?;
            }
            file.write_all(content.as_bytes())
                .map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;

            if read_optional(path)?.as_deref() != expected {
                return Err(format!(
                    "refusing to modify {}: file changed while Arca was preparing the update",
                    path.display()
                ));
            }
            if resolve_destination(path)? != destination {
                return Err(format!(
                    "refusing to modify {}: symlink target changed while Arca was preparing the update",
                    path.display()
                ));
            }
            fs::rename(&temp, &destination).map_err(|error| {
                format!(
                    "failed to atomically replace {}: {}",
                    destination.display(),
                    error
                )
            })?;
            record_transaction_file(
                &destination,
                content.as_bytes(),
                file_mode(&destination)?,
                PathIdentity::unknown(),
            )
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

pub(crate) fn write_if_unchanged_with_settings_lock(
    path: &Path,
    expected: Option<&str>,
    content: &str,
) -> Result<(), String> {
    let _lock = acquire_settings_lock(path)?;
    write_if_unchanged(path, expected, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(format!("arca-safe-write-{}-{}", name, std::process::id()))
    }

    #[test]
    fn refuses_to_replace_a_concurrent_edit() {
        let path = temp_file("concurrent");
        fs::write(&path, "newer").unwrap();
        let result = write_if_unchanged(&path, Some("older"), "arca");
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "newer");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn creates_and_replaces_files() {
        let _home = crate::testenv::TestHome::new("safe-write-create-replace");
        let path = temp_file("replace");
        let _ = fs::remove_file(&path);
        write_if_unchanged(&path, None, "first").unwrap();
        write_if_unchanged(&path, Some("first"), "second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn settings_lock_is_released_after_write() {
        let _home = crate::testenv::TestHome::new("safe-write-settings-lock");
        let path = temp_file("lock");
        let lock_path = append_suffix(&path, ".lockfile");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&lock_path);
        write_if_unchanged_with_settings_lock(&path, None, "locked").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "locked");
        assert!(lock_path.is_file());
        let reacquired = acquire_settings_lock(&path).unwrap();
        drop(reacquired);
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(lock_path);
    }

    #[cfg(unix)]
    #[test]
    fn settings_lock_rejects_a_symlinked_parent_without_writing_through_it() {
        use std::os::unix::fs::symlink;

        let temporary_root = fs::canonicalize(std::env::temp_dir()).unwrap();
        let base = temporary_root.join(format!(
            "arca-lock-parent-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let outside = temporary_root.join(format!(
            "arca-lock-outside-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&base).unwrap();
        fs::create_dir(&outside).unwrap();
        symlink(&outside, base.join("locks")).unwrap();
        let subject = base.join("locks/application-mutation");

        let Err(error) = acquire_settings_lock(&subject) else {
            panic!("symlinked settings lock parent was accepted");
        };

        assert!(error.contains("unsafe"), "{error}");
        assert!(!outside.join("application-mutation.lockfile").exists());
        fs::remove_file(base.join("locks")).unwrap();
        fs::remove_dir(base).unwrap();
        fs::remove_dir(outside).unwrap();
    }

    #[test]
    fn settings_lock_is_reentrant_on_the_owning_thread() {
        let path = temp_file("reentrant-lock");
        let lock_path = append_suffix(&path, ".lockfile");
        let _ = fs::remove_file(&lock_path);

        let outer = acquire_settings_lock(&path).unwrap();
        assert!(lock_path.exists());
        {
            let _inner = acquire_settings_lock(&path).unwrap();
            assert!(lock_path.exists());
        }
        assert!(
            lock_path.exists(),
            "dropping a nested guard must retain the outer lock"
        );
        drop(outer);
        assert!(lock_path.is_file());
        let reacquired = acquire_settings_lock(&path).unwrap();
        drop(reacquired);
        let _ = fs::remove_file(lock_path);
    }

    #[test]
    fn settings_lock_remains_held_when_reentrant_guards_drop_out_of_order() {
        let path = temp_file("reentrant-lock-out-of-order");
        let lock_path = append_suffix(&path, ".lockfile");
        let _ = fs::remove_file(&lock_path);

        let outer = acquire_settings_lock(&path).unwrap();
        let inner = acquire_settings_lock(&path).unwrap();
        drop(outer);

        let contender = open_settings_lock_file(&lock_path).unwrap();
        let error = contender.try_lock_exclusive().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::WouldBlock);

        drop(inner);
        contender.try_lock_exclusive().unwrap();
        FileExt::unlock(&contender).unwrap();
        let _ = fs::remove_file(lock_path);
    }

    #[test]
    fn settings_read_lock_does_not_initialize_an_empty_home() {
        let home = crate::testenv::TestHome::new("read-lock-empty");
        let path = crate::paths::settings_file();
        assert!(!home.home.join(".arca").exists());

        let guard = acquire_settings_read_lock_if_initialized(&path).unwrap();

        assert!(guard.is_none());
        assert!(
            !home.home.join(".arca").exists(),
            "a pure read must not create Arca storage"
        );
    }

    #[test]
    fn settings_read_lock_excludes_a_cooperating_writer() {
        use std::sync::mpsc;

        let home = crate::testenv::TestHome::new("read-lock-writer");
        let path = crate::paths::settings_file();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{}").unwrap();
        let read_guard = acquire_settings_read_lock_if_initialized(&path)
            .unwrap()
            .expect("existing settings should initialize a shared lock");
        let writer_path = path.clone();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            let guard = acquire_settings_lock(&writer_path).unwrap();
            acquired_tx.send(()).unwrap();
            drop(guard);
        });

        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(150))
                .is_err(),
            "an exclusive writer acquired while the workspace read lock was held"
        );
        drop(read_guard);
        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        writer.join().unwrap();

        assert!(home.home.join(".arca/settings.json.lockfile").is_file());
    }

    #[test]
    fn settings_read_lock_reenters_an_owned_exclusive_lock() {
        let _home = crate::testenv::TestHome::new("read-lock-reentrant");
        let path = crate::paths::settings_file();
        let exclusive = acquire_settings_lock(&path).unwrap();

        let read = acquire_settings_read_lock_if_initialized(&path)
            .unwrap()
            .expect("owned exclusive lock should be reusable for a read");

        drop(read);
        drop(exclusive);
        let reacquired = acquire_settings_lock(&path).unwrap();
        drop(reacquired);
    }

    #[test]
    fn rollback_bytes_use_the_captured_state_as_a_cas_precondition() {
        let path = temp_file("rollback-bytes-cas");
        let _ = fs::remove_file(&path);
        fs::write(&path, b"arca-partial").unwrap();
        fs::write(&path, b"external-edit").unwrap();

        let mode = file_mode(&path).unwrap();
        let error =
            write_bytes_if_unchanged(&path, Some((b"arca-partial", mode)), b"original", None)
                .unwrap_err();
        assert!(error.contains("changed before rollback"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), b"external-edit");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rollback_bytes_restore_atomically_when_the_cas_matches() {
        let _home = crate::testenv::TestHome::new("safe-write-rollback-success");
        let path = temp_file("rollback-bytes-success");
        let _ = fs::remove_file(&path);
        fs::write(&path, b"arca-partial").unwrap();

        let mode = file_mode(&path).unwrap();
        write_bytes_if_unchanged(&path, Some((b"arca-partial", mode)), b"original", None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"original");
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn tracked_write_refuses_a_replaced_parent_without_writing_through_its_symlink() {
        use std::os::unix::fs::symlink;

        let home = crate::testenv::TestHome::new("safe-write-parent-swap");
        let parent = home.home.join("reviewed-parent");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("config.json");
        let snapshot = capture_parent_directory(&target).unwrap();
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            std::slice::from_ref(&target),
            &BTreeMap::from([(target.clone(), snapshot)]),
        )
        .unwrap();

        fs::rename(&parent, home.home.join("retained-parent")).unwrap();
        let outside = home.home.join("outside-parent");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &parent).unwrap();

        let error = write_if_unchanged(&target, None, "must-not-escape").unwrap_err();
        drop(tracker);

        assert!(error.contains("asset_target_unsafe"), "{error}");
        assert!(!outside.join("config.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn live_existing_parent_swap_preserves_claim_until_canonical_parent_returns() {
        let home = crate::testenv::TestHome::new("safe-write-live-existing-parent-swap");
        let parent = home.home.join("reviewed-parent");
        let displaced = home.home.join("displaced-parent");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("config.json");
        fs::write(&target, "reviewed").unwrap();
        set_before_mutation_claim_hook({
            let parent = parent.clone();
            let displaced = displaced.clone();
            move |_| {
                fs::rename(&parent, &displaced).unwrap();
                fs::create_dir(&parent).unwrap();
            }
        });

        let error = write_if_unchanged(&target, Some("reviewed"), "arca").unwrap_err();

        assert!(error.contains("canonical mutation parent"), "{error}");
        assert!(
            !target.exists(),
            "the replacement namespace must stay untouched"
        );
        assert!(displaced.join("config.json").exists());
        assert!(global_mutation_intent_dir().is_dir());

        fs::remove_dir(&parent).unwrap();
        fs::rename(&displaced, &parent).unwrap();
        recover_global_mutation_intents().unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "reviewed");
        ensure_no_transaction_mutation_intents(&global_mutation_intent_dir()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn live_created_parent_swap_does_not_retire_the_create_intent() {
        let home = crate::testenv::TestHome::new("safe-write-live-created-parent-swap");
        let anchor = home.home.join("anchor");
        fs::create_dir(&anchor).unwrap();
        let created_root = anchor.join("created");
        let displaced = anchor.join("displaced");
        let target = created_root.join("nested/config.json");
        set_before_mutation_claim_hook({
            let created_root = created_root.clone();
            let displaced = displaced.clone();
            move |_| {
                fs::rename(&created_root, &displaced).unwrap();
                fs::create_dir_all(created_root.join("nested")).unwrap();
            }
        });

        let error = write_if_unchanged(&target, None, "arca").unwrap_err();

        assert!(
            error.contains("canonical mutation parent changed"),
            "{error}"
        );
        assert!(
            !target.exists(),
            "the replacement namespace must stay untouched"
        );
        assert_eq!(
            fs::read_to_string(displaced.join("nested/config.json")).unwrap(),
            "arca"
        );
        assert!(global_mutation_intent_dir().is_dir());

        fs::remove_dir_all(&created_root).unwrap();
        fs::rename(&displaced, &created_root).unwrap();
        recover_global_mutation_intents().unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "arca");
        ensure_no_transaction_mutation_intents(&global_mutation_intent_dir()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn live_journal_swap_cannot_redirect_intent_arming() {
        let home = crate::testenv::TestHome::new("safe-write-live-journal-swap");
        let target = home.home.join("config.json");
        let parent_snapshot = capture_parent_directory(&target).unwrap();
        let parent = open_parent_anchored(&parent_snapshot, false)
            .unwrap()
            .unwrap();
        let mut intent = prepare_mutation_intent(MutationIntentRequest {
            path: &target,
            parent: &parent_snapshot,
            mutation_parent_identity: open_parent_identity(&parent).unwrap(),
            guard_name: std::ffi::OsStr::new(".config.guard"),
            temp_name: Some(std::ffi::OsStr::new(".config.temp")),
            operation: MutationIntentOperation::Create,
            expected: &AnchoredPathState::Missing,
            desired: &AnchoredPathState::File {
                bytes: Zeroizing::new(b"planned".to_vec()),
                mode: Some(0o600),
                identity: PathIdentity::unknown(),
            },
        })
        .unwrap();
        let claims = global_mutation_intent_dir();
        let displaced = claims.with_file_name("claims-displaced");
        fs::rename(&claims, &displaced).unwrap();
        fs::create_dir(&claims).unwrap();
        set_private_directory(&claims).unwrap();

        let error = arm_mutation_intent(
            &mut intent,
            &AnchoredPathState::File {
                bytes: Zeroizing::new(b"planned".to_vec()),
                mode: Some(0o600),
                identity: PathIdentity {
                    device: Some(1),
                    inode: Some(1),
                },
            },
        )
        .unwrap_err();

        assert!(error.contains("journal identity changed"), "{error}");
        assert!(displaced
            .join(intent.record_path.file_name().unwrap())
            .is_file());
        assert!(claims.read_dir().unwrap().next().is_none());

        drop(intent);
        fs::remove_dir(&claims).unwrap();
        fs::rename(&displaced, &claims).unwrap();
        recover_global_mutation_intents().unwrap();
        ensure_no_transaction_mutation_intents(&claims).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn mutation_recovery_rejects_a_symlinked_journal() {
        use std::os::unix::fs::symlink;

        let home = crate::testenv::TestHome::new("safe-write-symlinked-journal");
        let claims = global_mutation_intent_dir();
        let claims_parent = claims.parent().unwrap();
        create_private_directory_all_durable(claims_parent).unwrap();
        let outside = home.home.join("outside-journal");
        fs::create_dir(&outside).unwrap();
        set_private_directory(&outside).unwrap();
        symlink(&outside, &claims).unwrap();

        let error = ensure_no_transaction_mutation_intents(&claims).unwrap_err();

        assert!(
            error.contains("durable directory component is unsafe"),
            "{error}"
        );
        assert!(outside.read_dir().unwrap().next().is_none());
        fs::remove_file(&claims).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacement_claim_preserves_an_edit_at_the_final_namespace_boundary() {
        let _home = crate::testenv::TestHome::new("safe-write-replace-race");
        let path = temp_file("claim-replace-race");
        let replacement = temp_file("claim-replace-race-external");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&replacement);
        fs::write(&path, "reviewed").unwrap();
        set_before_mutation_claim_hook({
            let replacement = replacement.clone();
            move |target| {
                fs::write(&replacement, "external").unwrap();
                fs::rename(&replacement, target).unwrap();
            }
        });

        let error = write_if_unchanged(&path, Some("reviewed"), "arca").unwrap_err();

        assert!(error.contains("claim boundary"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn removal_claim_preserves_an_edit_at_the_final_namespace_boundary() {
        let _home = crate::testenv::TestHome::new("safe-write-remove-race");
        let path = temp_file("claim-remove-race");
        let replacement = temp_file("claim-remove-race-external");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&replacement);
        fs::write(&path, "reviewed").unwrap();
        set_before_mutation_claim_hook({
            let replacement = replacement.clone();
            move |target| {
                fs::write(&replacement, "external").unwrap();
                fs::rename(&replacement, target).unwrap();
            }
        });

        let error = remove_if_unchanged(&path, "reviewed").unwrap_err();

        assert!(error.contains("removal claim boundary"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn active_transaction_rejects_an_unreviewed_write() {
        let home = crate::testenv::TestHome::new("safe-write-unreviewed-target");
        let reviewed = home.home.join("reviewed.json");
        let unreviewed = home.home.join("unreviewed.json");
        fs::write(&reviewed, "reviewed").unwrap();
        let parent = capture_parent_directory(&reviewed).unwrap();
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            std::slice::from_ref(&reviewed),
            &BTreeMap::from([(reviewed.clone(), parent)]),
        )
        .unwrap();

        let error = write_if_unchanged(&unreviewed, None, "forbidden").unwrap_err();
        drop(tracker);

        assert!(error.contains("unreviewed write"), "{error}");
        assert!(!unreviewed.exists());
    }

    #[cfg(unix)]
    #[test]
    fn repeated_transaction_write_binds_the_published_inode() {
        let home = crate::testenv::TestHome::new("safe-write-owned-inode");
        let path = home.home.join("config.json");
        let replacement = home.home.join("replacement.json");
        fs::write(&path, "reviewed").unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            std::slice::from_ref(&path),
            &BTreeMap::from([(path.clone(), parent)]),
        )
        .unwrap();
        write_if_unchanged(&path, Some("reviewed"), "first").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions();
        fs::write(&replacement, "first").unwrap();
        fs::set_permissions(&replacement, mode).unwrap();
        fs::rename(&replacement, &path).unwrap();

        let error = write_if_unchanged(&path, Some("first"), "second").unwrap_err();
        drop(tracker);

        assert!(
            error.contains("reviewed target changed before write"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "first");
    }

    #[cfg(unix)]
    fn private_transaction_state_from_disk(
        path: &Path,
        parent: &ParentDirectorySnapshot,
    ) -> TransactionPathState {
        match read_path_state_anchored(path, parent).unwrap() {
            AnchoredPathState::Missing => TransactionPathState::PrivateMissing,
            AnchoredPathState::File {
                bytes,
                mode,
                identity,
            } => transaction_private_file_state(&bytes, mode, identity).unwrap(),
            state => panic!("expected a private regular file, got {state:?}"),
        }
    }

    #[cfg(unix)]
    fn assert_private_transaction_records_omit(directory: &Path, sentinels: &[&[u8]]) {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return,
            Err(error) => panic!("failed to read {}: {error}", directory.display()),
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                assert_private_transaction_records_omit(&path, sentinels);
                continue;
            }
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let bytes = fs::read(&path).unwrap();
            for sentinel in sentinels {
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == *sentinel),
                    "private sentinel leaked into durable record {}",
                    path.display()
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_ordinary_wrappers_preserve_directory_reviewed_states() {
        let home = crate::testenv::TestHome::new("safe-write-ordinary-directory-review");
        let path = home.home.join("reviewed-directory");
        let evidence = home.home.join("post");
        fs::create_dir(&path).unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let parents = BTreeMap::from([(path.clone(), parent.clone())]);
        let reviewed = BTreeMap::from([(
            path.clone(),
            read_path_state_anchored(&path, &parent).unwrap(),
        )]);

        let tracker = begin_transaction_write_tracking_with_states(
            &evidence,
            std::slice::from_ref(&path),
            &parents,
            &reviewed,
        )
        .unwrap();
        drop(tracker);

        let tracker = resume_transaction_write_tracking(
            &evidence,
            std::slice::from_ref(&path),
            &parents,
            &reviewed,
            &BTreeMap::new(),
        )
        .unwrap();
        drop(tracker);
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_write_evidence_is_hash_only() {
        let home = crate::testenv::TestHome::new("safe-write-private-evidence");
        let path = home.home.join("private.json");
        let evidence = home.home.join("post");
        let initial = "private-initial";
        let sentinel = "private-transaction-sentinel-evidence";
        fs::write(&path, initial).unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let parent_snapshots = BTreeMap::from([(path.clone(), parent.clone())]);
        let private_paths = BTreeSet::from([path.clone()]);
        let reviewed_states = BTreeMap::from([(
            path.clone(),
            private_transaction_state_from_disk(&path, &parent),
        )]);
        let tracker = begin_transaction_write_tracking_with_private_states(
            &evidence,
            std::slice::from_ref(&path),
            &private_paths,
            &parent_snapshots,
            &reviewed_states,
        )
        .unwrap();

        write_private_if_unchanged(&path, Some(initial), sentinel).unwrap();

        let states = tracker.states();
        let TransactionPathState::PrivateFile { content_hash, .. } = states.get(&path).unwrap()
        else {
            panic!("private write was not retained as hash-only evidence");
        };
        assert_eq!(
            content_hash,
            &hex::encode(Sha256::digest(sentinel.as_bytes()))
        );
        assert_private_transaction_records_omit(&evidence, &[sentinel.as_bytes()]);
        drop(tracker);
        assert_eq!(
            load_transaction_write_states_with_private_paths(&evidence, &private_paths).unwrap(),
            states
        );
    }

    #[test]
    fn private_transaction_anchored_debug_redacts_file_bytes() {
        let sentinel = b"PRIVATE-ANCHORED-DEBUG-SENTINEL";
        let state = AnchoredPathState::File {
            bytes: Zeroizing::new(sentinel.to_vec()),
            mode: Some(0o600),
            identity: PathIdentity::unknown(),
        };
        let AnchoredPathState::File { bytes, .. } = &state else {
            unreachable!();
        };
        fn assert_zeroizing(_: &Zeroizing<Vec<u8>>) {}
        assert_zeroizing(bytes);

        let debug = format!("{state:?}");
        let cloned_debug = format!("{:?}", state.clone());

        assert!(!debug.contains("PRIVATE-ANCHORED-DEBUG-SENTINEL"));
        assert!(!debug.contains(&format!("{:?}", sentinel.to_vec())));
        assert!(!cloned_debug.contains(&format!("{:?}", sentinel.to_vec())));
        let file_bytes = FileBytes {
            bytes: Zeroizing::new(sentinel.to_vec()),
            mode: Some(0o600),
        };
        assert!(!format!("{file_bytes:?}").contains(&format!("{:?}", sentinel.to_vec())));
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_rejects_private_ordinary_state_parity_mismatches() {
        let home = crate::testenv::TestHome::new("safe-write-private-parity");
        let path = home.home.join("private.json");
        fs::write(&path, "private-initial").unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let AnchoredPathState::File {
            bytes,
            mode,
            identity,
        } = read_path_state_anchored(&path, &parent).unwrap()
        else {
            panic!("fixture is not a regular file");
        };
        let paths = std::slice::from_ref(&path);
        let parents = BTreeMap::from([(path.clone(), parent)]);
        let private_paths = BTreeSet::from([path.clone()]);
        let ordinary_state = BTreeMap::from([(
            path.clone(),
            TransactionPathState::File {
                bytes: bytes.to_vec(),
                mode,
                identity,
            },
        )]);

        let private_error = match begin_transaction_write_tracking_with_private_states(
            &home.home.join("private-post"),
            paths,
            &private_paths,
            &parents,
            &ordinary_state,
        ) {
            Ok(_) => panic!("ordinary state was accepted for a private transaction path"),
            Err(error) => error,
        };
        assert!(
            private_error.contains("private/ordinary parity"),
            "{private_error}"
        );

        let private_state = BTreeMap::from([(
            path.clone(),
            transaction_private_file_state(&bytes, mode, identity).unwrap(),
        )]);
        let ordinary_error = match begin_transaction_write_tracking_with_private_states(
            &home.home.join("ordinary-post"),
            paths,
            &BTreeSet::new(),
            &parents,
            &private_state,
        ) {
            Ok(_) => panic!("private state was accepted for an ordinary transaction path"),
            Err(error) => error,
        };
        assert!(
            ordinary_error.contains("private/ordinary parity"),
            "{ordinary_error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_resume_supports_repeated_writes_and_removals_hash_only() {
        let home = crate::testenv::TestHome::new("safe-write-private-resume");
        let path = home.home.join("private.json");
        let evidence = home.home.join("post");
        let initial = "private-resume-initial";
        let first = "private-transaction-sentinel-resume-first";
        let second = "private-transaction-sentinel-resume-second";
        let third = "private-transaction-sentinel-resume-third";
        fs::write(&path, initial).unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let parents = BTreeMap::from([(path.clone(), parent.clone())]);
        let private_paths = BTreeSet::from([path.clone()]);
        let reviewed_states = BTreeMap::from([(
            path.clone(),
            private_transaction_state_from_disk(&path, &parent),
        )]);
        let tracker = begin_transaction_write_tracking_with_private_states(
            &evidence,
            std::slice::from_ref(&path),
            &private_paths,
            &parents,
            &reviewed_states,
        )
        .unwrap();
        write_private_if_unchanged(&path, Some(initial), first).unwrap();
        drop(tracker);
        let written_states =
            load_transaction_write_states_with_private_paths(&evidence, &private_paths).unwrap();
        let tracker = resume_transaction_write_tracking_with_private_states(
            &evidence,
            std::slice::from_ref(&path),
            &private_paths,
            &parents,
            &reviewed_states,
            &written_states,
        )
        .unwrap();

        write_private_if_unchanged(&path, Some(first), second).unwrap();
        remove_if_unchanged(&path, second).unwrap();
        write_private_if_unchanged(&path, None, third).unwrap();

        let TransactionPathState::PrivateFile { content_hash, .. } =
            tracker.states().remove(&path).unwrap()
        else {
            panic!("final private state was not hash-only");
        };
        assert_eq!(content_hash, hex::encode(Sha256::digest(third.as_bytes())));
        assert_private_transaction_records_omit(
            &evidence,
            &[first.as_bytes(), second.as_bytes(), third.as_bytes()],
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_missing_restart_keeps_private_policy_and_hash_only_next_write() {
        let home = crate::testenv::TestHome::new("safe-write-private-missing-restart");
        let path = home.home.join("private.json");
        let evidence = home.home.join("post");
        let initial = "private-transaction-sentinel-missing-initial";
        let next = "private-transaction-sentinel-missing-next";
        fs::write(&path, initial).unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let parents = BTreeMap::from([(path.clone(), parent.clone())]);
        let private_paths = BTreeSet::from([path.clone()]);
        let reviewed_states = BTreeMap::from([(
            path.clone(),
            private_transaction_state_from_disk(&path, &parent),
        )]);
        let ordinary_reviewed = BTreeMap::from([(
            path.clone(),
            read_path_state_anchored(&path, &parent).unwrap(),
        )]);
        let tracker = begin_transaction_write_tracking_with_private_states(
            &evidence,
            std::slice::from_ref(&path),
            &private_paths,
            &parents,
            &reviewed_states,
        )
        .unwrap();

        remove_if_unchanged(&path, initial).unwrap();
        drop(tracker);

        let ordinary_load_error = load_transaction_write_states(&evidence).unwrap_err();
        assert!(
            ordinary_load_error.contains("private/ordinary parity"),
            "{ordinary_load_error}"
        );
        let written_states =
            load_transaction_write_states_with_private_paths(&evidence, &private_paths).unwrap();
        assert!(matches!(
            written_states.get(&path),
            Some(TransactionPathState::PrivateMissing)
        ));
        let ordinary_resume_error = match resume_transaction_write_tracking(
            &evidence,
            std::slice::from_ref(&path),
            &parents,
            &ordinary_reviewed,
            &written_states,
        ) {
            Ok(_) => panic!("ordinary resume accepted private-missing evidence"),
            Err(error) => error,
        };
        assert!(
            ordinary_resume_error.contains("private/ordinary parity"),
            "{ordinary_resume_error}"
        );
        let tracker = resume_transaction_write_tracking_with_private_states(
            &evidence,
            std::slice::from_ref(&path),
            &private_paths,
            &parents,
            &reviewed_states,
            &written_states,
        )
        .unwrap();

        write_private_if_unchanged(&path, None, next).unwrap();

        assert!(matches!(
            tracker.states().get(&path),
            Some(TransactionPathState::PrivateFile { .. })
        ));
        assert_private_transaction_records_omit(&evidence, &[initial.as_bytes(), next.as_bytes()]);
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_mutation_intent_recovery_persists_hash_only_state() {
        use std::os::unix::fs::PermissionsExt;

        let home = crate::testenv::TestHome::new("safe-write-private-intent-recovery");
        let path = home.home.join("private.json");
        let evidence = home.home.join("post");
        let claims = home.home.join("claims");
        let initial = "private-transaction-sentinel-recovery-initial";
        let desired_bytes = b"private-transaction-sentinel-recovery-desired";
        fs::write(&path, initial).unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let parents = BTreeMap::from([(path.clone(), parent.clone())]);
        let private_paths = BTreeSet::from([path.clone()]);
        let reviewed_states = BTreeMap::from([(
            path.clone(),
            private_transaction_state_from_disk(&path, &parent),
        )]);
        let tracker = begin_transaction_write_tracking_with_private_states(
            &evidence,
            std::slice::from_ref(&path),
            &private_paths,
            &parents,
            &reviewed_states,
        )
        .unwrap();
        let expected = read_path_state_anchored(&path, &parent).unwrap();
        let AnchoredPathState::File { mode, identity, .. } = expected else {
            panic!("fixture is not a regular file");
        };
        let desired = AnchoredPathState::File {
            bytes: Zeroizing::new(desired_bytes.to_vec()),
            mode,
            identity,
        };
        let open_parent = open_parent_anchored(&parent, false).unwrap().unwrap();
        let intent = begin_mutation_intent(MutationIntentRequest {
            path: &path,
            parent: &parent,
            mutation_parent_identity: open_parent_identity(&open_parent).unwrap(),
            guard_name: std::ffi::OsStr::new(".private-rewrite-guard"),
            temp_name: None,
            operation: MutationIntentOperation::Rewrite,
            expected: &read_path_state_anchored(&path, &parent).unwrap(),
            desired: &desired,
        })
        .unwrap()
        .unwrap();
        fs::write(&path, desired_bytes).unwrap();
        drop(intent);
        drop(tracker);
        assert_private_transaction_records_omit(&claims, &[initial.as_bytes(), desired_bytes]);

        let ordinary_recovery_error =
            recover_transaction_mutation_intents(&claims, &parents).unwrap_err();
        assert!(
            ordinary_recovery_error.contains("privacy"),
            "{ordinary_recovery_error}"
        );
        assert_private_transaction_records_omit(&claims, &[initial.as_bytes(), desired_bytes]);
        assert_private_transaction_records_omit(&evidence, &[initial.as_bytes(), desired_bytes]);

        recover_transaction_mutation_intents_with_private_paths(&claims, &parents, &private_paths)
            .unwrap();

        ensure_no_transaction_mutation_intents(&claims).unwrap();
        let written_states =
            load_transaction_write_states_with_private_paths(&evidence, &private_paths).unwrap();
        let TransactionPathState::PrivateFile {
            content_hash,
            mode: recovered_mode,
            ..
        } = written_states.get(&path).unwrap()
        else {
            panic!("recovered private state was not hash-only");
        };
        assert_eq!(content_hash, &hex::encode(Sha256::digest(desired_bytes)));
        assert_private_transaction_records_omit(&claims, &[initial.as_bytes(), desired_bytes]);
        assert_private_transaction_records_omit(&evidence, &[initial.as_bytes(), desired_bytes]);

        let tracker = resume_transaction_write_tracking_with_private_states(
            &evidence,
            std::slice::from_ref(&path),
            &private_paths,
            &parents,
            &reviewed_states,
            &written_states,
        )
        .unwrap();
        let replacement = home.home.join("replacement.json");
        fs::write(&replacement, desired_bytes).unwrap();
        if let Some(mode) = *recovered_mode {
            fs::set_permissions(&replacement, fs::Permissions::from_mode(mode)).unwrap();
        }
        fs::rename(&replacement, &path).unwrap();
        let error = write_private_if_unchanged(
            &path,
            Some(std::str::from_utf8(desired_bytes).unwrap()),
            "must-not-replace-an-unowned-inode",
        )
        .unwrap_err();
        assert!(error.contains("hash/mode/identity"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), desired_bytes);
        drop(tracker);
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_hash_rollback_rejects_hash_mode_and_identity_mismatches() {
        use std::os::unix::fs::PermissionsExt;

        let home = crate::testenv::TestHome::new("safe-write-private-hash-rollback");
        let path = home.home.join("private.json");
        let replacement = home.home.join("replacement.json");
        let bytes = b"private-rollback-owned";
        fs::write(&path, bytes).unwrap();
        let parent = capture_parent_directory(&path).unwrap();
        let AnchoredPathState::File { mode, identity, .. } =
            read_path_state_anchored(&path, &parent).unwrap()
        else {
            panic!("fixture is not a regular file");
        };
        let hash = hex::encode(Sha256::digest(bytes));

        let hash_error = remove_bytes_if_hash_unchanged_in_parent(
            &path,
            &parent,
            &"0".repeat(64),
            mode,
            identity,
        )
        .unwrap_err();
        assert!(hash_error.contains("hash/mode/identity"), "{hash_error}");

        let mode_error = remove_bytes_if_hash_unchanged_in_parent(
            &path,
            &parent,
            &hash,
            mode.map(|value| value ^ 0o100),
            identity,
        )
        .unwrap_err();
        assert!(mode_error.contains("hash/mode/identity"), "{mode_error}");

        fs::write(&replacement, bytes).unwrap();
        if let Some(mode) = mode {
            fs::set_permissions(&replacement, fs::Permissions::from_mode(mode)).unwrap();
        }
        fs::rename(&replacement, &path).unwrap();
        let identity_error = write_bytes_if_hash_unchanged_in_parent(
            &path,
            &parent,
            Some((&hash, mode, identity)),
            b"private-restored",
            mode,
        )
        .unwrap_err();
        assert!(
            identity_error.contains("hash/mode/identity"),
            "{identity_error}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_permissions_and_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let _home = crate::testenv::TestHome::new("safe-write-symlink");
        let target = temp_file("symlink-target");
        let link = temp_file("symlink-link");
        let _ = fs::remove_file(&target);
        let _ = fs::remove_file(&link);
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, &link).unwrap();

        write_if_unchanged(&link, Some("old"), "new").unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_file(link);
        let _ = fs::remove_file(target);
    }
}
