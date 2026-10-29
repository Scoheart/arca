pub use crate::domain::agents::{
    AgentConfigurationPatch, McpConfigurationPatch, ModelConfigurationPatch,
    SkillConfigurationPatch,
};
use crate::domain::types::{AgentDefinition, AgentSkillsDirectory};
use crate::resources::mcp::scanner::collapse_home;
use crate::settings::{load_settings, mutate_settings_checked, AgentConfigPathOverride, Settings};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Error, ErrorKind};
use std::sync::LazyLock;

/// An agent definition as surfaced to a UI: its stored config plus derived
/// has-path flags. `global`/`project` keep the raw stored `~/…` paths so the UI
/// can display and prefill the path editor.
#[derive(Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub format: String,
    pub key: String,
    pub has_global: bool,
    pub has_project: bool,
    pub enabled: bool,
    pub global: Option<String>,
    pub project: Option<String>,
    pub supported_transports: Vec<&'static str>,
    pub docs: Option<String>,
    pub note: Option<String>,
    pub category: String,
    pub evidence: String,
    pub verified_at: Option<String>,
    pub builtin: bool,
    pub skills_global_dir: Option<String>,
    pub skills_global_dirs: Vec<String>,
}

pub fn supports_transport(agent_id: &str, transport: &str) -> bool {
    load_agents()
        .get(agent_id)
        .is_some_and(|definition| definition_supports_transport(definition, transport))
}

fn definition_supports_transport(definition: &AgentDefinition, transport: &str) -> bool {
    definition
        .transports
        .as_ref()
        .map(|transports| transports.iter().any(|item| item == transport))
        .unwrap_or_else(|| matches!(transport, "stdio" | "http"))
}

fn supported_transports(definition: &AgentDefinition) -> Vec<&'static str> {
    ["stdio", "http"]
        .into_iter()
        .filter(|transport| definition_supports_transport(definition, transport))
        .collect()
}

/// 内置 agent 定义：编译期内嵌 root agents.json（与 TS CLI 共用的单一数据源）
const BUILTIN_AGENTS_JSON: &str = include_str!("../../data/agents.json");
const CATALOG_AGENTS_JSON: &str = include_str!("../../data/agent-catalog.json");


fn audited_agents() -> BTreeMap<String, AgentDefinition> {
    serde_json::from_str(BUILTIN_AGENTS_JSON).expect("agents.json must be valid")
}

pub fn builtin_agents() -> BTreeMap<String, AgentDefinition> {
    // Only immutable embedded definitions are cached. User overrides and install
    // probes are still read on every observation and before every mutation.
    static BUILTINS: LazyLock<BTreeMap<String, AgentDefinition>> = LazyLock::new(|| {
        let catalog = serde_json::from_str(CATALOG_AGENTS_JSON)
            .expect("agent-catalog.json must be valid");
        merge_builtin_definitions(catalog, audited_agents())
            .expect("builtin Agent Skills capabilities must be valid")
    });
    BUILTINS.clone()
}

fn merge_builtin_definitions(
    mut catalog: BTreeMap<String, AgentDefinition>,
    audited: BTreeMap<String, AgentDefinition>,
) -> Result<BTreeMap<String, AgentDefinition>, String> {
    for definition in catalog.values_mut() {
        definition.skills = None;
    }
    validate_audited_skill_capabilities(&audited)?;
    catalog.extend(audited);
    Ok(catalog)
}

fn validate_audited_skill_capabilities(
    audited: &BTreeMap<String, AgentDefinition>,
) -> Result<(), String> {
    validate_skill_capabilities(audited)
}

fn validate_skill_capabilities(agents: &BTreeMap<String, AgentDefinition>) -> Result<(), String> {
    let mut paths_by_target = BTreeMap::<String, String>::new();
    let mut targets_by_path = BTreeMap::<String, String>::new();

    for (agent_id, definition) in agents {
        let Some(capability) = definition.skills.as_ref() else {
            continue;
        };
        if !valid_skill_identifier(agent_id) {
            return Err(format!(
                "Skills capability Agent id {agent_id} must use lowercase letters, digits, and hyphens"
            ));
        }
        if !is_verified_skill_evidence(&capability.evidence) {
            return Err(format!(
                "Skills capability for {agent_id} requires official docs or official-source evidence"
            ));
        }
        if capability.docs.trim().is_empty() {
            return Err(format!(
                "Skills capability for {agent_id} requires documentation"
            ));
        }
        if capability.verified_at.trim().is_empty() {
            return Err(format!(
                "Skills capability for {agent_id} requires a verification date"
            ));
        }
        if capability.probes.is_empty() {
            return Err(format!(
                "Skills capability for {agent_id} requires an install probe"
            ));
        }
        let directories = std::iter::once((
            capability.target_id.as_str(),
            capability.global_dir.as_str(),
        ))
        .chain(
            capability
                .aliases
                .iter()
                .map(|alias| (alias.target_id.as_str(), alias.global_dir.as_str())),
        );

        for (target_id, path) in directories {
            if !valid_skill_identifier(target_id) {
                return Err(format!(
                    "Skills target {target_id} must use lowercase letters, digits, and hyphens"
                ));
            }
            validate_skill_directory(path)
                .map_err(|reason| format!("invalid Skills target for {agent_id}: {reason}"))?;

            if let Some(existing) = paths_by_target.get(target_id) {
                if existing != path {
                    return Err(format!(
                        "Skills target {target_id} maps to both {existing} and {path}"
                    ));
                }
            } else {
                paths_by_target.insert(target_id.to_string(), path.to_string());
            }

            if let Some(existing) = targets_by_path.get(path) {
                if existing != target_id {
                    return Err(format!(
                        "Skills path {path} maps to both {existing} and {target_id}"
                    ));
                }
            } else {
                targets_by_path.insert(path.to_string(), target_id.to_string());
            }
        }
    }

    Ok(())
}

