//! Runtime settings plus hydrated central asset catalogs.
//!
//! Arca's desktop app and the CLI share `~/.arca/`. Historically that meant a
//! sprawl of files: `registry/<name>__<transport>.json` (one per custom MCP),
//! `agents.json`, `disabled.json`, `state.json`, and a `.imported` marker. This
//! module originally collapsed all of it into one `settings.json`. Central MCP,
//! Model, and Skill metadata now live below `~/.arca/assets/{mcps,models,skills}`;
//! callers still receive one hydrated [`Settings`] value so domain code does
//! not need to know which physical document owns a field.
//!
//! Cross-tool rule: the desktop fully types the sections it owns
//! (`agents`/`registry`/`disabled`) and carries the CLI-owned ones
//! (`state`/`imported`) plus any unknown future keys (`extra`) through opaquely,
//! so a desktop write never clobbers data the CLI wrote, and vice versa. Every
//! mutation is read-whole → modify one section → write-whole (atomically).

use crate::domain::assets::{McpConsumptionRecord, ModelConsumptionRecord, TargetIncident};
use crate::domain::mcp::DisabledEntry;
use crate::domain::skill::ManagedSkillRecord;
use crate::domain::types::{
    AgentDefinition, ModelProfile, ModelProviderConfig, RegistryEntry, SourceDef,
};
use crate::paths::{
    backups_dir, mcp_catalog_file, model_catalog_file, arca_dir, registry_dir, settings_file,
    skill_catalog_file, user_agents_file,
};
use crate::safe_write::{acquire_settings_lock, remove_if_unchanged, write_private_if_unchanged};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::sync::Mutex;

