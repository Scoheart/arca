use arca_core::application::assets::{ConsumptionInventory, ConsumptionView};
use arca_core::application::skills::{SkillInventoryItem, SkillLocation, SkillsInventory};
use arca_core::domain::skill::SkillSource;
use serde_json::{json, Value};

pub fn safe_agent_view(agent: &arca_core::application::agents::AgentCapabilityView) -> Value {
    json!({
        "identity": {
            "id": agent.identity.id,
            "name": agent.identity.name,
            "enabled": agent.identity.enabled,
            "builtin": agent.identity.builtin,
            "category": agent.identity.category,
            "evidence": agent.identity.evidence,
            "verified_at": agent.identity.verified_at,
        },
        "installed": agent.installed,
        "runtime_detected": agent.runtime_detected,
        "config_detected": agent.config_detected,
        "capabilities": {
            "mcp": agent.capabilities.mcp.as_ref().map(|mcp| json!({
                "writable": mcp.writable,
                "config_path": mcp.config_path.as_deref().map(safe_path),
                "format": mcp.format,
                "key": mcp.key,
                "supported_transports": mcp.supported_transports,
            })),
            "model": agent.capabilities.model.as_ref().map(|model| json!({
                "mode": model.mode,
                "storage_authority": model.storage_authority,
                "installed": model.installed,
                "config_paths": model.config_paths.iter().map(|path| safe_path(path)).collect::<Vec<_>>(),
                "assigned_profiles": model.assigned_profiles,
                "active_profile": model.active_profile,
                "supports_multiple": model.supports_multiple,
                "supports_global_selection": model.supports_global_selection,
                "credential_mode": model.credential_mode,
                "supported_protocols": model.supported_protocols,
            })),
            "skill": agent.capabilities.skill.as_ref().map(|skill| json!({
                "installed": skill.installed,
                "target_id": skill.target_id,
                "global_dir": safe_path(&skill.global_dir),
                "alias_dirs": skill.alias_dirs.iter().map(|path| safe_path(path)).collect::<Vec<_>>(),
                "affected_agent_ids": skill.affected_agent_ids,
            })),
        },
    })
}

pub fn safe_model_candidate(
    candidate: &arca_core::application::assets::ModelAdoptionCandidate,
) -> Value {
    json!({
        "candidate_id": candidate.candidate_id,
        "agent_id": candidate.agent_id,
        "native_id": candidate.native_id,
        "managed_profile_id": candidate.managed_profile_id,
        "name": candidate.name,
        "provider": candidate.provider,
        "model_vendor": candidate.model_vendor,
        "protocol": candidate.protocol,
        "base_url": safe_url(&candidate.base_url),
        "model": candidate.model,
        "env_key": candidate.env_key,
        "active": candidate.active,
        "credential_kind": candidate.credential_kind,
        "status": candidate.status,
        // Parser diagnostics can quote malformed source lines, including a
        // credential literal. The machine projection exposes only a stable,
        // status-derived explanation.
        "reason": safe_model_reason(&candidate.status),
        "fingerprint": candidate.fingerprint,
        "settings_hash": candidate.settings_hash,
        "target_hash": candidate.target_hash,
        "candidate_hash": candidate.candidate_hash,
    })
}

fn safe_model_reason(
    status: &arca_core::application::assets::ModelAdoptionStatus,
) -> Option<&'static str> {
    use arca_core::application::assets::ModelAdoptionStatus;

    match status {
        ModelAdoptionStatus::Adoptable => None,
        ModelAdoptionStatus::NeedsCredential => Some("credential required"),
        ModelAdoptionStatus::Unsupported => Some("unsupported external model configuration"),
        ModelAdoptionStatus::Conflicted => Some("conflicting external model configuration"),
    }
}

pub fn safe_consumption_inventory(inventory: &ConsumptionInventory) -> Value {
    json!({
        "revision": inventory.revision,
        "observed_at": inventory.observed_at,
        "native_plugin_agents": inventory.native_plugin_agents,
        "host_mcps": inventory.host_mcps.iter().map(safe_host_mcp).collect::<Vec<_>>(),
        "native_plugins": inventory.native_plugins.iter().map(safe_native_plugin).collect::<Vec<_>>(),
        "consumptions": inventory.consumptions.iter().map(safe_consumption_view).collect::<Vec<_>>(),
        "external": inventory.external.iter().map(safe_consumption_view).collect::<Vec<_>>(),
        "capability_errors": inventory.capability_errors.iter().map(|diagnostic| json!({
            "capability": diagnostic.capability,
            "code": diagnostic.code,
        })).collect::<Vec<_>>(),
        "target_incidents": inventory.target_incidents.iter().map(safe_target_incident).collect::<Vec<_>>(),
    })
}

pub fn safe_target_incident(incident: &arca_core::application::assets::TargetIncident) -> Value {
    json!({
        "id": incident.id,
        "operation_id": incident.operation_id,
        "capability": incident.capability,
        "target_id": incident.target_id,
        "target_path": safe_path(&incident.target_path),
        "affected_agent_ids": incident.affected_agent_ids,
        "code": incident.code,
        "retryable": incident.retryable,
    })
}

