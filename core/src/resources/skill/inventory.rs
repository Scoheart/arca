use super::anchored::{AnchoredFileKind, AnchoredIdentity, AnchoredRoot};
#[cfg(test)]
use super::files::MAX_SKILL_BYTES;
use super::files::{
    classify_content, inspect_tree_anchored, validate_candidate_anchored,
    validate_candidate_anchored_private,
};
use super::{
    audit_skill, parse_manifest, InventoryState, ManagedSkillRecord, SkillAgentCapabilityView,
    SkillAgentView, SkillContentKind, SkillDetail, SkillError, SkillInventoryItem, SkillLocation,
    SkillRiskSummary, SkillSource, SkillTargetView, SkillUpdateState, SkillsInventory, SkillsPaths,
    ValidatedSkill,
};
use crate::agents::load_agents_from_settings;
use crate::domain::types::{AgentInstallProbe, AgentSkillsCapability};
use crate::settings::{load_settings_strict, Settings};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CString, OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_SKILL_MD_DETAIL_BYTES: usize = 1024 * 1024;
const MAX_INVENTORY_SETTINGS_RECORDS: u64 = 10_000;
const MAX_INVENTORY_ENTRIES: u64 = 10_000;
const MAX_INVENTORY_RETURNED_ITEMS: u64 = 10_000;
const MAX_INVENTORY_MANAGED_BYTES: u64 = 512 * 1024 * 1024;
const TEST_PROBE_ROOT_ENV: &str = "ARCA_TEST_PROBE_ROOT";

#[derive(Debug, Clone)]
struct CatalogSkillAgent {
    id: String,
    name: String,
    capability: AgentSkillsCapability,
    primary_target_id: String,
    declared_target_ids: BTreeSet<String>,
    installed: bool,
}

#[derive(Debug, Clone)]
struct PhysicalTarget {
    target_id: String,
    global_dir: String,
    canonical_root: PathBuf,
    observed_identity: Option<AnchoredIdentity>,
    primary_agent_ids: BTreeSet<String>,
    affected_agent_ids: BTreeSet<String>,
}

#[derive(Debug)]
struct TargetGraph {
    catalog_agents: BTreeMap<String, CatalogSkillAgent>,
    targets: BTreeMap<String, PhysicalTarget>,
    target_aliases: BTreeMap<String, String>,
    included_target_ids: BTreeSet<String>,
    agents: Vec<SkillAgentView>,
    target_views: Vec<SkillTargetView>,
}

#[derive(Debug, Clone)]
struct ItemMetadata {
    // Observed central bytes may differ from the installed record. Exact
    // managed links inherit this state; same-name external directories do not.
    locally_modified: bool,
    description: String,
    content_kind: SkillContentKind,
    source: Option<SkillSource>,
    resolved_revision: Option<String>,
    content_hash: Option<String>,
    risk: Option<SkillRiskSummary>,
    update: SkillUpdateState,
    installed_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Clone)]
struct ExternalSummary {
    description: String,
    content_kind: SkillContentKind,
}

#[derive(Debug)]
struct InventoryBudget {
    entries: u64,
    entry_limit: u64,
    returned_items: u64,
    managed_bytes: u64,
}

impl Default for InventoryBudget {
    fn default() -> Self {
        Self {
            entries: 0,
            entry_limit: MAX_INVENTORY_ENTRIES,
            returned_items: 0,
            managed_bytes: 0,
        }
    }
}

impl InventoryBudget {
    #[cfg(test)]
    fn with_entry_limit(entry_limit: u64) -> Self {
        Self {
            entry_limit,
            ..Self::default()
        }
    }

    fn add_entries(&mut self, added: u64) -> Result<(), SkillError> {
        charge_inventory_limit(
            &mut self.entries,
            added,
            "inventory_entries",
            self.entry_limit,
        )
    }

    fn push_item(
        &mut self,
        items: &mut Vec<SkillInventoryItem>,
        item: SkillInventoryItem,
    ) -> Result<(), SkillError> {
        charge_inventory_limit(
            &mut self.returned_items,
            1,
            "inventory_returned_items",
            MAX_INVENTORY_RETURNED_ITEMS,
        )?;
        items.push(item);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedIdentity {
    Central { name: String },
    Target { target_id: String, name: String },
}

pub fn list_skill_agents() -> Result<Vec<SkillAgentView>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    Ok(build_target_graph(&paths, &settings)?.agents)
}

/// Return the verified physical target and current install-probe result for a
/// canonical Agent id. `None` means the Agent has no audited Skills writer.
pub fn skill_agent_capability(
    agent_id: &str,
) -> Result<Option<SkillAgentCapabilityView>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    Ok(graph
        .catalog_agents
        .get(agent_id)
        .map(|agent| capability_view(&graph, agent)))
}

/// Return every audited Skill writer from one target-graph evaluation.
pub fn list_skill_agent_capabilities() -> Result<Vec<SkillAgentCapabilityView>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    Ok(graph
        .catalog_agents
        .values()
        .map(|agent| capability_view(&graph, agent))
        .collect())
}

/// All declared and canonical physical observation roots, including aliases
/// without a primary Agent. Derived from the same verified graph as inventory.
pub(crate) fn skill_observation_paths() -> Result<Vec<PathBuf>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let graph = build_target_graph(&paths, &strict_settings()?)?;
    let mut roots = graph.targets.values().map(|target| target.canonical_root.clone()).collect::<BTreeSet<_>>();
    for agent in graph.catalog_agents.values() {
        for raw in std::iter::once(&agent.capability.global_dir)
            .chain(agent.capability.aliases.iter().map(|alias| &alias.global_dir)) {
            roots.insert(paths.expand_user(raw).ok_or_else(|| invalid_source("the Skills observation path is unavailable"))?);
        }
    }
    Ok(roots.into_iter().collect())
}

fn capability_view(graph: &TargetGraph, agent: &CatalogSkillAgent) -> SkillAgentCapabilityView {
    let target = &graph.targets[&agent.primary_target_id];
    SkillAgentCapabilityView {
        id: agent.id.clone(),
        installed: agent.installed,
        target_id: target.target_id.clone(),
        global_dir: target.global_dir.clone(),
        affected_agent_ids: target.affected_agent_ids.iter().cloned().collect(),
    }
}

pub fn normalize_agent_selection(agent_ids: &[String]) -> Result<Vec<String>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    let selected_ids = validate_agent_selection(&graph, agent_ids, true)?;
    Ok(normalize_selected_targets(&graph, &selected_ids))
}

pub(crate) fn normalize_agent_selection_with_required_target(
    agent_ids: &[String],
    required_target_id: &str,
) -> Result<Vec<String>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    let selected_ids = validate_agent_selection(&graph, agent_ids, true)?;
    let Some(required_target_id) = graph.target_aliases.get(required_target_id) else {
        return Err(invalid_source(
            "the required physical target is unavailable",
        ));
    };
    if !graph.included_target_ids.contains(required_target_id) {
        return Err(invalid_source(
            "the required physical target is unavailable",
        ));
    }
    Ok(normalize_selected_targets_with_required(
        &graph,
        &selected_ids,
        &[required_target_id.to_owned()].into_iter().collect(),
    ))
}

pub(crate) fn normalize_assignment_enable(
    agent_ids: &[String],
    existing_target_ids: &BTreeSet<String>,
) -> Result<Vec<String>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    let mut selected_ids = validate_agent_selection(&graph, agent_ids, true)?;
    let mut retained_without_live_owner = BTreeSet::new();

    for declared_target_id in existing_target_ids {
        let Some(target_id) = graph.target_aliases.get(declared_target_id) else {
            return Err(invalid_source(
                "Skills settings reference an unknown physical target",
            ));
        };
        let mut represented = false;
        for agent in graph
            .catalog_agents
            .values()
            .filter(|agent| agent.installed)
        {
            if agent_declares_target(agent, target_id) {
                selected_ids.insert(agent.id.clone());
                represented = true;
            }
        }
        if !represented {
            retained_without_live_owner.insert(target_id.clone());
        }
    }

    let mut retained = normalize_selected_targets(&graph, &selected_ids);
    retained.extend(retained_without_live_owner);
    Ok(retained.into_iter().collect())
}

pub(crate) fn declared_targets_for_agents(
    agent_ids: &[String],
) -> Result<BTreeSet<String>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    let selected = validate_agent_selection(&graph, agent_ids, false)?;
    let mut targets = BTreeSet::new();
    for agent_id in selected {
        let agent = &graph.catalog_agents[&agent_id];
        targets.extend(agent.declared_target_ids.iter().cloned());
    }
    Ok(targets)
}

fn validate_agent_selection(
    graph: &TargetGraph,
    agent_ids: &[String],
    require_installed: bool,
) -> Result<BTreeSet<String>, SkillError> {
    let mut selected_ids = BTreeSet::new();

    for agent_id in agent_ids {
        if !valid_name(agent_id) {
            return Err(invalid_source("Agent selection contains an invalid id"));
        }
        if !selected_ids.insert(agent_id.clone()) {
            return Err(invalid_source("Agent selection contains a duplicate id"));
        }
        let Some(agent) = graph.catalog_agents.get(agent_id) else {
            return Err(invalid_source("Agent selection contains an unknown id"));
        };
        if require_installed && !agent.installed {
            return Err(invalid_source(
                "Agent selection requires current verified installation evidence",
            ));
        }
    }
    Ok(selected_ids)
}

fn normalize_selected_targets(graph: &TargetGraph, selected_ids: &BTreeSet<String>) -> Vec<String> {
    normalize_selected_targets_with_required(graph, selected_ids, &BTreeSet::new())
}

