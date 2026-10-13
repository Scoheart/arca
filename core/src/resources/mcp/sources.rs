//! MCP catalog **sources**: subscribed remote URLs and added local files.
//!
//! A source's servers are parsed from a cached copy on disk under
//! `~/.arca/assets/mcps/sources/<kind>/<id>.<ext>`. This module owns the parse / fetch /
//! store primitives; the Tauri commands in `commands.rs` orchestrate them.
//! There is no "builtin" source — the catalog is entirely user-driven.

use crate::domain::types::{McpConfig, RegistryEntry, RegistryOrigin, SourceDef};
use crate::paths::{local_sources_dir, remote_sources_dir, settings_file};
use crate::resources::mcp::adapter::get_adapter;
use crate::safe_write::{acquire_settings_lock, remove_if_unchanged, write_private_if_unchanged};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The cached-file path backing a source (`remote/<id>.<ext>` or
/// `local/<id>.<ext>`). `None` for any unrecognized kind.
pub fn cached_path(def: &SourceDef) -> Option<PathBuf> {
    let ext = if def.format == "toml" { "toml" } else { "json" };
    let file = format!("{}.{}", def.id, ext);
    match def.kind.as_str() {
        "remote" => Some(remote_sources_dir().join(file)),
        "local" => Some(local_sources_dir().join(file)),
        _ => None,
    }
}

fn origin_for(def: &SourceDef) -> RegistryOrigin {
    RegistryOrigin {
        kind: def.kind.clone(),
        agent: None,
        scope: None,
        source: Some(def.id.clone()),
    }
}

fn entry_from(name: String, cfg: McpConfig, origin: &RegistryOrigin) -> RegistryEntry {
    let config = cfg.into();
    RegistryEntry {
        name,
        description: String::new(),
        tags: Vec::new(),
        config,
        origin: Some(origin.clone()),
        repo: None,
    }
}

/// Parse a config file into registry entries. Tries the rich Arca array format
/// first (a JSON `[RegistryEntry, …]`, so curated collections keep their
/// descriptions/tags), else the standard `mcpServers` map via the adapter
/// (transport auto-detected by `McpConfig`).
pub fn parse_file(
    path: &Path,
    format: &str,
    key: &str,
    origin: &RegistryOrigin,
) -> Vec<RegistryEntry> {
    if format != "toml" {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(arr) = serde_json::from_str::<Vec<RegistryEntry>>(&content) {
                // Preserve an entry's own origin if it carries one (managed
                // manual/discovered files store explicit origins); otherwise tag
                // it with this source's origin.
                return arr
                    .into_iter()
                    .map(|mut e| {
                        if e.origin.is_none() {
                            e.origin = Some(origin.clone());
                        }
                        e
                    })
                    .collect();
            }
        }
    }
    get_adapter(format, key)
        .read(path)
        .into_iter()
        .map(|(name, cfg)| entry_from(name, cfg, origin))
        .collect()
}

/// Entries a source contributes to the catalog (from its cached file).
pub fn source_entries(def: &SourceDef) -> Vec<RegistryEntry> {
    let Some(path) = cached_path(def) else {
        return Vec::new();
    };
    if !path.exists() {
        return Vec::new();
    }
    parse_file(&path, &def.format, &def.key, &origin_for(def))
}

/// How many servers a source currently provides.
pub fn source_count(def: &SourceDef) -> u32 {
    source_entries(def).len() as u32
}

/// Fetch a remote URL's body as text (20s connect/read timeout; ureq caps the
/// body size internally).
pub fn fetch(url: &str) -> Result<String, String> {
    let agent = crate::network::build_ureq_agent(
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))),
    )?;
    agent
        .get(url)
        .call()
        .map_err(|e| format!("抓取失败: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("读取响应失败: {e}"))
}

/// Guess the format from a URL/path hint, else sniff the content (valid JSON ⇒
/// json, otherwise toml).
pub fn detect_format(hint: &str, content: &str) -> &'static str {
    let h = hint.to_lowercase();
    if h.ends_with(".toml") {
        return "toml";
    }
    if h.ends_with(".json") {
        return "json";
    }
    if serde_json::from_str::<serde_json::Value>(content).is_ok() {
        "json"
    } else {
        "toml"
    }
}

/// Reject content that isn't well-formed JSON/TOML before we store it.
pub fn validate_parseable(content: &str, format: &str) -> Result<(), String> {
    let ok = if format == "toml" {
        toml::from_str::<toml::Value>(content).is_ok()
    } else {
        serde_json::from_str::<serde_json::Value>(content).is_ok()
    };
    if ok {
        Ok(())
    } else {
        Err("内容不是有效的 JSON / TOML 配置".into())
    }
}

#[derive(Clone)]
enum SourceFileSnapshot {
    Missing,
    Present(String),
}

impl SourceFileSnapshot {
    fn expected(&self) -> Option<&str> {
        match self {
            Self::Missing => None,
            Self::Present(content) => Some(content),
        }
    }
}

fn snapshot_source_file(path: &Path) -> Result<SourceFileSnapshot, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(SourceFileSnapshot::Present(content)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(SourceFileSnapshot::Missing),
        Err(error) => Err(format!(
            "读取 source cache 失败 ({}): {error}",
            path.display()
        )),
    }
}

fn replace_source_file(
    path: &Path,
    expected: &SourceFileSnapshot,
    content: &str,
) -> Result<(), String> {
    write_private_if_unchanged(path, expected.expected(), content)
}