pub fn safe_host_mcp(item: &arca_core::domain::plugin::HostMcpObservation) -> Value {
    json!({
        "agent_id": item.agent_id, "server_id": item.server_id, "name": item.name,
        "provider": item.provider, "origin": item.origin, "presence": item.presence,
        "versions": item.versions, "source_paths": item.source_paths.iter().map(|path| safe_path(path)).collect::<Vec<_>>(),
        "docs_url": item.docs_url, "source_fingerprint": item.source_fingerprint,
    })
}

pub fn safe_native_plugin(item: &arca_core::domain::plugin::NativePluginObservation) -> Value {
    json!({
        "agent_id": item.agent_id, "plugin_id": item.plugin_id, "name": item.name,
        "version": item.version, "revision": item.revision, "presence": item.presence,
        "enabled": item.enabled,
        "source_paths": item.source_paths.iter().map(|path| safe_path(path)).collect::<Vec<_>>(),
        "components": item.components.iter().map(|component| json!({
            "kind": component.kind, "name": component.name,
            "source_path": safe_path(&component.source_path), "native_id": component.native_id,
        })).collect::<Vec<_>>(),
        "docs_url": item.docs_url, "source_fingerprint": item.source_fingerprint,
    })
}

pub fn safe_consumption_view(row: &ConsumptionView) -> Value {
    json!({
        "agent_id": row.agent_id,
        "asset": row.asset,
        "ownership": row.ownership,
        "desired": row.desired,
        "observed": row.observed,
        "enabled": row.enabled,
        "observed_enabled": row.observed_enabled,
        "active": row.active,
        "desired_active": row.desired_active,
        "status": row.status,
        "reason": row.reason,
        "observation_id": row.observation_id,
        "available_actions": row.available_actions,
        "affected_agent_ids": row.affected_agent_ids,
        "target": row.target.as_ref().map(|target| json!({
            "target_id": target.target_id,
            "global_dir": safe_path(&target.global_dir),
        })),
    })
}

pub fn safe_skill_inventory(inventory: &SkillsInventory) -> Value {
    json!({
        "items": inventory.items.iter().map(safe_skill_item).collect::<Vec<_>>(),
        "agents": inventory.agents.iter().map(|agent| json!({
            "id": agent.id,
            "name": agent.name,
            "target_id": agent.target_id,
            "global_dir": safe_path(&agent.global_dir),
            "affected_agent_ids": agent.affected_agent_ids,
            "docs_configured": !agent.docs.trim().is_empty(),
            "evidence": agent.evidence,
            "verified_at": agent.verified_at,
        })).collect::<Vec<_>>(),
        "capabilities": inventory.capabilities.iter().map(|capability| json!({
            "id": capability.id,
            "installed": capability.installed,
            "target_id": capability.target_id,
            "global_dir": safe_path(&capability.global_dir),
            "affected_agent_ids": capability.affected_agent_ids,
        })).collect::<Vec<_>>(),
        "targets": inventory.targets.iter().map(|target| json!({
            "target_id": target.target_id,
            "global_dir": safe_path(&target.global_dir),
            "primary_agent_ids": target.primary_agent_ids,
            "affected_agent_ids": target.affected_agent_ids,
            "assignable": target.assignable,
        })).collect::<Vec<_>>(),
        "recovery_error": inventory.recovery_error.as_ref().map(|_| "recovery_required"),
    })
}

pub fn safe_skill_item(item: &SkillInventoryItem) -> Value {
    json!({
        "identity": item.identity,
        "name": item.name,
        "description": item.description,
        "content_kind": item.content_kind,
        "states": item.states,
        "location": safe_skill_location(&item.location),
        "source": item.source.as_ref().map(safe_skill_source),
        "resolved_revision": item.resolved_revision,
        "content_hash": item.content_hash,
        "risk": item.risk,
        "update": {
            "available": item.update.available,
            "checked_at": item.update.checked_at,
            "resolved_revision": item.update.resolved_revision,
            "retry_at": item.update.retry_at,
        },
        "assigned_target_ids": item.assigned_target_ids,
        "affected_agent_ids": item.affected_agent_ids,
        "installed_at": item.installed_at,
        "updated_at": item.updated_at,
    })
}

fn safe_skill_location(location: &SkillLocation) -> Value {
    match location {
        SkillLocation::Central => json!({"kind": "central"}),
        SkillLocation::AgentTarget {
            target_id,
            global_dir,
        } => json!({
            "kind": "agent_target",
            "target_id": target_id,
            "global_dir": safe_path(global_dir),
        }),
    }
}

