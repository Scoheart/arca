//! Bounded, read-only summary cache. File identities are checked on every poll;
//! this cache never participates in capture writes or their authorization.
use super::{read_json, reject_symlink, FlowSummary};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;
use uuid::Uuid;

const MAX_SESSIONS: usize = 2;
const MAX_CACHE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FLOWS: usize = 5_000;
const MAX_REMOVALS: usize = 5_000;
const MAX_SUMMARY_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    length: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed: (i64, i64),
}

impl FileStamp {
    fn new(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

// Directory mtime changes for each new flow. Only its physical identity should
// reset the generation; ordinary additions are handled by sequence deltas.
#[derive(PartialEq, Eq)]
struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    created: Option<SystemTime>,
}

impl DirectoryIdentity {
    fn read(path: &Path) -> Result<Self, String> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "无法读取抓包请求目录。".to_owned())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("抓包请求目录必须是普通目录。".into());
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            created: metadata.created().ok(),
        })
    }
}

struct CachedSummary {
    stamp: FileStamp,
    value: Option<FlowSummary>,
    changed_at: u64,
}

struct SessionIndex {
    identity: DirectoryIdentity,
    session_stamp: FileStamp,
    generation: String,
    sequence: u64,
    oldest_cursor: u64,
    entries: BTreeMap<String, CachedSummary>,
    removals: VecDeque<(u64, String)>,
    used_at: u64,
    retained_bytes: u64,
}

#[derive(Default)]
struct SummaryCache {
    sessions: BTreeMap<PathBuf, SessionIndex>,
    clock: u64,
}

static CACHE: LazyLock<Mutex<SummaryCache>> = LazyLock::new(|| Mutex::new(SummaryCache::default()));

pub(super) struct SummaryDelta {
    pub revision: String,
    pub reset: bool,
    pub upserts: Vec<FlowSummary>,
    pub removed: Vec<String>,
    pub count: usize,
}

pub(super) fn query(directory: &Path, revision: Option<&str>) -> Result<SummaryDelta, String> {
    let summaries = directory.join("summaries");
    reject_symlink(directory)?;
    let identity = DirectoryIdentity::read(&summaries)?;
    let session_metadata = fs::symlink_metadata(directory.join("session.json"))
        .map_err(|_| "抓包记录不存在。".to_owned())?;
    if !session_metadata.is_file() || session_metadata.file_type().is_symlink() {
        return Err("抓包会话记录必须是普通文件。".into());
    }
    let session_stamp = FileStamp::new(&session_metadata);
    let mut cache = CACHE.lock().map_err(|_| "抓包摘要索引不可用。".to_owned())?;
    cache.clock = cache.clock.saturating_add(1);
    let used_at = cache.clock;
    if !cache.sessions.contains_key(directory) && cache.sessions.len() >= MAX_SESSIONS {
        if let Some(oldest) = cache.sessions.iter().min_by_key(|(_, index)| index.used_at).map(|(path, _)| path.clone()) {
            cache.sessions.remove(&oldest);
        }
    }
    let replace = cache.sessions.get(directory)
        .is_none_or(|index| index.identity != identity || index.session_stamp != session_stamp);
    if replace {
        cache.sessions.insert(directory.to_owned(), SessionIndex {
            identity, session_stamp, generation: Uuid::new_v4().to_string(), sequence: 0,
            oldest_cursor: 0, entries: BTreeMap::new(), removals: VecDeque::new(), used_at, retained_bytes: 0,
        });
    }
    let index = cache.sessions.get_mut(directory).expect("capture index was inserted");
    index.used_at = used_at;
    refresh(index, &summaries)?;
    let since = revision.and_then(|value| value.rsplit_once(':'))
        .filter(|(generation, _)| *generation == index.generation)
        .and_then(|(_, value)| value.parse::<u64>().ok())
        .filter(|sequence| *sequence >= index.oldest_cursor && *sequence <= index.sequence);
    let mut upserts = index.entries.values()
        .filter(|entry| since.is_none_or(|sequence| entry.changed_at > sequence))
        .filter_map(|entry| entry.value.clone()).collect::<Vec<_>>();
    upserts.sort_by(|left, right| right.started_at.total_cmp(&left.started_at).then_with(|| left.id.cmp(&right.id)));
    let removed = since.map(|sequence| index.removals.iter()
        .filter(|(changed_at, _)| *changed_at > sequence)
        .map(|(_, id)| id.clone()).collect::<BTreeSet<_>>().into_iter().collect())
        .unwrap_or_default();
    let result = SummaryDelta {
        revision: format!("{}:{}", index.generation, index.sequence), reset: since.is_none(),
        upserts, removed, count: index.entries.values().filter(|entry| entry.value.is_some()).count(),
    };
    // Session count alone does not bound large URLs/headers in summaries.
    // Eviction only discards a read cache: a later cursor safely gets a reset.
    while cache.sessions.values().map(|index| index.retained_bytes).sum::<u64>() > MAX_CACHE_BYTES {
        let oldest = cache.sessions.iter().min_by_key(|(_, index)| index.used_at).map(|(path, _)| path.clone());
        if let Some(path) = oldest { cache.sessions.remove(&path); } else { break; }
    }
    Ok(result)
}