fn valid_skill_identifier(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

pub(crate) fn is_verified_skill_evidence(evidence: &str) -> bool {
    matches!(evidence, "official" | "official-source")
}

fn validate_skill_directory(path: &str) -> Result<(), String> {
    if !path.starts_with("~/") || !path.ends_with("/skills") {
        return Err(format!("{path} must be a ~/.../skills path"));
    }
    let components: Vec<&str> = path[2..].split('/').collect();
    if components
        .iter()
        .any(|component| component.is_empty() || matches!(*component, "." | ".."))
    {
        return Err(format!("{path} contains an unsafe path component"));
    }
    if matches!(components.first(), Some(&".arca") | Some(&crate::brand::LEGACY_HOME)) {
        return Err(format!("{path} is inside Arca-managed storage"));
    }
    Ok(())
}

/// 优先读 settings.agents（与 CLI 共用），缺失或为空时用内置。
pub fn load_agents() -> BTreeMap<String, AgentDefinition> {
    load_agents_from_settings(&load_settings())
}

pub(crate) fn load_agents_from_settings(
    settings: &crate::settings::Settings,
) -> BTreeMap<String, AgentDefinition> {
    let mut agents = match settings.agents.clone() {
        Some(map) if !map.is_empty() => merge_builtin_updates(map),
        _ => builtin_agents(),
    };
    for (agent_id, path_override) in settings.agent_config_paths.iter().flatten() {
        let Some(definition) = agents.get_mut(agent_id) else {
            continue;
        };
        if let Some(mcp_key) = path_override.mcp_key.as_ref() {
            definition.key = mcp_key.clone();
        }
        if let Some(global_dir) = path_override.skills_global_dir.as_ref() {
            if let Some(capability) = definition.skills.as_mut() {
                if capability.global_dir != *global_dir {
                    capability.global_dir = global_dir.clone();
                    // A configured primary directory is a runtime declaration
                    // of its own. Reusing the audited target id would make one
                    // id point to both the catalog path and the override path
                    // when another Agent still declares the catalog target.
                    capability.target_id = format!("{agent_id}-configured");
                }
            }
        }
        if let Some(alias_dirs) = path_override.skills_alias_dirs.as_ref() {
            if let Some(capability) = definition.skills.as_mut() {
                let catalog = builtin_agents()
                    .get(agent_id)
                    .and_then(|definition| definition.skills.clone());
                capability.aliases = alias_dirs
                    .iter()
                    .enumerate()
                    .map(|(index, global_dir)| {
                        catalog
                            .as_ref()
                            .and_then(|catalog| {
                                std::iter::once(AgentSkillsDirectory {
                                    target_id: catalog.target_id.clone(),
                                    global_dir: catalog.global_dir.clone(),
                                })
                                .chain(catalog.aliases.clone())
                                .find(|directory| directory.global_dir == *global_dir)
                            })
                            .unwrap_or_else(|| AgentSkillsDirectory {
                                target_id: format!("{agent_id}-configured-alias-{}", index + 1),
                                global_dir: global_dir.clone(),
                            })
                    })
                    .collect();
            }
        }
    }
    agents
}

fn merge_builtin_updates(
    mut stored: BTreeMap<String, AgentDefinition>,
) -> BTreeMap<String, AgentDefinition> {
    let builtins = builtin_agents();
    let audited = audited_agents();
    for (id, definition) in &mut stored {
        if definition.builtin == Some(true) && !audited.contains_key(id) {
            definition.skills = None;
        }
    }
    for (id, current) in builtins {
        let Some(saved) = stored.get_mut(&id) else {
            stored.insert(id, current);
            continue;
        };
        // A user-created target may legitimately share an id with a broad
        // discovery-only record. Deep-audited definitions, however, own their
        // ids and wire schemas regardless of stale/malformed persisted flags.
        if saved.builtin != Some(true) && !audited.contains_key(&id) {
            continue;
        }
        let enabled = saved.enabled;
        let global = migrated_builtin_global(&id, saved, &current);
        // Definitions are audited data, while enabled/global-path
        // customizations are user state. Replacing from the current definition
        // prevents an old settings snapshot from retaining an obsolete codec,
        // project path, or a writable path for a now read-only catalog item.
        *saved = current;
        saved.enabled = enabled;
        saved.global = global;
    }
    stored
}

fn migrated_builtin_global(
    id: &str,
    saved: &AgentDefinition,
    current: &AgentDefinition,
) -> Option<String> {
    // No audited writer means no path override. This keeps catalog-only targets
    // read-only even when old settings claimed a path for them.
    current.global.as_ref()?;
    // These clients were read-only before their user-level contracts were verified. Do
    // not turn a stale guessed path into a writable override during promotion.
    if matches!(id, "qoderwork" | "workbuddy")
        && (saved.format == "unknown" || saved.key.is_empty())
    {
        return current.global.clone();
    }
    let stale = matches!(
        (id, saved.global.as_deref()),
        ("qoder", Some("~/.qoder/settings.json"))
            | (
                "qoder",
                Some("~/Library/Application Support/Qoder/SharedClientCache/mcp.json")
            )
            | (
                "qoder",
                Some("%APPDATA%/Qoder/SharedClientCache/mcp.json")
            )
            | (
                "qoder",
                Some(r"%APPDATA%\Qoder\SharedClientCache\mcp.json")
            )
            | ("amazon-q", Some("~/.aws/amazonq/mcp.json"))
            | (
                "cline",
                Some(
                    "~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"
                )
            )
            | ("continue", Some("~/.continue/config.json"))
            | ("qoderwork", Some("~/.qoderwork/settings.json"))
    );
    if stale {
        current.global.clone()
    } else {
        saved.global.clone().or_else(|| current.global.clone())
    }
}

fn agent_overrides(map: &BTreeMap<String, AgentDefinition>) -> BTreeMap<String, AgentDefinition> {
    let builtins = builtin_agents();
    map.iter()
        .filter(|(id, definition)| builtins.get(*id) != Some(*definition))
        .map(|(id, definition)| (id.clone(), definition.clone()))
        .collect()
}

fn mcp_writer_changed(before: &AgentDefinition, after: &AgentDefinition) -> bool {
    before.global != after.global
        || before.project != after.project
        || before.format != after.format
        || before.key != after.key
        || before.key_path != after.key_path
        || before.codec != after.codec
        || before.layout != after.layout
        || before.identity_field != after.identity_field
        || before.transports != after.transports
        || before.root_defaults != after.root_defaults
}

fn skill_target_ids(definition: &AgentDefinition) -> BTreeSet<&str> {
    definition
        .skills
        .iter()
        .flat_map(|capability| {
            std::iter::once(capability.target_id.as_str()).chain(
                capability
                    .aliases
                    .iter()
                    .map(|alias| alias.target_id.as_str()),
            )
        })
        .collect()
}

fn has_assigned_skill_target(
    settings: &crate::settings::Settings,
    target_ids: &BTreeSet<&str>,
) -> bool {
    settings
        .skill_assignments
        .iter()
        .flatten()
        .any(|(_, consumers)| {
            consumers
                .iter()
                .any(|consumer| target_ids.contains(consumer.as_str()))
        })
}

/// Raw Agent catalog writers may edit presentation metadata and unconsumed
/// targets. Once a target has desired resources, changing its writer contract
/// must use the reviewed asset coordinator so old and new destinations are
/// included in one bound plan.
fn ensure_definition_change_is_unconsumed(
    settings: &Settings,
    id: &str,
    before: &AgentDefinition,
    after: &AgentDefinition,
) -> std::io::Result<()> {
    if mcp_writer_changed(before, after)
        && settings
            .mcp_consumptions
            .as_ref()
            .and_then(|consumptions| consumptions.get(id))
            .is_some_and(|records| !records.is_empty())
    {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "agent_definition_in_use: MCP writer 已有 desired resources；请通过统一 Agent capability 审阅计划修改"
        ));
    }
    if before.skills != after.skills {
        let targets = skill_target_ids(before)
            .into_iter()
            .chain(skill_target_ids(after))
            .collect();
        if has_assigned_skill_target(settings, &targets) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "agent_definition_in_use: Skills target 已有 desired resources；请通过统一 Agent capability 审阅计划修改"
            ));
        }
    }
    Ok(())
}