fn restore_source_file(
    path: &Path,
    written: &str,
    previous: &SourceFileSnapshot,
) -> Result<(), String> {
    match previous {
        SourceFileSnapshot::Missing => remove_if_unchanged(path, written),
        SourceFileSnapshot::Present(content) => {
            write_private_if_unchanged(path, Some(written), content)
        }
    }
}

fn remove_source_file(path: &Path, snapshot: &SourceFileSnapshot) -> Result<(), String> {
    match snapshot {
        SourceFileSnapshot::Missing => Ok(()),
        SourceFileSnapshot::Present(content) => remove_if_unchanged(path, content),
    }
}

/// Atomically replace a source cache while refusing to overwrite a concurrent
/// edit. Source caches can contain MCP environment/header values, so they use
/// the same private-file policy as `settings.json`.
pub fn write_source_file(path: &Path, content: &str) -> Result<(), String> {
    // This primitive remains public for compatibility, so it must participate
    // in the same cross-process mutation boundary as reviewed asset commits.
    // Source administration already holds this lock and re-enters here.
    let _settings_guard = acquire_settings_lock(&settings_file())?;
    let previous = snapshot_source_file(path)?;
    replace_source_file(path, &previous, content)
}

static NEXT_CANDIDATE_FILE: AtomicU64 = AtomicU64::new(0);

/// Parse candidate content without exposing it through an enabled source. This
/// is necessary for refresh review: replacing the live cache before comparing
/// catalogs would make the "before" projection already contain the new data.
fn parse_candidate_content(def: &SourceDef, content: &str) -> Result<Vec<RegistryEntry>, String> {
    let cache = cached_path(def).ok_or("无法确定缓存路径")?;
    let parent = cache.parent().ok_or("source cache 缺少父目录")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let file_name = cache
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source");
    let candidate = parent.join(format!(
        ".{file_name}.arca-candidate-{}-{}",
        std::process::id(),
        NEXT_CANDIDATE_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    write_private_if_unchanged(&candidate, None, content)?;
    let entries = parse_file(&candidate, &def.format, &def.key, &origin_for(def));
    remove_if_unchanged(&candidate, content).map_err(|error| {
        format!(
            "候选 source 已解析，但无法清理临时文件 {}: {error}",
            candidate.display()
        )
    })?;
    Ok(entries)
}

/// Reduce a user-selected local file to the MCP data Arca owns before caching it.
/// This prevents unrelated account, history, model, or credential fields from
/// being duplicated under `~/.arca/assets/mcps/sources/local/`.
fn sanitized_local_content(
    path: &Path,
    content: &str,
    format: &str,
    key: &str,
) -> Result<String, String> {
    if format != "toml" {
        if let Ok(entries) = serde_json::from_str::<Vec<RegistryEntry>>(content) {
            return serde_json::to_string_pretty(&entries)
                .map(|text| text + "\n")
                .map_err(|e| e.to_string());
        }
    }

    let configs = get_adapter(format, key).read(path);
    if format == "toml" {
        let mut root: BTreeMap<String, BTreeMap<String, McpConfig>> = BTreeMap::new();
        root.insert(key.to_string(), configs);
        toml::to_string_pretty(&root).map_err(|e| e.to_string())
    } else {
        let mut root = serde_json::Map::new();
        root.insert(
            key.to_string(),
            serde_json::to_value(configs).map_err(|e| e.to_string())?,
        );
        serde_json::to_string_pretty(&serde_json::Value::Object(root))
            .map(|text| text + "\n")
            .map_err(|e| e.to_string())
    }
}

/// Current local time as an ISO-ish stamp.
pub fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// A collision-resistant source id: `<kind>-<slug>-<timestamp-ms>`.
pub fn gen_id(kind: &str, label: &str) -> String {
    let slug: String = label
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_lowercase();
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S%3f");
    if slug.is_empty() {
        format!("{kind}-{ts}")
    } else {
        format!("{kind}-{slug}-{ts}")
    }
}

/// Best-effort display label from a URL (its host, else the whole URL).
pub fn host_of(url: &str) -> String {
    let no_scheme = url.split("://").nth(1).unwrap_or(url);
    no_scheme
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(url)
        .to_string()
}

// ── Source management (subscribe / local / refresh / toggle / remove) ──────
//
// These orchestrate the primitives above over `settings.sources`, and are shared
// by both front-ends. The desktop's native file-picker command stays desktop-side
// and calls `add_local` with the chosen path.

use crate::resources::mcp::registry::{builtin_registry, DISCOVERED_ID, MANUAL_ID};
use crate::resources::mcp::scanner::{collapse_home, expand_tilde};
use crate::settings::{load_settings, load_settings_strict, mutate_settings_checked, Settings};
use serde::Serialize;

const CURATED_SOURCE_NAME: &str = "Arca 精选";
const LEGACY_CURATED_SOURCE_NAME: &str = "官方精选合集";

fn is_curated_source(def: &SourceDef) -> bool {
    def.kind == "local"
        && def.path.is_none()
        && matches!(
            def.name.as_str(),
            CURATED_SOURCE_NAME | LEGACY_CURATED_SOURCE_NAME
        )
}

fn curated_collection_content() -> Result<String, String> {
    serde_json::to_string_pretty(&builtin_registry()).map_err(|error| error.to_string())
}

/// A source as shown in a UI: its stored definition plus a live server count.
#[derive(Debug, Serialize)]
pub struct SourceView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub format: String,
    pub enabled: bool,
    pub added_at: Option<String>,
    pub synced_at: Option<String>,
    pub server_count: u32,
    pub error: Option<String>,
    /// True for the two auto-managed sources ("手动添加" / "自动探索") — the UI
    /// hides refresh/remove for these to avoid accidental data loss.
    pub managed: bool,
}

pub(crate) fn to_view(def: SourceDef, count: u32) -> SourceView {
    let managed = def.id == MANUAL_ID || def.id == DISCOVERED_ID;
    let name = if def.name == LEGACY_CURATED_SOURCE_NAME {
        CURATED_SOURCE_NAME.to_string()
    } else {
        def.name
    };
    SourceView {
        id: def.id,
        kind: def.kind,
        name,
        url: def.url,
        path: def.path,
        format: def.format,
        enabled: def.enabled,
        added_at: def.added_at,
        synced_at: def.synced_at,
        server_count: count,
        error: def.error,
        managed,
    }
}

fn io_invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidInput, message.into())
}

