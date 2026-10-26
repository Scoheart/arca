//! Agent discovery, capability, and configuration use cases.

pub use crate::domain::agents::{
    AgentCapabilitySet, AgentCapabilityView, AgentConfigurationPatch, AgentIdentityView,
    McpAgentCapabilityView, ModelAgentCapabilityView, ModelStorageAuthority,
    SkillAgentCapabilityView,
};
use crate::domain::error::{CoreError, CoreResult};
use std::collections::BTreeMap;

pub use crate::agents::AgentInfo;
pub use crate::domain::agents::{
    McpConfigurationPatch, ModelConfigurationPatch, SkillConfigurationPatch,
};
pub use crate::domain::types::{
    AgentDefinition, AgentInstallProbe, AgentSkillsCapability, AgentSkillsDirectory,
};

pub fn load_agents() -> BTreeMap<String, AgentDefinition> {
    super::gate::read(crate::agents::load_agents)
}

pub fn list_infos() -> Vec<AgentInfo> {
    super::gate::read(crate::agents::list_infos)
}

pub fn supports_transport(agent_id: &str, transport: &str) -> bool {
    super::gate::read(|| crate::agents::supports_transport(agent_id, transport))
}

pub fn put(id: String, definition: AgentDefinition, allow_overwrite: bool) -> Result<(), String> {
    super::gate::write(|| crate::agents::put(id, definition, allow_overwrite))
}

pub fn set_enabled(agent_id: &str, enabled: bool) -> Result<(), String> {
    super::gate::write(|| crate::agents::set_enabled(agent_id, enabled))
}

/// Project every Agent through one typed capability graph. Frontends no longer
/// need to join MCP-shaped Agent rows with separate Model and Skill catalogs.
pub fn list_capabilities() -> CoreResult<Vec<AgentCapabilityView>> {
    super::gate::read(|| {
        // Agent identity and MCP/Model capabilities remain useful when the
        // Skills directory is temporarily unreadable. The Skills inventory
        // has its own query and diagnostic surface; do not turn that local
        // failure into an empty global Agent picker.
        let skill_capabilities =
            crate::resources::skill::list_skill_agent_capabilities().unwrap_or_default();
        Ok(list_capabilities_with_skills(&skill_capabilities))
    })
}