fn prepare_agent_definition(
    id: &str,
    mut definition: AgentDefinition,
    existing: Option<&AgentDefinition>,
) -> Result<AgentDefinition, String> {
    if let Some(existing) = existing {
        let requested_skills = definition.skills.take();
        if existing.builtin == Some(true) {
            if existing.global.is_none() {
                return Err("该 Agent 尚无可写的全局配置定义".into());
            }
            // Built-in wire schemas are audited product contracts. All callers,
            // not just the UI, may override only global path and enabled state.
            definition.project = existing.project.clone();
            definition.format = existing.format.clone();
            definition.key = builtin_agents()
                .get(id)
                .map(|candidate| candidate.key.clone())
                .unwrap_or_else(|| existing.key.clone());
        }
        copy_internal_metadata(&mut definition, existing);
        if existing.builtin != Some(true) {
            // Legacy MCP-only editors do not carry the optional Skills object.
            // Treat omission as "keep" so updating an MCP writer cannot silently
            // delete an independently managed custom Skill capability.
            if let Some(requested_skills) = requested_skills {
                definition.skills = Some(requested_skills);
            }
        }
    } else {
        definition.builtin = Some(false);
        definition.name.get_or_insert_with(|| id.to_string());
        definition.category.get_or_insert_with(|| "custom".into());
        definition.evidence.get_or_insert_with(|| "custom".into());
    }
    definition.global = definition
        .global
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(collapse_home);
    definition.project = definition
        .project
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(capability) = definition.skills.as_mut() {
        capability.global_dir = collapse_home(capability.global_dir.trim());
        for alias in &mut capability.aliases {
            alias.global_dir = collapse_home(alias.global_dir.trim());
        }
        if capability.aliases.len() > 15 {
            return Err("Skills 配置目录最多 16 个".into());
        }
    }

    let has_mcp = definition.global.is_some();
    let has_skills = definition.skills.is_some();
    if !has_mcp && !has_skills {
        return Err("全局配置路径不能为空".into());
    }
    if has_mcp {
        if definition.key.trim().is_empty() {
            return Err("配置 key 不能为空".into());
        }
        if !matches!(definition.format.as_str(), "json" | "toml" | "yaml") {
            return Err("配置格式仅支持 JSON、TOML 或 YAML".into());
        }
        if definition.layout.as_deref() == Some("list") && definition.identity_field.is_none() {
            return Err("列表型配置必须指定 identity_field".into());
        }
        definition.codec.get_or_insert_with(|| "standard".into());
        definition.layout.get_or_insert_with(|| "map".into());
        definition
            .transports
            .get_or_insert_with(|| vec!["stdio".into(), "http".into()]);
    } else {
        // Keep a Skill-only Agent free of an implied MCP writer. These fields
        // are wire metadata and have no meaning without a global MCP path.
        definition.project = None;
        definition.format.clear();
        definition.key.clear();
        definition.key_path = false;
        definition.codec = None;
        definition.layout = None;
        definition.identity_field = None;
        definition.transports = Some(Vec::new());
        definition.root_defaults = None;
    }
    Ok(definition)
}

fn persist_prepared_agent(
    id: &str,
    expected: Option<&AgentDefinition>,
    definition: AgentDefinition,
    allow_overwrite: bool,
) -> Result<(), String> {
    let id = id.to_string();
    let expected = expected.cloned();
    mutate_settings_checked(move |settings| {
        let mut agents = load_agents_from_settings(settings);
        let current = agents.get(&id).cloned();
        if current != expected {
            return Err(Error::new(
                ErrorKind::WouldBlock,
                format!("agent_definition_stale: {id} 在编辑期间已变化；请刷新后重试"),
            ));
        }
        if !allow_overwrite && current.is_some() {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                format!("agent 已存在: {id}"),
            ));
        }
        if let Some(current) = current.as_ref() {
            ensure_definition_change_is_unconsumed(settings, &id, current, &definition)?;
        }
        agents.insert(id.clone(), definition);
        validate_skill_capabilities(&agents)
            .map_err(|message| Error::new(ErrorKind::InvalidInput, message))?;
        let overrides = agent_overrides(&agents);
        settings.agents = (!overrides.is_empty()).then_some(overrides);
        Ok(())
    })
    .map_err(|error| error.to_string())
}

/// Validate + normalize an agent definition, then persist it (merged over
/// builtin/existing defs in `settings.agents`). `allow_overwrite` distinguishes
/// create (errors on an existing id) from edit (replaces in place). MCP and
/// Skills paths are collapsed to `~/…`. A custom definition may expose MCP,
/// Skills, or both; only MCP-capable definitions require a global config path,
/// key, and wire format.
pub fn put(id: String, mut def: AgentDefinition, allow_overwrite: bool) -> Result<(), String> {
    let id = id.trim().to_string();
    if id.is_empty() {
        return Err("agent id 不能为空".into());
    }
    let agents = load_agents();
    let existing = agents.get(&id).cloned();
    if !allow_overwrite && existing.is_some() {
        return Err(format!("agent 已存在: {id}"));
    }
    def = prepare_agent_definition(&id, def, existing.as_ref())?;
    persist_prepared_agent(&id, existing.as_ref(), def, allow_overwrite)
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let id = id.to_string();
    mutate_settings_checked(move |settings| {
        let mut agents = load_agents_from_settings(settings);
        let definition = agents
            .get_mut(&id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("agent 不存在: {id}")))?;
        definition.enabled = enabled;
        let overrides = agent_overrides(&agents);
        settings.agents = (!overrides.is_empty()).then_some(overrides);
        Ok(())
    })
    .map_err(|error| error.to_string())
}

/// Update only the capabilities present in `patch`. This is the canonical
/// configuration API for MCP-only, Model-only, Skill-only, and mixed Agents.
pub fn update_configuration_patch(
    id: String,
    patch: AgentConfigurationPatch,
) -> Result<(), String> {
    let expected = current_configuration_patch(&id)?;
    let normalized = normalize_configuration_patch(&id, patch)?;
    apply_direct_configuration_patch(&id, &expected, &normalized)
}

fn apply_direct_configuration_patch(
    id: &str,
    expected: &AgentConfigurationPatch,
    patch: &AgentConfigurationPatch,
) -> Result<(), String> {
    let id = id.to_string();
    let expected = expected.clone();
    let patch = patch.clone();
    mutate_settings_checked(move |settings| {
        let current = current_configuration_patch_for_settings(settings, &id)
            .map_err(|message| Error::new(ErrorKind::InvalidData, message))?;
        if current != expected {
            return Err(Error::new(
                ErrorKind::WouldBlock,
                format!("agent_configuration_stale: {id} 在编辑期间已变化；请刷新后重试"),
            ));
        }
        ensure_direct_patch_is_unconsumed(settings, &id, &current, &patch)?;
        apply_configuration_patch_to_settings(settings, &id, &patch, None)
    })
    .map_err(|error| error.to_string())
}

fn ensure_direct_patch_is_unconsumed(
    settings: &Settings,
    id: &str,
    current: &AgentConfigurationPatch,
    patch: &AgentConfigurationPatch,
) -> std::io::Result<()> {
    let mcp_changed = patch
        .mcp
        .as_ref()
        .is_some_and(|candidate| current.mcp.as_ref() != Some(candidate));
    let model_changed = patch
        .model
        .as_ref()
        .is_some_and(|candidate| current.model.as_ref() != Some(candidate));
    let skill_changed = patch
        .skill
        .as_ref()
        .is_some_and(|candidate| current.skill.as_ref() != Some(candidate));
    if !mcp_changed && !model_changed && !skill_changed {
        return Ok(());
    }

    if mcp_changed
        && settings
            .mcp_consumptions
            .as_ref()
            .and_then(|consumptions| consumptions.get(id))
            .is_some_and(|records| !records.is_empty())
    {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "agent_configuration_in_use: MCP capability 已有 desired resources；请使用统一 plan/commit",
        ));
    }
    if model_changed && !settings.model_selection(id).profiles.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "agent_configuration_in_use: Model capability 已有 desired resources；请使用统一 plan/commit",
        ));
    }
    if skill_changed {
        let agents = load_agents_from_settings(settings);
        let targets = agents.get(id).map(skill_target_ids).unwrap_or_default();
        if has_assigned_skill_target(settings, &targets) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "agent_configuration_in_use: Skill capability 已有 desired resources；请使用统一 plan/commit",
            ));
        }
    }
    Ok(())
}

pub fn current_configuration_patch(id: &str) -> Result<AgentConfigurationPatch, String> {
    current_configuration_patch_for_settings(&load_settings(), id)
}

pub(crate) fn current_configuration_patch_for_settings(
    settings: &Settings,
    id: &str,
) -> Result<AgentConfigurationPatch, String> {
    let agents = load_agents_from_settings(settings);
    let mcp = agents.get(id).and_then(|agent| {
        agent.global.clone().map(|path| McpConfigurationPatch {
            path,
            key: Some(agent.key.clone()),
        })
    });
    let model = crate::resources::model::configured_path_strings_checked(settings, id)?
        .map(|paths| ModelConfigurationPatch { paths });
    let skill = agents
        .get(id)
        .and_then(|agent| agent.skills.as_ref())
        .map(|capability| SkillConfigurationPatch {
            global_dir: capability.global_dir.clone(),
            alias_dirs: capability
                .aliases
                .iter()
                .map(|alias| alias.global_dir.clone())
                .collect(),
        });
    if mcp.is_none() && model.is_none() && skill.is_none() {
        return Err(format!("agent 不存在或没有可配置能力: {id}"));
    }
    Ok(AgentConfigurationPatch { mcp, model, skill })
}