fn normalize_selected_targets_with_required(
    graph: &TargetGraph,
    selected_ids: &BTreeSet<String>,
    required_target_ids: &BTreeSet<String>,
) -> Vec<String> {
    let mut retained = selected_ids
        .iter()
        .map(|agent_id| graph.catalog_agents[agent_id].primary_target_id.clone())
        .collect::<BTreeSet<_>>();
    retained.extend(required_target_ids.iter().cloned());

    loop {
        let mut candidates: Vec<(usize, String)> = retained
            .iter()
            .map(|target_id| {
                let target = &graph.targets[target_id];
                let coverage = target.affected_agent_ids.intersection(selected_ids).count();
                (coverage, target_id.clone())
            })
            .collect();
        candidates.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        let removable = candidates.into_iter().find_map(|(_, target_id)| {
            if required_target_ids.contains(&target_id) {
                return None;
            }
            let selected_primary: Vec<&String> = graph.targets[&target_id]
                .primary_agent_ids
                .intersection(selected_ids)
                .collect();
            let all_observed_elsewhere = !selected_primary.is_empty()
                && selected_primary.iter().all(|agent_id| {
                    retained.iter().any(|other_id| {
                        other_id != &target_id
                            && graph.targets[other_id]
                                .affected_agent_ids
                                .contains(*agent_id)
                    })
                });
            all_observed_elsewhere.then_some(target_id)
        });

        let Some(target_id) = removable else {
            break;
        };
        retained.remove(&target_id);
    }

    retained.into_iter().collect()
}

fn agent_declares_target(agent: &CatalogSkillAgent, target_id: &str) -> bool {
    agent.declared_target_ids.contains(target_id)
}

pub fn list_inventory() -> Result<SkillsInventory, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    let mut inventory = build_inventory(&paths, &settings, &graph)?;
    inventory.recovery_error = inventory_recovery_error()?;
    Ok(inventory)
}

/// Limit expensive tree inspection to selected Agents' primary/alias roots.
/// The graph remains complete so shared-directory affected Agent IDs are kept.
pub(crate) fn list_inventory_for_agents(
    settings: &Settings,
    agent_ids: &BTreeSet<String>,
) -> Result<SkillsInventory, SkillError> {
    if agent_ids.is_empty() { return list_inventory_for_settings(settings); }
    let paths = SkillsPaths::resolve_from_env()?;
    let mut graph = build_target_graph(&paths, settings)?;
    let selected_targets = graph.catalog_agents.values()
        .filter(|agent| agent_ids.contains(&agent.id))
        .flat_map(|agent| agent.declared_target_ids.iter().cloned()).collect::<BTreeSet<_>>();
    graph.included_target_ids.retain(|target_id| selected_targets.contains(target_id));
    graph.target_views.retain(|target| selected_targets.contains(&target.target_id));
    let names = settings.skill_assignments.iter().flatten().filter_map(|(name, targets)| {
        targets.iter().any(|target| graph.target_aliases.get(target).is_some_and(|canonical| selected_targets.contains(canonical)))
            .then(|| name.clone())
    }).collect::<BTreeSet<_>>();
    let mut inventory = build_inventory_filtered(&paths, settings, &graph, Some(&names))?;
    inventory.recovery_error = inventory_recovery_error()?;
    Ok(inventory)
}

pub(crate) fn list_inventory_for_settings(
    settings: &Settings,
) -> Result<SkillsInventory, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let graph = build_target_graph(&paths, settings)?;
    build_inventory(&paths, settings, &graph)
}

pub(crate) fn skill_agent_capability_for_settings(
    settings: &Settings,
    agent_id: &str,
) -> Result<Option<SkillAgentCapabilityView>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let graph = build_target_graph(&paths, settings)?;
    Ok(graph.catalog_agents.get(agent_id).map(|agent| {
        let target = &graph.targets[&agent.primary_target_id];
        SkillAgentCapabilityView {
            id: agent.id.clone(),
            installed: agent.installed,
            target_id: target.target_id.clone(),
            global_dir: target.global_dir.clone(),
            affected_agent_ids: target.affected_agent_ids.iter().cloned().collect(),
        }
    }))
}

pub(crate) fn canonical_skill_assignments(
    settings: &Settings,
) -> Result<BTreeMap<String, BTreeSet<String>>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let graph = build_target_graph(&paths, settings)?;
    Ok(settings
        .skill_assignments
        .iter()
        .flatten()
        .map(|(name, target_ids)| {
            let canonical = target_ids
                .iter()
                .filter_map(|target_id| graph.target_aliases.get(target_id).cloned())
                .collect();
            (name.clone(), canonical)
        })
        .collect())
}

pub(crate) fn canonical_skill_target_path(global_dir: &str) -> Result<PathBuf, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let expanded = paths
        .expand_user(global_dir)
        .ok_or_else(|| invalid_source("the Skills target path is unavailable"))?;
    canonicalize_deepest(&expanded)
}

fn inventory_recovery_error() -> Result<Option<String>, SkillError> {
    match super::transaction::has_pending_recovery() {
        Ok(false) => Ok(None),
        Ok(true) | Err(_) => Ok(Some("A pending Skills operation requires recovery.".into())),
    }
}

fn build_inventory(
    paths: &SkillsPaths,
    settings: &Settings,
    graph: &TargetGraph,
) -> Result<SkillsInventory, SkillError> {
    build_inventory_filtered(paths, settings, graph, None)
}

fn build_inventory_filtered(
    paths: &SkillsPaths,
    settings: &Settings,
    graph: &TargetGraph,
    central_names: Option<&BTreeSet<String>>,
) -> Result<SkillsInventory, SkillError> {
    let mut items = Vec::new();
    let mut metadata_by_name = BTreeMap::new();
    let mut budget = InventoryBudget::default();

    let central = scan_central(
        paths,
        settings,
        graph,
        &mut budget,
        &mut items,
        &mut metadata_by_name,
        central_names,
    )?;
    scan_targets(
        paths,
        settings,
        graph,
        central.as_ref(),
        &metadata_by_name,
        &mut budget,
        &mut items,
    )?;
    items.sort_by(|left, right| left.identity.cmp(&right.identity));

    Ok(SkillsInventory {
        items,
        agents: graph.agents.clone(),
        capabilities: graph
            .catalog_agents
            .values()
            .map(|agent| capability_view(graph, agent))
            .collect(),
        targets: graph.target_views.clone(),
        recovery_error: None,
    })
}

pub fn get_skill_detail(identity: &str) -> Result<SkillDetail, SkillError> {
    let parsed = parse_identity(identity)?;
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    let inventory = build_inventory(&paths, &settings, &graph)?;
    let mut item = inventory
        .items
        .into_iter()
        .find(|item| item.identity == identity)
        .ok_or_else(|| invalid_source("the requested Skill inventory identity is unavailable"))?;

    if item.states.contains(&InventoryState::Missing)
        || item.states.contains(&InventoryState::BrokenLink)
        || item.states.contains(&InventoryState::ConflictingLink)
    {
        return Ok(SkillDetail {
            item,
            files: Vec::new(),
            skill_md: String::new(),
            skill_md_truncated: false,
        });
    }

    let content_root = detail_content_root(&parsed, &paths, &graph)?;
    let files = inspect_tree_anchored(&content_root).map_err(sanitize_detail_error)?;
    item.content_kind = classify_content(&files);
    let (skill_md, skill_md_truncated) =
        read_skill_md_bounded(&content_root, MAX_SKILL_MD_DETAIL_BYTES)?;

    Ok(SkillDetail {
        item,
        files,
        skill_md,
        skill_md_truncated,
    })
}

/// Build the heavier read-only projection used by historical migration. The
/// regular inventory intentionally avoids walking every external tree; this
/// background scan validates and hashes only user-level Agent directories so
/// exact copies can be grouped without weakening normal list performance.
pub fn list_migration_candidates() -> Result<Vec<SkillInventoryItem>, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = strict_settings()?;
    let graph = build_target_graph(&paths, &settings)?;
    let inventory = build_inventory(&paths, &settings, &graph)?;
    let mut items = inventory.items;

    for item in items.iter_mut().filter(|item| {
        matches!(item.location, SkillLocation::AgentTarget { .. })
            && item.states.contains(&InventoryState::External)
    }) {
        if !item.states.contains(&InventoryState::Missing)
            && !item.states.contains(&InventoryState::BrokenLink)
            && !item.states.contains(&InventoryState::ConflictingLink)
        {
            if let Ok(parsed) = parse_identity(&item.identity) {
                if let Ok(content_root) = detail_content_root(&parsed, &paths, &graph) {
                    if let Ok(validated) = validate_candidate_anchored_private(&content_root) {
                        item.content_hash = Some(validated.content_hash);
                        item.content_kind = classify_content(&validated.files);
                        // Planning repeats the bounded validation and audit. A
                        // scan-time audit failure leaves the row blocked rather
                        // than hiding it or silently approving risk.
                        item.risk = audit_skill(content_root.canonical_path()).ok();
                    }
                }
            }
        }
    }
    items.sort_by(|left, right| left.identity.cmp(&right.identity));
    Ok(items)
}

