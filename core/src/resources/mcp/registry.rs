//! MCP central asset catalog.

use crate::domain::types::{RegistryEntry, RegistryOrigin, SourceDef};
use crate::paths::settings_file;
use crate::resources::mcp::sources::{cached_path, source_entries};
use crate::safe_write::{acquire_settings_lock, write_private_if_unchanged, SettingsLock};
use crate::settings::{load_settings, load_settings_strict, mutate_settings, Settings};
use serde::Serialize;
use std::fs;
use std::path::Path;

/// The bundled "official" collection, embedded at compile time from the repo-root
/// data/registry.json. It is **not a built-in catalog base** — the catalog is
/// entirely source-driven. Exposed only so a client can opt in through
/// `sources::add_official`.
const BUILTIN_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/registry.json"
));

fn mutation_lock() -> std::io::Result<SettingsLock> {
    acquire_settings_lock(&settings_file()).map_err(std::io::Error::other)
}

pub fn builtin_registry() -> Vec<RegistryEntry> {
    serde_json::from_str(BUILTIN_JSON).expect("registry.json must be valid")
}

/// Ids of the two **managed** local sources. Manually-created and auto-discovered
/// entries are stored as ordinary local-source files under
/// `~/.arca/assets/mcps/sources/local/<id>.json`, exactly like an added local file — not in
/// `settings.registry`. They appear in the Sources list like any other source.
pub const MANUAL_ID: &str = "manual";
pub const DISCOVERED_ID: &str = "discovered";

/// An in-memory `SourceDef` for a managed local source (enough to resolve its
/// cached file path and parse it). Not persisted by itself.
fn managed_def(id: &str, name: &str) -> SourceDef {
    SourceDef {
        id: id.into(),
        kind: "local".into(),
        name: name.into(),
        url: None,
        path: None,
        format: "json".into(),
        key: "mcpServers".into(),
        enabled: true,
        added_at: Some(super::sources::now_iso()),
        synced_at: None,
        server_count: None,
        error: None,
    }
}

/// Register a managed source in `settings.sources` if it isn't there yet.
fn ensure_managed(settings: &mut Settings, id: &str, name: &str) {
    let list = settings.sources.get_or_insert_with(Vec::new);
    if !list.iter().any(|d| d.id == id) {
        list.push(managed_def(id, name));
    }
}