fn safe_skill_source(source: &SkillSource) -> Value {
    match source {
        SkillSource::Github {
            owner,
            repo,
            subpath,
            requested_ref,
            pinned,
        } => json!({
            "kind": "github",
            "owner": owner,
            "repo": repo,
            "subpath": subpath,
            "requested_ref": requested_ref,
            "pinned": pinned,
        }),
        SkillSource::Local { subpath, .. } => {
            json!({"kind": "local", "subpath": subpath, "path_redacted": true})
        }
        SkillSource::Archive { subpath, .. } => {
            json!({"kind": "archive", "subpath": subpath, "path_redacted": true})
        }
        SkillSource::Imported { .. } => {
            json!({"kind": "imported", "paths_redacted": true})
        }
    }
}

pub fn safe_path(path: &str) -> String {
    if path == "~" || path.starts_with("~/") || !std::path::Path::new(path).is_absolute() {
        return path.to_string();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::Path::new(&home);
        let path_value = std::path::Path::new(path);
        if let Ok(relative) = path_value.strip_prefix(home) {
            return if relative.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", relative.display())
            };
        }
    }
    "<absolute-path-redacted>".into()
}

pub fn safe_url(url: &str) -> String {
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let Some((scheme, remainder)) = without_query.split_once("://") else {
        return "<redacted>".into();
    };
    // Isolate the authority before removing userinfo. An `@` in the path is
    // ordinary path data and must never be mistaken for the host boundary.
    let authority_with_userinfo = remainder.split('/').next().unwrap_or(remainder);
    let authority = authority_with_userinfo
        .rsplit_once('@')
        .map_or(authority_with_userinfo, |(_, host)| host);
    format!("{scheme}://{authority}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_paths_outside_home_are_redacted() {
        let rendered = safe_path("/private/example/secret/skills");
        assert_eq!(rendered, "<absolute-path-redacted>");
    }

    #[test]
    fn tilde_paths_remain_useful() {
        assert_eq!(safe_path("~/.agents/skills"), "~/.agents/skills");
    }

    #[test]
    fn model_discovery_reason_is_status_derived_and_cannot_echo_parser_input() {
        use arca_core::application::assets::ModelAdoptionStatus;

        let parser_sentinel = "token = SECRET_SENTINEL";
        let rendered = safe_model_reason(&ModelAdoptionStatus::Unsupported).unwrap();
        assert_eq!(rendered, "unsupported external model configuration");
        assert!(!rendered.contains(parser_sentinel));
    }

    #[test]
    fn safe_url_never_promotes_path_or_credentials_into_the_authority() {
        assert_eq!(
            safe_url("https://api.example.com/v1/@URL_PATH_SECRET_SENTINEL?q=SECRET#SECRET"),
            "https://api.example.com"
        );
        assert_eq!(
            safe_url("https://user:password@api.example.com/private"),
            "https://api.example.com"
        );
    }

    #[test]
    fn target_incidents_redact_paths_in_json() {
        let inventory = ConsumptionInventory {
            target_incidents: vec![arca_core::application::assets::TargetIncident {
                id: "target-1".into(),
                operation_id: "operation-1".into(),
                capability: arca_core::application::assets::AssetCapability::Mcp,
                target_id: "target-1".into(),
                target_path: "/private/user/.qoder/mcp.json".into(),
                affected_agent_ids: vec!["qoder".into()],
                code: "target_recovery_required".into(),
                retryable: true,
            }],
            ..Default::default()
        };
        let encoded = safe_consumption_inventory(&inventory).to_string();
        assert!(encoded.contains("target_recovery_required"));
        assert!(!encoded.contains("/private/user"));
        assert!(encoded.contains("<absolute-path-redacted>"));
    }

    #[test]
    fn skill_recovery_diagnostics_are_generic_in_json() {
        let inventory = SkillsInventory {
            items: Vec::new(),
            agents: Vec::new(),
            capabilities: Vec::new(),
            targets: Vec::new(),
            recovery_error: Some(
                "failed near SECRET_SENTINEL at /private/user/.arca/skills-staging".into(),
            ),
        };
        let encoded = safe_skill_inventory(&inventory).to_string();
        assert!(encoded.contains("recovery_required"));
        assert!(!encoded.contains("SECRET_SENTINEL"));
        assert!(!encoded.contains("/private/user"));
    }

    #[test]
    fn skill_agent_docs_are_never_exposed_in_json() {
        let docs = "https://user:token@example.com/private?secret=DOCS_SENTINEL";
        let inventory = SkillsInventory {
            items: Vec::new(),
            agents: vec![arca_core::application::skills::SkillAgentView {
                id: "custom".into(),
                name: "Custom".into(),
                target_id: "custom-user".into(),
                global_dir: "/private/user/skills".into(),
                affected_agent_ids: vec!["custom".into()],
                docs: docs.into(),
                evidence: "official".into(),
                verified_at: "2026-08-04".into(),
            }],
            capabilities: Vec::new(),
            targets: Vec::new(),
            recovery_error: None,
        };
        let encoded = safe_skill_inventory(&inventory).to_string();
        assert!(encoded.contains("docs_configured"));
        assert!(!encoded.contains("DOCS_SENTINEL"));
        assert!(!encoded.contains("user:token"));
        assert!(!encoded.contains("/private/user"));
    }
}