pub(crate) fn normalize_configuration_patch(
    id: &str,
    patch: AgentConfigurationPatch,
) -> Result<AgentConfigurationPatch, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("agent id 不能为空".into());
    }
    if patch == AgentConfigurationPatch::default() {
        return Err("至少需要配置一个 Agent 能力".into());
    }

    let agents = load_agents();
    let mcp = match patch.mcp {
        Some(input) => {
            let current = agents
                .get(id)
                .ok_or_else(|| format!("agent 不存在: {id}"))?;
            if current.global.is_none() {
                return Err("该 Agent 尚无可写的 MCP 配置".into());
            }
            let path = input.path.trim();
            if path.is_empty() {
                return Err("MCP 配置路径不能为空".into());
            }
            let key = input.key.as_deref().unwrap_or(current.key.as_str()).trim();
            if key.is_empty() {
                return Err("MCP 配置键不能为空".into());
            }
            let structured_key = current.key_path
                || (current.format == "toml" && current.layout.as_deref() == Some("list"));
            if structured_key && key.split('.').any(str::is_empty) {
                return Err("MCP 配置键路径无效：不能包含空层级".into());
            }
            Some(McpConfigurationPatch {
                path: collapse_home(path),
                key: Some(key.to_string()),
            })
        }
        None => None,
    };

    let model = match patch.model {
        Some(input) => {
            let defaults = crate::resources::model::default_config_paths(id)
                .ok_or_else(|| "该 Agent 尚未接入 Model writer".to_string())?;
            Some(ModelConfigurationPatch {
                paths: crate::resources::model::normalize_config_paths(
                    &input.paths,
                    defaults.len(),
                )?,
            })
        }
        None => None,
    };

    let skill = match patch.skill {
        Some(input) => {
            if agents
                .get(id)
                .and_then(|definition| definition.skills.as_ref())
                .is_none()
            {
                return Err("该 Agent 尚未接入 Skills writer".into());
            }
            let global_dir = collapse_home(input.global_dir.trim());
            validate_skill_directory(&global_dir)
                .map_err(|reason| format!("Skills 配置路径无效: {reason}"))?;
            let mut seen = BTreeSet::from([global_dir.clone()]);
            let mut alias_dirs = Vec::new();
            for path in input.alias_dirs {
                let path = collapse_home(path.trim());
                validate_skill_directory(&path)
                    .map_err(|reason| format!("Skills 兼容目录无效: {reason}"))?;
                if seen.insert(path.clone()) {
                    alias_dirs.push(path);
                }
            }
            if alias_dirs.len() > 15 {
                return Err("Skills 配置目录最多 16 个".into());
            }
            Some(SkillConfigurationPatch {
                global_dir,
                alias_dirs,
            })
        }
        None => None,
    };

    Ok(AgentConfigurationPatch { mcp, model, skill })
}

pub(crate) fn apply_configuration_patch(
    id: &str,
    patch: &AgentConfigurationPatch,
    skill_assignments: Option<BTreeMap<String, BTreeSet<String>>>,
) -> Result<(), String> {
    mutate_settings_checked(|settings| {
        apply_configuration_patch_to_settings(settings, id, patch, skill_assignments.as_ref())
    })
    .map_err(|error| error.to_string())
}