pub(crate) fn list_capabilities_with_skills(
    skill_capabilities: &[crate::resources::skill::SkillAgentCapabilityView],
) -> Vec<AgentCapabilityView> {
    let definitions = crate::agents::load_agents();
    let runtime_probes: BTreeMap<_, _> = definitions.iter().filter_map(|(id, definition)| {
        definition.skills.as_ref().map(|capability| (id.clone(), capability.probes.clone()))
    }).collect();
    let infos = crate::agents::list_infos_from_definitions(definitions);
    let mut aliases = BTreeMap::new();
    let mut views = BTreeMap::new();

    for info in infos {
        let alias_dirs = info
            .skills_global_dir
            .as_ref()
            .map(|primary| {
                info.skills_global_dirs
                    .iter()
                    .filter(|path| *path != primary)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        aliases.insert(info.id.clone(), alias_dirs);

        let mcp = info.global.clone().map(|config_path| {
            let observed_config =
                crate::resources::mcp::scanner::expand_tilde(&config_path).is_file();
            (
                observed_config,
                McpAgentCapabilityView {
                    writable: true,
                    config_path: Some(config_path),
                    format: info.format.clone(),
                    key: info.key.clone(),
                    supported_transports: info
                        .supported_transports
                        .iter()
                        .map(|transport| (*transport).to_string())
                        .collect(),
                },
            )
        });
        let installed = mcp.as_ref().is_some_and(|(observed, _)| *observed);
        let instructions = super::instructions::supported(&info.id);
        views.insert(
            info.id.clone(),
            AgentCapabilityView {
                identity: AgentIdentityView {
                    id: info.id,
                    name: info.name,
                    enabled: info.enabled,
                    builtin: info.builtin,
                    category: info.category,
                    evidence: info.evidence,
                    docs: info.docs,
                    note: info.note,
                    verified_at: info.verified_at,
                },
                installed,
                runtime_detected: None,
                config_detected: false,
                capabilities: AgentCapabilitySet {
                    instructions,
                    mcp: mcp.map(|(_, capability)| capability),
                    ..AgentCapabilitySet::default()
                },
            },
        );
    }

    for model in crate::resources::model::list_agents() {
        let entry = views
            .entry(model.id.clone())
            .or_insert_with(|| AgentCapabilityView {
                identity: AgentIdentityView {
                    id: model.id.clone(),
                    name: model.name.clone(),
                    enabled: true,
                    builtin: true,
                    category: "model".into(),
                    evidence: "official".into(),
                    docs: Some(model.docs.clone()),
                    note: Some(model.note.clone()),
                    verified_at: None,
                },
                installed: false,
                runtime_detected: None,
                config_detected: false,
                capabilities: AgentCapabilitySet::default(),
            });
        entry.installed |= model.installed;
        entry.capabilities.model = Some(ModelAgentCapabilityView {
            mode: model.mode,
            storage_authority: model.storage_authority,
            installed: model.installed,
            config_paths: model.config_paths,
            assigned_profiles: model.assigned_profiles,
            active_profile: model.active_profile,
            supports_multiple: model.supports_multiple,
            supports_global_selection: model.supports_global_selection,
            credential_mode: model.credential_mode,
            supported_protocols: model.supported_protocols,
        });
    }

    for skill in skill_capabilities {
        let entry = views
            .entry(skill.id.clone())
            .or_insert_with(|| AgentCapabilityView {
                identity: AgentIdentityView {
                    id: skill.id.clone(),
                    name: skill.id.clone(),
                    enabled: true,
                    builtin: true,
                    category: "skill".into(),
                    evidence: "official".into(),
                    docs: None,
                    note: None,
                    verified_at: None,
                },
                installed: false,
                runtime_detected: None,
                config_detected: false,
                capabilities: AgentCapabilitySet::default(),
            });
        entry.installed |= skill.installed;
        entry.capabilities.skill = Some(SkillAgentCapabilityView {
            installed: skill.installed,
            target_id: skill.target_id.clone(),
            global_dir: skill.global_dir.clone(),
            alias_dirs: aliases.remove(&skill.id).unwrap_or_default(),
            affected_agent_ids: skill.affected_agent_ids.clone(),
        });
    }

    views.into_values().map(|mut view| {
        let config_exists = |path: &str| crate::resources::mcp::scanner::expand_tilde(path).exists();
        view.config_detected = view.capabilities.mcp.as_ref()
            .and_then(|mcp| mcp.config_path.as_deref()).is_some_and(config_exists)
            || view.capabilities.model.as_ref().is_some_and(|model| model.config_paths.iter().any(|path| config_exists(path)))
            || view.capabilities.skill.as_ref().is_some_and(|skill| {
                config_exists(&skill.global_dir) || skill.alias_dirs.iter().any(|path| config_exists(path))
            });
        let probes = runtime_probes.get(&view.identity.id).map(Vec::as_slice).unwrap_or_default();
        view.runtime_detected = super::agent_launch::official_runtime_detected(&view.identity.id, &view.identity.category, probes);
        view.installed = view.runtime_detected.unwrap_or(view.config_detected);
        view
    }).collect()
}

pub fn get_configuration_patch(agent_id: &str) -> CoreResult<AgentConfigurationPatch> {
    super::gate::read(|| {
        crate::agents::current_configuration_patch(agent_id).map_err(CoreError::from)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{AgentInstallProbe, AgentSkillsCapability};
    use std::collections::BTreeSet;

    #[test]
    fn runtime_installation_does_not_require_a_configuration_file() {
        let home = crate::testenv::TestHome::new("agent-runtime-without-config");
        let contents = home.home.join("Applications/OpenCode.app/Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(contents.join("Info.plist"), "fixture").unwrap();
        let views = list_capabilities().unwrap();
        let desktop = views.iter().find(|view| view.identity.id == "opencode-desktop").unwrap();
        assert_eq!(desktop.runtime_detected, Some(true));
        assert!(desktop.installed);
        assert!(!desktop.config_detected);
    }

    #[test]
    fn capability_projection_has_no_duplicate_agent_ids() {
        let views = list_capabilities().unwrap();
        let ids = views
            .iter()
            .map(|view| view.identity.id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), views.len());
        assert!(views.iter().any(|view| view.capabilities.model.is_some()));
        assert!(views.iter().any(|view| view.capabilities.skill.is_some()));
    }

    #[test]
    fn model_capability_projection_preserves_native_selection_contract() {
        let _home = crate::testenv::TestHome::new("agent-model-selection-capability");
        let views = list_capabilities().unwrap();
        let cli = views.iter().find(|view| view.identity.id == "qoder-cli").unwrap()
            .capabilities.model.as_ref().unwrap();
        let desktop = views.iter().find(|view| view.identity.id == "qoder-desktop").unwrap()
            .capabilities.model.as_ref().unwrap();
        assert!(cli.supports_multiple && cli.supports_global_selection);
        assert!(desktop.supports_multiple && !desktop.supports_global_selection);
    }

    #[test]
    fn model_capabilities_declare_one_exact_storage_authority() {
        let views = list_capabilities().unwrap();
        let authority = |agent_id: &str| {
            views
                .iter()
                .find(|view| view.identity.id == agent_id)
                .and_then(|view| view.capabilities.model.as_ref())
                .map(|model| model.storage_authority)
        };
        for agent_id in [
            "pi",
            "grok-build",
            "opencode",
            "kilo-code",
            "qwen-code",
            "crush",
            "mistral-vibe",
            "hermes",
            "factory-droid",
            "goose",
            "qoder-desktop",
            "qoder-cli",
        ] {
            assert_eq!(
                authority(agent_id),
                Some(ModelStorageAuthority::NativeRegistry)
            );
        }
        for agent_id in ["claude-code", "claude-desktop", "codex"] {
            assert_eq!(authority(agent_id), Some(ModelStorageAuthority::ArcaMapping));
        }
        for agent_id in ["minimax-code", "qoder"] {
            assert_eq!(authority(agent_id), Some(ModelStorageAuthority::Guided));
        }
    }

    #[test]
    fn custom_skill_only_agent_projects_without_an_mcp_capability() {
        let _home = crate::testenv::TestHome::new("app-custom-skill");
        put(
            "custom-skill".into(),
            AgentDefinition {
                enabled: true,
                skills: Some(AgentSkillsCapability {
                    target_id: "custom-skill-user".into(),
                    global_dir: "~/.custom-skill/skills".into(),
                    aliases: Vec::new(),
                    docs: "https://example.com/custom-skill".into(),
                    evidence: "official-source".into(),
                    verified_at: "2026-07-23".into(),
                    probes: vec![AgentInstallProbe::Path {
                        path: "/Applications/Custom Skill Missing.app".into(),
                    }],
                }),
                ..AgentDefinition::default()
            },
            false,
        )
        .unwrap();

        let views = list_capabilities().unwrap();
        let custom = views
            .iter()
            .find(|view| view.identity.id == "custom-skill")
            .unwrap();
        assert!(!custom.identity.builtin);
        assert_eq!(custom.identity.category, "custom");
        assert!(custom.capabilities.mcp.is_none());
        assert!(custom.capabilities.model.is_none());
        let skill = custom.capabilities.skill.as_ref().unwrap();
        assert_eq!(skill.target_id, "custom-skill-user");
        assert_eq!(skill.global_dir, "~/.custom-skill/skills");
    }
}

/// Read-only application/CLI discovery; absent runtime probes mean unknown.
pub fn runtime_detected(agent_id: &str) -> Option<bool> {
    super::gate::read(|| {
        let definitions = crate::agents::load_agents();
        let definition = definitions.get(agent_id)?;
        let category = definition.category.as_deref().unwrap_or("cli");
        let probes = definition.skills.as_ref().map(|capability| capability.probes.as_slice()).unwrap_or_default();
        super::agent_launch::official_runtime_detected(agent_id, category, probes)
    })
}