fn read_array_for_update(path: &Path) -> std::io::Result<(Vec<RegistryEntry>, Option<String>)> {
    let source = match fs::read_to_string(path) {
        Ok(source) => Some(source),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let list = match source.as_deref() {
        Some(source) => serde_json::from_str::<Vec<RegistryEntry>>(source)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        None => Vec::new(),
    };
    Ok((list, source))
}

fn write_array(path: &Path, expected: Option<&str>, list: &[RegistryEntry]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(list)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_private_if_unchanged(path, expected, &json).map_err(std::io::Error::other)
}

/// The entries currently stored in a managed source's file (origins preserved).
fn managed_entries(id: &str) -> Vec<RegistryEntry> {
    source_entries(&managed_def(id, id))
}

/// Upsert `entry` (by composite key) into a managed source's rich-array file,
/// creating the source registration on first use.
fn write_managed(id: &str, name: &str, entry: RegistryEntry) -> std::io::Result<()> {
    mutate_settings(|s| ensure_managed(s, id, name))?;
    let path = cached_path(&managed_def(id, name)).expect("managed source has a cached path");
    let (mut list, source) = read_array_for_update(&path)?;
    let key = entry.key();
    list.retain(|e| e.key() != key);
    list.push(entry);
    write_array(&path, source.as_deref(), &list)
}

/// Remove the entry matching `target_key` from a managed source's file.
fn remove_managed(id: &str, name: &str, target_key: &str) -> std::io::Result<()> {
    let path = cached_path(&managed_def(id, name)).expect("managed source has a cached path");
    let (mut list, source) = read_array_for_update(&path)?;
    let before = list.len();
    list.retain(|e| e.key() != target_key);
    if list.len() != before {
        write_array(&path, source.as_deref(), &list)?;
    }
    Ok(())
}

/// Store a user-created / edited catalog entry into the **manual** local source
/// (the user's own layer, highest precedence). Origin is normalized to "manual".
pub fn write_manual_entry(entry: &RegistryEntry) -> std::io::Result<()> {
    let _settings_guard = mutation_lock()?;
    let mut e = entry.clone();
    e.origin = Some(RegistryOrigin {
        kind: "manual".into(),
        agent: None,
        scope: None,
        source: Some(MANUAL_ID.into()),
    });
    write_managed(MANUAL_ID, "手动添加", e)
}

/// Store an auto-discovered entry into the **discovered** local source. The
/// entry keeps its `origin` ("discovered" + source app).
pub fn write_discovered_entry(entry: &RegistryEntry) -> std::io::Result<()> {
    let _settings_guard = mutation_lock()?;
    write_managed(DISCOVERED_ID, "自动探索", entry.clone())
}

/// Every copy of every entry from all enabled sources, concatenated in
/// precedence order (low→high), **without** deduping:
///   external sources (subscribed remote + added local, in list order)
///     < discovered (auto-detected)
///     < manual (the user's own edits win over everything).
/// The last copy of a given composite key in this order is the one that "wins"
/// in `read_registry`. Each entry carries its own `origin`.
fn enabled_source_entries_in_order_for(defs: &[SourceDef]) -> Vec<RegistryEntry> {
    let mut out: Vec<RegistryEntry> = Vec::new();
    // 1. external sources (everything that isn't a managed source), in order.
    for def in defs
        .iter()
        .filter(|d| d.enabled && d.id != MANUAL_ID && d.id != DISCOVERED_ID)
    {
        out.extend(source_entries(def));
    }
    // 2. discovered layer.
    if let Some(def) = defs.iter().find(|d| d.id == DISCOVERED_ID && d.enabled) {
        out.extend(source_entries(def));
    }
    // 3. manual layer (highest precedence).
    if let Some(def) = defs.iter().find(|d| d.id == MANUAL_ID && d.enabled) {
        out.extend(source_entries(def));
    }
    out
}

fn enabled_source_entries_in_order() -> Vec<RegistryEntry> {
    let defs = load_settings().sources.unwrap_or_default();
    enabled_source_entries_in_order_for(&defs)
}

/// All catalog entries, assembled from every enabled source and deduped by
/// composite key (`name::transport`) with the precedence order above (manual
/// wins). This is the **effective** catalog used by install / scan / edit
/// propagation — its dedup semantics are load-bearing, do not change them.
pub fn read_registry() -> Vec<RegistryEntry> {
    use std::collections::BTreeMap;
    let mut by_key: BTreeMap<String, RegistryEntry> = BTreeMap::new();
    for e in enabled_source_entries_in_order() {
        by_key.insert(e.key(), e);
    }
    by_key.into_values().collect()
}

/// One entry copy plus whether it's the one that wins precedence (in effect).
#[derive(Debug, Clone, Serialize)]
pub struct CatalogItem {
    pub entry: RegistryEntry,
    /// True for the copy that `read_registry` keeps (highest precedence for its
    /// composite key); false for copies shadowed by a higher-precedence source.
    pub in_effect: bool,
}

#[derive(Debug, Serialize)]
pub struct RegistrySnapshot {
    pub entries: Vec<RegistryEntry>,
    pub catalog: Vec<CatalogItem>,
    pub custom_keys: Vec<String>,
    pub sources: Vec<super::sources::SourceView>,
}

/// Read each source once, then derive all three library views from that read.
/// This is a point-in-time observation, not a cache used by mutation planning.
pub fn read_registry_snapshot() -> RegistrySnapshot {
    let defs = load_settings().sources.unwrap_or_default();
    let manual = managed_entries(MANUAL_ID);
    let custom_keys = manual.iter().map(RegistryEntry::key).collect();
    let mut sources = Vec::new();
    let mut ordered = Vec::new();
    let mut discovered = None;
    let mut enabled_manual = None;
    for def in defs {
        let entries = if def.id == MANUAL_ID && def.kind == "local"
            && def.format == "json" && def.key == "mcpServers" {
            manual.clone()
        } else {
            source_entries(&def)
        };
        let count = entries.len() as u32;
        if def.enabled {
            match def.id.as_str() {
                DISCOVERED_ID => { discovered.get_or_insert(entries); }
                MANUAL_ID => { enabled_manual.get_or_insert(entries); }
                _ => ordered.extend(entries),
            }
        }
        sources.push(super::sources::to_view(def, count));
    }
    ordered.extend(discovered.into_iter().flatten());
    ordered.extend(enabled_manual.into_iter().flatten());
    let catalog = flag_in_effect(ordered);
    let mut entries: Vec<_> = catalog.iter().filter(|item| item.in_effect)
        .map(|item| item.entry.clone()).collect();
    entries.sort_by_cached_key(RegistryEntry::key);
    RegistrySnapshot { entries, catalog, custom_keys, sources }
}

/// Every copy of every entry from all enabled sources (**not** deduped), each
/// flagged with whether it's the in-effect (winning) copy for its composite key.
/// For display only — lets the Registry show shadowed copies that `read_registry`
/// would drop, and mark which source's copy actually takes effect.
pub fn read_registry_all() -> Vec<CatalogItem> {
    flag_in_effect(enabled_source_entries_in_order())
}

/// Project every source copy from one already-loaded settings snapshot.
///
/// Asset planning uses this form so all semantic preconditions share the same
/// settings read instead of silently reloading a different source definition
/// list halfway through the projection.
pub(crate) fn read_registry_all_for_settings(settings: &Settings) -> Vec<CatalogItem> {
    flag_in_effect(enabled_source_entries_in_order_for(
        settings.sources.as_deref().unwrap_or_default(),
    ))
}

/// Flag each entry with whether it's the last (highest-precedence) copy of its
/// composite key in `ordered` — the copy `read_registry`'s dedup would keep.
/// Pure over its input, so it's unit-testable without touching `~/.arca`.
fn flag_in_effect(ordered: Vec<RegistryEntry>) -> Vec<CatalogItem> {
    use std::collections::HashMap;
    let mut last_idx: HashMap<String, usize> = HashMap::new();
    for (i, e) in ordered.iter().enumerate() {
        last_idx.insert(e.key(), i);
    }
    ordered
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let in_effect = last_idx.get(&entry.key()) == Some(&i);
            CatalogItem { entry, in_effect }
        })
        .collect()
}

/// Composite keys of entries in the **manual** source — i.e. user overrides that
/// the UI can revert ("恢复默认").
pub fn user_override_keys() -> Vec<String> {
    managed_entries(MANUAL_ID).iter().map(|e| e.key()).collect()
}

/// All entries currently stored in the **manual** source (the user's own,
/// hand-added / edited layer). Used to export them as a shareable config file.
pub fn manual_entries() -> Vec<RegistryEntry> {
    managed_entries(MANUAL_ID)
}

/// Remove a user override (`name`+`transport`) from the manual source. If another
/// source still provides that key, it shows through again. A missing entry is a
/// no-op success.
pub fn delete_registry_entry(name: &str, transport: &str) -> std::io::Result<()> {
    let _settings_guard = mutation_lock()?;
    remove_managed(MANUAL_ID, "手动添加", &format!("{name}::{transport}"))
}

/// Remove an auto-discovered entry (`name`+`transport`) from the discovered
/// source. It may reappear on the next scan if still present in an agent's config.
/// A missing entry is a no-op success.
pub fn delete_discovered_entry(name: &str, transport: &str) -> std::io::Result<()> {
    let _settings_guard = mutation_lock()?;
    remove_managed(DISCOVERED_ID, "自动探索", &format!("{name}::{transport}"))
}