fn scan_central(
    paths: &SkillsPaths,
    settings: &Settings,
    graph: &TargetGraph,
    budget: &mut InventoryBudget,
    items: &mut Vec<SkillInventoryItem>,
    metadata_by_name: &mut BTreeMap<String, ItemMetadata>,
    selected_names: Option<&BTreeSet<String>>,
) -> Result<Option<AnchoredRoot>, SkillError> {
    let records = settings.managed_skills.as_ref();
    let central = open_optional_inventory_root(
        &paths.skills_dir(),
        "the central Skills inventory could not be opened safely",
    )?;
    let mut entries = if let Some(root) = central.as_ref() {
        inventory_names(
            root,
            budget,
            "the central Skills inventory could not be read safely",
        )?
    } else {
        BTreeMap::new()
    };

    if let Some(names) = selected_names { entries.retain(|name, _| names.contains(name)); }

    for (name, record) in records.into_iter().flatten() {
        if selected_names.is_some_and(|names| !names.contains(name)) { continue; }
        let (states, observed) = match (central.as_ref(), entries.remove(name)) {
            (Some(root), Some(entry_name)) => {
                classify_managed_central(root, &entry_name, name, record, budget)?
            }
            _ => (BTreeSet::from([InventoryState::Missing]), None),
        };
        let mut metadata = metadata_from_record(record);
        // Never offer the installed fingerprint as evidence for bytes that
        // failed validation. Adoption must bind the actual observed candidate.
        metadata.content_hash = observed.as_ref().map(|candidate| candidate.content_hash.clone());
        metadata.locally_modified = states.contains(&InventoryState::LocallyModified);
        if metadata.locally_modified { metadata.risk = None; }
        if let Some(observed) = observed {
            metadata.description = observed.manifest.description;
            metadata.content_kind = classify_content(&observed.files);
        }
        let item = make_item(
            name,
            SkillLocation::Central,
            states,
            metadata.clone(),
            assigned_target_ids(settings, graph, name),
            affected_for_assignments(settings, graph, name),
        );
        metadata_by_name.insert(name.clone(), metadata);
        budget.push_item(items, item)?;
    }

    for (name, entry_name) in entries {
        let mut states = BTreeSet::from([InventoryState::External]);
        let root = central
            .as_ref()
            .expect("directory entries require an open central root");
        let path = root.canonical_path().join(&name);
        let identity = root
            .stat_entry(
                &root.root_directory().map_err(|error| {
                    sanitize_inventory_error(
                        error,
                        "the central Skills inventory could not be read safely",
                    )
                })?,
                &entry_name,
                &path,
            )
            .map_err(|error| {
                sanitize_inventory_error(
                    error,
                    "a central Skill entry could not be inspected safely",
                )
            })?;
        let summary = match identity.kind {
            AnchoredFileKind::Directory => {
                let child = open_child_directory(root, &entry_name, &identity, &path)?;
                try_external_summary(&child, &name)?
            }
            AnchoredFileKind::Symlink => {
                states = classify_link(root, &entry_name, &identity, &path, None)?;
                None
            }
            _ => None,
        };
        let metadata = metadata_from_external(summary);
        let item = make_item(
            &name,
            SkillLocation::Central,
            states,
            metadata.clone(),
            assigned_target_ids(settings, graph, &name),
            affected_for_assignments(settings, graph, &name),
        );
        metadata_by_name.insert(name, metadata);
        budget.push_item(items, item)?;
    }
    Ok(central)
}

fn scan_targets(
    paths: &SkillsPaths,
    settings: &Settings,
    graph: &TargetGraph,
    central: Option<&AnchoredRoot>,
    metadata_by_name: &BTreeMap<String, ItemMetadata>,
    budget: &mut InventoryBudget,
    items: &mut Vec<SkillInventoryItem>,
) -> Result<(), SkillError> {
    let records = settings.managed_skills.as_ref();
    let assignments = settings.skill_assignments.as_ref();

    for target_id in &graph.included_target_ids {
        let target = &graph.targets[target_id];
        let current_root = paths
            .expand_user(&target.global_dir)
            .ok_or_else(|| invalid_source("a verified target path is no longer valid"))?;
        let location = SkillLocation::AgentTarget {
            target_id: target_id.clone(),
            global_dir: target.global_dir.clone(),
        };
        let affected: Vec<String> = target.affected_agent_ids.iter().cloned().collect();
        let mut seen = BTreeSet::new();
        let target_root = open_verified_target_root(&current_root, target)?;

        if let Some(root) = target_root.as_ref() {
            for (name, entry_name) in inventory_names(
                root,
                budget,
                "a verified Agent Skills target could not be read safely",
            )? {
                seen.insert(name.clone());
                let (mut states, external_summary) =
                    classify_target_entry(paths, central, root, &name, &entry_name)?;
                let readable = states.contains(&InventoryState::Assigned)
                    || external_summary.is_some();
                let metadata = if states.contains(&InventoryState::Assigned) {
                    metadata_by_name.get(&name).cloned().unwrap_or_else(|| {
                        let mut metadata = records.and_then(|records| records.get(&name))
                            .map(metadata_from_record).unwrap_or_else(|| metadata_from_external(None));
                        // A scoped query may not hash unassigned central names.
                        metadata.content_hash = None;
                        metadata
                    })
                } else {
                    // A same-name external directory or foreign link is not a
                    // projection of the central record and has no such source.
                    metadata_from_external(external_summary)
                };
                if states.contains(&InventoryState::Assigned) && metadata.locally_modified {
                    states.insert(InventoryState::LocallyModified);
                }
                let mut item = make_item(
                    &name,
                    location.clone(),
                    states,
                    metadata,
                    assigned_target_ids(settings, graph, &name),
                    affected.clone(),
                );
                if readable {
                    item.consumer_agent_ids = graph.agents.iter()
                        .filter(|agent| affected.contains(&agent.id))
                        .map(|agent| agent.id.clone())
                        .collect();
                }
                budget.push_item(items, item)?;
            }
        }

        for (name, _) in assignments.into_iter().flatten() {
            if assigned_target_ids(settings, graph, name).contains(target_id)
                && !seen.contains(name)
            {
                let metadata = records
                    .and_then(|records| records.get(name))
                    .map(metadata_from_record)
                    .or_else(|| metadata_by_name.get(name).cloned())
                    .unwrap_or_else(|| metadata_from_external(None));
                let item = make_item(
                    name,
                    location.clone(),
                    BTreeSet::from([InventoryState::Missing]),
                    metadata,
                    assigned_target_ids(settings, graph, name),
                    affected.clone(),
                );
                budget.push_item(items, item)?;
            }
        }
    }
    Ok(())
}

fn classify_target_entry(
    paths: &SkillsPaths,
    central: Option<&AnchoredRoot>,
    target: &AnchoredRoot,
    name: &str,
    entry_name: &std::ffi::CStr,
) -> Result<(BTreeSet<InventoryState>, Option<ExternalSummary>), SkillError> {
    let directory = target.root_directory().map_err(|error| {
        sanitize_inventory_error(
            error,
            "a verified Agent Skills target could not be read safely",
        )
    })?;
    let entry_path = target.canonical_path().join(name);
    let identity = target
        .stat_entry(&directory, entry_name, &entry_path)
        .map_err(|error| {
            sanitize_inventory_error(
                error,
                "an Agent Skills target entry could not be inspected safely",
            )
        })?;
    match identity.kind {
        AnchoredFileKind::Symlink => Ok((
            classify_link(
                target,
                entry_name,
                &identity,
                &entry_path,
                Some((paths, central, name)),
            )?,
            None,
        )),
        AnchoredFileKind::Directory => {
            let child = open_child_directory(target, entry_name, &identity, &entry_path)?;
            Ok((
                BTreeSet::from([InventoryState::External]),
                try_external_summary(&child, name)?,
            ))
        }
        _ => Ok((BTreeSet::from([InventoryState::External]), None)),
    }
}

fn classify_managed_central(
    root: &AnchoredRoot,
    entry_name: &std::ffi::CStr,
    name: &str,
    record: &ManagedSkillRecord,
    budget: &mut InventoryBudget,
) -> Result<(BTreeSet<InventoryState>, Option<ValidatedSkill>), SkillError> {
    let directory = root.root_directory().map_err(|error| {
        sanitize_inventory_error(
            error,
            "the central Skills inventory could not be read safely",
        )
    })?;
    let entry_path = root.canonical_path().join(name);
    let identity = root
        .stat_entry(&directory, entry_name, &entry_path)
        .map_err(|error| {
            sanitize_inventory_error(error, "a managed Skill could not be inspected safely")
        })?;
    match identity.kind {
        AnchoredFileKind::Directory => {
            let child = open_child_directory(root, entry_name, &identity, &entry_path)?;
            match validate_managed_candidate(&child, budget) {
                Ok(validated) if validated.manifest.name == name => {
                    let mut states = BTreeSet::from([InventoryState::Managed]);
                    if validated.content_hash != record.content_hash {
                        states.insert(InventoryState::LocallyModified);
                    }
                    Ok((states, Some(validated)))
                }
                Ok(_)
                | Err(SkillError::InvalidManifest { .. })
                | Err(SkillError::UnsafePath { .. }) => {
                    Ok((BTreeSet::from([InventoryState::LocallyModified]), None))
                }
                Err(error @ SkillError::LimitExceeded { .. }) => Err(error),
                Err(error) => Err(sanitize_inventory_error(
                    error,
                    "a managed Skill changed while it was being validated",
                )),
            }
        }
        AnchoredFileKind::Symlink => classify_link(root, entry_name, &identity, &entry_path, None)
            .map(|states| (states, None)),
        _ => Ok((BTreeSet::from([InventoryState::LocallyModified]), None)),
    }
}

fn validate_managed_candidate(
    root: &AnchoredRoot,
    budget: &mut InventoryBudget,
) -> Result<ValidatedSkill, SkillError> {
    validate_candidate_anchored(
        root,
        &mut budget.managed_bytes,
        MAX_INVENTORY_MANAGED_BYTES,
        "inventory_managed_content",
        &mut budget.entries,
        budget.entry_limit,
        "inventory_entries",
    )
}

fn open_optional_inventory_root(
    path: &Path,
    message: &'static str,
) -> Result<Option<AnchoredRoot>, SkillError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(SkillError::Io {
            message: message.into(),
            path: None,
        }),
        Ok(metadata) if !metadata.file_type().is_dir() => Err(SkillError::Conflict {
            message: "an inventory root is no longer a directory".into(),
            path: String::new(),
        }),
        Ok(_) => {
            let expected = AnchoredRoot::inspect_directory(path)
                .map_err(|error| sanitize_inventory_error(error, message))?;
            AnchoredRoot::open_expected(path, &expected)
                .map(Some)
                .map_err(|error| sanitize_inventory_error(error, message))
        }
    }
}

fn open_verified_target_root(
    path: &Path,
    target: &PhysicalTarget,
) -> Result<Option<AnchoredRoot>, SkillError> {
    open_verified_target_root_after(path, target, || {})
}