fn apply_configuration_patch_to_settings(
    settings: &mut Settings,
    id: &str,
    patch: &AgentConfigurationPatch,
    skill_assignments: Option<&BTreeMap<String, BTreeSet<String>>>,
) -> std::io::Result<()> {
    let id = id.to_string();
    let model_defaults = crate::resources::model::default_config_paths(&id);
    let builtin_skill_defaults = builtin_agents()
        .get(&id)
        .and_then(|definition| definition.skills.as_ref())
        .cloned();

    let skill_defaults = builtin_skill_defaults.clone().or_else(|| {
        settings
            .agents
            .as_ref()
            .and_then(|agents| agents.get(&id))
            .filter(|definition| definition.builtin != Some(true))
            .and_then(|definition| definition.skills.clone())
    });
    let default_mcp_key = if let Some(mcp) = &patch.mcp {
        let mut agents = load_agents_from_settings(settings);
        let builtins = builtin_agents();
        let default_mcp_key = agents
            .get(&id)
            .filter(|definition| definition.builtin == Some(true))
            .and_then(|_| builtins.get(&id))
            .map(|definition| definition.key.clone());
        let definition = agents
            .get_mut(&id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("agent 不存在: {id}")))?;
        definition.global = Some(mcp.path.clone());
        let mcp_key = mcp
            .key
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "MCP 配置键不能为空"))?;
        definition.key = default_mcp_key.clone().unwrap_or_else(|| mcp_key.clone());
        for (agent_id, definition) in &mut agents {
            if let Some(builtin) = builtins.get(agent_id) {
                definition.skills = builtin.skills.clone();
            }
        }
        let overrides = agent_overrides(&agents);
        settings.agents = (!overrides.is_empty()).then_some(overrides);
        default_mcp_key
    } else {
        None
    };

    let path_overrides = settings.agent_config_paths.get_or_insert_default();
    let path_override = path_overrides.entry(id.clone()).or_default();
    if let Some(mcp) = &patch.mcp {
        let mcp_key = mcp
            .key
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "MCP 配置键不能为空"))?;
        path_override.mcp_key = default_mcp_key
            .as_ref()
            .and_then(|default| (default != mcp_key).then(|| mcp_key.clone()));
    }
    if let Some(model) = &patch.model {
        path_override.model_paths = model_defaults
            .as_ref()
            .is_some_and(|defaults| defaults != &model.paths)
            .then_some(model.paths.clone());
    }
    if let Some(skill) = &patch.skill {
        path_override.skills_global_dir = skill_defaults.as_ref().and_then(|defaults| {
            (defaults.global_dir != skill.global_dir).then_some(skill.global_dir.clone())
        });
        let default_alias_dirs = skill_defaults
            .as_ref()
            .map(|defaults| {
                defaults
                    .aliases
                    .iter()
                    .map(|alias| alias.global_dir.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        path_override.skills_alias_dirs =
            (default_alias_dirs != skill.alias_dirs).then_some(skill.alias_dirs.clone());
    }
    if path_override == &AgentConfigPathOverride::default() {
        path_overrides.remove(&id);
    }
    if path_overrides.is_empty() {
        settings.agent_config_paths = None;
    }
    if let Some(assignments) = skill_assignments {
        settings.skill_assignments = (!assignments.is_empty()).then_some(assignments.clone());
    }
    Ok(())
}

/// List all agent definitions as `AgentInfo` view rows.
pub fn list_infos() -> Vec<AgentInfo> {
    list_infos_from_definitions(load_agents())
}

pub(crate) fn list_infos_from_definitions(definitions: BTreeMap<String, AgentDefinition>) -> Vec<AgentInfo> {
    definitions
        .into_iter()
        .map(|(id, d)| {
            let skills_global_dir = d
                .skills
                .as_ref()
                .map(|capability| capability.global_dir.clone());
            let skills_global_dirs = d
                .skills
                .as_ref()
                .map(|capability| {
                    std::iter::once(capability.global_dir.clone())
                        .chain(
                            capability
                                .aliases
                                .iter()
                                .map(|alias| alias.global_dir.clone()),
                        )
                        .collect()
                })
                .unwrap_or_default();
            AgentInfo {
                supported_transports: supported_transports(&d),
                name: d.name.clone().unwrap_or_else(|| id.clone()),
                id,
                format: d.format,
                key: d.key,
                has_global: d.global.is_some(),
                has_project: d.project.is_some(),
                enabled: d.enabled,
                global: d.global,
                project: d.project,
                docs: d.docs,
                note: d.note,
                category: d.category.unwrap_or_else(|| "custom".into()),
                evidence: d.evidence.unwrap_or_else(|| "custom".into()),
                verified_at: d.verified_at,
                builtin: d.builtin == Some(true),
                skills_global_dir,
                skills_global_dirs,
            }
        })
        .collect()
}

fn copy_internal_metadata(definition: &mut AgentDefinition, existing: &AgentDefinition) {
    definition.builtin = existing.builtin;
    definition.name = existing.name.clone();
    definition.docs = existing.docs.clone();
    definition.note = existing.note.clone();
    definition.category = existing.category.clone();
    definition.evidence = existing.evidence.clone();
    definition.verified_at = existing.verified_at.clone();
    definition.codec = existing.codec.clone();
    definition.layout = existing.layout.clone();
    definition.identity_field = existing.identity_field.clone();
    definition.transports = existing.transports.clone();
    definition.root_defaults = existing.root_defaults.clone();
    definition.key_path = existing.key_path;
    definition.skills = existing.skills.clone();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assets::{
        McpConsumptionRecord, ModelAgentSelection, ModelConsumptionRecord,
    };
    use crate::domain::mcp::OverridePatch;
    use crate::domain::types::{AgentInstallProbe, AgentSkillsCapability, AgentSkillsDirectory};
    use crate::settings::mutate_settings;

    fn skills_capability(target_id: &str, global_dir: &str) -> AgentSkillsCapability {
        AgentSkillsCapability {
            target_id: target_id.into(),
            global_dir: global_dir.into(),
            aliases: Vec::new(),
            docs: "https://example.com/skills".into(),
            evidence: "official".into(),
            verified_at: "2026-07-16".into(),
            probes: vec![AgentInstallProbe::Path {
                path: "/Applications/Example.app".into(),
            }],
        }
    }

    #[test]
    fn builtin_catalog_and_transport_metadata_load() {
        let a = builtin_agents();
        assert_eq!(audited_agents().len(), 95);
        let catalog: BTreeMap<String, AgentDefinition> =
            serde_json::from_str(CATALOG_AGENTS_JSON).unwrap();
        assert_eq!(catalog.len(), 280);
        assert_eq!(a.len(), 311);
        assert_eq!(a["claude-code"].key, "mcpServers");
        assert_eq!(a["codex"].format, "toml");
        assert_eq!(
            a.iter()
                .filter_map(|(id, definition)| definition.key_path.then_some(id.as_str()))
                .collect::<Vec<_>>(),
            vec!["amp", "zcode"]
        );
        assert!(!definition_supports_transport(&a["claude-desktop"], "http"));
        assert!(definition_supports_transport(&a["claude-desktop"], "stdio"));
        assert!(definition_supports_transport(&a["claude-code"], "http"));
    }

    #[test]
    fn pi_uses_its_official_native_mcp_contract() {
        assert_eq!(
            audited_agents()["pi"].name.as_deref(),
            Some("Pi Coding Agent")
        );
        assert_eq!(audited_agents()["pi"].evidence.as_deref(), Some("official"));
        assert_eq!(audited_agents()["pi"].codec.as_deref(), Some("pi"));
        assert_eq!(audited_agents()["pi"].docs.as_deref(), Some("https://pi.dev/docs/latest/mcp"));
    }

    #[test]
    fn amazon_q_ide_and_cli_remain_distinct_catalog_surfaces() {
        let agents = builtin_agents();
        let ide = &agents["amazon-q"];
        assert_eq!(ide.name.as_deref(), Some("Amazon Q Developer IDE"));
        assert_eq!(ide.category.as_deref(), Some("plugin"));
        assert_eq!(ide.global.as_deref(), Some("~/.aws/amazonq/default.json"));
        assert_eq!(
            ide.docs.as_deref(),
            Some("https://docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/mcp-ide.html")
        );

        let cli = &agents["amazon-q-cli"];
        assert_eq!(cli.name.as_deref(), Some("Amazon Q CLI"));
        assert!(cli.global.is_none());
        assert!(cli.transports.as_ref().is_some_and(Vec::is_empty));
    }

    #[test]
    fn verified_skill_capabilities_are_data_driven() {
        let agents = builtin_agents();
        let capability_ids: Vec<&str> = agents
            .iter()
            .filter_map(|(id, definition)| definition.skills.as_ref().map(|_| id.as_str()))
            .collect();
        let audited = audited_agents();
        let audited_ids = audited.iter().filter_map(|(id, definition)|
            definition.skills.as_ref().map(|_| id.as_str())).collect::<Vec<_>>();
        assert_eq!(capability_ids, audited_ids);

        let workbuddy = agents["workbuddy"].skills.as_ref().unwrap();
        assert_eq!(workbuddy.target_id, "workbuddy-ai-user");
        assert_eq!(workbuddy.global_dir, "~/.workbuddy-ai/skills");
        assert!(workbuddy.aliases.is_empty());

        let codex = agents["codex"].skills.as_ref().unwrap();
        assert_eq!(codex.target_id, "agents-user");
        assert_eq!(codex.global_dir, "~/.agents/skills");
        assert!(codex.aliases.is_empty());

        let cursor = agents["cursor"].skills.as_ref().unwrap();
        assert_eq!(cursor.global_dir, "~/.cursor/skills");
        assert_eq!(cursor.aliases[0].target_id, "agents-user");
        assert_eq!(cursor.aliases[0].global_dir, "~/.agents/skills");

        let cursor_cli = agents["cursor-cli"].skills.as_ref().unwrap();
        assert_eq!(cursor_cli.global_dir, "~/.cursor/skills");
        assert_eq!(
            cursor_cli
                .probes
                .iter()
                .map(|probe| match probe {
                    AgentInstallProbe::Command { name } => name.as_str(),
                    AgentInstallProbe::Path { .. } => "path",
                    AgentInstallProbe::MacBundle { .. } => "bundle",
                })
                .collect::<Vec<_>>(),
            vec!["path", "cursor-agent"]
        );

        let warp = agents["warp"].skills.as_ref().unwrap();
        assert_eq!(warp.global_dir, "~/.agents/skills");
        assert!(warp
            .aliases
            .iter()
            .any(|alias| alias.global_dir == "~/.codex/skills"));

        let codewhale = agents["codewhale"].skills.as_ref().unwrap();
        assert_eq!(codewhale.global_dir, "~/.codewhale/skills");
        assert_eq!(
            codewhale
                .aliases
                .iter()
                .map(|alias| alias.global_dir.as_str())
                .collect::<Vec<_>>(),
            vec!["~/.agents/skills", "~/.claude/skills"]
        );

        let stakpak = agents["stakpak"].skills.as_ref().unwrap();
        assert_eq!(stakpak.global_dir, "~/.stakpak/skills");

        assert_eq!(
            agents["docker-agent"].skills.as_ref().unwrap().global_dir,
            "~/.agents/skills"
        );
        let cortex = agents["cortex-code"].skills.as_ref().unwrap();
        assert_eq!(cortex.global_dir, "~/.snowflake/cortex/skills");
        assert_eq!(cortex.aliases[0].global_dir, "~/.claude/skills");

        let dirac = agents["dirac"].skills.as_ref().unwrap();
        assert_eq!(dirac.global_dir, "~/.agents/skills");
        assert_eq!(
            dirac
                .aliases
                .iter()
                .map(|alias| alias.global_dir.as_str())
                .collect::<Vec<_>>(),
            vec!["~/.dirac/skills", "~/.claude/skills", "~/.ai/skills"]
        );

        let minion = agents["minion-code"].skills.as_ref().unwrap();
        assert_eq!(minion.global_dir, "~/.minion/skills");
        assert_eq!(minion.aliases[0].global_dir, "~/.claude/skills");

        for id in ["cortex-code", "dirac", "minion-code"] {
            assert!(agents[id].global.is_none());
            assert!(agents[id].transports.as_ref().is_some_and(Vec::is_empty));
        }
        for capability in agents.values().filter_map(|agent| agent.skills.as_ref()) {
            assert!(!capability.docs.is_empty());
            assert!(matches!(
                capability.evidence.as_str(),
                "official" | "official-source"
            ));
            assert!(!capability.probes.is_empty());
        }
    }

    #[test]
    fn agent_info_projects_every_trusted_skills_directory() {
        let _home = crate::testenv::TestHome::new("agent-info-skills-path");
        let infos = list_infos();
        let codex = infos.iter().find(|agent| agent.id == "codex").unwrap();
        let claude_desktop = infos
            .iter()
            .find(|agent| agent.id == "claude-desktop")
            .unwrap();

        assert_eq!(codex.skills_global_dir.as_deref(), Some("~/.agents/skills"));
        let cursor = infos.iter().find(|agent| agent.id == "cursor").unwrap();
        assert_eq!(
            cursor.skills_global_dirs,
            ["~/.cursor/skills", "~/.agents/skills"]
        );
        let cursor_cli = infos
            .iter()
            .find(|agent| agent.id == "cursor-cli")
            .unwrap();
        assert_eq!(
            cursor_cli.skills_global_dirs,
            ["~/.cursor/skills", "~/.agents/skills"]
        );
        assert_eq!(claude_desktop.skills_global_dir, None);
        for (id, expected) in [
            ("cortex-code", "~/.snowflake/cortex/skills"),
            ("dirac", "~/.agents/skills"),
            ("minion-code", "~/.minion/skills"),
        ] {
            let info = infos.iter().find(|agent| agent.id == id).unwrap();
            assert!(!info.has_global);
            assert_eq!(info.skills_global_dir.as_deref(), Some(expected));
        }
    }

    #[test]
    fn skill_capabilities_require_complete_official_evidence() {
        let mut unofficial = skills_capability("unofficial-user", "~/.unofficial/skills");
        unofficial.evidence = "catalog".into();
        let mut undocumented = skills_capability("undocumented-user", "~/.undocumented/skills");
        undocumented.docs = "  ".into();
        let mut unverified = skills_capability("unverified-user", "~/.unverified/skills");
        unverified.verified_at = "".into();
        let mut unprobed = skills_capability("unprobed-user", "~/.unprobed/skills");
        unprobed.probes.clear();

        for (id, capability) in [
            ("unofficial", unofficial),
            ("undocumented", undocumented),
            ("unverified", unverified),
            ("unprobed", unprobed),
        ] {
            let agents = BTreeMap::from([(
                id.into(),
                AgentDefinition {
                    skills: Some(capability),
                    ..Default::default()
                },
            )]);
            assert!(
                validate_skill_capabilities(&agents).is_err(),
                "accepted incomplete Skills evidence for {id}"
            );
        }
    }

    #[test]
    fn catalog_only_skill_capability_cannot_survive_merge() {
        let catalog = BTreeMap::from([(
            "catalog-forged".into(),
            AgentDefinition {
                builtin: Some(false),
                skills: Some(skills_capability(
                    "catalog-forged-user",
                    "~/.catalog-forged/skills",
                )),
                ..Default::default()
            },
        )]);

        let merged = merge_builtin_definitions(catalog, audited_agents()).unwrap();

        assert!(merged["catalog-forged"].skills.is_none());
    }

    #[test]
    fn audited_skill_capabilities_must_match_approved_agent_set() {
        let mut missing = audited_agents();
        missing.get_mut("codex").unwrap().skills = None;
        assert!(merge_builtin_definitions(BTreeMap::new(), missing).is_err());

        let mut extra = audited_agents();
        extra.insert(
            "extra-audited".into(),
            AgentDefinition {
                builtin: Some(true),
                skills: Some(skills_capability(
                    "extra-audited-user",
                    "~/.extra-audited/skills",
                )),
                ..Default::default()
            },
        );
        assert!(merge_builtin_definitions(BTreeMap::new(), extra).is_err());
    }

    #[test]
    fn skill_evidence_accepts_official_source_but_rejects_community_claims() {
        let mut source = BTreeMap::from([(
            "source-backed".into(),
            AgentDefinition {
                skills: Some(skills_capability(
                    "source-backed-user",
                    "~/.source-backed/skills",
                )),
                ..Default::default()
            },
        )]);
        source
            .get_mut("source-backed")
            .unwrap()
            .skills
            .as_mut()
            .unwrap()
            .evidence = "official-source".into();
        validate_skill_capabilities(&source).unwrap();

        source
            .get_mut("source-backed")
            .unwrap()
            .skills
            .as_mut()
            .unwrap()
            .evidence = "community".into();
        assert!(validate_skill_capabilities(&source)
            .unwrap_err()
            .contains("official docs or official-source"));
    }

    #[test]
    fn skill_target_validation_rejects_unsafe_paths_and_contradictions() {
        for path in [
            "/tmp/skills",
            "~/.agents/not-skills",
            "~/.agents//skills",
            "~/.agents/./skills",
            "~/.agents/../skills",
            "~/.arca/assets/skills/items",
        ] {
            let agents = BTreeMap::from([(
                "unsafe".into(),
                AgentDefinition {
                    skills: Some(skills_capability("unsafe-user", path)),
                    ..Default::default()
                },
            )]);
            assert!(
                validate_skill_capabilities(&agents).is_err(),
                "accepted unsafe Skills path: {path}"
            );
        }

        let mut unsafe_alias = skills_capability("safe-user", "~/.safe/skills");
        unsafe_alias.aliases.push(AgentSkillsDirectory {
            target_id: "unsafe-alias".into(),
            global_dir: "~/.arca/assets/skills/items".into(),
        });
        let agents = BTreeMap::from([(
            "unsafe-alias".into(),
            AgentDefinition {
                skills: Some(unsafe_alias),
                ..Default::default()
            },
        )]);
        assert!(validate_skill_capabilities(&agents).is_err());

        let conflicting_target = BTreeMap::from([
            (
                "one".into(),
                AgentDefinition {
                    skills: Some(skills_capability("shared-user", "~/.one/skills")),
                    ..Default::default()
                },
            ),
            (
                "two".into(),
                AgentDefinition {
                    skills: Some(skills_capability("shared-user", "~/.two/skills")),
                    ..Default::default()
                },
            ),
        ]);
        assert!(validate_skill_capabilities(&conflicting_target).is_err());

        let conflicting_path = BTreeMap::from([
            (
                "one".into(),
                AgentDefinition {
                    skills: Some(skills_capability("one-user", "~/.shared/skills")),
                    ..Default::default()
                },
            ),
            (
                "two".into(),
                AgentDefinition {
                    skills: Some(skills_capability("two-user", "~/.shared/skills")),
                    ..Default::default()
                },
            ),
        ]);
        assert!(validate_skill_capabilities(&conflicting_path).is_err());
    }

    #[test]
    fn custom_skill_only_agents_persist_and_load_capabilities() {
        let capability = skills_capability("custom-skill-user", "~/.custom-skill/skills");
        let mut stored = builtin_agents();
        stored.insert(
            "custom-skill".into(),
            AgentDefinition {
                global: None,
                format: String::new(),
                key: String::new(),
                enabled: true,
                builtin: Some(false),
                skills: Some(capability.clone()),
                ..Default::default()
            },
        );
        assert_eq!(
            merge_builtin_updates(stored)["custom-skill"].skills,
            Some(capability.clone())
        );

        let _home = crate::testenv::TestHome::new("agent-custom-skill-only");
        put(
            "custom-skill".into(),
            AgentDefinition {
                global: None,
                format: "not-an-mcp-format".into(),
                key: "not-an-mcp-key".into(),
                enabled: true,
                skills: Some(capability.clone()),
                ..Default::default()
            },
            false,
        )
        .unwrap();

        let settings = load_settings();
        let persisted = &settings.agents.as_ref().unwrap()["custom-skill"];
        assert_eq!(persisted.skills.as_ref(), Some(&capability));
        assert!(persisted.global.is_none());
        assert!(persisted.format.is_empty());
        assert!(persisted.key.is_empty());
        assert_eq!(persisted.transports, Some(Vec::new()));

        let loaded = load_agents();
        assert_eq!(loaded["custom-skill"].skills.as_ref(), Some(&capability));
        assert!(loaded["custom-skill"].global.is_none());

        update_configuration_patch(
            "custom-skill".into(),
            AgentConfigurationPatch {
                skill: Some(SkillConfigurationPatch {
                    global_dir: "~/.custom-skill-v2/skills".into(),
                    alias_dirs: vec!["~/.custom-skill-shared/skills".into()],
                }),
                ..AgentConfigurationPatch::default()
            },
        )
        .unwrap();

        let configured = load_agents();
        let configured_skill = configured["custom-skill"].skills.as_ref().unwrap();
        assert_eq!(configured_skill.global_dir, "~/.custom-skill-v2/skills");
        assert_eq!(
            configured_skill.aliases[0].global_dir,
            "~/.custom-skill-shared/skills"
        );
        assert!(configured["custom-skill"].global.is_none());
    }

    #[test]
    fn custom_mcp_only_update_preserves_existing_skill_capability() {
        let _home = crate::testenv::TestHome::new("agent-custom-mixed-update");
        let capability = skills_capability("custom-mixed-user", "~/.custom-mixed/skills");
        put(
            "custom-mixed".into(),
            AgentDefinition {
                global: Some("~/.custom-mixed/mcp.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                skills: Some(capability.clone()),
                ..Default::default()
            },
            false,
        )
        .unwrap();

        put(
            "custom-mixed".into(),
            AgentDefinition {
                global: Some("~/.custom-mixed/moved.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                skills: None,
                ..Default::default()
            },
            true,
        )
        .unwrap();

        let updated = &load_agents()["custom-mixed"];
        assert_eq!(
            updated.global.as_deref(),
            Some("~/.custom-mixed/moved.json")
        );
        assert_eq!(updated.skills.as_ref(), Some(&capability));
    }

    #[test]
    fn prepared_agent_write_merges_unrelated_changes_and_rejects_stale_target() {
        let _home = crate::testenv::TestHome::new("agent-atomic-cas");
        let definition = |path: &str| AgentDefinition {
            global: Some(path.into()),
            format: "json".into(),
            key: "mcpServers".into(),
            enabled: true,
            ..Default::default()
        };
        put(
            "custom-first".into(),
            definition("~/.custom-first/mcp.json"),
            false,
        )
        .unwrap();

        let expected = load_agents()["custom-first"].clone();
        let prepared = prepare_agent_definition(
            "custom-first",
            definition("~/.custom-first/moved.json"),
            Some(&expected),
        )
        .unwrap();
        put(
            "custom-second".into(),
            definition("~/.custom-second/mcp.json"),
            false,
        )
        .unwrap();

        persist_prepared_agent("custom-first", Some(&expected), prepared, true).unwrap();
        let merged = load_agents();
        assert!(merged.contains_key("custom-second"));
        assert_eq!(
            merged["custom-first"].global.as_deref(),
            Some("~/.custom-first/moved.json")
        );

        let expected = merged["custom-first"].clone();
        let stale = prepare_agent_definition(
            "custom-first",
            definition("~/.custom-first/stale.json"),
            Some(&expected),
        )
        .unwrap();
        set_enabled("custom-first", false).unwrap();

        let error =
            persist_prepared_agent("custom-first", Some(&expected), stale, true).unwrap_err();
        assert!(error.starts_with("agent_definition_stale:"), "{error}");
        let current = &load_agents()["custom-first"];
        assert!(!current.enabled);
        assert_eq!(
            current.global.as_deref(),
            Some("~/.custom-first/moved.json")
        );
    }

    #[test]
    fn direct_configuration_path_changes_fail_closed_when_in_use() {
        let _home = crate::testenv::TestHome::new("agent-direct-config-in-use");
        put(
            "custom-mcp-config".into(),
            AgentDefinition {
                global: Some("~/.custom-mcp-config/mcp.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
        mutate_settings(|settings| {
            settings
                .mcp_consumptions
                .get_or_insert_default()
                .entry("custom-mcp-config".into())
                .or_default()
                .insert(
                    "fixture::stdio".into(),
                    McpConsumptionRecord {
                        asset_key: "fixture::stdio".into(),
                        enabled: true,
                        overrides: OverridePatch::default(),
                    },
                );
            settings.set_model_selection(
                "grok-build",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        "fixture".into(),
                        ModelConsumptionRecord {
                            profile_id: "fixture".into(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some("fixture".into()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let mcp_before = current_configuration_patch("custom-mcp-config").unwrap();
        let error = update_configuration_patch(
            "custom-mcp-config".into(),
            AgentConfigurationPatch {
                mcp: Some(McpConfigurationPatch {
                    path: "~/.custom-mcp-config/moved.json".into(),
                    key: None,
                }),
                ..AgentConfigurationPatch::default()
            },
        )
        .unwrap_err();
        assert!(error.starts_with("agent_configuration_in_use:"), "{error}");
        assert_eq!(
            current_configuration_patch("custom-mcp-config").unwrap(),
            mcp_before
        );

        let model_before = current_configuration_patch("grok-build").unwrap();
        let error = update_configuration_patch(
            "grok-build".into(),
            AgentConfigurationPatch {
                model: Some(ModelConfigurationPatch {
                    paths: vec!["~/.grok/moved.toml".into()],
                }),
                ..AgentConfigurationPatch::default()
            },
        )
        .unwrap_err();
        assert!(error.starts_with("agent_configuration_in_use:"), "{error}");
        assert_eq!(
            current_configuration_patch("grok-build").unwrap(),
            model_before
        );
    }

    #[test]
    fn direct_agent_writer_rejects_an_in_use_mcp_contract_change() {
        let _home = crate::testenv::TestHome::new("agent-in-use-writer");
        put(
            "custom-mcp".into(),
            AgentDefinition {
                global: Some("~/.custom-mcp/config.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
        mutate_settings(|settings| {
            settings
                .mcp_consumptions
                .get_or_insert_default()
                .entry("custom-mcp".into())
                .or_default()
                .insert(
                    "fixture::stdio".into(),
                    McpConsumptionRecord {
                        asset_key: "fixture::stdio".into(),
                        enabled: true,
                        overrides: OverridePatch::default(),
                    },
                );
        })
        .unwrap();

        let mut moved = load_agents()["custom-mcp"].clone();
        moved.global = Some("~/.custom-mcp/moved.json".into());
        let error = put("custom-mcp".into(), moved, true).unwrap_err();
        assert!(error.starts_with("agent_definition_in_use:"), "{error}");

        let mut metadata_only = load_agents()["custom-mcp"].clone();
        metadata_only.enabled = false;
        put("custom-mcp".into(), metadata_only, true).unwrap();
        assert!(!load_agents()["custom-mcp"].enabled);
    }

    #[test]
    fn unaudited_builtin_skill_capability_cannot_survive_merge() {
        let mut stored = builtin_agents();
        stored.get_mut("devin").unwrap().skills =
            Some(skills_capability("forged-user", "~/.forged/skills"));

        assert!(merge_builtin_updates(stored)["devin"].skills.is_none());
    }

    #[test]
    fn stale_builtin_paths_migrate_without_touching_custom_agents() {
        let mut stored = builtin_agents();
        stored.get_mut("qoder").unwrap().global = Some("~/.qoder/settings.json".into());
        stored.get_mut("amazon-q").unwrap().global = Some("~/.aws/amazonq/mcp.json".into());
        stored.get_mut("amazon-q").unwrap().project = Some(".amazonq/mcp.json".into());
        stored.get_mut("cline").unwrap().global = Some(
            "~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json".into(),
        );
        stored.get_mut("cline").unwrap().enabled = false;
        stored.get_mut("continue").unwrap().global = Some("~/.continue/config.json".into());
        stored.get_mut("continue").unwrap().project = Some(".continue/config.json".into());
        stored.get_mut("qoderwork").unwrap().global = Some("~/.custom/qoderwork.json".into());
        stored.get_mut("qoderwork").unwrap().format = "unknown".into();
        stored.get_mut("qoderwork").unwrap().key.clear();
        stored.get_mut("workbuddy").unwrap().global = Some("~/.guessed/mcp.json".into());
        stored.get_mut("workbuddy").unwrap().format = "unknown".into();
        stored.get_mut("workbuddy").unwrap().key.clear();
        let custom = AgentDefinition {
            global: Some("~/.custom/mcp.json".into()),
            project: None,
            format: "json".into(),
            key: "mcpServers".into(),
            enabled: false,
            builtin: Some(false),
            ..Default::default()
        };
        stored.insert("custom".into(), custom.clone());

        let merged = merge_builtin_updates(stored);

        assert_eq!(merged["qoder"].global.as_deref(), Some("~/.qoder/mcp.json"));
        assert_eq!(
            merged["amazon-q"].global.as_deref(),
            Some("~/.aws/amazonq/default.json")
        );
        assert_eq!(
            merged["amazon-q"].project.as_deref(),
            Some(".amazonq/default.json")
        );
        assert_eq!(
            merged["cline"].global.as_deref(),
            Some("~/.cline/data/settings/cline_mcp_settings.json")
        );
        assert!(!merged["cline"].enabled);
        assert_eq!(
            merged["continue"].global.as_deref(),
            Some("~/.continue/config.yaml")
        );
        assert!(merged["continue"].project.is_none());
        assert_eq!(
            merged["qoderwork"].global.as_deref(),
            Some("~/.qoderwork/mcp.json")
        );
        assert_eq!(merged["workbuddy"].global.as_deref(), Some("~/.workbuddy-ai/mcp.json"));
        assert_eq!(merged["custom"], custom);
    }

    #[test]
    fn stale_qoder_desktop_paths_migrate_without_touching_custom_paths() {
        for stale in [
            "~/.qoder/settings.json",
            "~/Library/Application Support/Qoder/SharedClientCache/mcp.json",
            "%APPDATA%/Qoder/SharedClientCache/mcp.json",
            r"%APPDATA%\Qoder\SharedClientCache\mcp.json",
        ] {
            let mut stored = builtin_agents();
            stored.get_mut("qoder").unwrap().global = Some(stale.into());

            assert_eq!(
                merge_builtin_updates(stored)["qoder"].global.as_deref(),
                Some("~/.qoder/mcp.json"),
                "stale Qoder Desktop path should migrate: {stale}",
            );
        }

        let mut customized = builtin_agents();
        customized.get_mut("qoder").unwrap().global = Some("~/.custom/qoder-mcp.json".into());
        assert_eq!(
            merge_builtin_updates(customized)["qoder"].global.as_deref(),
            Some("~/.custom/qoder-mcp.json")
        );
    }

    #[test]
    fn stale_qoderwork_settings_path_migrates_to_mcp_file() {
        let mut stored = builtin_agents();
        stored.get_mut("qoderwork").unwrap().global = Some("~/.qoderwork/settings.json".into());

        let merged = merge_builtin_updates(stored);

        assert_eq!(
            merged["qoderwork"].global.as_deref(),
            Some("~/.qoderwork/mcp.json")
        );
    }

    #[test]
    fn audited_schema_wins_but_custom_catalog_collision_survives() {
        let mut stored = builtin_agents();
        stored.insert(
            "codex".into(),
            AgentDefinition {
                global: Some("~/.custom/codex.toml".into()),
                project: Some("unsafe-project.yaml".into()),
                format: "yaml".into(),
                key: "unsafe".into(),
                enabled: false,
                builtin: Some(false),
                ..Default::default()
            },
        );
        let custom_chatgpt = AgentDefinition {
            global: Some("~/.custom/chatgpt.json".into()),
            project: None,
            format: "json".into(),
            key: "mcpServers".into(),
            enabled: true,
            builtin: Some(false),
            ..Default::default()
        };
        stored.insert("chatgpt".into(), custom_chatgpt.clone());

        let merged = merge_builtin_updates(stored);
        assert_eq!(merged["codex"].format, "toml");
        assert_eq!(merged["codex"].key, "mcp_servers");
        assert_eq!(
            merged["codex"].project.as_deref(),
            Some(".codex/config.toml")
        );
        assert_eq!(
            merged["codex"].global.as_deref(),
            Some("~/.custom/codex.toml")
        );
        assert!(!merged["codex"].enabled);
        assert_eq!(merged["codex"].builtin, Some(true));
        assert_eq!(merged["chatgpt"], custom_chatgpt);
    }

    #[test]
    fn persistence_keeps_only_builtin_overrides_and_custom_agents() {
        let mut agents = builtin_agents();
        assert!(agent_overrides(&agents).is_empty());

        agents.get_mut("codex").unwrap().enabled = false;
        agents.insert(
            "custom".into(),
            AgentDefinition {
                global: Some("~/.custom/mcp.json".into()),
                project: None,
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                builtin: Some(false),
                ..Default::default()
            },
        );

        let overrides = agent_overrides(&agents);
        assert_eq!(overrides.len(), 2);
        assert!(overrides.contains_key("codex"));
        assert!(overrides.contains_key("custom"));
    }

    #[test]
    fn put_locks_builtin_schema_and_rejects_catalog_only_targets() {
        let _home = crate::testenv::TestHome::new("agent-schema-lock");
        let mut codex = builtin_agents()["codex"].clone();
        codex.global = Some("~/.custom-codex.toml".into());
        codex.project = Some(".unsafe/project.yaml".into());
        codex.format = "yaml".into();
        codex.key = "unsafe".into();
        codex.codec = Some("goose".into());
        codex.layout = Some("list".into());
        codex.identity_field = Some("unsafe".into());
        codex.transports = Some(vec!["stdio".into()]);
        codex.key_path = true;
        codex.builtin = Some(false);

        put("codex".into(), codex, true).unwrap();
        let stored = load_agents();
        let codex = &stored["codex"];
        assert_eq!(codex.global.as_deref(), Some("~/.custom-codex.toml"));
        assert_eq!(codex.project.as_deref(), Some(".codex/config.toml"));
        assert_eq!(codex.format, "toml");
        assert_eq!(codex.key, "mcp_servers");
        assert_eq!(codex.codec.as_deref(), Some("codex"));
        assert_eq!(codex.layout.as_deref(), Some("map"));
        assert!(!codex.key_path);
        assert_eq!(codex.builtin, Some(true));
        assert!(definition_supports_transport(codex, "http"));

        let attempted_devin = AgentDefinition {
            global: Some("~/.devin/mcp.json".into()),
            project: None,
            format: "json".into(),
            key: "mcpServers".into(),
            enabled: true,
            builtin: Some(false),
            ..Default::default()
        };
        assert!(put("devin".into(), attempted_devin, true).is_err());
    }
}