fn refresh(index: &mut SessionIndex, directory: &Path) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut next = BTreeMap::new();
    let mut bytes = 0_u64;
    for (scanned, entry) in fs::read_dir(directory).map_err(|_| "无法读取抓包请求。".to_owned())?.enumerate() {
        // Atomic writers briefly leave .tmp files alongside their summaries.
        // Count valid records separately so a temporary file cannot evict a
        // real row at the 5000-flow boundary; still bound directory traversal.
        if scanned >= MAX_FLOWS * 2 { return Err("抓包摘要目录项超过读取上限。".into()); }
        let entry = entry.map_err(|_| "无法完整读取抓包请求目录。".to_owned())?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") { continue; }
        let Some(id) = path.file_stem().and_then(|value| value.to_str()).map(str::to_owned) else { continue; };
        if Uuid::parse_str(&id).is_err() { continue; }
        let Ok(metadata) = fs::symlink_metadata(&path) else { continue; };
        if !metadata.is_file() || metadata.file_type().is_symlink() { continue; }
        if seen.len() >= MAX_FLOWS { return Err("抓包摘要条数超过会话读取上限。".into()); }
        bytes = bytes.saturating_add(metadata.len());
        if bytes > MAX_SUMMARY_BYTES { return Err("抓包摘要超过会话读取上限。".into()); }
        let stamp = FileStamp::new(&metadata);
        seen.insert(id.clone());
        if index.entries.get(&id).is_some_and(|previous| previous.stamp == stamp) { continue; }
        // A concurrent atomic rewrite is retried on the next poll. Never cache
        // bytes under the identity of a different version of the source file.
        #[cfg(test)]
        PARSED_SUMMARIES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let value = read_json::<FlowSummary>(&path).ok()
            .filter(|flow| flow.id == id && flow.started_at.is_finite());
        let stable = fs::symlink_metadata(&path).is_ok_and(|after| {
            after.is_file() && !after.file_type().is_symlink() && FileStamp::new(&after) == stamp
        });
        if !stable { continue; }
        next.insert(id, (stamp, value));
    }
    if DirectoryIdentity::read(directory)? != index.identity {
        return Err("抓包请求目录在读取期间发生变化，请重试。".into());
    }
    let sequence = index.sequence.saturating_add(1);
    let mut changed = false;
    let absent = index.entries.keys().filter(|id| !seen.contains(*id)).cloned().collect::<Vec<_>>();
    for id in absent {
        if index.entries.remove(&id).is_some_and(|entry| entry.value.is_some()) {
            index.removals.push_back((sequence, id)); changed = true;
        }
    }
    for (id, (stamp, value)) in next {
        let previous = index.entries.get(&id);
        let different = previous.and_then(|entry| entry.value.as_ref()) != value.as_ref();
        let changed_at = if different { sequence } else { previous.map_or(0, |entry| entry.changed_at) };
        if different {
            if value.is_none() { index.removals.push_back((sequence, id.clone())); }
            changed = true;
        }
        index.entries.insert(id, CachedSummary { stamp, value, changed_at });
    }
    if changed { index.sequence = sequence; }
    while index.removals.len() > MAX_REMOVALS {
        if let Some((sequence, _)) = index.removals.pop_front() { index.oldest_cursor = sequence; }
    }
    // JSON lengths plus conservative node/stamp/key overhead; no additional
    // copy of the cached strings is allocated to calculate the budget.
    index.retained_bytes = bytes.saturating_add(index.entries.len() as u64 * 384)
        .saturating_add(index.removals.len() as u64 * 96);
    Ok(())
}