fn entries_for(
    def: &SourceDef,
    replacements: &BTreeMap<String, Vec<RegistryEntry>>,
) -> Vec<RegistryEntry> {
    replacements
        .get(&def.id)
        .cloned()
        .unwrap_or_else(|| source_entries(def))
}

/// Project the effective catalog from an explicit settings/source snapshot.
/// This mirrors `registry::read_registry` precedence while letting a caller
/// substitute staged entries for one source.
fn effective_catalog(
    sources: &[SourceDef],
    replacements: &BTreeMap<String, Vec<RegistryEntry>>,
) -> BTreeMap<String, RegistryEntry> {
    let mut by_key = BTreeMap::new();
    for def in sources
        .iter()
        .filter(|def| def.enabled && def.id != MANUAL_ID && def.id != DISCOVERED_ID)
    {
        for entry in entries_for(def, replacements) {
            by_key.insert(entry.key(), entry);
        }
    }
    for managed_id in [DISCOVERED_ID, MANUAL_ID] {
        if let Some(def) = sources
            .iter()
            .find(|def| def.enabled && def.id == managed_id)
        {
            for entry in entries_for(def, replacements) {
                by_key.insert(entry.key(), entry);
            }
        }
    }
    by_key
}

/// Source administration is not itself a reviewed asset operation. It may
/// therefore change only catalog keys with no desired Agent consumers. A used
/// key must go through the central asset plan/commit flow, which binds both the
/// reviewed catalog and every affected target.
fn ensure_no_unreviewed_desired_change(
    settings: &Settings,
    candidate_sources: &[SourceDef],
    before_replacements: &BTreeMap<String, Vec<RegistryEntry>>,
    after_replacements: &BTreeMap<String, Vec<RegistryEntry>>,
) -> std::io::Result<()> {
    let current_sources = settings.sources.as_deref().unwrap_or_default();
    let before = effective_catalog(current_sources, before_replacements);
    let after = effective_catalog(candidate_sources, after_replacements);
    let changed: BTreeSet<String> = before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect();
    if changed.is_empty() {
        return Ok(());
    }

    let mut affected = BTreeMap::<String, BTreeSet<String>>::new();
    for (agent_id, records) in settings.mcp_consumptions.iter().flatten() {
        for (stored_key, record) in records {
            for key in [stored_key, &record.asset_key] {
                if changed.contains(key) {
                    affected
                        .entry(key.clone())
                        .or_default()
                        .insert(agent_id.clone());
                }
            }
        }
    }
    if affected.is_empty() {
        return Ok(());
    }

    let details = affected
        .into_iter()
        .map(|(key, agents)| {
            format!(
                "{key} ({})",
                agents.into_iter().collect::<Vec<_>>().join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Err(io_invalid(format!(
        "source 变更会修改已被 Agent desired consumption 引用的 MCP 资产: {details}；请通过统一资源审阅计划更新或先解除关系"
    )))
}

fn push_source(def: &SourceDef, entries: Vec<RegistryEntry>) -> Result<(), String> {
    let def = def.clone();
    mutate_settings_checked(move |settings| {
        let current = settings.sources.clone().unwrap_or_default();
        if current.iter().any(|source| source.id == def.id) {
            return Err(io_invalid(format!("source id 已存在: {}", def.id)));
        }
        let mut candidate = current;
        candidate.push(def.clone());
        let replacements = BTreeMap::from([(def.id.clone(), entries)]);
        ensure_no_unreviewed_desired_change(settings, &candidate, &BTreeMap::new(), &replacements)?;
        settings.sources = Some(candidate);
        Ok(())
    })
    .map_err(|error| error.to_string())
}

fn register_cached_source(
    def: &SourceDef,
    path: &Path,
    content: &str,
    entries: Vec<RegistryEntry>,
) -> Result<(), String> {
    let previous = snapshot_source_file(path)?;
    replace_source_file(path, &previous, content)?;
    if let Err(error) = push_source(def, entries) {
        return match restore_source_file(path, content, &previous) {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(format!(
                "{error}; source cache 回滚失败 ({}): {rollback_error}",
                path.display()
            )),
        };
    }
    Ok(())
}

fn entries_from_snapshot(
    def: &SourceDef,
    snapshot: &SourceFileSnapshot,
) -> Result<Vec<RegistryEntry>, String> {
    match snapshot {
        SourceFileSnapshot::Missing => Ok(Vec::new()),
        SourceFileSnapshot::Present(content) => parse_candidate_content(def, content),
    }
}

/// List every source with a live `server_count`.
pub fn list_views() -> Vec<SourceView> {
    load_settings()
        .sources
        .unwrap_or_default()
        .into_iter()
        .map(|d| {
            let count = source_count(&d);
            to_view(d, count)
        })
        .collect()
}

/// Subscribe to a remote config URL: fetch, validate, cache it under
/// `~/.arca/assets/mcps/sources/remote/<id>`, and register it as an enabled source.
pub(crate) struct PreparedSubscription {
    def: SourceDef,
    path: PathBuf,
    body: String,
    entries: Vec<RegistryEntry>,
}

pub fn subscribe(url: String, name: Option<String>) -> Result<SourceView, String> {
    commit_subscription(probe_subscription(url, name)?)
}

pub(crate) fn commit_subscription(prepared: PreparedSubscription) -> Result<SourceView, String> {
    let PreparedSubscription { def, path, body, entries } = prepared;
    let count = entries.len() as u32;
    register_cached_source(&def, &path, &body, entries)?;
    Ok(to_view(def, count))
}

pub(crate) fn probe_subscription(url: String, name: Option<String>) -> Result<PreparedSubscription, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("URL 不能为空".into());
    }
    let body = fetch(&url)?;
    let format = detect_format(&url, &body).to_string();
    validate_parseable(&body, &format)?;
    let display = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| host_of(&url));
    let now = now_iso();
    let mut def = SourceDef::new_remote(gen_id("remote", &display), display, url, format, now);
    let path = cached_path(&def).ok_or("无法确定缓存路径")?;
    let entries = parse_candidate_content(&def, &body)?;
    let count = entries.len() as u32;
    def.server_count = Some(count);
    if count == 0 {
        def.error = Some("未在该文件中发现 MCP server".into());
    }
    Ok(PreparedSubscription { def, path, body, entries })
}