fn open_verified_target_root_after<F>(
    declared_path: &Path,
    target: &PhysicalTarget,
    after_declared_check: F,
) -> Result<Option<AnchoredRoot>, SkillError>
where
    F: FnOnce(),
{
    let Some(expected) = target.observed_identity.as_ref() else {
        match fs::symlink_metadata(declared_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(SkillError::Io {
                    message: "a verified Agent Skills target could not be inspected safely".into(),
                    path: None,
                });
            }
            Ok(_) => return Err(target_root_conflict("appeared during inventory")),
        }
        after_declared_check();
        return match fs::symlink_metadata(declared_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(SkillError::Io {
                message: "a verified Agent Skills target could not be inspected safely".into(),
                path: None,
            }),
            Ok(_) => Err(target_root_conflict("appeared during inventory")),
        };
    };

    verify_declared_target_destination(declared_path, &target.canonical_root)?;
    after_declared_check();
    match fs::symlink_metadata(&target.canonical_root) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(target_root_conflict("is no longer a physical directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(target_root_conflict("disappeared during inventory"));
        }
        Err(_) => {
            return Err(SkillError::Io {
                message: "a verified Agent Skills target could not be inspected safely".into(),
                path: None,
            });
        }
    }
    let opened = AnchoredRoot::open_expected(&target.canonical_root, expected).map_err(
        |error| match error {
            SkillError::UnsafePath { .. } | SkillError::Conflict { .. } => {
                target_root_conflict("changed physical identity")
            }
            other => sanitize_inventory_error(
                other,
                "a verified Agent Skills target could not be opened safely",
            ),
        },
    )?;
    verify_declared_target_destination(declared_path, &target.canonical_root)?;
    if opened.canonical_path() != target.canonical_root {
        return Err(target_root_conflict("changed physical identity"));
    }
    Ok(Some(opened))
}

fn verify_declared_target_destination(
    declared_path: &Path,
    expected: &Path,
) -> Result<(), SkillError> {
    match canonicalize_deepest(declared_path) {
        Ok(actual) if actual == expected => Ok(()),
        _ => Err(target_root_conflict("changed physical location")),
    }
}

fn target_root_conflict(reason: &str) -> SkillError {
    SkillError::Conflict {
        message: format!("a verified Agent Skills target {reason}"),
        path: String::new(),
    }
}

fn inventory_names(
    root: &AnchoredRoot,
    budget: &mut InventoryBudget,
    message: &'static str,
) -> Result<BTreeMap<String, CString>, SkillError> {
    let directory = root
        .root_directory()
        .map_err(|error| sanitize_inventory_error(error, message))?;
    let names = root
        .read_directory_budgeted(
            &directory,
            root.canonical_path(),
            budget.entries,
            budget.entry_limit,
            "inventory_entries",
        )
        .map_err(|error| sanitize_inventory_error(error, message))?;
    budget.add_entries(names.len() as u64)?;
    Ok(names
        .into_iter()
        .filter_map(|name| {
            let text = std::str::from_utf8(name.as_bytes()).ok()?.to_string();
            valid_name(&text).then_some((text, name))
        })
        .collect())
}

fn open_child_directory(
    root: &AnchoredRoot,
    name: &std::ffi::CStr,
    identity: &AnchoredIdentity,
    path: &Path,
) -> Result<AnchoredRoot, SkillError> {
    let directory = root.root_directory().map_err(|error| {
        sanitize_inventory_error(error, "an inventory root could not be read safely")
    })?;
    let child = root
        .open_directory_entry(&directory, name, identity, path)
        .map_err(|error| {
            sanitize_inventory_error(error, "a Skill directory changed during inventory")
        })?;
    AnchoredRoot::from_open_directory(child, path.to_path_buf(), identity).map_err(|error| {
        sanitize_inventory_error(error, "a Skill directory changed during inventory")
    })
}

fn classify_link(
    root: &AnchoredRoot,
    name: &std::ffi::CStr,
    identity: &AnchoredIdentity,
    entry_path: &Path,
    expected_central: Option<(&SkillsPaths, Option<&AnchoredRoot>, &str)>,
) -> Result<BTreeSet<InventoryState>, SkillError> {
    let directory = root.root_directory().map_err(|error| {
        sanitize_inventory_error(error, "a Skill link could not be read safely")
    })?;
    let raw_target = root
        .read_link_entry(&directory, name, identity, entry_path)
        .map_err(|error| {
            sanitize_inventory_error(error, "a Skill link could not be read safely")
        })?;
    let Ok(raw_target) = std::str::from_utf8(&raw_target) else {
        return Ok(BTreeSet::from([InventoryState::ConflictingLink]));
    };
    let Some(resolved) = resolve_link_path(entry_path, raw_target) else {
        return Ok(BTreeSet::from([InventoryState::BrokenLink]));
    };
    if resolved == entry_path {
        return Ok(BTreeSet::from([InventoryState::BrokenLink]));
    }

    if let Some((paths, central, skill_name)) = expected_central {
        let central_root = match central {
            Some(root) => root.canonical_path().to_path_buf(),
            None => canonicalize_deepest(&paths.skills_dir())?,
        };
        let expected = central_root.join(skill_name);
        if physical_entry_path(&resolved)? == expected {
            let Some(central) = central else {
                return Ok(BTreeSet::from([InventoryState::BrokenLink]));
            };
            return match open_existing_named_directory(central, skill_name)? {
                Some(_) => Ok(BTreeSet::from([InventoryState::Assigned])),
                None => Ok(BTreeSet::from([InventoryState::BrokenLink])),
            };
        }
    }

    Ok(BTreeSet::from([match inspect_link_chain(&resolved)? {
        LinkDestination::Broken => InventoryState::BrokenLink,
        LinkDestination::Existing => InventoryState::ConflictingLink,
    }]))
}

fn physical_entry_path(path: &Path) -> Result<PathBuf, SkillError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_source("a Skill link destination has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid_source("a Skill link destination has no name"))?;
    Ok(canonicalize_deepest(parent)?.join(name))
}

enum LinkDestination {
    Broken,
    Existing,
}

fn inspect_link_chain(path: &Path) -> Result<LinkDestination, SkillError> {
    let mut current = path.to_path_buf();
    let mut seen = BTreeSet::new();
    for _ in 0..=40 {
        if !seen.insert(current.clone()) {
            return Ok(LinkDestination::Broken);
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LinkDestination::Broken);
            }
            Err(error) if is_symlink_loop_error(&error) => {
                return Ok(LinkDestination::Broken);
            }
            Err(_) => {
                return Err(SkillError::Io {
                    message: "a Skill link destination could not be inspected safely".into(),
                    path: None,
                });
            }
        };
        if !metadata.file_type().is_symlink() {
            return Ok(LinkDestination::Existing);
        }
        let target = fs::read_link(&current).map_err(|_| SkillError::Io {
            message: "a Skill link destination could not be inspected safely".into(),
            path: None,
        })?;
        let Some(next) = resolve_link_path(&current, target.as_os_str()) else {
            return Ok(LinkDestination::Broken);
        };
        current = next;
    }
    Ok(LinkDestination::Broken)
}

#[cfg(unix)]
fn is_symlink_loop_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error())
}

#[cfg(not(unix))]
fn is_symlink_loop_error(_error: &std::io::Error) -> bool {
    false
}

fn resolve_link_path(entry_path: &Path, target: impl AsRef<OsStr>) -> Option<PathBuf> {
    let target = Path::new(target.as_ref());
    let joined = if target.is_absolute() {
        target.to_path_buf()
    } else {
        entry_path.parent()?.join(target)
    };
    lexical_absolute(&joined).ok()
}

fn open_existing_named_directory(
    root: &AnchoredRoot,
    name: &str,
) -> Result<Option<AnchoredRoot>, SkillError> {
    let path = root.canonical_path().join(name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(SkillError::Io {
                message: "the central Skill could not be inspected safely".into(),
                path: None,
            });
        }
        Ok(metadata) if !metadata.file_type().is_dir() => return Ok(None),
        Ok(_) => {}
    }
    let entry_name = CString::new(name).expect("validated Skill names contain no NUL");
    let directory = root.root_directory().map_err(|error| {
        sanitize_inventory_error(error, "the central Skill could not be read safely")
    })?;
    let identity = root
        .stat_entry(&directory, &entry_name, &path)
        .map_err(|error| {
            sanitize_inventory_error(error, "the central Skill changed during inventory")
        })?;
    open_child_directory(root, &entry_name, &identity, &path).map(Some)
}

fn make_item(
    name: &str,
    location: SkillLocation,
    mut states: BTreeSet<InventoryState>,
    metadata: ItemMetadata,
    assigned_target_ids: Vec<String>,
    affected_agent_ids: Vec<String>,
) -> SkillInventoryItem {
    if metadata.update.available {
        states.insert(InventoryState::UpdateAvailable);
    }
    let identity = match &location {
        SkillLocation::Central => format!("central:{name}"),
        SkillLocation::AgentTarget { target_id, .. } => {
            format!("target:{target_id}:{name}")
        }
    };
    SkillInventoryItem {
        identity,
        name: name.into(),
        description: metadata.description,
        content_kind: metadata.content_kind,
        states,
        location,
        source: metadata.source,
        resolved_revision: metadata.resolved_revision,
        content_hash: metadata.content_hash,
        risk: metadata.risk,
        update: metadata.update,
        assigned_target_ids,
        affected_agent_ids,
        consumer_agent_ids: Vec::new(),
        installed_at: metadata.installed_at,
        updated_at: metadata.updated_at,
    }
}

fn metadata_from_record(record: &ManagedSkillRecord) -> ItemMetadata {
    ItemMetadata {
        locally_modified: false,
        description: record.description.clone(),
        content_kind: record.content_kind.clone(),
        source: Some(record.source.clone()),
        resolved_revision: record.resolved_revision.clone(),
        content_hash: Some(record.content_hash.clone()),
        risk: Some(record.risk.clone()),
        update: record.update.clone(),
        installed_at: Some(record.installed_at.clone()),
        updated_at: Some(record.updated_at.clone()),
    }
}

fn metadata_from_external(summary: Option<ExternalSummary>) -> ItemMetadata {
    ItemMetadata {
        locally_modified: false,
        description: summary
            .as_ref()
            .map(|summary| summary.description.clone())
            .unwrap_or_default(),
        content_kind: summary
            .map(|summary| summary.content_kind)
            .unwrap_or(SkillContentKind::Instructions),
        source: None,
        resolved_revision: None,
        content_hash: None,
        risk: None,
        update: SkillUpdateState::default(),
        installed_at: None,
        updated_at: None,
    }
}