#[cfg(test)]
static PARSED_SUMMARIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use std::sync::atomic::Ordering;

    fn session(home: &TestHome) -> PathBuf {
        let id = Uuid::new_v4().to_string();
        let directory = home.home.join(".arca/captures").join(&id);
        fs::create_dir_all(directory.join("summaries")).unwrap();
        fs::write(directory.join("session.json"), serde_json::to_vec(&super::super::CaptureSession {
            id, agent_id: "codex".into(), agent_name: "Codex".into(), target_id: "fixture".into(),
            target_name: "Fixture".into(), pids: vec![], proxy_url: None, started_at: "2026-10-05T00:00:00Z".into(), loopback_tls: vec![],
        }).unwrap()).unwrap();
        directory
    }

    fn flow(directory: &Path, id: &str, status: u16) {
        let data = FlowSummary { id: id.into(), method: "GET".into(), url: "https://example.invalid/test".into(),
            status: Some(status), started_at: 1.0, duration_ms: Some(3), error: None,
            phase: Some("complete".into()), updated_at: None, request_bytes: None,
            response_bytes: None, time_to_first_byte_ms: None, content_type: None, stream_type: None };
        fs::write(directory.join("summaries").join(format!("{id}.json")), serde_json::to_vec(&data).unwrap()).unwrap();
    }

    #[test]
    fn unchanged_polls_parse_nothing_and_changed_rows_produce_only_their_delta() {
        let home = TestHome::new("capture-summary-delta");
        let directory = session(&home);
        let first_id = Uuid::new_v4().to_string();
        let second_id = Uuid::new_v4().to_string();
        flow(&directory, &first_id, 200);
        flow(&directory, &second_id, 200);
        let first = query(&directory, None).unwrap();
        assert!(first.reset);
        assert_eq!(first.upserts.len(), 2);
        let parsed = PARSED_SUMMARIES.load(Ordering::SeqCst);
        let idle = query(&directory, Some(&first.revision)).unwrap();
        assert_eq!(PARSED_SUMMARIES.load(Ordering::SeqCst), parsed);
        assert!(!idle.reset);
        assert!(idle.upserts.is_empty() && idle.removed.is_empty());
        // Atomic replacement tests inode-based invalidation even at equal size.
        let path = directory.join("summaries").join(format!("{first_id}.json"));
        let old = directory.join("old.json");
        fs::rename(&path, old).unwrap();
        flow(&directory, &first_id, 404);
        let changed = query(&directory, Some(&idle.revision)).unwrap();
        assert_eq!(changed.upserts.len(), 1);
        assert_eq!(changed.upserts[0].status, Some(404));
        fs::remove_file(directory.join("summaries").join(format!("{second_id}.json"))).unwrap();
        let deleted = query(&directory, Some(&changed.revision)).unwrap();
        assert_eq!(deleted.removed, vec![second_id]);
        assert_eq!(deleted.count, 1);
        assert!(query(&directory, Some("expired:1")).unwrap().reset);
    }

    #[test]
    fn session_cache_is_bounded_and_eviction_resets_the_cursor() {
        let home = TestHome::new("capture-cache-eviction");
        let first = session(&home);
        let revision = query(&first, None).unwrap().revision;
        for _ in 0..MAX_SESSIONS { query(&session(&home), None).unwrap(); }
        assert_eq!(CACHE.lock().unwrap().sessions.len(), MAX_SESSIONS);
        assert!(query(&first, Some(&revision)).unwrap().reset);
    }

    #[cfg(unix)]
    #[test]
    fn cached_summary_is_removed_if_replaced_with_a_symlink() {
        let home = TestHome::new("capture-cache-symlink");
        let directory = session(&home);
        let id = Uuid::new_v4().to_string();
        flow(&directory, &id, 200);
        let first = query(&directory, None).unwrap();
        let path = directory.join("summaries").join(format!("{id}.json"));
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(directory.join("session.json"), &path).unwrap();
        let delta = query(&directory, Some(&first.revision)).unwrap();
        assert_eq!(delta.removed, vec![id]);
        assert_eq!(delta.count, 0);
    }

    #[test]
    fn cli_summaries_do_not_change_another_process_session_state() {
        let home = TestHome::new("capture-cli-summary-state");
        let directory = session(&home);
        let id = directory.file_name().unwrap().to_str().unwrap();
        let status = directory.join("status.json");
        let bytes = br#"{"state":"running","message":"live","pids":[123],"flow_count":0}"#;
        fs::write(&status, bytes).unwrap();
        assert!(super::super::summaries(id).unwrap().is_empty());
        assert_eq!(fs::read(status).unwrap(), bytes);
        // Desktop can recover its controls after an interrupted prior run;
        // this local liveness projection does not rewrite the saved status.
        assert_eq!(super::super::snapshot(id).unwrap().status.state, "stopped");
        assert_eq!(fs::read(directory.join("status.json")).unwrap(), bytes);
    }
}