/// Register a local config file as a source: read and validate it, then cache only
/// its MCP entries under `~/.arca/assets/mcps/sources/local/<id>`. Unrelated user settings are
/// never copied into Arca's source store.
pub fn add_local(path: String, name: Option<String>) -> Result<SourceView, String> {
    let src = expand_tilde(&path);
    let content = fs::read_to_string(&src).map_err(|e| format!("读取文件失败: {e}"))?;
    let format = detect_format(&path, &content).to_string();
    validate_parseable(&content, &format)?;
    let display = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            Path::new(&path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("本地配置")
                .to_string()
        });
    let now = now_iso();
    let mut def = SourceDef::new_local(
        gen_id("local", &display),
        display,
        Some(collapse_home(&path)),
        format,
        now,
    );
    let cache = cached_path(&def).ok_or("无法确定缓存路径")?;
    let sanitized = sanitized_local_content(&src, &content, &def.format, &def.key)?;
    let entries = parse_candidate_content(&def, &sanitized)?;
    let count = entries.len() as u32;
    def.server_count = Some(count);
    if count == 0 {
        def.error = Some("未在该文件中发现 MCP server".into());
    }
    register_cached_source(&def, &cache, &sanitized, entries)?;
    Ok(to_view(def, count))
}

/// Add the bundled curated collection as an opt-in *local* source (not part of
/// the default catalog). Serializes the embedded `data/registry.json`.
pub fn add_official() -> Result<SourceView, String> {
    let content = curated_collection_content()?;
    let now = now_iso();
    let mut def = SourceDef::new_local(
        gen_id("local", "curated"),
        CURATED_SOURCE_NAME.into(),
        None,
        "json".into(),
        now,
    );
    let cache = cached_path(&def).ok_or("无法确定缓存路径")?;
    let entries = parse_candidate_content(&def, &content)?;
    let count = entries.len() as u32;
    def.server_count = Some(count);
    register_cached_source(&def, &cache, &content, entries)?;
    Ok(to_view(def, count))
}

fn require_user_managed_source(id: &str) -> Result<(), String> {
    if id == MANUAL_ID || id == DISCOVERED_ID {
        return Err("source_managed: Arca maintains this source; edit individual central MCP assets instead".into());
    }
    Ok(())
}

/// Re-fetch (remote) or re-copy (local) a source's file and update its status.
pub(crate) struct PreparedSourceRefresh {
    original: SourceDef,
    path: PathBuf,
    previous: SourceFileSnapshot,
    previous_entries: Vec<RegistryEntry>,
}

pub(crate) struct ProbedSourceRefresh {
    prepared: PreparedSourceRefresh,
    fetched: Result<String, String>,
}

pub fn refresh(id: String) -> Result<SourceView, String> {
    reconcile_refresh(probe_refresh(prepare_refresh(id)?))
}

pub(crate) fn prepare_refresh(id: String) -> Result<PreparedSourceRefresh, String> {
    require_user_managed_source(&id)?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let Some(original) = settings
        .sources
        .unwrap_or_default()
        .into_iter()
        .find(|d| d.id == id)
    else {
        return Err("source 不存在".into());
    };
    if original.kind == "remote" && original.url.is_none() { return Err("该来源缺少 URL".into()); }
    let path = cached_path(&original).ok_or("无法确定缓存路径")?;
    let previous = snapshot_source_file(&path)?;
    let previous_entries = entries_from_snapshot(&original, &previous)?;

    Ok(PreparedSourceRefresh { original, path, previous, previous_entries })
}