fn assigned_target_ids(settings: &Settings, graph: &TargetGraph, name: &str) -> Vec<String> {
    settings
        .skill_assignments
        .as_ref()
        .and_then(|assignments| assignments.get(name))
        .map(|target_ids| {
            target_ids
                .iter()
                .filter_map(|target_id| graph.target_aliases.get(target_id).cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

fn affected_for_assignments(settings: &Settings, graph: &TargetGraph, name: &str) -> Vec<String> {
    let mut affected = BTreeSet::new();
    for target_id in assigned_target_ids(settings, graph, name) {
        if let Some(target) = graph.targets.get(&target_id) {
            affected.extend(target.affected_agent_ids.iter().cloned());
        }
    }
    affected.into_iter().collect()
}

fn try_external_summary(
    root: &AnchoredRoot,
    expected_name: &str,
) -> Result<Option<ExternalSummary>, SkillError> {
    let skill_md = match read_skill_md_bounded(root, MAX_SKILL_MD_DETAIL_BYTES) {
        Ok((skill_md, _)) => skill_md,
        Err(error @ SkillError::Conflict { .. })
        | Err(error @ SkillError::LimitExceeded { .. }) => {
            return Err(sanitize_inventory_error(
                error,
                "an external Skill changed while its summary was read",
            ));
        }
        Err(_) => return Ok(None),
    };
    let Ok(manifest) = parse_manifest(root.canonical_path(), &skill_md) else {
        return Ok(None);
    };
    if manifest.name != expected_name {
        return Ok(None);
    }
    Ok(Some(ExternalSummary {
        description: manifest.description,
        // List scans intentionally do not walk external trees. Detail loading
        // performs the full bounded inspection when the user selects the row.
        content_kind: SkillContentKind::Instructions,
    }))
}

fn parse_identity(identity: &str) -> Result<ParsedIdentity, SkillError> {
    if let Some(name) = identity.strip_prefix("central:") {
        if valid_name(name) {
            return Ok(ParsedIdentity::Central { name: name.into() });
        }
        return Err(invalid_source("the Skill inventory identity is invalid"));
    }
    if let Some(rest) = identity.strip_prefix("target:") {
        let Some((target_id, name)) = rest.split_once(':') else {
            return Err(invalid_source("the Skill inventory identity is invalid"));
        };
        if valid_name(target_id) && valid_name(name) {
            return Ok(ParsedIdentity::Target {
                target_id: target_id.into(),
                name: name.into(),
            });
        }
    }
    Err(invalid_source("the Skill inventory identity is invalid"))
}

fn detail_content_root(
    identity: &ParsedIdentity,
    paths: &SkillsPaths,
    graph: &TargetGraph,
) -> Result<AnchoredRoot, SkillError> {
    let central = open_optional_inventory_root(
        &paths.skills_dir(),
        "the central Skills detail root could not be opened safely",
    )?;
    match identity {
        ParsedIdentity::Central { name } => central
            .as_ref()
            .ok_or_else(|| SkillError::Io {
                message: "the central Skill detail is unavailable".into(),
                path: None,
            })
            .and_then(|root| {
                open_existing_named_directory(root, name)?.ok_or_else(|| SkillError::Io {
                    message: "the central Skill detail is unavailable".into(),
                    path: None,
                })
            }),
        ParsedIdentity::Target { target_id, name } => {
            let target = graph
                .targets
                .get(target_id)
                .ok_or_else(|| invalid_source("the Skill target identity is unknown"))?;
            let expanded = paths
                .expand_user(&target.global_dir)
                .ok_or_else(|| invalid_source("the Skill target identity is unavailable"))?;
            let target_root =
                open_verified_target_root(&expanded, target)?.ok_or_else(|| SkillError::Io {
                    message: "the Skill target detail is unavailable".into(),
                    path: None,
                })?;
            let directory = target_root.root_directory().map_err(|error| {
                sanitize_inventory_error(error, "the Skill target detail is unavailable")
            })?;
            let entry_name = CString::new(name.as_str())
                .expect("validated Skill names do not contain NUL bytes");
            let entry = target_root.canonical_path().join(name);
            let identity = target_root
                .stat_entry(&directory, &entry_name, &entry)
                .map_err(|error| {
                    sanitize_inventory_error(error, "the Skill target detail is unavailable")
                })?;
            if identity.kind == AnchoredFileKind::Symlink {
                let states = classify_link(
                    &target_root,
                    &entry_name,
                    &identity,
                    &entry,
                    Some((paths, central.as_ref(), name)),
                )?;
                if !states.contains(&InventoryState::Assigned) {
                    return Err(invalid_source(
                        "the Skill target link is not managed by Arca",
                    ));
                }
                return central
                    .as_ref()
                    .and_then(|root| open_existing_named_directory(root, name).transpose())
                    .transpose()?
                    .ok_or_else(|| SkillError::Io {
                        message: "the central Skill detail is unavailable".into(),
                        path: None,
                    });
            }
            if identity.kind != AnchoredFileKind::Directory {
                return Err(SkillError::Conflict {
                    message: "the Skill target detail is no longer a directory".into(),
                    path: String::new(),
                });
            }
            open_child_directory(&target_root, &entry_name, &identity, &entry).map_err(|error| {
                sanitize_inventory_error(error, "the Skill target detail is unavailable")
            })
        }
    }
}

fn sanitize_detail_error(error: SkillError) -> SkillError {
    match error {
        SkillError::LimitExceeded { .. }
        | SkillError::InvalidSource { .. }
        | SkillError::Network { .. }
        | SkillError::PlanStale { .. }
        | SkillError::ConfirmationRequired { .. }
        | SkillError::RecoveryRequired { .. } => error,
        SkillError::Conflict { message, .. } => SkillError::Conflict {
            message,
            path: String::new(),
        },
        _ => SkillError::Io {
            message: "the Skill tree could not be inspected safely".into(),
            path: None,
        },
    }
}

#[cfg(unix)]
fn read_skill_md_bounded(
    root: &AnchoredRoot,
    maximum: usize,
) -> Result<(String, bool), SkillError> {
    let directory = root.root_directory().map_err(|error| {
        sanitize_inventory_error(error, "the Skill detail root could not be read safely")
    })?;
    let name = CString::new("SKILL.md").expect("static filename contains no NUL");
    let path = root.canonical_path().join("SKILL.md");
    let identity = root
        .stat_entry(&directory, &name, &path)
        .map_err(|_| SkillError::Io {
            message: "SKILL.md could not be inspected safely".into(),
            path: None,
        })?;
    if identity.kind != AnchoredFileKind::Regular || identity.links != 1 {
        return Err(invalid_source(
            "SKILL.md must be a regular private tree file",
        ));
    }
    let mut file = root
        .open_regular_entry(&directory, &name, &identity, &path)
        .map_err(|error| sanitize_inventory_error(error, "SKILL.md could not be opened safely"))?;
    let requested = identity.size.min(maximum as u64);
    let mut bytes = Vec::with_capacity(requested as usize);
    (&mut file)
        .take(requested)
        .read_to_end(&mut bytes)
        .map_err(|_| SkillError::Io {
            message: "SKILL.md could not be read safely".into(),
            path: None,
        })?;
    root.verify_regular_file(&file, &identity, &path)
        .map_err(|error| {
            sanitize_inventory_error(error, "SKILL.md changed while it was being read")
        })?;
    if bytes.len() as u64 != requested {
        return Err(SkillError::Conflict {
            message: "SKILL.md changed while it was being read".into(),
            path: String::new(),
        });
    }
    decode_skill_md(bytes, identity.size > maximum as u64)
}

#[cfg(not(unix))]
fn read_skill_md_bounded(
    _root: &AnchoredRoot,
    _maximum: usize,
) -> Result<(String, bool), SkillError> {
    Err(super::anchored::unsupported_platform())
}

fn decode_skill_md(bytes: Vec<u8>, truncated: bool) -> Result<(String, bool), SkillError> {
    match String::from_utf8(bytes) {
        Ok(text) => Ok((text, truncated)),
        Err(error) if truncated && error.utf8_error().error_len().is_none() => {
            let boundary = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(boundary);
            Ok((
                String::from_utf8(bytes).expect("valid_up_to is a UTF-8 boundary"),
                true,
            ))
        }
        Err(_) => Err(SkillError::InvalidManifest {
            message: "SKILL.md must be valid UTF-8".into(),
            path: String::new(),
        }),
    }
}

fn strict_settings() -> Result<Settings, SkillError> {
    load_settings_strict().map_err(|_| SkillError::Io {
        message: "Arca settings could not be read safely".into(),
        path: None,
    })
}

fn charge_inventory_limit(
    current: &mut u64,
    added: u64,
    limit: &'static str,
    allowed: u64,
) -> Result<(), SkillError> {
    let actual = current.saturating_add(added);
    if actual > allowed {
        return Err(SkillError::LimitExceeded {
            limit: limit.into(),
            actual,
            allowed,
        });
    }
    *current = actual;
    Ok(())
}

fn sanitize_inventory_error(error: SkillError, message: &'static str) -> SkillError {
    match error {
        SkillError::LimitExceeded { .. }
        | SkillError::InvalidSource { .. }
        | SkillError::Network { .. }
        | SkillError::PlanStale { .. }
        | SkillError::ConfirmationRequired { .. }
        | SkillError::RecoveryRequired { .. } => error,
        SkillError::Conflict { .. } => SkillError::Conflict {
            message: message.into(),
            path: String::new(),
        },
        _ => SkillError::Io {
            message: message.into(),
            path: None,
        },
    }
}

fn build_target_graph(paths: &SkillsPaths, settings: &Settings) -> Result<TargetGraph, SkillError> {
    validate_skill_settings(settings)?;
    let mut catalog_agents = BTreeMap::new();
    let mut targets = BTreeMap::<String, PhysicalTarget>::new();
    let mut targets_by_path = BTreeMap::<PathBuf, String>::new();
    let mut target_aliases = BTreeMap::<String, String>::new();

    for (id, definition) in load_agents_from_settings(settings) {
        let Some(capability) = definition.skills else {
            continue;
        };
        if !valid_name(&id)
            || !valid_name(&capability.target_id)
            || !crate::agents::is_verified_skill_evidence(&capability.evidence)
            || capability.docs.trim().is_empty()
            || capability.verified_at.trim().is_empty()
            || capability.probes.is_empty()
        {
            return Err(invalid_source(
                "the verified Agent Skills catalog is inconsistent",
            ));
        }
        let installed = capability
            .probes
            .iter()
            .any(|probe| probe_installed(probe, paths));
        let primary_target_id = register_target(
            paths,
            &mut targets,
            &mut targets_by_path,
            &mut target_aliases,
            &capability.target_id,
            &capability.global_dir,
            Some(&id),
        )?;
        let mut declared_target_ids = BTreeSet::from([primary_target_id.clone()]);
        for alias in &capability.aliases {
            declared_target_ids.insert(register_target(
                paths,
                &mut targets,
                &mut targets_by_path,
                &mut target_aliases,
                &alias.target_id,
                &alias.global_dir,
                None,
            )?);
        }
        let agent = CatalogSkillAgent {
            id: id.clone(),
            name: definition.name.unwrap_or_else(|| id.clone()),
            capability: capability.clone(),
            primary_target_id,
            declared_target_ids,
            installed,
        };
        catalog_agents.insert(id, agent);
    }

    for agent in catalog_agents.values().filter(|agent| agent.installed) {
        for target_id in &agent.declared_target_ids {
            targets
                .get_mut(target_id)
                .expect("verified target was registered")
                .affected_agent_ids
                .insert(agent.id.clone());
        }
    }

    let mut included_target_ids = BTreeSet::new();
    for agent in catalog_agents.values().filter(|agent| agent.installed) {
        included_target_ids.extend(agent.declared_target_ids.iter().cloned());
    }
    for target_id in settings
        .skill_assignments
        .iter()
        .flat_map(|assignments| assignments.values())
        .flatten()
    {
        let Some(canonical_target_id) = target_aliases.get(target_id) else {
            return Err(invalid_source(
                "Skills settings reference an unknown physical target",
            ));
        };
        included_target_ids.insert(canonical_target_id.clone());
    }

    let agents = catalog_agents
        .values()
        .filter(|agent| agent.installed)
        .map(|agent| {
            let capability = &agent.capability;
            let target = &targets[&agent.primary_target_id];
            SkillAgentView {
                id: agent.id.clone(),
                name: agent.name.clone(),
                target_id: target.target_id.clone(),
                global_dir: target.global_dir.clone(),
                affected_agent_ids: target.affected_agent_ids.iter().cloned().collect(),
                docs: capability.docs.clone(),
                evidence: capability.evidence.clone(),
                verified_at: capability.verified_at.clone(),
            }
        })
        .collect();
    let target_views = included_target_ids
        .iter()
        .map(|target_id| {
            let target = &targets[target_id];
            SkillTargetView {
                target_id: target.target_id.clone(),
                global_dir: target.global_dir.clone(),
                primary_agent_ids: target.primary_agent_ids.iter().cloned().collect(),
                affected_agent_ids: target.affected_agent_ids.iter().cloned().collect(),
                assignable: target.primary_agent_ids.iter().any(|agent_id| {
                    catalog_agents
                        .get(agent_id)
                        .is_some_and(|agent| agent.installed)
                }),
            }
        })
        .collect();

    Ok(TargetGraph {
        catalog_agents,
        targets,
        target_aliases,
        included_target_ids,
        agents,
        target_views,
    })
}

fn validate_skill_settings(settings: &Settings) -> Result<(), SkillError> {
    let managed_records = settings
        .managed_skills
        .as_ref()
        .map_or(0, |records| records.len() as u64);
    let assignment_records = settings
        .skill_assignments
        .as_ref()
        .map_or(0, |assignments| {
            assignments.values().fold(0_u64, |total, target_ids| {
                total.saturating_add((target_ids.len() as u64).max(1))
            })
        });
    enforce_inventory_limit(
        "inventory_settings_records",
        managed_records.saturating_add(assignment_records),
        MAX_INVENTORY_SETTINGS_RECORDS,
    )?;
    if let Some(records) = &settings.managed_skills {
        for (name, record) in records {
            if !valid_name(name) || record.name != *name {
                return Err(invalid_source(
                    "managed Skills settings contain an invalid identity",
                ));
            }
        }
    }
    if let Some(assignments) = &settings.skill_assignments {
        for (name, target_ids) in assignments {
            if !valid_name(name) || target_ids.iter().any(|target_id| !valid_name(target_id)) {
                return Err(invalid_source(
                    "Skills assignment settings contain an invalid identity",
                ));
            }
        }
    }
    Ok(())
}

fn enforce_inventory_limit(
    limit: &'static str,
    actual: u64,
    allowed: u64,
) -> Result<(), SkillError> {
    if actual > allowed {
        return Err(SkillError::LimitExceeded {
            limit: limit.into(),
            actual,
            allowed,
        });
    }
    Ok(())
}

fn register_target(
    paths: &SkillsPaths,
    targets: &mut BTreeMap<String, PhysicalTarget>,
    targets_by_path: &mut BTreeMap<PathBuf, String>,
    target_aliases: &mut BTreeMap<String, String>,
    target_id: &str,
    global_dir: &str,
    primary_agent_id: Option<&str>,
) -> Result<String, SkillError> {
    if !valid_name(target_id) {
        return Err(invalid_source(
            "the verified Agent Skills catalog contains an invalid target id",
        ));
    }
    let expanded = paths.expand_user(global_dir).ok_or_else(|| {
        invalid_source("the verified Agent Skills catalog contains an unsafe target")
    })?;
    if !expanded.is_absolute() {
        return Err(invalid_source(
            "the verified Agent Skills catalog contains a relative target",
        ));
    }
    let canonical_root = canonicalize_deepest(&expanded)?;
    let observed_identity = match fs::symlink_metadata(&canonical_root) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(invalid_source(
                "a verified Agent Skills target is not a directory",
            ));
        }
        Ok(_) => Some(
            AnchoredRoot::inspect_directory(&canonical_root).map_err(|_| {
                invalid_source("a verified Agent Skills target could not be inspected safely")
            })?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(invalid_source(
                "a verified Agent Skills target could not be inspected safely",
            ));
        }
    };

    if let Some(canonical_target_id) = target_aliases.get(target_id) {
        let existing = &targets[canonical_target_id];
        if existing.canonical_root != canonical_root
            || existing.observed_identity != observed_identity
        {
            return Err(invalid_source(
                "one verified target id resolves to multiple physical directories",
            ));
        }
        if let Some(agent_id) = primary_agent_id {
            targets
                .get_mut(canonical_target_id)
                .expect("canonical target alias must resolve")
                .primary_agent_ids
                .insert(agent_id.into());
        }
        return Ok(canonical_target_id.clone());
    }
    if let Some(canonical_target_id) = targets_by_path.get(&canonical_root).cloned() {
        let existing = &targets[&canonical_target_id];
        if existing.observed_identity != observed_identity {
            return Err(invalid_source(
                "a verified physical target changed while it was inspected",
            ));
        }
        target_aliases.insert(target_id.into(), canonical_target_id.clone());
        if let Some(agent_id) = primary_agent_id {
            targets
                .get_mut(&canonical_target_id)
                .expect("canonical physical target must exist")
                .primary_agent_ids
                .insert(agent_id.into());
        }
        return Ok(canonical_target_id);
    }

    let target = targets
        .entry(target_id.into())
        .or_insert_with(|| PhysicalTarget {
            target_id: target_id.into(),
            global_dir: global_dir.into(),
            canonical_root: canonical_root.clone(),
            observed_identity,
            primary_agent_ids: BTreeSet::new(),
            affected_agent_ids: BTreeSet::new(),
        });
    if let Some(agent_id) = primary_agent_id {
        target.primary_agent_ids.insert(agent_id.into());
    }
    targets_by_path.insert(canonical_root, target_id.into());
    target_aliases.insert(target_id.into(), target_id.into());
    Ok(target_id.into())
}

fn canonicalize_deepest(path: &Path) -> Result<PathBuf, SkillError> {
    let normalized = lexical_absolute(path)?;
    let mut cursor = normalized.as_path();
    let mut missing = Vec::<OsString>::new();

    loop {
        match fs::canonicalize(cursor) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if fs::symlink_metadata(cursor).is_ok() {
                    return Err(invalid_source(
                        "a verified target has a broken existing parent",
                    ));
                }
                let Some(name) = cursor.file_name() else {
                    return Err(invalid_source("a verified target has no resolvable parent"));
                };
                missing.push(name.to_os_string());
                cursor = cursor
                    .parent()
                    .ok_or_else(|| invalid_source("a verified target has no resolvable parent"))?;
            }
            Err(_) => {
                return Err(invalid_source(
                    "a verified target could not be resolved safely",
                ));
            }
        }
    }
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, SkillError> {
    if !path.is_absolute() {
        return Err(invalid_source("an inventory path must be absolute"));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str())
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(invalid_source("an inventory path escapes its root"));
                }
            }
        }
    }
    Ok(normalized)
}