/// One-time migration: fold any legacy `settings.registry` entries into the
/// managed source files (discovered→discovered, everything else→manual), then
/// clear `settings.registry`. Idempotent (no-op once the section is empty).
pub fn migrate_registry_to_sources() -> std::io::Result<bool> {
    let _settings_guard = mutation_lock()?;
    let Some(entries) = load_settings_strict()?.registry.filter(|r| !r.is_empty()) else {
        return Ok(false);
    };
    for e in entries {
        let discovered = e
            .origin
            .as_ref()
            .map(|o| o.kind == "discovered")
            .unwrap_or(false);
        if discovered {
            write_discovered_entry(&e)?;
        } else {
            write_manual_entry(&e)?;
        }
    }
    mutate_settings(|s| s.registry = None)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use std::collections::HashSet;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn builtin_collection_is_valid() {
        let entries = builtin_registry();
        assert!(!entries.is_empty(), "curated collection must not be empty");

        let mut keys = HashSet::new();
        let secrets_are_placeholders = |values: &std::collections::HashMap<String, String>| {
            values.iter().all(|(key, value)| {
                let key = key.to_ascii_lowercase();
                let sensitive = ["key", "token", "secret", "password"]
                    .iter()
                    .any(|needle| key.contains(needle));
                !sensitive || value.is_empty() || value.contains('$')
            })
        };

        for entry in entries {
            assert!(
                !entry.name.trim().is_empty(),
                "entry name must not be empty"
            );
            assert!(
                !entry.description.trim().is_empty(),
                "{}: description must not be empty",
                entry.name
            );
            assert!(
                !entry.tags.is_empty(),
                "{}: tags must not be empty",
                entry.name
            );
            assert!(
                keys.insert(entry.key()),
                "{}: duplicate identity",
                entry.name
            );

            match (&entry.config.stdio, &entry.config.http) {
                (Some(stdio), None) => {
                    assert!(
                        !stdio.command.trim().is_empty(),
                        "{}: stdio command must not be empty",
                        entry.name
                    );
                    if let Some(args) = &stdio.args {
                        assert!(
                            args.iter().all(|arg| !arg.trim().is_empty()),
                            "{}: stdio args must not contain empty values",
                            entry.name
                        );
                    }
                    if let Some(env) = &stdio.env {
                        assert!(
                            secrets_are_placeholders(env),
                            "{}: do not embed secrets in the public collection",
                            entry.name
                        );
                    }
                }
                (None, Some(http)) => {
                    assert!(
                        !http.kind.trim().is_empty(),
                        "{}: HTTP type is required",
                        entry.name
                    );
                    assert!(
                        http.url.starts_with("https://"),
                        "{}: remote URL must use HTTPS",
                        entry.name
                    );
                    if let Some(headers) = &http.headers {
                        assert!(
                            secrets_are_placeholders(headers),
                            "{}: do not embed secrets in the public collection",
                            entry.name
                        );
                    }
                }
                _ => panic!("{}: configure exactly one transport", entry.name),
            }

            if let Some(repo) = &entry.repo {
                assert!(
                    repo.starts_with("https://"),
                    "{}: repository URL must use HTTPS",
                    entry.name
                );
            }
            match entry.source_url.as_deref() {
                Some(source_url) => assert!(
                    source_url.starts_with("https://"),
                    "{}: official source URL must use HTTPS",
                    entry.name
                ),
                None => assert!(
                    entry.name == "luma",
                    "{}: curated templates record an official source URL",
                    entry.name
                ),
            }
        }
    }

    fn builtin_entry<'a>(
        entries: &'a [RegistryEntry],
        name: &str,
        transport: &str,
    ) -> &'a RegistryEntry {
        entries
            .iter()
            .find(|entry| entry.name == name && entry.transport() == transport)
            .unwrap_or_else(|| panic!("missing curated entry {name}::{transport}"))
    }

    type StdioRegistryCase<'a> = (
        &'a str,
        &'a str,
        &'a [&'a str],
        &'a str,
        &'a str,
        &'a [&'a str],
    );

    #[test]
    fn curated_everything_test_matches_official_stdio_config() {
        let entries = builtin_registry();
        let entry = builtin_entry(&entries, "everything-test", "stdio");
        let stdio = entry.config.stdio.as_ref().unwrap();

        assert_eq!(stdio.command, "npx");
        assert_eq!(
            stdio.args.as_deref().unwrap(),
            ["-y", "@modelcontextprotocol/server-everything"]
        );
        assert!(
            stdio.env.is_none(),
            "everything-test requires no credentials"
        );
        assert_eq!(
            entry.repo.as_deref(),
            Some("https://github.com/modelcontextprotocol/servers/tree/main/src/everything")
        );
        assert!(
            ["development", "testing", "official"]
                .iter()
                .all(|tag| entry.tags.iter().any(|value| value == tag)),
            "everything-test required tags drifted"
        );
    }

    #[test]
    fn curated_web_research_entries_match_verified_configs() {
        let entries = builtin_registry();
        let stdio_cases: [StdioRegistryCase<'_>; 6] = [
            (
                "tavily",
                "npx",
                &["-y", "tavily-mcp@latest"],
                "TAVILY_API_KEY",
                "https://github.com/tavily-ai/tavily-mcp",
                &["web", "search", "crawl", "official"],
            ),
            (
                "firecrawl",
                "npx",
                &["-y", "firecrawl-mcp"],
                "FIRECRAWL_API_KEY",
                "https://github.com/firecrawl/firecrawl-mcp-server",
                &["web", "search", "crawl", "official"],
            ),
            (
                "brave-search",
                "npx",
                &[
                    "-y",
                    "@brave/brave-search-mcp-server",
                    "--transport",
                    "stdio",
                ],
                "BRAVE_API_KEY",
                "https://github.com/brave/brave-search-mcp-server",
                &["web", "search", "news", "images", "official"],
            ),
            (
                "perplexity",
                "npx",
                &["-y", "@perplexity-ai/mcp-server"],
                "PERPLEXITY_API_KEY",
                "https://github.com/perplexityai/modelcontextprotocol",
                &["search", "research", "citations", "ai", "official"],
            ),
            (
                "serper",
                "uvx",
                &["serper-mcp-server"],
                "SERPER_API_KEY",
                "https://github.com/garylab/serper-mcp-server",
                &["web", "search", "google", "community"],
            ),
            (
                "kagi",
                "uvx",
                &["kagimcp"],
                "KAGI_API_KEY",
                "https://github.com/kagisearch/kagimcp",
                &["web", "search", "privacy", "official"],
            ),
        ];

        for (name, command, args, env_key, repo, tags) in stdio_cases {
            let entry = builtin_entry(&entries, name, "stdio");
            let stdio = entry.config.stdio.as_ref().unwrap();
            assert_eq!(stdio.command, command, "{name}: command drifted");
            assert_eq!(
                stdio
                    .args
                    .as_deref()
                    .unwrap()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                args,
                "{name}: args drifted"
            );
            let env = stdio.env.as_ref().unwrap();
            assert_eq!(env.len(), 1, "{name}: unexpected environment fields");
            assert_eq!(
                env.get(env_key).map(String::as_str),
                Some(""),
                "{name}: credential must remain an empty placeholder"
            );
            assert_eq!(entry.repo.as_deref(), Some(repo), "{name}: repo drifted");
            assert!(
                tags.iter()
                    .all(|tag| entry.tags.iter().any(|value| value == tag)),
                "{name}: required tags drifted"
            );
        }

        let http_cases = [
            (
                "exa",
                "https://mcp.exa.ai/mcp?tools=web_search_advanced_exa",
                "https://exa.ai/docs/reference/exa-mcp",
                &["search", "research", "ai", "official"][..],
            ),
            (
                "serpapi",
                "https://mcp.serpapi.com/mcp",
                "https://serpapi.com/integrations/mcp",
                &["web", "search", "serp", "official"][..],
            ),
            (
                "jina",
                "https://mcp.jina.ai/v1",
                "https://github.com/jina-ai/MCP",
                &["web", "search", "extraction", "research", "official"][..],
            ),
            (
                "you",
                "https://api.you.com/mcp?profile=free",
                "https://you.com/docs/agents/mcp-server",
                &["search", "research", "official", "free"][..],
            ),
        ];

        for (name, url, repo, tags) in http_cases {
            let entry = builtin_entry(&entries, name, "http");
            let http = entry.config.http.as_ref().unwrap();
            assert_eq!(http.kind, "http", "{name}: HTTP type drifted");
            assert_eq!(http.url, url, "{name}: endpoint drifted");
            assert_eq!(entry.repo.as_deref(), Some(repo), "{name}: repo drifted");
            assert!(
                tags.iter()
                    .all(|tag| entry.tags.iter().any(|value| value == tag)),
                "{name}: required tags drifted"
            );
        }

        let you = builtin_entry(&entries, "you", "http");
        let you_http = you.config.http.as_ref().unwrap();
        assert!(
            you_http.headers.is_none(),
            "you: free profile must stay keyless"
        );
        assert!(
            !you_http.url.contains("/mcp/research") && !you_http.url.contains("/mcp/finance"),
            "you: do not attach the free profile to research or finance endpoints"
        );
        assert!(
            you.description.contains("you-search")
                && you.description.contains("you-discover")
                && you.description.contains("100"),
            "you: description must keep the free-profile tool and quota limits"
        );

        let serpapi = builtin_entry(&entries, "serpapi", "http");
        assert_eq!(
            serpapi
                .config
                .http
                .as_ref()
                .unwrap()
                .headers
                .as_ref()
                .unwrap()
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer ${SERPAPI_API_KEY}")
        );
        assert!(builtin_entry(&entries, "serper", "stdio")
            .tags
            .iter()
            .all(|tag| tag != "official"));
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name == "firecrawl")
                .map(RegistryEntry::transport)
                .collect::<HashSet<_>>(),
            HashSet::from(["http", "stdio"])
        );
        assert_eq!(
            builtin_entry(&entries, "firecrawl", "http")
                .config
                .http
                .as_ref()
                .unwrap()
                .url,
            "https://mcp.firecrawl.dev/v2/mcp"
        );
    }

    #[test]
    fn curated_verified_mcp_candidates_match_official_templates() {
        let entries = builtin_registry();
        assert_eq!(entries.len(), 180, "curated collection entry count drifted");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<HashSet<_>>()
                .len(),
            178,
            "curated collection name count drifted"
        );

        let context7 = builtin_entry(&entries, "context7", "http");
        let context7_http = context7.config.http.as_ref().unwrap();
        assert_eq!(context7_http.url, "https://mcp.context7.com/mcp");
        assert_eq!(
            context7_http
                .headers
                .as_ref()
                .unwrap()
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer ${CONTEXT7_API_KEY}")
        );
        assert!(
            context7_http
                .headers
                .as_ref()
                .unwrap()
                .keys()
                .all(|key| key != "CONTEXT7_API_KEY"),
            "context7 remote auth header is Authorization"
        );
        assert!(context7.description.contains("mcp/oauth"));
        assert_eq!(
            context7.source_url.as_deref(),
            Some("https://context7.com/docs/resources/all-clients")
        );
        assert!(
            builtin_entry(&entries, "luma", "stdio").source_url.is_none(),
            "luma has no recorded first-party documentation URL"
        );

        let github = builtin_entry(&entries, "github", "http");
        assert_eq!(
            github.config.http.as_ref().unwrap().url,
            "https://api.githubcopilot.com/mcp/"
        );
        assert!(github.config.http.as_ref().unwrap().headers.is_none());
        let slack = builtin_entry(&entries, "slack", "http");
        assert_eq!(
            slack.config.http.as_ref().unwrap().url,
            "https://mcp.slack.com/mcp"
        );
        assert!(slack.config.http.as_ref().unwrap().headers.is_none());
        let chrome = builtin_entry(&entries, "chrome-devtools", "stdio");
        assert_eq!(
            chrome
                .config
                .stdio
                .as_ref()
                .unwrap()
                .args
                .as_deref()
                .unwrap(),
            ["-y", "chrome-devtools-mcp@latest"]
        );
        let supabase_stdio = builtin_entry(&entries, "supabase", "stdio");
        let supabase_local = supabase_stdio.config.stdio.as_ref().unwrap();
        assert_eq!(
            supabase_local.args.as_deref().unwrap(),
            ["-y", "@supabase/mcp-server-supabase@latest", "--read-only"]
        );
        assert_eq!(
            supabase_local
                .env
                .as_ref()
                .unwrap()
                .get("SUPABASE_ACCESS_TOKEN")
                .map(String::as_str),
            Some("")
        );

        let stdio_cases: [StdioRegistryCase<'_>; 5] = [
            (
                "serena",
                "serena",
                &["start-mcp-server", "--context=desktop-app"],
                "",
                "https://github.com/oraios/serena",
                &["development", "code", "official"],
            ),
            (
                "playwright",
                "npx",
                &["@playwright/mcp@latest"],
                "",
                "https://github.com/microsoft/playwright-mcp",
                &["browser", "automation", "official"],
            ),
            (
                "qdrant",
                "uvx",
                &["mcp-server-qdrant"],
                "",
                "https://github.com/qdrant/mcp-server-qdrant",
                &["database", "ai", "official"],
            ),
            (
                "reference-memory",
                "npx",
                &["-y", "@modelcontextprotocol/server-memory"],
                "",
                "https://github.com/modelcontextprotocol/servers/blob/main/src/memory/README.md",
                &["memory", "reference"],
            ),
            (
                "dbhub",
                "npx",
                &["-y", "@bytebase/dbhub@latest", "--transport", "stdio"],
                "DSN",
                "https://github.com/bytebase/dbhub",
                &["database", "official"],
            ),
        ];
        for (name, command, args, env_key, repo, tags) in stdio_cases {
            let entry = builtin_entry(&entries, name, "stdio");
            let stdio = entry.config.stdio.as_ref().unwrap();
            assert_eq!(stdio.command, command, "{name}: command drifted");
            assert_eq!(
                stdio
                    .args
                    .as_deref()
                    .unwrap()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                args,
                "{name}: args drifted"
            );
            if env_key.is_empty() {
                if name == "qdrant" {
                    let env = stdio.env.as_ref().unwrap();
                    assert_eq!(env.len(), 2, "{name}: unexpected environment fields");
                    assert_eq!(
                        env.get("QDRANT_URL").map(String::as_str),
                        Some("http://localhost:6333")
                    );
                    assert_eq!(
                        env.get("COLLECTION_NAME").map(String::as_str),
                        Some("my-collection")
                    );
                    assert!(
                        env.get("QDRANT_API_KEY").is_none(),
                        "{name}: do not send a blank API key"
                    );
                } else {
                    assert!(stdio.env.is_none(), "{name}: unexpected environment");
                }
            } else {
                let env = stdio.env.as_ref().unwrap();
                assert_eq!(env.len(), 1, "{name}: unexpected environment fields");
                assert_eq!(
                    env.get(env_key).map(String::as_str),
                    Some(""),
                    "{name}: credential placeholder drifted"
                );
            }
            assert_eq!(entry.repo.as_deref(), Some(repo), "{name}: repo drifted");
            assert!(
                tags.iter()
                    .all(|tag| entry.tags.iter().any(|value| value == tag)),
                "{name}: required tags drifted"
            );
        }

        let http_cases = [
            (
                "microsoft-learn",
                "https://learn.microsoft.com/api/mcp",
                "https://learn.microsoft.com/en-us/training/support/mcp",
                &["development", "documentation", "official"][..],
            ),
            (
                "mem0",
                "https://mcp.mem0.ai/mcp",
                "https://docs.mem0.ai/platform/mem0-mcp",
                &["ai", "memory", "official"][..],
            ),
            (
                "notion",
                "https://mcp.notion.com/mcp",
                "https://developers.notion.com/guides/mcp/get-started-with-mcp",
                &["documentation", "productivity", "official"][..],
            ),
            (
                "atlassian-rovo",
                "https://mcp.atlassian.com/v2/mcp",
                "https://developer.atlassian.com/cloud/rovo-mcp/",
                &["productivity", "development", "official"][..],
            ),
            (
                "supabase",
                "https://mcp.supabase.com/mcp",
                "https://supabase.com/docs/guides/getting-started/mcp",
                &["cloud", "database", "official"][..],
            ),
        ];
        for (name, url, repo, tags) in http_cases {
            let entry = builtin_entry(&entries, name, "http");
            let http = entry.config.http.as_ref().unwrap();
            assert_eq!(http.kind, "http", "{name}: HTTP type drifted");
            assert_eq!(http.url, url, "{name}: endpoint drifted");
            assert!(
                http.headers.is_none(),
                "{name}: OAuth template must not preset a header"
            );
            assert_eq!(entry.repo.as_deref(), Some(repo), "{name}: repo drifted");
            assert!(
                tags.iter()
                    .all(|tag| entry.tags.iter().any(|value| value == tag)),
                "{name}: required tags drifted"
            );
        }

        let qdrant = builtin_entry(&entries, "qdrant", "stdio");
        assert!(qdrant.description.contains("no authentication"));
        assert!(builtin_entry(&entries, "dbhub", "stdio")
            .description
            .contains("DBHUB_AUTH_TOKEN"));
        let reference_memory = builtin_entry(&entries, "reference-memory", "stdio");
        assert!(reference_memory
            .description
            .contains("Reference implementation"));
        assert!(reference_memory.tags.iter().all(|tag| tag != "official"));
        assert!(builtin_entry(&entries, "mem0", "http")
            .description
            .contains("OAuth"));
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.name == "supabase")
                .count(),
            2
        );
    }

    #[test]
    fn curated_expanded_catalog_has_verified_identities() {
        let entries = builtin_registry();
        let cases = [
        ("linear", "http", "https://mcp.linear.app/mcp"),
        ("sentry", "http", "https://mcp.sentry.dev/mcp"),
        ("datadog", "http", "https://mcp.datadoghq.com/v1/mcp"),
        ("grafana", "stdio", "uvx mcp-grafana"),
        ("gitlab", "http", "https://gitlab.com/api/v4/mcp"),
        ("redis", "stdio", "uvx --from redis-mcp-server@latest redis-mcp-server --url redis://localhost:6379/0"),
        ("terraform", "stdio", "docker run -i --rm hashicorp/terraform-mcp-server"),
        ("mongodb", "stdio", "npx -y mongodb-mcp-server@latest --readOnly"),
        ("prisma", "http", "https://mcp.prisma.io/mcp"),
        ("neon", "http", "https://mcp.neon.tech/mcp"),
        ("wordpress-com", "http", "https://public-api.wordpress.com/wpcom/v2/mcp/v1"),
        ("zapier", "http", "https://mcp.zapier.com/api/v1/connect"),
        ("aws-knowledge", "http", "https://knowledge-mcp.global.api.aws"),
        ("posthog", "http", "https://mcp.posthog.com/mcp"),
        ("canva", "stdio", "npx -y @canva/cli@latest mcp"),
        ("webflow", "http", "https://mcp.webflow.com/mcp"),
        ("asana", "http", "https://mcp.asana.com/v2/mcp"),
        ("honeycomb", "http", "https://mcp.honeycomb.io/mcp"),
        ("pagerduty", "http", "https://mcp.pagerduty.com/mcp"),
        ("huggingface", "http", "https://huggingface.co/mcp"),
        ("pulumi", "http", "https://mcp.ai.pulumi.com/mcp"),
        ("azure", "stdio", "npx -y @azure/mcp@latest server start"),
        ("shopify-dev", "stdio", "npx -y @shopify/dev-mcp@latest"),
        ("salesforce", "stdio", "npx -y @salesforce/mcp --orgs DEFAULT_TARGET_ORG --toolsets orgs,metadata,data,users"),
        ("docker-mcp-gateway", "stdio", "docker mcp gateway run"),
        ("clickhouse", "stdio", "uv run --with mcp-clickhouse --python 3.12 mcp-clickhouse"),
        ("amplitude", "http", "https://mcp.amplitude.com/mcp"),
        ("clickup", "http", "https://mcp.clickup.com/mcp"),
        ("mixpanel", "http", "https://mcp.mixpanel.com/mcp"),
        ("newrelic", "http", "https://mcp.newrelic.com/mcp/"),
        ("descope", "http", "https://mcp.descope.com"),
        ("shortcut", "http", "https://api.shortcutco.com/mcp"),
        ("klaviyo", "http", "https://mcp.klaviyo.com/mcp"),
        ("next-devtools", "stdio", "npx -y next-devtools-mcp@latest"),
        ("browser-use", "http", "https://api.browser-use.com/v3/mcp"),
        ("browserbase", "http", "https://mcp.browserbase.com/mcp?browserbaseApiKey=${BROWSERBASE_API_KEY}"),
        ("apify", "http", "https://mcp.apify.com"),
        ("deepwiki", "http", "https://mcp.deepwiki.com/mcp"),
        ("nuxt-ui", "http", "https://ui.nuxt.com/mcp"),
        ("reference-filesystem", "stdio", "npx -y @modelcontextprotocol/server-filesystem --read-only /path/to/allowed-dir"),
        ("reference-sequential-thinking", "stdio", "npx -y @modelcontextprotocol/server-sequential-thinking"),
        ("reference-fetch", "stdio", "uvx mcp-server-fetch"),
        ("reference-git", "stdio", "uvx mcp-server-git --repository /path/to/git/repo"),
        ("reference-time", "stdio", "uvx mcp-server-time"),
        ("google-mcp-toolbox", "stdio", "npx @toolbox-sdk/server --config tools.yaml"),
        ("kubernetes-community", "stdio", "npx mcp-server-kubernetes"),
        ("desktop-commander", "stdio", "npx -y @wonderwhy-er/desktop-commander@latest"),
        ("monday", "http", "https://mcp.monday.com/mcp"),
        ("squarespace", "http", "https://mcp.squarespace.com/mcp"),
        ("steel", "http", "https://mcp.steel.dev/mcp"),
        ("upstash-redis", "stdio", "npx -y @upstash/redis-mcp"),
        ("intercom", "http", "https://mcp.intercom.com/mcp"),
        ("semgrep", "http", "https://semgrep.dev/api/mcp"),
        ("render", "http", "https://mcp.render.com/mcp"),
        ("twilio-docs", "http", "https://mcp.twilio.com/docs"),
        ("replicate", "stdio", "npx -y replicate-mcp"),
        ("sanity", "http", "https://mcp.sanity.io"),
        ("resend", "http", "https://mcp.resend.com/mcp"),
        ("draw-io", "http", "https://mcp.draw.io/mcp"),
        ("together-docs", "http", "https://docs.together.ai/mcp"),
        ("pinecone", "stdio", "npx -y @pinecone-database/mcp"),
        ("chroma", "stdio", "uvx chroma-mcp"),
        ("elevenlabs", "stdio", "uvx elevenlabs-mcp"),
        ("paypal", "stdio", "npx -y @paypal/mcp --tools=all"),
        ("xero", "stdio", "npx -y @xeroapi/xero-mcp-server@latest"),
        ("elasticsearch", "stdio", "docker run -i --rm -e ES_URL -e ES_API_KEY docker.elastic.co/mcp/elasticsearch stdio"),
        ("netlify", "stdio", "npx -y @netlify/mcp"),
        ("railway", "http", "https://mcp.railway.com"),
        ("motherduck", "stdio", "uvx mcp-server-motherduck --db-path /absolute/path/to/your.duckdb"),
        ("prefect", "stdio", "uvx --from prefect-mcp prefect-mcp-server"),
        ("deepgram", "stdio", "deepgram-mcp"),
        ("confluent", "stdio", "npx -y @confluentinc/mcp-confluent --config ./config.yaml"),
        ("buildkite", "http", "https://mcp.buildkite.com/mcp"),
        ("contentful", "stdio", "npx -y @contentful/mcp-server"),
        ("appwrite", "http", "https://mcp.appwrite.io/"),
        ("langfuse-docs", "http", "https://langfuse.com/api/mcp"),
        ("convex", "stdio", "npx -y convex@latest mcp start"),
        ("gitbook", "http", "https://mcp.gitbook.com/mcp"),
        ("square", "http", "https://mcp.squareup.com/mcp"),
        ("planetscale", "http", "https://mcp.pscale.dev/mcp/planetscale"),
        ("hostinger", "http", "https://mcp.hostinger.com"),
        ("snowflake", "http", "https://YOUR_ACCOUNT.snowflakecomputing.com/api/v2/databases/YOUR_DATABASE/schemas/YOUR_SCHEMA/mcp-servers/YOUR_SERVER"),
        ("n8n", "http", "https://YOUR_N8N_HOST/mcp-server/http"),
        ("firebase", "stdio", "npx -y firebase-tools@latest mcp"),
        ("calendly", "http", "https://mcp.calendly.com"),
        ("circleci", "http", "https://mcp.circleci.com/v1/mcp"),
        ("mailgun", "stdio", "npx -y @mailgun/mcp-server"),
        ("doppler", "stdio", "npx -y @dopplerhq/mcp-server"),
        ("inngest", "http", "https://api.inngest.com/mcp"),
        ("braintrust", "http", "https://api.braintrust.dev/mcp"),
        ("daytona", "stdio", "daytona-v0 mcp start"),
        ("statsig", "http", "https://api.statsig.com/v3/mcp"),
        ];
        assert_eq!(cases.len(), 92);
        for (name, transport, signature) in cases {
            let entry = builtin_entry(&entries, name, transport);
            let actual = if transport == "http" {
                entry.config.http.as_ref().unwrap().url.clone()
            } else {
                let stdio = entry.config.stdio.as_ref().unwrap();
                let mut parts = vec![stdio.command.clone()];
                if let Some(args) = &stdio.args {
                    parts.extend(args.iter().cloned());
                }
                parts.join(" ")
            };
            assert_eq!(actual, signature, "{name}");
        }
        let browser_use = builtin_entry(&entries, "browser-use", "http");
        assert_eq!(
            browser_use
                .config
                .http
                .as_ref()
                .unwrap()
                .headers
                .as_ref()
                .unwrap()
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer ${BROWSER_USE_API_KEY}")
        );
        let browserbase = builtin_entry(&entries, "browserbase", "http");
        assert!(browserbase
            .config
            .http
            .as_ref()
            .unwrap()
            .url
            .contains("browserbaseApiKey=${BROWSERBASE_API_KEY}"));
        assert!(browserbase.config.http.as_ref().unwrap().headers.is_none());
        assert!(builtin_entry(&entries, "apify", "http")
            .config
            .http
            .as_ref()
            .unwrap()
            .headers
            .is_none());
        assert!(builtin_entry(&entries, "apify", "http")
            .description
            .contains("no Authorization header"));
        assert_eq!(
            builtin_entry(&entries, "monday", "http")
                .config
                .http
                .as_ref()
                .unwrap()
                .headers
                .as_ref()
                .unwrap()
                .get("Api-Version")
                .map(String::as_str),
            Some("2026-07")
        );
        assert_eq!(
            builtin_entry(&entries, "upstash-redis", "stdio")
                .config
                .stdio
                .as_ref()
                .unwrap()
                .env
                .as_ref()
                .unwrap()
                .get("UPSTASH_REDIS_REST_TOKEN")
                .map(String::as_str),
            Some("")
        );
        assert_eq!(
            builtin_entry(&entries, "paypal", "stdio")
                .config
                .stdio
                .as_ref()
                .unwrap()
                .env
                .as_ref()
                .unwrap()
                .get("PAYPAL_ENVIRONMENT")
                .map(String::as_str),
            Some("SANDBOX")
        );
        assert_eq!(
            builtin_entry(&entries, "grafana", "stdio")
                .config
                .stdio
                .as_ref()
                .unwrap()
                .env
                .as_ref()
                .unwrap()
                .get("GRAFANA_SERVICE_ACCOUNT_TOKEN")
                .map(String::as_str),
            Some("")
        );
        assert_eq!(
            builtin_entry(&entries, "kubernetes-community", "stdio")
                .config
                .stdio
                .as_ref()
                .unwrap()
                .env
                .as_ref()
                .unwrap()
                .get("ALLOW_ONLY_READONLY_TOOLS")
                .map(String::as_str),
            Some("true")
        );
        assert!(entries.iter().all(|entry| entry.name != "google-drive"));
        assert!(builtin_entry(&entries, "reference-filesystem", "stdio")
            .description
            .contains("Reference implementation"));
        assert!(builtin_entry(&entries, "desktop-commander", "stdio")
            .tags
            .iter()
            .any(|tag| tag == "community"));
    }

    #[test]
    fn curated_2026_11_25_additions_match_verified_configs() {
        let entries = builtin_registry();
        let cases = [
        ("1password", "stdio", "1password-mcp"),
        ("21st-dev-magic", "http", "https://21st.dev/api/mcp"),
        ("adobe", "http", "https://adobe-creativity.adobe.io/mcp"),
        ("ahrefs", "http", "https://api.ahrefs.com/mcp/mcp"),
        ("airbyte", "http", "https://mcp.airbyte.com/mcp"),
        ("airtable", "http", "https://mcp.airtable.com/mcp"),
        ("aiven", "http", "https://mcp.aiven.live/mcp"),
        ("alchemy", "http", "https://mcp.alchemy.com/mcp"),
        ("algolia", "http", "https://mcp.algolia.com/mcp"),
        ("alibaba-cloud-ops", "stdio", "uvx alibaba-cloud-ops-mcp-server@latest"),
        ("alibaba-devops", "http", "https://openapi-rdc.aliyuncs.com/ai/mcp"),
        ("alpaca", "stdio", "uvx alpaca-mcp-server"),
        ("anki", "stdio", "npx -y @ankimcp/anki-mcp-server --stdio"),
        ("antv-chart", "stdio", "npx -y @antv/mcp-server-chart"),
        ("apache-doris", "stdio", "doris-mcp-server --transport stdio"),
        ("apollo-io", "http", "https://mcp.apollo.io/mcp"),
        ("ashby", "http", "https://mcp.ashbyhq.com/mcp/v1"),
        ("auth0", "stdio", "npx -y @auth0/auth0-mcp-server run"),
        ("axiom", "http", "https://mcp.axiom.co/mcp"),
        ("azure-devops", "http", "https://mcp.dev.azure.com/{organization}"),
        ("blender", "stdio", "uvx mcp-for-blender"),
        ("booking", "http", "https://demandapi-mcp.booking.com/v1/sp/mcp/"),
        ("brex", "http", "https://api.brex.com/mcp"),
        ("bright-data", "http", "https://mcp.brightdata.com/mcp?token=${BRIGHT_DATA_API_TOKEN}"),
        ("buddy", "http", "https://mcp.buddy.works/mcp"),
        ("cal-com", "stdio", "npx @calcom/cal-mcp@latest"),
        ("clerk", "http", "https://mcp.clerk.com/mcp"),
        ("coingecko", "http", "https://mcp.api.coingecko.com/mcp"),
        ("couchbase", "stdio", "uvx couchbase-mcp-server operational"),
        ("crowdstrike", "stdio", "uvx falcon-mcp"),
        ("dbt", "http", "https://YOUR_DBT_HOST_URL/api/ai/v1/mcp/"),
        ("deepl", "http", "https://mcp.deepl.com/v1/mcp"),
        ("devcycle", "http", "https://mcp.devcycle.com/mcp"),
        ("docling", "stdio", "uvx --from=docling-mcp docling-mcp-server"),
        ("excalidraw", "http", "https://mcp.excalidraw.com"),
        ("expo", "http", "https://mcp.expo.dev/mcp"),
        ("fireflies", "http", "https://api.fireflies.ai/mcp"),
        ("firefox-devtools", "stdio", "npx -y @mozilla/firefox-mcp@latest --headless --viewport 1280x720"),
        ("gamma", "http", "https://mcp.gamma.app/mcp"),
        ("gitkraken", "stdio", "gk mcp"),
        ("gitmcp", "http", "https://gitmcp.io/{owner}/{repo}"),
        ("globalping", "http", "https://mcp.globalping.dev/mcp"),
        ("google-bigquery", "http", "https://bigquery.googleapis.com/mcp"),
        ("incident-io", "http", "https://mcp.incident.io/mcp"),
        ("launchdarkly", "http", "https://mcp.launchdarkly.com/mcp"),
        ("lucid", "http", "https://mcp.lucid.app/mcp"),
        ("mailchimp", "http", "https://mandrillapp.com/mcp"),
        ("obsidian", "stdio", "uvx mcp-obsidian"),
        ("snyk", "stdio", "snyk mcp -t stdio"),
        ("zoominfo", "http", "https://mcp.zoominfo.com/mcp"),
        ];
        assert_eq!(cases.len(), 50);
        for (name, transport, signature) in cases {
            let entry = builtin_entry(&entries, name, transport);
            let actual = if transport == "http" {
                entry.config.http.as_ref().unwrap().url.clone()
            } else {
                let stdio = entry.config.stdio.as_ref().unwrap();
                let mut parts = vec![stdio.command.clone()];
                if let Some(args) = &stdio.args {
                    parts.extend(args.iter().cloned());
                }
                parts.join(" ")
            };
            assert_eq!(actual, signature, "{name}");
            assert!(
                entry.source_url.as_deref().is_some_and(|url| url.starts_with("https://")),
                "{name}: official source URL is required"
            );
        }
        assert!(builtin_entry(&entries, "apify", "http")
            .config
            .http
            .as_ref()
            .unwrap()
            .headers
            .is_none());
        assert_eq!(
            builtin_entry(&entries, "mailchimp", "http")
                .config
                .http
                .as_ref()
                .unwrap()
                .headers
                .as_ref()
                .unwrap()
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer ${MAILCHIMP_API_KEY}")
        );
        assert!(builtin_entry(&entries, "obsidian", "stdio")
            .tags
            .iter()
            .any(|tag| tag == "community"));
        assert!(entries.iter().all(|entry| entry.name != "google-drive"));
    }

    #[test]
    fn managed_def_paths_are_stable() {
        assert!(cached_path(&managed_def(MANUAL_ID, "手动添加"))
            .unwrap()
            .ends_with("local/manual.json"));
        assert!(cached_path(&managed_def(DISCOVERED_ID, "自动探索"))
            .unwrap()
            .ends_with("local/discovered.json"));
    }

    fn stdio_entry(name: &str, kind: &str, source: &str) -> RegistryEntry {
        use crate::domain::types::{RegistryConfig, StdioConfig};
        RegistryEntry {
            name: name.into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: source.into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: Some(RegistryOrigin {
                kind: kind.into(),
                agent: None,
                scope: None,
                source: Some(source.into()),
            }),
            repo: None,
            source_url: None,
        }
    }

    #[test]
    fn public_registry_writer_waits_for_the_settings_filesystem_lock() {
        let _home = TestHome::new("registry-public-writer-lock");
        let settings_guard = acquire_settings_lock(&settings_file()).unwrap();
        let entry = stdio_entry("serialized", "manual", "serialized-server");
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        let writer = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = write_manual_entry(&entry).map_err(|error| error.to_string());
            done_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(150)).is_err(),
            "public catalog writer bypassed the settings filesystem lock"
        );

        drop(settings_guard);
        let result = done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(result.is_ok(), "{result:?}");
        writer.join().unwrap();
        assert!(manual_entries()
            .iter()
            .any(|entry| entry.key() == "serialized::stdio"));
    }

    #[test]
    fn flag_in_effect_keeps_all_copies_marks_last_winner() {
        // Same composite key (context7::stdio) from a remote source then the
        // manual source — mirrors read_registry's ordering (manual last/wins).
        let ordered = vec![
            stdio_entry("context7", "remote", "official"),
            stdio_entry("solo", "manual", "manual"),
            stdio_entry("context7", "manual", "manual"),
        ];
        let items = flag_in_effect(ordered);
        // Nothing is deduped away — all three copies survive.
        assert_eq!(items.len(), 3);
        // The two context7 copies: only the later (manual) one is in effect.
        let ctx: Vec<&CatalogItem> = items
            .iter()
            .filter(|i| i.entry.name == "context7")
            .collect();
        assert_eq!(ctx.len(), 2);
        let remote = ctx
            .iter()
            .find(|i| i.entry.origin.as_ref().unwrap().kind == "remote")
            .unwrap();
        let manual = ctx
            .iter()
            .find(|i| i.entry.origin.as_ref().unwrap().kind == "manual")
            .unwrap();
        assert!(
            !remote.in_effect,
            "shadowed remote copy must not be in effect"
        );
        assert!(manual.in_effect, "manual copy wins precedence");
        // A single-source entry is trivially in effect.
        let solo = items.iter().find(|i| i.entry.name == "solo").unwrap();
        assert!(solo.in_effect);
    }
}