pub const SETTINGS_VERSION: u32 = 6;
const ASSET_CATALOG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpIconPreference {
    pub kind: String,
    pub value: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UiSettings {
    #[serde(default)]
    pub pinned_agents: Vec<String>,
    /// Desktop language override. Missing means follow the operating system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp_icons: BTreeMap<String, McpIconPreference>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Arca-owned network preferences. Proxy credentials are intentionally not
/// supported here so settings.json never becomes a credential store.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct NetworkSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// User-selected write locations that are independent from an Agent's audited
/// wire schema. MCP section keys plus Model and Skills paths live here so a
/// location override never mutates the built-in codec contract. Instructions
/// locations select native editor sources without asset projection. Skills keeps
/// one primary write directory plus zero or more compatibility read targets.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AgentConfigPathOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_paths: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_global_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_alias_dirs: Option<Vec<String>>,
    /// Explicit local Instructions locations, keyed by stable native source ID.
    /// These only select files for Arca's editor; they never project resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_paths: Option<BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelAgentRuntimeState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_applied_profile_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The whole `~/.arca/settings.json` document.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// User agent map. Absent ⇒ fall back to the builtin agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<BTreeMap<String, AgentDefinition>>,
    /// Per-Agent MCP key and Model/Skills path overrides. These are kept
    /// separate from audited Agent definitions so changing a location never
    /// changes a codec, target identity, evidence, or install probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_config_paths: Option<BTreeMap<String, AgentConfigPathOverride>>,
    /// Explicit runtime launch preferences; independent of managed assets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_launch: Option<BTreeMap<String, crate::domain::agents::LaunchPreferences>>,
    /// Global terminal used when launching CLI Agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_terminal: Option<String>,
    /// User/custom/override registry entries (manual + discovered + overrides),
    /// layered over the entries contributed by `sources` on read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<Vec<RegistryEntry>>,
    /// User-added catalog sources (subscribed remote URLs + local files). Their
    /// servers are parsed from cached files under
    /// `~/.arca/assets/mcps/sources/` on read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<SourceDef>>,
    /// Disable snapshots, keyed by agent id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<BTreeMap<String, Vec<DisabledEntry>>>,
    /// Reusable Models that reference shared Provider connection records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profiles: Option<BTreeMap<String, ModelProfile>>,
    /// Shared Provider connection/configuration instances referenced by Model
    /// Profiles. Credentials remain outside settings.json in Keychain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_providers: Option<BTreeMap<String, ModelProviderConfig>>,
    /// Current/active managed Model Profile per Agent. This legacy-compatible
    /// pointer is retained so older Arca builds do not lose the current model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_assignments: Option<BTreeMap<String, String>>,
    /// Installed Model Profiles per Agent, including disabled relationships.
    /// Older settings are projected into this collection from
    /// `model_assignments` and persisted on the next Model mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_consumptions: Option<BTreeMap<String, BTreeMap<String, ModelConsumptionRecord>>>,
    /// Per-Agent default API Key delivery. Missing entries mean plaintext.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_delivery_defaults: Option<BTreeMap<String, crate::domain::assets::ApiKeyDelivery>>,
    /// Per-Agent runtime state needed to restore externally owned model state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_agent_runtime: Option<BTreeMap<String, ModelAgentRuntimeState>>,
    /// Desired MCP consumption, keyed by canonical Agent id and then by the
    /// stable central Registry asset key (`name::transport`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_consumptions: Option<BTreeMap<String, BTreeMap<String, McpConsumptionRecord>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_skills: Option<BTreeMap<String, ManagedSkillRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_assignments: Option<BTreeMap<String, BTreeSet<String>>>,
    /// Enabled state for desired managed Skill targets. Missing records from
    /// older settings default to enabled and are materialized on mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_consumptions:
        Option<BTreeMap<String, BTreeMap<String, crate::domain::assets::SkillConsumptionRecord>>>,
    /// Unresolved Agent projection writes keyed by a stable physical-target
    /// identity. They are operational state, not a global recovery gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_incidents: Option<BTreeMap<String, TargetIncident>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_update_checked_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiSettings>,
    /// Optional proxy used by Arca-owned outbound requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkSettings>,
    /// CLI-owned: last applied state. Opaque to the desktop — carried through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    /// CLI-owned: first-scan import marker (ISO timestamp). Carried through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported: Option<String>,
    /// Forward-compat: any unknown top-level keys survive a round-trip so an
    /// older binary never drops a newer one's data.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct McpAssetCatalog {
    #[serde(default)]
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry: Option<Vec<RegistryEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sources: Option<Vec<SourceDef>>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct ModelAssetCatalog {
    #[serde(default)]
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    providers: Option<BTreeMap<String, ModelProviderConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profiles: Option<BTreeMap<String, ModelProfile>>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct SkillAssetCatalog {
    #[serde(default)]
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skills: Option<BTreeMap<String, ManagedSkillRecord>>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Default)]
struct PersistedDocuments {
    settings: Option<String>,
    mcp: Option<String>,
    model: Option<String>,
    skill: Option<String>,
}

impl Settings {
    pub fn model_agent_runtime_state(&self, agent_id: &str) -> Option<&ModelAgentRuntimeState> {
        self.model_agent_runtime.as_ref()?.get(agent_id)
    }

    pub fn set_model_agent_runtime_state(&mut self, agent_id: &str, state: ModelAgentRuntimeState) {
        let mut state = state;
        if let Some(existing) = self.model_agent_runtime_state(agent_id) {
            for (key, value) in &existing.extra {
                state
                    .extra
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        self.model_agent_runtime
            .get_or_insert_default()
            .insert(agent_id.to_string(), state);
    }

    pub fn remove_model_agent_runtime_state(
        &mut self,
        agent_id: &str,
    ) -> Option<ModelAgentRuntimeState> {
        let all = self.model_agent_runtime.as_mut()?;
        let (previous, remove_agent) = {
            let state = all.get_mut(agent_id)?;
            let previous = state.clone();
            state.previous_applied_profile_id = None;
            (previous, state.extra.is_empty())
        };
        if remove_agent {
            all.remove(agent_id);
        }
        if all.is_empty() {
            self.model_agent_runtime = None;
        }
        Some(previous)
    }

    /// Hydrate Provider-owned connection fields for runtime consumers while
    /// keeping them out of persisted Model records.
    fn hydrate_model_provider_fields(&mut self) {
        let Some(providers) = self.model_providers.as_ref() else {
            return;
        };
        for profile in self
            .model_profiles
            .iter_mut()
            .flatten()
            .map(|(_, profile)| profile)
        {
            let Some(provider) = profile
                .provider_id
                .as_deref()
                .and_then(|provider_id| providers.get(provider_id))
            else {
                continue;
            };
            profile.provider = provider.provider.clone();
            profile.base_url = provider.base_url.clone();
            profile.endpoint_path = provider
                .protocols
                .get(&profile.protocol)
                .map(|protocol| protocol.endpoint_path.clone())
                .unwrap_or_default();
            profile.env_key = provider.env_key.clone();
        }
    }

    /// Return the canonical Model state while transparently projecting legacy
    /// `model_assignments` into an enabled installed relationship.
    pub fn model_selection(&self, agent_id: &str) -> crate::domain::assets::ModelAgentSelection {
        use crate::domain::assets::{ModelAgentSelection, ModelConsumptionRecord};

        let mut selection = ModelAgentSelection {
            profiles: self
                .model_consumptions
                .as_ref()
                .and_then(|all| all.get(agent_id))
                .cloned()
                .unwrap_or_default(),
            active_profile_id: self
                .model_assignments
                .as_ref()
                .and_then(|all| all.get(agent_id))
                .cloned(),
            default_delivery: self
                .model_delivery_defaults
                .as_ref()
                .and_then(|defaults| defaults.get(agent_id).cloned())
                .unwrap_or(crate::domain::assets::ApiKeyDelivery::Plaintext),
        };
        if let Some(profile_id) = selection.active_profile_id.clone() {
            let credential = selection.inherited_credential();
            selection
                .profiles
                .entry(profile_id.clone())
                .or_insert(ModelConsumptionRecord {
                    profile_id,
                    enabled: true,
                    credential,
                    last_selected_at: None,
                });
        }
        selection
    }

    /// Persist one canonical Model state in both the multi-profile collection
    /// and the legacy-compatible active pointer.
    pub fn set_model_selection(
        &mut self,
        agent_id: &str,
        selection: crate::domain::assets::ModelAgentSelection,
    ) {
        let all = self.model_consumptions.get_or_insert_default();
        if selection.profiles.is_empty() {
            all.remove(agent_id);
        } else {
            all.insert(agent_id.to_string(), selection.profiles);
        }
        let assignments = self.model_assignments.get_or_insert_default();
        match selection.active_profile_id {
            Some(profile_id) => {
                assignments.insert(agent_id.to_string(), profile_id);
            }
            None => {
                assignments.remove(agent_id);
            }
        }
        let defaults = self.model_delivery_defaults.get_or_insert_default();
        if selection.default_delivery == crate::domain::assets::ApiKeyDelivery::Plaintext {
            defaults.remove(agent_id);
            if defaults.is_empty() {
                self.model_delivery_defaults = None;
            }
        } else {
            defaults.insert(agent_id.to_string(), selection.default_delivery);
        }
    }
}

/// Serializes read-modify-write within this process. Each transaction also
/// holds the filesystem settings lock for cooperative cross-process writers.
static LOCK: Mutex<()> = Mutex::new(());

/// Read the whole settings document. Missing or corrupt file ⇒ defaults
/// (empty user data), matching the per-file tolerance this replaced.
pub fn load_settings() -> Settings {
    load_settings_strict().unwrap_or_default()
}

/// Read only Agent preferences. Native host discovery must not hydrate asset
/// catalogs or depend on model credentials and catalog health.
pub(crate) fn load_agent_preferences() -> std::io::Result<Settings> {
    #[derive(Deserialize, Default)]
    struct AgentPreferences {
        version: Option<u32>,
        agents: Option<BTreeMap<String, AgentDefinition>>,
        agent_config_paths: Option<BTreeMap<String, AgentConfigPathOverride>>,
    }
    let path = settings_file();
    let preferences: AgentPreferences = match read_optional(&path)? {
        Some(content) => serde_json::from_str(&content).map_err(|error| invalid_document(&path, error))?,
        None => AgentPreferences::default(),
    };
    if preferences.version.is_some_and(|version| version != SETTINGS_VERSION) {
        return Err(Error::new(ErrorKind::InvalidData, "unsupported Agent preference schema"));
    }
    Ok(Settings { version: preferences.version, agents: preferences.agents,
        agent_config_paths: preferences.agent_config_paths, ..Settings::default() })
}

fn read_optional(path: &Path) -> std::io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn invalid_document(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::new(
        ErrorKind::InvalidData,
        format!(
            "refusing to replace invalid Arca document at {}: {}",
            path.display(),
            error
        ),
    )
}

fn parse_catalog<T>(path: &Path) -> std::io::Result<(Option<T>, Option<String>)>
where
    T: for<'de> Deserialize<'de>,
{
    let original = read_optional(path)?;
    let parsed = original
        .as_deref()
        .map(|content| serde_json::from_str(content).map_err(|error| invalid_document(path, error)))
        .transpose()?;
    Ok((parsed, original))
}

fn validate_catalog_version(path: &Path, version: u32) -> std::io::Result<()> {
    if version > ASSET_CATALOG_VERSION {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "refusing to read asset catalog schema {version} at {} with supported schema {ASSET_CATALOG_VERSION}",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn require_matching_legacy<T: PartialEq>(
    path: &Path,
    label: &str,
    legacy: &Option<T>,
    current: &Option<T>,
) -> std::io::Result<()> {
    if legacy.is_some() && legacy != current {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "refusing to choose between conflicting legacy and unified {label} data at {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn canonicalize_persisted_profiles(
    profiles: &Option<BTreeMap<String, ModelProfile>>,
) -> Option<BTreeMap<String, ModelProfile>> {
    let mut profiles = profiles.clone();
    for profile in profiles
        .iter_mut()
        .flatten()
        .map(|(_, profile)| profile)
        .filter(|profile| profile.provider_id.is_some())
    {
        profile.provider.clear();
        profile.base_url.clear();
        profile.endpoint_path.clear();
        profile.env_key = None;
    }
    profiles
}

fn parse_for_update(path: &Path) -> std::io::Result<(Settings, PersistedDocuments)> {
    let settings_original = read_optional(path)?;
    let mut settings = match settings_original.as_deref() {
        Some(content) => {
            serde_json::from_str(content).map_err(|error| invalid_document(path, error))?
        }
        None => Settings::default(),
    };
    if settings
        .version
        .is_some_and(|version| version > SETTINGS_VERSION)
    {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "refusing to read Arca settings schema {} with supported schema {SETTINGS_VERSION}",
                settings.version.unwrap_or_default()
            ),
        ));
    }

    let mcp_path = mcp_catalog_file();
    let model_path = model_catalog_file();
    let skill_path = skill_catalog_file();
    let (mcp, mcp_original) = parse_catalog::<McpAssetCatalog>(&mcp_path)?;
    let (model, model_original) = parse_catalog::<ModelAssetCatalog>(&model_path)?;
    let (skill, skill_original) = parse_catalog::<SkillAssetCatalog>(&skill_path)?;

    if let Some(catalog) = mcp {
        validate_catalog_version(&mcp_path, catalog.version)?;
        require_matching_legacy(
            &mcp_path,
            "MCP registry",
            &settings.registry,
            &catalog.registry,
        )?;
        require_matching_legacy(
            &mcp_path,
            "MCP sources",
            &settings.sources,
            &catalog.sources,
        )?;
        settings.registry = catalog.registry;
        settings.sources = catalog.sources;
    }
    if let Some(catalog) = model {
        validate_catalog_version(&model_path, catalog.version)?;
        require_matching_legacy(
            &model_path,
            "Model providers",
            &settings.model_providers,
            &catalog.providers,
        )?;
        require_matching_legacy(
            &model_path,
            "Model profiles",
            &canonicalize_persisted_profiles(&settings.model_profiles),
            &canonicalize_persisted_profiles(&catalog.profiles),
        )?;
        settings.model_providers = catalog.providers;
        settings.model_profiles = catalog.profiles;
    }
    if let Some(catalog) = skill {
        validate_catalog_version(&skill_path, catalog.version)?;
        require_matching_legacy(
            &skill_path,
            "Skill metadata",
            &settings.managed_skills,
            &catalog.skills,
        )?;
        settings.managed_skills = catalog.skills;
    }
    settings.hydrate_model_provider_fields();
    Ok((
        settings,
        PersistedDocuments {
            settings: settings_original,
            mcp: mcp_original,
            model: model_original,
            skill: skill_original,
        },
    ))
}

pub(crate) fn load_settings_strict() -> std::io::Result<Settings> {
    parse_for_update(&settings_file()).map(|(settings, _)| settings)
}

fn save_with_expected(
    path: &Path,
    settings: &Settings,
    original: &PersistedDocuments,
) -> std::io::Result<()> {
    let mut runtime = settings.clone();
    runtime.version.get_or_insert(SETTINGS_VERSION);
    if runtime.version.unwrap_or_default() >= 4 {
        for profile in runtime
            .model_profiles
            .iter_mut()
            .flatten()
            .map(|(_, profile)| profile)
        {
            profile.provider.clear();
            profile.base_url.clear();
            profile.endpoint_path.clear();
            profile.env_key = None;
        }
    }

    let mcp = McpAssetCatalog {
        version: ASSET_CATALOG_VERSION,
        registry: runtime.registry.take(),
        sources: runtime.sources.take(),
        extra: original
            .mcp
            .as_deref()
            .and_then(|content| serde_json::from_str::<McpAssetCatalog>(content).ok())
            .map(|catalog| catalog.extra)
            .unwrap_or_default(),
    };
    let model = ModelAssetCatalog {
        version: ASSET_CATALOG_VERSION,
        providers: runtime.model_providers.take(),
        profiles: runtime.model_profiles.take(),
        extra: original
            .model
            .as_deref()
            .and_then(|content| serde_json::from_str::<ModelAssetCatalog>(content).ok())
            .map(|catalog| catalog.extra)
            .unwrap_or_default(),
    };
    let skill = SkillAssetCatalog {
        version: ASSET_CATALOG_VERSION,
        skills: runtime.managed_skills.take(),
        extra: original
            .skill
            .as_deref()
            .and_then(|content| serde_json::from_str::<SkillAssetCatalog>(content).ok())
            .map(|catalog| catalog.extra)
            .unwrap_or_default(),
    };

    let mut writes = Vec::new();
    if original.mcp.is_some() || mcp.registry.is_some() || mcp.sources.is_some() {
        writes.push((
            mcp_catalog_file(),
            original.mcp.as_deref(),
            serde_json::to_string_pretty(&mcp)
                .map_err(|error| Error::new(ErrorKind::InvalidData, error))?,
        ));
    }
    if original.model.is_some() || model.providers.is_some() || model.profiles.is_some() {
        writes.push((
            model_catalog_file(),
            original.model.as_deref(),
            serde_json::to_string_pretty(&model)
                .map_err(|error| Error::new(ErrorKind::InvalidData, error))?,
        ));
    }
    if original.skill.is_some() || skill.skills.is_some() {
        writes.push((
            skill_catalog_file(),
            original.skill.as_deref(),
            serde_json::to_string_pretty(&skill)
                .map_err(|error| Error::new(ErrorKind::InvalidData, error))?,
        ));
    }
    writes.push((
        path.to_path_buf(),
        original.settings.as_deref(),
        serde_json::to_string_pretty(&runtime)
            .map_err(|error| Error::new(ErrorKind::InvalidData, error))?,
    ));

    let mut applied: Vec<(std::path::PathBuf, Option<&str>, String)> = Vec::new();
    for (write_path, expected, desired) in writes {
        if expected == Some(desired.as_str()) {
            continue;
        }
        if let Err(error) = write_private_if_unchanged(&write_path, expected, &desired) {
            let mut rollback_failed = None;
            for (applied_path, previous, written) in applied.into_iter().rev() {
                let restored = match previous {
                    Some(previous) => {
                        write_private_if_unchanged(&applied_path, Some(written.as_str()), previous)
                    }
                    None => remove_if_unchanged(&applied_path, &written),
                };
                if let Err(rollback_error) = restored {
                    rollback_failed = Some(rollback_error);
                    break;
                }
            }
            return Err(Error::other(match rollback_failed {
                Some(rollback_error) => format!(
                    "recovery_required: asset catalog write failed ({error}); rollback failed ({rollback_error})"
                ),
                None => error.to_string(),
            }));
        }
        applied.push((write_path, expected, desired));
    }
    Ok(())
}

/// Persist the whole settings document (pretty JSON, atomic). Stamps `version`.
pub fn save_settings(settings: &Settings) -> std::io::Result<()> {
    let path = settings_file();
    // Global lock order is filesystem → process. Asset transactions already
    // hold the filesystem lock while calling nested settings mutations; taking
    // these in the opposite order lets a concurrent raw writer stall both
    // threads until the filesystem-lock timeout.
    let _filesystem_guard = acquire_settings_lock(&path).map_err(Error::other)?;
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_, original) = parse_for_update(&path)?;
    save_with_expected(&path, settings, &original)
}

/// Load → apply `f` to one section → save under the process and filesystem
/// transaction locks. Returns whatever `f` returns once the save succeeds.
pub fn mutate_settings<F, R>(f: F) -> std::io::Result<R>
where
    F: FnOnce(&mut Settings) -> R,
{
    let path = settings_file();
    let _filesystem_guard = acquire_settings_lock(&path).map_err(Error::other)?;
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (mut settings, original) = parse_for_update(&path)?;
    let out = f(&mut settings);
    save_with_expected(&path, &settings, &original)?;
    Ok(out)
}

/// Load → validate and apply `f` to one section → save under the process and
/// filesystem transaction locks. A closure error leaves the snapshot unwritten.
pub fn mutate_settings_checked<F, R>(f: F) -> std::io::Result<R>
where
    F: FnOnce(&mut Settings) -> std::io::Result<R>,
{
    let path = settings_file();
    let _filesystem_guard = acquire_settings_lock(&path).map_err(Error::other)?;
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (mut settings, original) = parse_for_update(&path)?;
    let out = f(&mut settings)?;
    save_with_expected(&path, &settings, &original)?;
    Ok(out)
}

/// Move legacy MCP sources below `~/.arca/assets` and extract central
/// metadata from the legacy consolidated settings document into one catalog per
/// asset domain. The operation is idempotent. If a crash leaves both the old
/// fields and a new catalog, the strict loader accepts them only when they are
/// byte-semantically equal; the next run then removes the legacy fields.
pub fn migrate_asset_layout_if_needed() -> std::io::Result<bool> {
    let path = settings_file();
    let _filesystem_guard = acquire_settings_lock(&path).map_err(Error::other)?;
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let moved = crate::paths::migrate_legacy_mcp_sources()?;
    let (mut settings, original) = parse_for_update(&path)?;
    let legacy_asset_fields = original
        .settings
        .as_deref()
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
        .and_then(|value| value.as_object().cloned())
        .is_some_and(|object| {
            [
                "registry",
                "sources",
                "model_profiles",
                "model_providers",
                "managed_skills",
            ]
            .iter()
            .any(|key| object.contains_key(*key))
        });
    let needs_schema_upgrade = settings.version.unwrap_or_default() < SETTINGS_VERSION;
    if !moved && !legacy_asset_fields && !needs_schema_upgrade {
        return Ok(false);
    }
    settings.version = Some(SETTINGS_VERSION);
    save_with_expected(&path, &settings, &original)?;
    Ok(true)
}

/// Move `from` into `legacy_dir` if it exists.
fn archive(from: &Path, legacy_dir: &Path, name: &str) -> std::io::Result<()> {
    if from.exists() {
        fs::create_dir_all(legacy_dir)?;
        fs::rename(from, legacy_dir.join(name))?;
    }
    Ok(())
}

/// One-time migration: if `settings.json` is absent but legacy files exist, fold
/// them into a fresh `settings.json`, then move the old files aside into
/// `~/.arca/backups/legacy-<ts>/` (reversible, not deleted). Idempotent: a no-op
/// once `settings.json` exists.
pub fn migrate_if_needed() -> std::io::Result<bool> {
    let settings_path = settings_file();
    if settings_path.exists() {
        return Ok(false);
    }

    let reg_dir = registry_dir();
    let agents_path = user_agents_file();
    let disabled_path = arca_dir().join("disabled.json");
    let state_path = arca_dir().join("state.json");
    let imported_path = arca_dir().join(".imported");

    let has_legacy = || {
        reg_dir.is_dir()
            || agents_path.exists()
            || disabled_path.exists()
            || state_path.exists()
            || imported_path.exists()
    };
    // Avoid creating ~/.arca solely to discover that there is nothing to
    // migrate. Once legacy state exists, serialize the complete check, write,
    // and archival decision with every other cooperating settings writer.
    if !has_legacy() {
        return Ok(false);
    }

    let _filesystem_guard = acquire_settings_lock(&settings_path).map_err(Error::other)?;
    let _process_guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    // Another process may have completed migration or created fresh settings
    // while this process was waiting for the filesystem lock.
    if settings_path.exists() || !has_legacy() {
        return Ok(false);
    }

    let mut s = Settings {
        version: Some(1),
        ..Default::default()
    };

    // registry/*.json → registry[]  (dedup by composite key, last file wins)
    if reg_dir.is_dir() {
        let mut by_key: BTreeMap<String, RegistryEntry> = BTreeMap::new();
        if let Ok(rd) = fs::read_dir(&reg_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Ok(c) = fs::read_to_string(&p) {
                        if let Ok(entry) = serde_json::from_str::<RegistryEntry>(&c) {
                            by_key.insert(entry.key(), entry);
                        }
                    }
                }
            }
        }
        if !by_key.is_empty() {
            s.registry = Some(by_key.into_values().collect());
        }
    }
    // agents.json → agents
    if let Ok(c) = fs::read_to_string(&agents_path) {
        if let Ok(map) = serde_json::from_str::<BTreeMap<String, AgentDefinition>>(&c) {
            s.agents = Some(map);
        }
    }
    // disabled.json → disabled
    if let Ok(c) = fs::read_to_string(&disabled_path) {
        if let Ok(map) = serde_json::from_str::<BTreeMap<String, Vec<DisabledEntry>>>(&c) {
            s.disabled = Some(map);
        }
    }
    // state.json → state (opaque)
    if let Ok(c) = fs::read_to_string(&state_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&c) {
            s.state = Some(v);
        }
    }
    // .imported → imported
    if let Ok(c) = fs::read_to_string(&imported_path) {
        let t = c.trim();
        if !t.is_empty() {
            s.imported = Some(t.to_string());
        }
    }

    // Only archive the legacy files once the new file is safely written.
    // The caller already owns both locks, so use the internal CAS writer
    // directly instead of re-entering the non-reentrant process mutex.
    save_with_expected(&settings_path, &s, &PersistedDocuments::default())?;
    let stamp = super::paths::backup_timestamp();
    let legacy_dir = backups_dir().join(format!("legacy-{stamp}"));
    archive(&reg_dir, &legacy_dir, "registry")?;
    archive(&agents_path, &legacy_dir, "agents.json")?;
    archive(&disabled_path, &legacy_dir, "disabled.json")?;
    archive(&state_path, &legacy_dir, "state.json")?;
    archive(&imported_path, &legacy_dir, ".imported")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        ModelProfile, ModelProtocol, ModelProviderConfig, ModelProviderProtocolConfig,
        RegistryConfig, StdioConfig,
    };
    use crate::safe_write::acquire_settings_lock;
    use crate::testenv::TestHome;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn host_agent_preferences_ignore_unrelated_catalog_damage_and_preserve_disablement() {
        let _home = TestHome::new("native-agent-preferences");
        let mut cursor = crate::agents::load_agents_from_settings(&Settings::default())["cursor"].clone();
        cursor.enabled = false;
        fs::create_dir_all(arca_dir()).unwrap();
        fs::write(settings_file(), serde_json::to_vec(&serde_json::json!({
            "version": SETTINGS_VERSION,
            "agents": { "cursor": cursor }
        })).unwrap()).unwrap();
        fs::create_dir_all(model_catalog_file().parent().unwrap()).unwrap();
        fs::write(model_catalog_file(), "invalid model catalog").unwrap();
        let preferences = load_agent_preferences().unwrap();
        assert!(preferences.model_profiles.is_none());
        assert!(preferences.model_providers.is_none());
        assert!(!crate::agents::load_agents_from_settings(&preferences)["cursor"].enabled);
        fs::write(settings_file(), "invalid settings").unwrap();
        assert!(load_agent_preferences().is_err());
    }

    #[test]
    fn unknown_and_cli_sections_survive_roundtrip() {
        // A settings doc the CLI wrote (state + imported + a future key) must
        // come back intact after the desktop deserializes and re-serializes it.
        let json = r#"{
            "version": 1,
            "registry": [],
            "state": {"active": [{"name":"git","scope":"global","agents":["claude-code"]}]},
            "imported": "2026-01-02T03:04:05",
            "futureThing": {"a": 1}
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(s.state.is_some());
        assert_eq!(s.imported.as_deref(), Some("2026-01-02T03:04:05"));
        assert!(s.extra.contains_key("futureThing"));
        let back = serde_json::to_string(&s).unwrap();
        assert!(back.contains("futureThing"));
        assert!(back.contains("\"active\""));
        assert!(back.contains("2026-01-02T03:04:05"));
    }

    #[test]
    fn claude_desktop_model_agent_runtime_state_roundtrips_and_helpers_prune_empty_map() {
        let json = r#"{
            "model_agent_runtime": {
                "claude-desktop": {
                    "previous_applied_profile_id": "cc-switch-id",
                    "futureRuntimeField": {"keep": true}
                }
            },
            "futureThing": {"keep": true}
        }"#;
        let mut settings: Settings = serde_json::from_str(json).unwrap();

        assert_eq!(
            settings
                .model_agent_runtime_state("claude-desktop")
                .and_then(|state| state.previous_applied_profile_id.as_deref()),
            Some("cc-switch-id")
        );
        settings.set_model_agent_runtime_state(
            "another-agent",
            ModelAgentRuntimeState {
                previous_applied_profile_id: Some("external-id".into()),
                ..Default::default()
            },
        );
        settings.set_model_agent_runtime_state(
            "claude-desktop",
            ModelAgentRuntimeState {
                previous_applied_profile_id: Some("updated-id".into()),
                ..Default::default()
            },
        );
        let encoded = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            encoded["model_agent_runtime"]["claude-desktop"]["previous_applied_profile_id"],
            "updated-id"
        );
        assert_eq!(
            encoded["model_agent_runtime"]["claude-desktop"]["futureRuntimeField"]["keep"],
            true
        );
        assert_eq!(
            encoded["model_agent_runtime"]["another-agent"]["previous_applied_profile_id"],
            "external-id"
        );
        assert_eq!(encoded["futureThing"]["keep"], true);

        settings.remove_model_agent_runtime_state("claude-desktop");
        let encoded = serde_json::to_value(&settings).unwrap();
        assert!(encoded["model_agent_runtime"]["claude-desktop"]
            .get("previous_applied_profile_id")
            .is_none());
        assert_eq!(
            encoded["model_agent_runtime"]["claude-desktop"]["futureRuntimeField"]["keep"],
            true
        );
        settings.remove_model_agent_runtime_state("another-agent");
        assert!(settings
            .model_agent_runtime_state("claude-desktop")
            .is_some());
        settings
            .model_agent_runtime
            .as_mut()
            .unwrap()
            .get_mut("claude-desktop")
            .unwrap()
            .extra
            .clear();
        settings.remove_model_agent_runtime_state("claude-desktop");
        assert!(settings.model_agent_runtime.is_none());
        assert!(serde_json::to_value(settings)
            .unwrap()
            .get("model_agent_runtime")
            .is_none());
    }

    #[test]
    fn v6_persists_provider_connection_only_once_and_hydrates_models_on_read() {
        let _home = TestHome::new("settings-model-provider-v6");
        let provider = ModelProviderConfig {
            id: "team-provider".into(),
            name: "Team Provider".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                ModelProtocol::OpenaiResponses,
                ModelProviderProtocolConfig {
                    endpoint_path: "/v1/responses".into(),
                },
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: Some("TEAM_API_KEY".into()),
        };
        let profile = ModelProfile {
            id: "team-model".into(),
            name: "Team Model".into(),
            provider_id: Some(provider.id.clone()),
            provider: provider.provider.clone(),
            model_vendor: None,
            native_ids: BTreeMap::new(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: provider.base_url.clone(),
            endpoint_path: provider.protocols[&ModelProtocol::OpenaiResponses]
                .endpoint_path
                .clone(),
            model: "model-id".into(),
            env_key: provider.env_key.clone(),
            context_window: None,
            max_output_tokens: None,
            reasoning: None,
        };
        save_settings(&Settings {
            version: Some(SETTINGS_VERSION),
            model_providers: Some(BTreeMap::from([(provider.id.clone(), provider.clone())])),
            model_profiles: Some(BTreeMap::from([(profile.id.clone(), profile)])),
            ..Default::default()
        })
        .unwrap();

        let raw: Value = serde_json::from_str(
            &fs::read_to_string(model_catalog_file()).expect("Model catalog should exist"),
        )
        .unwrap();
        let persisted = &raw["profiles"]["team-model"];
        assert!(persisted.get("provider").is_none());
        assert!(persisted.get("base_url").is_none());
        assert!(persisted.get("endpoint_path").is_none());
        assert!(persisted.get("env_key").is_none());
        assert_eq!(persisted["provider_id"], provider.id);

        let hydrated = load_settings_strict().unwrap().model_profiles.unwrap();
        assert_eq!(hydrated["team-model"].provider, provider.provider);
        assert_eq!(hydrated["team-model"].base_url, provider.base_url);
        assert_eq!(
            hydrated["team-model"].endpoint_path,
            provider.protocols[&ModelProtocol::OpenaiResponses].endpoint_path
        );
        assert_eq!(hydrated["team-model"].env_key, provider.env_key);
    }

    #[test]
    fn skill_sections_and_unknown_fields_survive_settings_roundtrip() {
        let json = r#"{
          "managed_skills": {},
          "skill_assignments": {"review-changes":["claude-user"]},
          "skill_consumptions": {
            "review-changes": {
              "claude-user": {
                "name": "review-changes",
                "target_id": "claude-user",
                "enabled": false
              }
            }
          },
          "skill_update_checked_at": "2026-07-16T08:00:00Z",
          "future_section": {"keep": true}
        }"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert!(settings.managed_skills.as_ref().unwrap().is_empty());
        assert!(
            settings.skill_assignments.as_ref().unwrap()["review-changes"].contains("claude-user")
        );
        assert!(
            !settings.skill_consumptions.as_ref().unwrap()["review-changes"]["claude-user"].enabled
        );
        assert_eq!(
            settings.skill_update_checked_at.as_deref(),
            Some("2026-07-16T08:00:00Z")
        );
        let encoded = serde_json::to_value(settings).unwrap();
        assert_eq!(
            encoded["skill_assignments"]["review-changes"][0],
            "claude-user"
        );
        assert_eq!(
            encoded["skill_consumptions"]["review-changes"]["claude-user"]["enabled"],
            false
        );
        assert_eq!(encoded["future_section"]["keep"], true);
    }

    #[test]
    fn consumption_and_existing_assignments_survive_settings_roundtrip() {
        let json = r#"{
          "mcp_consumptions": {
            "claude-code": {
              "github::stdio": {
                "asset_key": "github::stdio",
                "enabled": true,
                "overrides": {"args":["--read-only"]}
              }
            }
          },
          "model_assignments": {"claude-code":"work"},
          "skill_assignments": {"review-changes":["claude-user"]},
          "state": {"active":[]},
          "imported": "2026-07-18T00:00:00Z",
          "future_consumption_field": {"keep":true}
        }"#;

        let settings: Settings = serde_json::from_str(json).unwrap();
        let record = &settings.mcp_consumptions.as_ref().unwrap()["claude-code"]["github::stdio"];
        assert_eq!(record.asset_key, "github::stdio");
        assert_eq!(
            record.overrides.args.as_deref(),
            Some(["--read-only".to_string()].as_slice())
        );

        let encoded = serde_json::to_value(settings).unwrap();
        assert_eq!(encoded["model_assignments"]["claude-code"], "work");
        assert_eq!(
            encoded["skill_assignments"]["review-changes"][0],
            "claude-user"
        );
        assert_eq!(encoded["state"]["active"], serde_json::json!([]));
        assert_eq!(encoded["imported"], "2026-07-18T00:00:00Z");
        assert_eq!(encoded["future_consumption_field"]["keep"], true);
    }

    #[test]
    fn legacy_model_assignment_projects_and_persists_as_multi_profile_selection() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"model_assignments":{"grok-build":"work"}}"#).unwrap();

        let mut selection = settings.model_selection("grok-build");
        assert_eq!(selection.active_profile_id.as_deref(), Some("work"));
        assert!(selection.profiles["work"].enabled);
        assert!(settings.model_consumptions.is_none());

        selection.profiles.insert(
            "personal".into(),
            ModelConsumptionRecord {
                profile_id: "personal".into(),
                enabled: true,
                credential: Default::default(),
                last_selected_at: None,
            },
        );
        settings.set_model_selection("grok-build", selection);
        assert!(settings.model_consumptions.as_ref().unwrap()["grok-build"].contains_key("work"));
        assert!(
            settings.model_consumptions.as_ref().unwrap()["grok-build"].contains_key("personal")
        );
        assert_eq!(
            settings.model_assignments.as_ref().unwrap()["grok-build"],
            "work"
        );
    }

    #[test]
    fn strict_loader_defaults_missing_and_rejects_corrupt_settings() {
        let th = crate::testenv::TestHome::new("settings-strict");
        assert!(load_settings_strict().unwrap().extra.is_empty());

        let path = th.home.join(".arca/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"managed_skills": ["#).unwrap();

        let error = load_settings_strict().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(load_settings().managed_skills.is_none());
    }

    #[test]
    fn registry_section_mutation_preserves_passthrough() {
        // Simulate: load a CLI-authored doc, mutate the registry section in
        // memory, ensure state/extra are still there.
        let json = r#"{"state":{"active":[]},"weird":true}"#;
        let mut s: Settings = serde_json::from_str(json).unwrap();
        let entry = RegistryEntry {
            name: "x".into(),
            description: "".into(),
            tags: vec![],
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "c".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        };
        s.registry.get_or_insert_with(Vec::new).push(entry);
        let back = serde_json::to_string(&s).unwrap();
        assert!(back.contains("\"weird\":true"));
        assert!(back.contains("\"active\""));
        assert!(back.contains("\"name\":\"x\""));
    }

    #[test]
    fn mutation_refuses_to_replace_corrupt_settings() {
        let th = crate::testenv::TestHome::new("settings-corrupt");
        let path = th.home.join(".arca/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"registry": ["#;
        std::fs::write(&path, original).unwrap();

        let result = mutate_settings(|settings| settings.imported = Some("now".into()));

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn ui_section_and_unknown_fields_survive_settings_roundtrip() {
        let json = r#"{
      "ui": {"pinned_agents": ["claude-code", "codex"], "locale": "en-US"},
      "future_section": {"keep": true}
    }"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(
            settings.ui.as_ref().unwrap().pinned_agents,
            vec!["claude-code", "codex"]
        );
        assert_eq!(
            settings.ui.as_ref().unwrap().locale.as_deref(),
            Some("en-US")
        );
        let encoded = serde_json::to_value(settings).unwrap();
        assert_eq!(encoded["ui"]["pinned_agents"][0], "claude-code");
        assert_eq!(encoded["ui"]["locale"], "en-US");
        assert_eq!(encoded["future_section"]["keep"], true);
    }

    #[test]
    fn network_section_and_unknown_fields_survive_settings_roundtrip() {
        let json = r#"{
      "network": {
        "proxy_url": "http://127.0.0.1:7890",
        "future_network_key": {"keep": true}
      },
      "future_section": {"keep": true}
    }"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(
            settings.network.as_ref().unwrap().proxy_url.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        let encoded = serde_json::to_value(settings).unwrap();
        assert_eq!(encoded["network"]["future_network_key"]["keep"], true);
        assert_eq!(encoded["future_section"]["keep"], true);
    }

    #[test]
    fn mutation_refuses_a_concurrent_settings_change() {
        let home = crate::testenv::TestHome::new("settings-concurrent");
        let path = home.home.join(".arca/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"imported":"original"}"#).unwrap();

        let result = mutate_settings(|settings| {
            settings.imported = Some("candidate".into());
            std::fs::write(&path, r#"{"imported":"concurrent"}"#).unwrap();
        });

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            r#"{"imported":"concurrent"}"#,
        );
    }

    #[test]
    fn mutation_waits_for_settings_lock_before_reading_document() {
        let home = crate::testenv::TestHome::new("settings-filesystem-lock");
        let path = home.home.join(".arca/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"imported":"original","future":{"value":1}}"#).unwrap();
        let filesystem_lock = acquire_settings_lock(&path).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();

        let mutation = std::thread::spawn(move || {
            mutate_settings(|settings| {
                entered_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
                settings.imported = Some("mutation".into());
            })
        });

        let entered_while_locked = entered_rx.recv_timeout(Duration::from_millis(150)).is_ok();
        std::fs::write(&path, r#"{"imported":"serialized","future":{"value":2}}"#).unwrap();
        drop(filesystem_lock);
        continue_tx.send(()).unwrap();
        let result = mutation.join().unwrap();

        assert!(
            !entered_while_locked,
            "settings mutation entered before the filesystem lock was released"
        );
        result.unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved["imported"], "mutation");
        assert_eq!(saved["future"]["value"], 2);
    }

    #[test]
    fn settings_writers_take_the_filesystem_lock_before_the_process_mutex() {
        let home = crate::testenv::TestHome::new("settings-lock-order");
        let path = home.home.join(".arca/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"imported":"original"}"#).unwrap();

        let filesystem_lock = acquire_settings_lock(&path).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            mutate_settings(|settings| settings.imported = Some("writer".into()))
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let process_guard = LOCK
            .try_lock()
            .expect("a writer waiting on the filesystem lock must not hold the process mutex");
        drop(process_guard);

        // This models an asset transaction: the current thread already owns
        // the filesystem lock, then enters a nested settings mutation.
        mutate_settings(|settings| settings.imported = Some("transaction".into())).unwrap();
        drop(filesystem_lock);
        writer.join().unwrap().unwrap();

        assert_eq!(
            load_settings_strict().unwrap().imported.as_deref(),
            Some("writer")
        );
    }

    #[test]
    fn legacy_migration_rechecks_settings_after_waiting_for_filesystem_lock() {
        let home = crate::testenv::TestHome::new("migration-lock");
        let arca = home.home.join(".arca");
        let path = arca.join("settings.json");
        std::fs::create_dir_all(&arca).unwrap();
        std::fs::write(arca.join("agents.json"), r#"{"legacy":{"name":"Legacy"}}"#).unwrap();

        let filesystem_lock = acquire_settings_lock(&path).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let migration = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            migrate_if_needed().unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let fresh = r#"{"imported":"created-while-migration-waited"}"#;
        std::fs::write(&path, fresh).unwrap();
        drop(filesystem_lock);
        migration.join().unwrap();

        assert_eq!(std::fs::read_to_string(path).unwrap(), fresh);
        assert!(
            arca.join("agents.json").exists(),
            "legacy inputs must not be archived when fresh settings already won"
        );
    }

    #[test]
    fn checked_mutation_does_not_write_when_closure_fails() {
        let home = crate::testenv::TestHome::new("settings-checked-failure");
        let path = home.home.join(".arca/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"imported":"original"}"#;
        std::fs::write(&path, original).unwrap();

        let result = mutate_settings_checked(|settings| -> std::io::Result<()> {
            settings.imported = Some("candidate".into());
            Err(Error::other("validation failed"))
        });

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn future_schema_is_never_downgraded_by_an_older_writer() {
        let home = crate::testenv::TestHome::new("settings-future-version");
        let path = home.home.join(".arca/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = format!(
            r#"{{"version":{},"future":{{"nested":"keep"}}}}"#,
            SETTINGS_VERSION + 1
        );
        std::fs::write(&path, &original).unwrap();

        let error = mutate_settings(|settings| {
            settings.imported = Some("must-not-write".into());
        })
        .unwrap_err();

        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn semantic_noop_mutation_preserves_the_settings_inode() {
        use std::os::unix::fs::MetadataExt;

        let _home = crate::testenv::TestHome::new("settings-noop-inode");
        mutate_settings(|settings| {
            settings.ui.get_or_insert_with(UiSettings::default).locale = Some("en-US".into());
        })
        .unwrap();
        let path = settings_file();
        let before = fs::metadata(&path).unwrap();

        mutate_settings(|_| ()).unwrap();

        let after = fs::metadata(path).unwrap();
        assert_eq!(before.dev(), after.dev());
        assert_eq!(before.ino(), after.ino());
    }

    #[test]
    fn model_delivery_defaults_omit_plaintext_and_restore_it() {
        use crate::domain::assets::ApiKeyDelivery;

        let _home = TestHome::new("settings-delivery-default");
        mutate_settings(|settings| {
            let mut selection = settings.model_selection("opencode");
            selection.default_delivery = ApiKeyDelivery::Env;
            settings.set_model_selection("opencode", selection);
        })
        .unwrap();
        let loaded = load_settings();
        assert_eq!(
            loaded.model_selection("opencode").default_delivery,
            ApiKeyDelivery::Env
        );
        assert_eq!(
            loaded
                .model_delivery_defaults
                .as_ref()
                .and_then(|defaults| defaults.get("opencode"))
                .copied(),
            Some(ApiKeyDelivery::Env)
        );

        mutate_settings(|settings| {
            let mut selection = settings.model_selection("opencode");
            selection.default_delivery = ApiKeyDelivery::Plaintext;
            settings.set_model_selection("opencode", selection);
        })
        .unwrap();
        let loaded = load_settings();
        assert_eq!(
            loaded.model_selection("opencode").default_delivery,
            ApiKeyDelivery::Plaintext
        );
        assert!(loaded.model_delivery_defaults.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn settings_are_written_with_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let th = crate::testenv::TestHome::new("settings-mode");
        mutate_settings(|settings| settings.imported = Some("now".into())).unwrap();

        let mode = std::fs::metadata(th.home.join(".arca/settings.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