fn probe_installed(probe: &AgentInstallProbe, paths: &SkillsPaths) -> bool {
    match probe {
        AgentInstallProbe::Path { path } => paths
            .expand_user(path)
            .map(|path| remap_system_probe_path(path, paths.user_home()))
            .is_some_and(|path| path.exists()),
        AgentInstallProbe::Command { name } => command_exists(name, paths.user_home()),
        AgentInstallProbe::MacBundle { bundle_id } => {
            mac_bundle_exists(bundle_id, paths.user_home())
        }
    }
}

fn command_exists(name: &str, user_home: &Path) -> bool {
    let command = Path::new(name);
    if name.is_empty()
        || command.components().count() != 1
        || !matches!(command.components().next(), Some(Component::Normal(_)))
    {
        return false;
    }

    let mut directories = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    directories.extend([
        user_home.join(".local/bin"),
        user_home.join(".cargo/bin"),
        remap_system_root(Path::new("/opt/homebrew/bin"), user_home),
        remap_system_root(Path::new("/usr/local/bin"), user_home),
    ]);

    directories
        .into_iter()
        .map(|directory| directory.join(name))
        .any(|candidate| is_executable_regular_file(&candidate))
}

fn is_executable_regular_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(target_os = "macos")]
fn mac_bundle_exists(bundle_id: &str, user_home: &Path) -> bool {
    let roots = [
        remap_system_root(Path::new("/Applications"), user_home),
        user_home.join("Applications"),
    ];
    let mut scanned = 0_u64;
    roots
        .into_iter()
        .any(|root| mac_bundle_exists_in_root(&root, bundle_id, &mut scanned).unwrap_or(false))
}