/// No global application or settings lock may span this fetch. The original
/// source definition and cached bytes are revalidated during reconciliation.
pub(crate) fn probe_refresh(prepared: PreparedSourceRefresh) -> ProbedSourceRefresh {
    let original = &prepared.original;
    let curated = is_curated_source(&original);
    let fetched: Result<String, String> = match original.kind.as_str() {
        "remote" => {
            match original.url.as_deref() {
                Some(url) => fetch(url),
                None => Err("该来源缺少 URL".into()),
            }
        }
        "local" => match original.path.as_ref() {
            Some(p) => {
                let src = expand_tilde(p);
                fs::read_to_string(&src).map_err(|e| format!("读取原文件失败: {e}"))
            }
            None if curated => curated_collection_content(),
            None => Err("该本地来源没有可刷新的原文件".into()),
        },
        _ => Err("不支持刷新该来源".into()),
    };

    ProbedSourceRefresh { prepared, fetched }
}

pub(crate) fn reconcile_refresh(probed: ProbedSourceRefresh) -> Result<SourceView, String> {
    let ProbedSourceRefresh { prepared, fetched } = probed;
    let PreparedSourceRefresh { original, path, previous, previous_entries } = prepared;
    let curated = is_curated_source(&original);
    let body = match fetched {
        Ok(body) => body,
        Err(fetch_error) => {
            let mut updated = original.clone();
            updated.error = Some(fetch_error);
            updated.server_count = Some(previous_entries.len() as u32);
            let saved = updated.clone();
            let expected = original.clone();
            mutate_settings_checked(move |settings| {
                let list = settings
                    .sources
                    .as_mut()
                    .ok_or_else(|| io_invalid("source 不存在"))?;
                let current = list
                    .iter_mut()
                    .find(|source| source.id == expected.id)
                    .ok_or_else(|| io_invalid("source 不存在"))?;
                if current != &expected {
                    return Err(io_invalid(format!(
                        "source {} 在刷新期间已变化，拒绝覆盖",
                        expected.id
                    )));
                }
                *current = saved;
                Ok(())
            })
            .map_err(|error| error.to_string())?;
            return Ok(to_view(updated, previous_entries.len() as u32));
        }
    };

    validate_parseable(&body, &original.format)?;
    let cached_body = if curated {
        body
    } else if original.kind == "local" {
        let source = original
            .path
            .as_deref()
            .map(expand_tilde)
            .ok_or("该本地来源没有可刷新的原文件")?;
        sanitized_local_content(&source, &body, &original.format, &original.key)?
    } else {
        body
    };
    let candidate_entries = parse_candidate_content(&original, &cached_body)?;
    let count = candidate_entries.len() as u32;
    let mut updated = original.clone();
    updated.synced_at = Some(now_iso());
    updated.server_count = Some(count);
    updated.error = (count == 0).then(|| "未在该文件中发现 MCP server".into());

    let mut cache_written = false;
    let settings_result = mutate_settings_checked(|settings| {
        let mut candidate_sources = settings.sources.clone().unwrap_or_default();
        let Some(index) = candidate_sources
            .iter()
            .position(|source| source.id == original.id)
        else {
            return Err(io_invalid("source 不存在"));
        };
        if candidate_sources[index] != original {
            return Err(io_invalid(format!(
                "source {} 在刷新期间已变化，拒绝覆盖",
                original.id
            )));
        }
        candidate_sources[index] = updated.clone();
        let before = BTreeMap::from([(original.id.clone(), previous_entries.clone())]);
        let after = BTreeMap::from([(original.id.clone(), candidate_entries.clone())]);
        ensure_no_unreviewed_desired_change(settings, &candidate_sources, &before, &after)?;
        // Keep the settings filesystem lock across the final review check and
        // cache replacement so another process cannot add a desired consumer
        // in between them.
        replace_source_file(&path, &previous, &cached_body).map_err(Error::other)?;
        cache_written = true;
        settings.sources = Some(candidate_sources);
        Ok(())
    });
    if let Err(error) = settings_result {
        if cache_written {
            return match restore_source_file(&path, &cached_body, &previous) {
                Ok(()) => Err(error.to_string()),
                Err(rollback_error) => Err(format!(
                    "{error}; source cache 回滚失败 ({}): {rollback_error}",
                    path.display()
                )),
            };
        }
        return Err(error.to_string());
    }
    Ok(to_view(updated, count))
}

/// Enable or disable a source (its servers join/leave the catalog).
pub fn set_enabled(id: String, enabled: bool) -> Result<(), String> {
    mutate_settings_checked(move |settings| {
        let mut candidate_sources = settings.sources.clone().unwrap_or_default();
        let source = candidate_sources
            .iter_mut()
            .find(|source| source.id == id)
            .ok_or_else(|| io_invalid("source 不存在"))?;
        if source.enabled == enabled {
            return Ok(());
        }
        source.enabled = enabled;
        ensure_no_unreviewed_desired_change(
            settings,
            &candidate_sources,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )?;
        settings.sources = Some(candidate_sources);
        Ok(())
    })
    .map_err(|error| error.to_string())
}