#[cfg(target_os = "macos")]
fn mac_bundle_exists_in_root(
    root_path: &Path,
    bundle_id: &str,
    scanned: &mut u64,
) -> Result<bool, SkillError> {
    let Some(root) = open_optional_inventory_root(
        root_path,
        "an approved Applications root could not be opened safely",
    )?
    else {
        return Ok(false);
    };
    let directory = root.root_directory()?;
    let names = root.read_directory_budgeted(
        &directory,
        root.canonical_path(),
        *scanned,
        MAX_INVENTORY_ENTRIES,
        "mac_bundle_entries",
    )?;
    *scanned = scanned.saturating_add(names.len() as u64);
    for name in names {
        let Some(name_text) = std::str::from_utf8(name.as_bytes()).ok() else {
            continue;
        };
        if Path::new(name_text).extension() != Some(OsStr::new("app")) {
            continue;
        }
        let app_path = root.canonical_path().join(name_text);
        let app_identity = root.stat_entry(&directory, &name, &app_path)?;
        if app_identity.kind != AnchoredFileKind::Directory {
            continue;
        }
        let app = open_child_directory(&root, &name, &app_identity, &app_path)?;
        let Some(contents) = open_probe_child_directory(&app, "Contents")? else {
            continue;
        };
        let contents_directory = contents.root_directory()?;
        let info_name = CString::new("Info.plist").expect("static filename contains no NUL");
        let info_path = contents.canonical_path().join("Info.plist");
        let info_identity = match contents.stat_entry(&contents_directory, &info_name, &info_path) {
            Ok(identity) => identity,
            Err(_) => continue,
        };
        if info_identity.kind != AnchoredFileKind::Regular
            || info_identity.links != 1
            || info_identity.size > MAX_SKILL_MD_DETAIL_BYTES as u64
        {
            continue;
        }
        let mut file = contents.open_regular_entry(
            &contents_directory,
            &info_name,
            &info_identity,
            &info_path,
        )?;
        let mut bytes = Vec::with_capacity(info_identity.size as usize);
        (&mut file)
            .take(info_identity.size)
            .read_to_end(&mut bytes)
            .map_err(|_| SkillError::Io {
                message: "an Agent bundle plist could not be read safely".into(),
                path: None,
            })?;
        contents.verify_regular_file(&file, &info_identity, &info_path)?;
        if bytes.len() as u64 != info_identity.size {
            continue;
        }
        let found = plist::Value::from_reader(std::io::Cursor::new(bytes))
            .ok()
            .and_then(|value| {
                value
                    .as_dictionary()
                    .and_then(|dict| dict.get("CFBundleIdentifier"))
                    .and_then(plist::Value::as_string)
                    .map(str::to_owned)
            });
        if found.as_deref() == Some(bundle_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
fn open_probe_child_directory(
    root: &AnchoredRoot,
    name: &str,
) -> Result<Option<AnchoredRoot>, SkillError> {
    let directory = root.root_directory()?;
    let entry_name = CString::new(name).expect("static directory name contains no NUL");
    let path = root.canonical_path().join(name);
    let identity = match root.stat_entry(&directory, &entry_name, &path) {
        Ok(identity) => identity,
        Err(_) => return Ok(None),
    };
    if identity.kind != AnchoredFileKind::Directory {
        return Ok(None);
    }
    open_child_directory(root, &entry_name, &identity, &path).map(Some)
}

#[cfg(not(target_os = "macos"))]
fn mac_bundle_exists(_bundle_id: &str, _user_home: &Path) -> bool {
    false
}

fn remap_system_probe_path(path: PathBuf, user_home: &Path) -> PathBuf {
    let applications = Path::new("/Applications");
    if let Ok(relative) = path.strip_prefix(applications) {
        return remap_system_root(applications, user_home).join(relative);
    }
    path
}

fn remap_system_root(production: &Path, user_home: &Path) -> PathBuf {
    let Some(root) = guarded_test_probe_root(user_home) else {
        return production.to_path_buf();
    };
    production
        .strip_prefix(Path::new("/"))
        .map(|relative| root.join(relative))
        .unwrap_or_else(|_| production.to_path_buf())
}

fn guarded_test_probe_root(user_home: &Path) -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os(TEST_PROBE_ROOT_ENV)?);
    let arca_home = crate::brand::env_os("ARCA_HOME").map(PathBuf::from)?;
    if !root.is_absolute() || !user_home.is_absolute() || !arca_home.is_absolute() {
        return None;
    }
    if root != user_home || arca_home != root.join(".arca") {
        return None;
    }
    if [&root, user_home, &arca_home].into_iter().any(|path| {
        fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    }) {
        return None;
    }
    if lexical_absolute(&root).ok().as_ref() != Some(&root)
        || lexical_absolute(user_home).ok().as_deref() != Some(user_home)
        || lexical_absolute(&arca_home).ok().as_ref() != Some(&arca_home)
    {
        return None;
    }
    let physical_root = canonicalize_deepest(&root).ok()?;
    let physical_home = canonicalize_deepest(user_home).ok()?;
    let physical_arca = canonicalize_deepest(&arca_home).ok()?;
    let expected_arca = canonicalize_deepest(&root.join(".arca")).ok()?;
    (physical_root == root
        && physical_home == user_home
        && physical_arca == arca_home
        && physical_root == physical_home
        && physical_arca == expected_arca)
        .then_some(root)
}

fn invalid_source(message: &str) -> SkillError {
    SkillError::InvalidSource {
        message: message.into(),
    }
}

fn valid_name(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[test]
    fn observation_roots_include_alias_only_directories_without_creating_them() {
        let home = TestHome::new("skill-alias-watch-roots");
        let mut definition = crate::agents::load_agents()["cursor"].clone();
        let capability = definition.skills.as_mut().unwrap();
        capability.target_id = "audit-primary".into();
        capability.global_dir = "~/audit-primary/skills".into();
        capability.aliases = vec![crate::domain::types::AgentSkillsDirectory {
            target_id: "audit-alias".into(), global_dir: "~/audit-alias/skills".into(),
        }];
        crate::settings::mutate_settings(|settings| {
            settings.agents.get_or_insert_default().insert("audit-agent".into(), definition);
        }).unwrap();
        let paths = skill_observation_paths().unwrap();
        assert!(paths.contains(&home.home.join("audit-primary/skills")));
        assert!(paths.contains(&home.home.join("audit-alias/skills")));
        assert!(!home.home.join("audit-alias").exists());
    }

    #[cfg(unix)]
    #[test]
    fn guarded_probe_root_remaps_only_a_matching_test_home() {
        let th = TestHome::new("skill-probe-root-guard");
        let production = PathBuf::from("/opt/homebrew/bin");
        assert_eq!(
            remap_system_root(&production, &th.home),
            th.home.join("opt/homebrew/bin")
        );

        std::env::set_var(TEST_PROBE_ROOT_ENV, th.home.join("mismatch"));
        assert_eq!(remap_system_root(&production, &th.home), production);
        assert_eq!(
            remap_system_probe_path(PathBuf::from("/Applications/Cursor.app"), &th.home),
            PathBuf::from("/Applications/Cursor.app")
        );

        std::env::set_var(TEST_PROBE_ROOT_ENV, &th.home);
        std::env::set_var("ARCA_HOME", th.home.join("other-arca"));
        assert_eq!(remap_system_root(&production, &th.home), production);

        std::env::set_var("ARCA_HOME", th.home.join(".arca"));
        let other_home = th.home.join("other-home");
        fs::create_dir(&other_home).unwrap();
        assert!(guarded_test_probe_root(&other_home).is_none());

        std::env::set_var("ARCA_HOME", "relative-arca");
        assert_eq!(remap_system_root(&production, &th.home), production);

        let real_arca = th.home.join("real-arca");
        fs::create_dir(&real_arca).unwrap();
        std::os::unix::fs::symlink(&real_arca, th.home.join(".arca")).unwrap();
        std::env::set_var(TEST_PROBE_ROOT_ENV, &th.home);
        std::env::set_var("ARCA_HOME", th.home.join(".arca"));
        assert!(guarded_test_probe_root(&th.home).is_none());

        let lexical_alias = th.home.join("nested/..");
        std::env::set_var(TEST_PROBE_ROOT_ENV, &lexical_alias);
        std::env::set_var("ARCA_HOME", lexical_alias.join(".arca"));
        assert!(guarded_test_probe_root(&lexical_alias).is_none());

        let symlink_alias = th.home.join("home-alias");
        std::os::unix::fs::symlink(&th.home, &symlink_alias).unwrap();
        std::env::set_var(TEST_PROBE_ROOT_ENV, &symlink_alias);
        std::env::set_var("ARCA_HOME", symlink_alias.join(".arca"));
        assert!(guarded_test_probe_root(&symlink_alias).is_none());

        let real_parent = th.home.join("real-parent");
        let nested_home = real_parent.join("nested-home");
        fs::create_dir_all(&nested_home).unwrap();
        let parent_alias = th.home.join("parent-alias");
        std::os::unix::fs::symlink(&real_parent, &parent_alias).unwrap();
        let parent_aliased_home = parent_alias.join("nested-home");
        std::env::set_var(TEST_PROBE_ROOT_ENV, &parent_aliased_home);
        std::env::set_var("ARCA_HOME", parent_aliased_home.join(".arca"));
        assert!(guarded_test_probe_root(&parent_aliased_home).is_none());

        std::env::set_var(TEST_PROBE_ROOT_ENV, "relative-home");
        std::env::set_var("ARCA_HOME", "relative-home/.arca");
        assert!(guarded_test_probe_root(Path::new("relative-home")).is_none());
    }

    #[test]
    fn identity_parser_accepts_only_opaque_catalog_shaped_values() {
        assert_eq!(
            parse_identity("central:safe").unwrap(),
            ParsedIdentity::Central {
                name: "safe".into()
            }
        );
        assert_eq!(
            parse_identity("target:cursor-user:safe").unwrap(),
            ParsedIdentity::Target {
                target_id: "cursor-user".into(),
                name: "safe".into()
            }
        );
        for invalid in [
            "central:../safe",
            "central:/tmp/safe",
            "target:cursor-user:/tmp/safe",
            "target:cursor-user:safe:extra",
        ] {
            assert!(parse_identity(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn inventory_budget_boundaries_are_exact_without_large_allocations() {
        assert!(enforce_inventory_limit(
            "inventory_settings_records",
            MAX_INVENTORY_SETTINGS_RECORDS,
            MAX_INVENTORY_SETTINGS_RECORDS,
        )
        .is_ok());
        assert_eq!(
            enforce_inventory_limit(
                "inventory_settings_records",
                MAX_INVENTORY_SETTINGS_RECORDS + 1,
                MAX_INVENTORY_SETTINGS_RECORDS,
            ),
            Err(SkillError::LimitExceeded {
                limit: "inventory_settings_records".into(),
                actual: MAX_INVENTORY_SETTINGS_RECORDS + 1,
                allowed: MAX_INVENTORY_SETTINGS_RECORDS,
            })
        );

        for (name, allowed) in [
            ("inventory_entries", MAX_INVENTORY_ENTRIES),
            ("inventory_returned_items", MAX_INVENTORY_RETURNED_ITEMS),
            ("inventory_managed_content", MAX_INVENTORY_MANAGED_BYTES),
            ("skill", MAX_SKILL_BYTES),
        ] {
            let mut current = allowed;
            assert!(charge_inventory_limit(&mut current, 0, name, allowed).is_ok());
            assert_eq!(current, allowed);
            assert_eq!(
                charge_inventory_limit(&mut current, 1, name, allowed),
                Err(SkillError::LimitExceeded {
                    limit: name.into(),
                    actual: allowed + 1,
                    allowed,
                })
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn managed_tree_walks_share_the_global_inventory_entry_budget() {
        fn run(extra_nested_directory: bool) -> Result<(), SkillError> {
            let th = TestHome::new(if extra_nested_directory {
                "inventory-entry-plus-one"
            } else {
                "inventory-entry-exact"
            });
            let central_path = th.home.join("central");
            for name in ["first", "second"] {
                let skill = central_path.join(name);
                fs::create_dir_all(&skill).unwrap();
                fs::write(
                    skill.join("SKILL.md"),
                    format!("---\nname: {name}\ndescription: {name}\n---\n"),
                )
                .unwrap();
            }
            if extra_nested_directory {
                fs::create_dir(central_path.join("second/empty")).unwrap();
            }

            let central = AnchoredRoot::open(&central_path).unwrap();
            let mut budget = InventoryBudget::with_entry_limit(4);
            let names = inventory_names(&central, &mut budget, "fixture inventory read")?;
            for (name, entry_name) in names {
                let directory = central.root_directory()?;
                let path = central.canonical_path().join(&name);
                let identity = central.stat_entry(&directory, &entry_name, &path)?;
                let child = open_child_directory(&central, &entry_name, &identity, &path)?;
                validate_managed_candidate(&child, &mut budget)?;
            }
            Ok(())
        }

        assert!(run(false).is_ok(), "the exact shared boundary was rejected");
        assert_eq!(
            run(true),
            Err(SkillError::LimitExceeded {
                limit: "inventory_entries".into(),
                actual: 5,
                allowed: 4,
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn anchored_known_root_open_rejects_identity_and_type_replacements() {
        let th = TestHome::new("inventory-known-root-race");
        let root = th.home.join("target");
        let original = th.home.join("target-original");
        fs::create_dir(&root).unwrap();
        let expected = AnchoredRoot::inspect_directory(&root).unwrap();
        let target = PhysicalTarget {
            target_id: "fixture-user".into(),
            global_dir: root.to_string_lossy().into_owned(),
            canonical_root: canonicalize_deepest(&root).unwrap(),
            observed_identity: Some(expected),
            primary_agent_ids: BTreeSet::new(),
            affected_agent_ids: BTreeSet::new(),
        };
        fs::rename(&root, &original).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(matches!(
            open_verified_target_root(&root, &target),
            Err(SkillError::Conflict { .. })
        ));

        fs::remove_dir(&root).unwrap();
        fs::write(&root, "replacement file").unwrap();
        assert!(matches!(
            open_verified_target_root(&root, &target),
            Err(SkillError::Conflict { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn declared_target_symlink_retarget_and_break_are_conflicts() {
        let th = TestHome::new("inventory-target-link-race");
        let first = th.home.join("first-target");
        let second = th.home.join("second-target");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let declared = th.home.join("declared-target");
        std::os::unix::fs::symlink(&first, &declared).unwrap();
        let canonical_root = canonicalize_deepest(&declared).unwrap();
        let target = PhysicalTarget {
            target_id: "fixture-user".into(),
            global_dir: declared.to_string_lossy().into_owned(),
            observed_identity: Some(AnchoredRoot::inspect_directory(&canonical_root).unwrap()),
            canonical_root,
            primary_agent_ids: BTreeSet::new(),
            affected_agent_ids: BTreeSet::new(),
        };

        let retargeted = match open_verified_target_root_after(&declared, &target, || {
            fs::remove_file(&declared).unwrap();
            std::os::unix::fs::symlink(&second, &declared).unwrap();
        }) {
            Err(error) => error,
            Ok(_) => panic!("retargeted declared target was accepted"),
        };
        assert!(matches!(retargeted, SkillError::Conflict { .. }));
        assert!(!serde_json::to_string(&retargeted)
            .unwrap()
            .contains(th.home.to_string_lossy().as_ref()));

        fs::remove_file(&declared).unwrap();
        std::os::unix::fs::symlink(&first, &declared).unwrap();
        let broken = match open_verified_target_root_after(&declared, &target, || {
            fs::remove_file(&declared).unwrap();
            std::os::unix::fs::symlink(th.home.join("missing"), &declared).unwrap();
        }) {
            Err(error) => error,
            Ok(_) => panic!("broken declared target was accepted"),
        };
        assert!(matches!(broken, SkillError::Conflict { .. }));
        assert!(!serde_json::to_string(&broken)
            .unwrap()
            .contains(th.home.to_string_lossy().as_ref()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_bundle_probe_reads_only_disposable_top_level_info_plists() {
        let th = TestHome::new("skill-bundle-probe");
        let contents = th.home.join("Applications/Fixture.app/Contents");
        fs::create_dir_all(&contents).unwrap();
        write_test_plist(&contents.join("Info.plist"), "com.example.fixture");

        assert!(mac_bundle_exists("com.example.fixture", &th.home));
        assert!(!mac_bundle_exists("com.example.missing", &th.home));

        let outside_app = th.home.join("outside/SymlinkApp.app");
        fs::create_dir_all(outside_app.join("Contents")).unwrap();
        write_test_plist(
            &outside_app.join("Contents/Info.plist"),
            "com.example.symlink-app",
        );
        std::os::unix::fs::symlink(&outside_app, th.home.join("Applications/SymlinkApp.app"))
            .unwrap();

        let outside_contents = th.home.join("outside/Contents");
        fs::create_dir_all(&outside_contents).unwrap();
        write_test_plist(
            &outside_contents.join("Info.plist"),
            "com.example.symlink-contents",
        );
        fs::create_dir_all(th.home.join("Applications/SymlinkContents.app")).unwrap();
        std::os::unix::fs::symlink(
            &outside_contents,
            th.home.join("Applications/SymlinkContents.app/Contents"),
        )
        .unwrap();

        let symlink_plist_contents = th.home.join("Applications/SymlinkPlist.app/Contents");
        fs::create_dir_all(&symlink_plist_contents).unwrap();
        let outside_plist = th.home.join("outside/Info.plist");
        write_test_plist(&outside_plist, "com.example.symlink-plist");
        std::os::unix::fs::symlink(&outside_plist, symlink_plist_contents.join("Info.plist"))
            .unwrap();

        for bundle_id in [
            "com.example.symlink-app",
            "com.example.symlink-contents",
            "com.example.symlink-plist",
        ] {
            assert!(
                !mac_bundle_exists(bundle_id, &th.home),
                "accepted {bundle_id}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    fn write_test_plist(path: &Path, bundle_id: &str) {
        let mut dictionary = plist::Dictionary::new();
        dictionary.insert(
            "CFBundleIdentifier".into(),
            plist::Value::String(bundle_id.into()),
        );
        plist::to_file_xml(path, &plist::Value::Dictionary(dictionary)).unwrap();
    }
}

/// Runtime detection excludes configuration directories that Arca itself can create.
/// An installed executable under a product `bin` directory still counts, because
/// writing that product's config does not create the binary.
pub(crate) fn detect_agent_runtime(probes: &[AgentInstallProbe]) -> Option<bool> {
    let paths = SkillsPaths::resolve_from_env().ok()?;
    let runtime: Vec<_> = probes.iter().filter(|probe| match probe {
        AgentInstallProbe::Command { .. } | AgentInstallProbe::MacBundle { .. } => true,
        AgentInstallProbe::Path { path } => path.ends_with(".app") || path.contains("/bin/"),
    }).collect();
    if runtime.is_empty() { return None; }
    Some(runtime.into_iter().any(|probe| probe_installed(probe, &paths)))
}