/// Remove a source and delete its cached file.
pub fn remove(id: String) -> Result<(), String> {
    require_user_managed_source(&id)?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let expected = settings
        .sources
        .unwrap_or_default()
        .into_iter()
        .find(|source| source.id == id)
        .ok_or("source 不存在")?;
    let path = cached_path(&expected).ok_or("无法确定缓存路径")?;
    let cache = snapshot_source_file(&path)?;
    let expected_for_mutation = expected.clone();
    let (removed, index) = mutate_settings_checked(move |settings| {
        let mut candidate_sources = settings.sources.clone().unwrap_or_default();
        let Some(index) = candidate_sources
            .iter()
            .position(|source| source.id == expected_for_mutation.id)
        else {
            return Err(io_invalid("source 不存在"));
        };
        if candidate_sources[index] != expected_for_mutation {
            return Err(io_invalid(format!(
                "source {} 在删除期间已变化，拒绝覆盖",
                expected_for_mutation.id
            )));
        }
        let removed = candidate_sources.remove(index);
        ensure_no_unreviewed_desired_change(
            settings,
            &candidate_sources,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )?;
        settings.sources = Some(candidate_sources);
        Ok((removed, index))
    })
    .map_err(|error| error.to_string())?;

    if let Err(error) = remove_source_file(&path, &cache) {
        let rollback_id = removed.id.clone();
        let rollback = mutate_settings_checked(move |settings| {
            let list = settings.sources.get_or_insert_default();
            if list.iter().any(|source| source.id == rollback_id) {
                return Err(io_invalid(format!(
                    "无法回滚 source {rollback_id}: id 已重新出现"
                )));
            }
            list.insert(index.min(list.len()), removed);
            Ok(())
        });
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(format!(
                "{error}; source registration 回滚失败: {rollback_error}"
            )),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assets::McpConsumptionRecord;
    use crate::domain::mcp::OverridePatch;
    use crate::paths::{local_sources_dir, settings_file};
    use crate::settings::mutate_settings;
    use crate::testenv::TestHome;

    fn tmp(name: &str, ext: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("arca-src-{}-{}.{}", name, std::process::id(), ext));
        d
    }

    fn origin() -> RegistryOrigin {
        RegistryOrigin {
            kind: "local".into(),
            agent: None,
            scope: None,
            source: Some("s1".into()),
        }
    }

    fn write_local_source(path: &Path, version: &str) {
        fs::write(
            path,
            format!(r#"{{"mcpServers":{{"docs":{{"command":"npx","args":["{version}"]}}}}}}"#),
        )
        .unwrap();
    }

    fn add_desired_consumption(agent_id: &str, asset_key: &str) {
        mutate_settings(|settings| {
            settings
                .mcp_consumptions
                .get_or_insert_default()
                .entry(agent_id.to_string())
                .or_default()
                .insert(
                    asset_key.to_string(),
                    McpConsumptionRecord {
                        asset_key: asset_key.to_string(),
                        enabled: true,
                        overrides: OverridePatch::default(),
                    },
                );
        })
        .unwrap();
    }

    #[test]
    fn refresh_rejects_a_source_changed_while_the_probe_was_outside_the_gate() {
        let home = TestHome::new("src-refresh-definition-race");
        let source = home.home.join("source.json");
        write_local_source(&source, "v1");
        let view = add_local(source.to_string_lossy().into_owned(), None).unwrap();
        let prepared = prepare_refresh(view.id.clone()).unwrap();
        write_local_source(&source, "v2");
        let probed = probe_refresh(prepared);
        set_enabled(view.id.clone(), false).unwrap();
        assert!(reconcile_refresh(probed).is_err());
        let current = load_settings().sources.unwrap().into_iter().find(|source| source.id == view.id).unwrap();
        assert!(!current.enabled);
        assert_eq!(source_entries(&current)[0].config.stdio.as_ref().unwrap().args.as_ref().unwrap(), &["v1"]);
    }

    #[test]
    fn refresh_never_overwrites_cache_changed_during_the_unlocked_probe() {
        let home = TestHome::new("src-refresh-cache-race");
        let source = home.home.join("source.json");
        write_local_source(&source, "v1");
        let view = add_local(source.to_string_lossy().into_owned(), None).unwrap();
        let prepared = prepare_refresh(view.id).unwrap();
        let cache = prepared.path.clone();
        write_local_source(&source, "v2");
        let probed = probe_refresh(prepared);
        let concurrent = "[]\n";
        fs::write(&cache, concurrent).unwrap();
        assert!(reconcile_refresh(probed).is_err());
        assert_eq!(fs::read_to_string(cache).unwrap(), concurrent);
    }

    #[test]
    fn parses_standard_mcpservers_map() {
        let p = tmp("map", "json");
        fs::write(
            &p,
            r#"{"mcpServers":{
                "git":{"command":"npx","args":["-y","git-mcp"]},
                "wiki":{"url":"https://deepwiki.example/mcp","type":"http"}
            }}"#,
        )
        .unwrap();
        let mut entries = parse_file(&p, "json", "mcpServers", &origin());
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "git");
        assert_eq!(entries[0].transport(), "stdio");
        assert_eq!(entries[0].origin.as_ref().unwrap().kind, "local");
        assert_eq!(entries[1].name, "wiki");
        assert_eq!(entries[1].transport(), "http");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn parses_rich_registry_array_keeping_meta() {
        let p = tmp("arr", "json");
        fs::write(
            &p,
            r#"[{"name":"filesystem","description":"local fs","tags":["files"],
                "config":{"stdio":{"command":"npx","args":["-y","fs"]}}}]"#,
        )
        .unwrap();
        let entries = parse_file(&p, "json", "mcpServers", &origin());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description, "local fs"); // rich meta preserved
        assert_eq!(entries[0].tags, vec!["files".to_string()]);
        assert_eq!(
            entries[0].origin.as_ref().unwrap().source.as_deref(),
            Some("s1")
        );
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn local_toml_cache_contains_only_mcp_table() {
        let p = tmp("toml-private", "toml");
        let content = r#"model = "private-model"
token = "must-not-be-cached"

[history]
persistence = "save-all"

[mcp_servers.github]
command = "npx"
args = ["-y", "github-mcp"]
"#;
        fs::write(&p, content).unwrap();

        let cached = sanitized_local_content(&p, content, "toml", "mcp_servers").unwrap();

        assert!(cached.contains("[mcp_servers.github]"));
        assert!(cached.contains("command = \"npx\""));
        assert!(!cached.contains("private-model"));
        assert!(!cached.contains("must-not-be-cached"));
        assert!(!cached.contains("history"));
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn detect_and_validate() {
        assert_eq!(detect_format("https://x/y.toml", "irrelevant"), "toml");
        assert_eq!(detect_format("nohint", "{\"a\":1}"), "json");
        assert!(validate_parseable("{\"a\":1}", "json").is_ok());
        assert!(validate_parseable("not json", "json").is_err());
        assert_eq!(
            host_of("https://raw.githubusercontent.com/x/y/main/f.json"),
            "raw.githubusercontent.com"
        );
    }

    #[test]
    fn cached_path_by_kind() {
        let mut d = SourceDef {
            id: "abc".into(),
            kind: "remote".into(),
            name: "n".into(),
            url: None,
            path: None,
            format: "json".into(),
            key: "mcpServers".into(),
            enabled: true,
            added_at: None,
            synced_at: None,
            server_count: None,
            error: None,
        };
        assert!(cached_path(&d).unwrap().ends_with("remote/abc.json"));
        d.kind = "local".into();
        d.format = "toml".into();
        assert!(cached_path(&d).unwrap().ends_with("local/abc.toml"));
    }

    #[test]
    fn legacy_curated_source_name_is_normalized() {
        let def = SourceDef {
            id: "curated".into(),
            kind: "remote".into(),
            name: LEGACY_CURATED_SOURCE_NAME.into(),
            url: None,
            path: None,
            format: "json".into(),
            key: "mcpServers".into(),
            enabled: true,
            added_at: None,
            synced_at: None,
            server_count: None,
            error: None,
        };

        assert_eq!(to_view(def, 0).name, CURATED_SOURCE_NAME);
    }

    #[test]
    fn curated_source_refreshes_from_the_current_embedded_collection() {
        let _home = TestHome::new("curated-refresh");
        let view = add_official().unwrap();
        let source = load_settings()
            .sources
            .unwrap()
            .into_iter()
            .find(|source| source.id == view.id)
            .unwrap();
        let cache = cached_path(&source).unwrap();
        fs::write(&cache, "[]").unwrap();

        let refreshed = refresh(view.id).unwrap();
        assert_eq!(refreshed.server_count, builtin_registry().len() as u32);

        let current = load_settings()
            .sources
            .unwrap()
            .into_iter()
            .find(|source| source.id == refreshed.id)
            .unwrap();
        let everything = source_entries(&current)
            .into_iter()
            .find(|entry| entry.name == "everything-test")
            .expect("refreshed Arca 精选 must include everything-test");
        let stdio = everything.config.stdio.unwrap();
        assert_eq!(stdio.command, "npx");
        assert_eq!(
            stdio.args.unwrap(),
            ["-y", "@modelcontextprotocol/server-everything"]
        );
    }

    #[test]
    fn source_changes_fail_closed_for_a_desired_consumer() {
        let home = TestHome::new("src-review");
        let first = home.home.join("first.json");
        write_local_source(&first, "v1");
        let view = add_local(first.to_string_lossy().into_owned(), Some("first".into())).unwrap();
        let source_before = load_settings()
            .sources
            .unwrap()
            .into_iter()
            .find(|source| source.id == view.id)
            .unwrap();
        let cache = cached_path(&source_before).unwrap();
        let cache_before = fs::read_to_string(&cache).unwrap();
        add_desired_consumption("claude-code", "docs::stdio");

        write_local_source(&first, "v2");
        let refresh_error = refresh(view.id.clone()).unwrap_err();
        assert!(
            refresh_error.contains("desired consumption"),
            "{refresh_error}"
        );
        assert_eq!(fs::read_to_string(&cache).unwrap(), cache_before);
        assert_eq!(
            load_settings()
                .sources
                .unwrap()
                .into_iter()
                .find(|source| source.id == view.id)
                .unwrap(),
            source_before
        );

        let toggle_error = set_enabled(view.id.clone(), false).unwrap_err();
        assert!(
            toggle_error.contains("desired consumption"),
            "{toggle_error}"
        );
        assert!(
            load_settings()
                .sources
                .as_ref()
                .unwrap()
                .iter()
                .find(|source| source.id == view.id)
                .unwrap()
                .enabled
        );

        let remove_error = remove(view.id.clone()).unwrap_err();
        assert!(
            remove_error.contains("desired consumption"),
            "{remove_error}"
        );
        assert!(load_settings()
            .sources
            .as_ref()
            .unwrap()
            .iter()
            .any(|source| source.id == view.id));
        assert!(cache.exists());

        let second = home.home.join("second.json");
        write_local_source(&second, "v3");
        let add_error =
            add_local(second.to_string_lossy().into_owned(), Some("second".into())).unwrap_err();
        assert!(add_error.contains("desired consumption"), "{add_error}");
        assert_eq!(load_settings().sources.unwrap().len(), 1);
        assert_eq!(fs::read_dir(local_sources_dir()).unwrap().count(), 1);
    }

    #[test]
    fn automatic_sources_cannot_be_deleted_or_refreshed_through_core() {
        let _home = TestHome::new("src-managed-protection");
        let entry = builtin_registry().into_iter().next().unwrap();
        crate::resources::mcp::registry::write_manual_entry(&entry).unwrap();
        crate::resources::mcp::registry::write_discovered_entry(&entry).unwrap();
        let settings_before = fs::read(settings_file()).unwrap();
        let caches = load_settings().sources.unwrap().iter()
            .filter(|source| source.id == MANUAL_ID || source.id == DISCOVERED_ID)
            .map(|source| {
                let path = cached_path(source).unwrap();
                let content = fs::read(&path).unwrap();
                (path, content)
            }).collect::<Vec<_>>();
        assert_eq!(caches.len(), 2);
        for id in [MANUAL_ID, DISCOVERED_ID] {
            assert!(remove(id.into()).unwrap_err().starts_with("source_managed:"));
            assert!(refresh(id.into()).unwrap_err().starts_with("source_managed:"));
        }
        assert_eq!(fs::read(settings_file()).unwrap(), settings_before);
        for (path, content) in caches {
            assert_eq!(fs::read(path).unwrap(), content);
        }
    }

    #[test]
    fn registration_rolls_cache_back_when_settings_transaction_fails() {
        let home = TestHome::new("src-setfail");
        let source = home.home.join("source.json");
        write_local_source(&source, "v1");
        let settings_path = settings_file();
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(&settings_path, "{ invalid settings").unwrap();

        let error = add_local(
            source.to_string_lossy().into_owned(),
            Some("rollback".into()),
        )
        .unwrap_err();

        assert!(error.contains("invalid Arca settings"), "{error}");
        assert_eq!(
            fs::read_to_string(settings_path).unwrap(),
            "{ invalid settings"
        );
        assert_eq!(
            fs::read_dir(local_sources_dir())
                .map(|entries| entries.count())
                .unwrap_or(0),
            0,
            "failed registration must not leave an orphan cache"
        );
    }

    #[cfg(unix)]
    #[test]
    fn refresh_restores_cache_when_settings_save_fails() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let home = TestHome::new("src-refresh-rollback");
        let source = home.home.join("source.json");
        write_local_source(&source, "v1");
        let view = add_local(
            source.to_string_lossy().into_owned(),
            Some("refresh".into()),
        )
        .unwrap();
        let def = load_settings()
            .sources
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == view.id)
            .unwrap();
        let cache = cached_path(&def).unwrap();
        let cache_before = fs::read_to_string(&cache).unwrap();
        fs::write(
            &source,
            r#"{"mcpServers":{"docs":{"command":"npx","args":["v2"]},"search":{"command":"npx","args":["search"]}}}"#,
        )
        .unwrap();

        let settings_path = settings_file();
        let settings_before = fs::read_to_string(&settings_path).unwrap();
        let read_only_dir = home.home.join("read-only-settings");
        fs::create_dir(&read_only_dir).unwrap();
        let read_only_settings = read_only_dir.join("settings.json");
        fs::write(&read_only_settings, &settings_before).unwrap();
        fs::remove_file(&settings_path).unwrap();
        symlink(&read_only_settings, &settings_path).unwrap();
        fs::set_permissions(&read_only_dir, fs::Permissions::from_mode(0o500)).unwrap();

        let result = refresh(view.id);
        fs::set_permissions(&read_only_dir, fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            result.is_err(),
            "read-only settings target must reject save"
        );
        assert_eq!(
            fs::read_to_string(&cache).unwrap(),
            cache_before,
            "cache must return to the exact pre-refresh content"
        );
        assert_eq!(
            fs::read_to_string(&read_only_settings).unwrap(),
            settings_before
        );
    }

    #[cfg(unix)]
    #[test]
    fn remove_restores_registration_when_cache_delete_fails() {
        use std::os::unix::fs::PermissionsExt;

        let home = TestHome::new("src-remove-rollback");
        let source = home.home.join("source.json");
        write_local_source(&source, "v1");
        let view = add_local(source.to_string_lossy().into_owned(), Some("remove".into())).unwrap();
        let def = load_settings()
            .sources
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == view.id)
            .unwrap();
        let cache = cached_path(&def).unwrap();
        let parent = cache.parent().unwrap();
        let original_permissions = fs::metadata(parent).unwrap().permissions();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();

        let result = remove(view.id.clone());
        fs::set_permissions(parent, original_permissions).unwrap();

        assert!(
            result.is_err(),
            "read-only cache directory must reject delete"
        );
        assert!(cache.exists());
        assert!(
            load_settings()
                .sources
                .as_ref()
                .unwrap()
                .iter()
                .any(|source| source.id == view.id),
            "settings removal must roll back when cache deletion fails"
        );
    }
}
