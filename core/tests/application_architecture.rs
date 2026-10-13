#![cfg(unix)]

use arca_core::application::operations::{
    CancelOperationRequest, CommitOperationRequest, OperationCommitResult, OperationPlan,
    PlanOperationRequest,
};
use arca_core::application::ArcaCore;
use arca_core::domain::agents::{AgentConfigurationPatch, ModelConfigurationPatch};
use arca_core::domain::assets::{
    AssetCommitRequest, CentralAssetDraft, PlanUpdateAgentCapabilitiesRequest,
    PlanUpdateCentralAssetRequest,
};
use arca_core::testenv::TestHome;
use arca_core::types::{RegistryConfig, RegistryEntry, StdioConfig};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn mcp_entry(name: &str) -> RegistryEntry {
    RegistryEntry {
        name: name.into(),
        description: "facade fixture".into(),
        tags: vec!["test".into()],
        config: RegistryConfig {
            stdio: Some(StdioConfig {
                command: "fixture-server".into(),
                args: None,
                env: None,
                cwd: None,
            }),
            http: None,
        },
        origin: None,
        repo: None,
    }
}

#[test]
fn unified_facade_creates_an_asset_and_returns_a_revisioned_snapshot() {
    let _home = TestHome::new("application-facade");
    let plan = ArcaCore::plan(PlanOperationRequest::UpdateCentralAsset(
        PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: None,
                entry: Box::new(mcp_entry("facade")),
            },
        },
    ))
    .unwrap();
    let OperationPlan::Asset { plan } = plan else {
        panic!("MCP lifecycle must use the asset coordinator");
    };
    let plan = *plan;
    let result = ArcaCore::commit(CommitOperationRequest::Asset {
        request: AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        },
    })
    .unwrap();
    assert!(matches!(result, OperationCommitResult::Asset { .. }));

    let snapshot = ArcaCore::snapshot().unwrap();
    assert_eq!(snapshot.revision.len(), 64);
    assert!(snapshot
        .assets
        .mcp
        .iter()
        .any(|entry| entry.name == "facade"));
    assert!(snapshot
        .agents
        .iter()
        .any(|agent| agent.capabilities.model.is_some()));
    assert!(snapshot
        .agents
        .iter()
        .any(|agent| agent.capabilities.skill.is_some()));
}

#[test]
fn workspace_snapshot_is_read_only() {
    let home = TestHome::new("workspace-read-only");
    let snapshot = ArcaCore::snapshot().unwrap();
    assert_eq!(snapshot.revision.len(), 64);
    assert!(!home.home.join(".arca/settings.json").exists());
    assert!(!home.home.join(".arca/assets/skills/items").exists());
    assert!(std::fs::symlink_metadata(home.home.join(".arca/skills")).is_err());
}

#[test]
fn capability_patch_does_not_require_unrelated_mcp_or_skill_fields() {
    let _home = TestHome::new("capability-patch");
    let before = arca_core::application::agents::get_configuration_patch("grok-build").unwrap();
    let model = before.model.expect("Grok Build has a Model writer");
    let mut paths = model.paths.clone();
    paths[0] = "~/.grok-build/config-refactored.json".into();
    let plan = ArcaCore::plan(PlanOperationRequest::UpdateAgentCapabilities(
        PlanUpdateAgentCapabilitiesRequest {
            agent_id: "grok-build".into(),
            patch: AgentConfigurationPatch {
                model: Some(ModelConfigurationPatch {
                    paths: paths.clone(),
                }),
                ..AgentConfigurationPatch::default()
            },
        },
    ))
    .unwrap();
    let OperationPlan::Asset { plan } = plan else {
        panic!("Agent capability configuration must use the asset coordinator");
    };
    ArcaCore::commit(CommitOperationRequest::Asset {
        request: AssetCommitRequest {
            operation_id: plan.operation_id.clone(),
            candidate_hash: plan.candidate_hash.clone(),
        },
    })
    .unwrap();
    let after = arca_core::application::agents::get_configuration_patch("grok-build").unwrap();
    assert_eq!(after.model.unwrap().paths, paths);
    assert_eq!(after.mcp, before.mcp);
    assert_eq!(after.skill, before.skill);
}

#[test]
fn workspace_revision_is_stable_for_multiple_mcp_assets() {
    let _home = TestHome::new("workspace-stable-revision");
    for name in ["zeta", "alpha"] {
        arca_core::resources::mcp::registry::write_manual_entry(&mcp_entry(name)).unwrap();
    }
    let first = ArcaCore::snapshot().unwrap();
    let second = ArcaCore::snapshot().unwrap();
    assert_eq!(first.revision, second.revision);
    assert_eq!(
        first
            .assets
            .mcp
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
}

#[test]
fn workspace_revision_canonicalizes_unordered_mcp_maps() {
    let _home = TestHome::new("workspace-canonical-maps");
    let mut entry = mcp_entry("canonical");
    entry.config.stdio.as_mut().unwrap().env = Some(HashMap::from([
        ("ALPHA".into(), "one".into()),
        ("BETA".into(), "two".into()),
    ]));
    arca_core::resources::mcp::registry::write_manual_entry(&entry).unwrap();
    let first = ArcaCore::snapshot().unwrap();

    entry.config.stdio.as_mut().unwrap().env = Some(HashMap::from([
        ("BETA".into(), "two".into()),
        ("ALPHA".into(), "one".into()),
    ]));
    arca_core::resources::mcp::registry::write_manual_entry(&entry).unwrap();
    let second = ArcaCore::snapshot().unwrap();

    assert_eq!(first.revision, second.revision);
}

#[test]
fn cancelling_an_asset_plan_is_idempotent() {
    let _home = TestHome::new("application-cancel");
    let plan = ArcaCore::plan(PlanOperationRequest::UpdateCentralAsset(
        PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: None,
                entry: Box::new(mcp_entry("cancel-me")),
            },
        },
    ))
    .unwrap();
    let OperationPlan::Asset { plan } = plan else {
        panic!("MCP lifecycle must use the asset coordinator");
    };
    let request = CancelOperationRequest::Asset {
        operation_id: plan.operation_id.clone(),
    };
    ArcaCore::cancel(request.clone()).unwrap();
    ArcaCore::cancel(request).unwrap();
}

#[test]
fn domain_layer_has_no_infrastructure_or_resource_engine_dependencies() {
    let domain = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domain");
    let forbidden = [
        "crate::agents",
        "crate::settings",
        "crate::registry",
        "crate::models",
        "crate::skills",
        "crate::network",
        "crate::ops",
        "crate::sources",
        "crate::scanner",
        "crate::safe_write",
        "crate::paths",
        "crate::types",
        "crate::r#override",
        "crate::resources",
    ];
    for path in rust_files(&domain) {
        let source = fs::read_to_string(&path).unwrap();
        for dependency in forbidden {
            assert!(
                !source.contains(dependency),
                "{} must not depend on {dependency}",
                path.display()
            );
        }
    }
}

#[test]
fn settings_contract_depends_on_domain_dtos_not_resource_engines() {
    let settings = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/settings.rs");
    let source = fs::read_to_string(&settings).unwrap();
    for dependency in [
        "crate::disabled",
        "crate::skills",
        "crate::resources",
        "crate::consumption",
    ] {
        assert!(
            !source.contains(dependency),
            "{} must not depend on resource DTO path {dependency}",
            settings.display()
        );
    }
    assert!(source.contains("crate::domain::mcp::DisabledEntry"));
    assert!(source.contains("crate::domain::skill::ManagedSkillRecord"));
}

#[test]
fn rust_frontends_depend_on_the_application_boundary() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core has repository parent");
    let forbidden = [
        "arca_core::consumption",
        "arca_core::ops",
        "arca_core::registry",
        "arca_core::models",
        "arca_core::skills",
        "arca_core::sources",
        "arca_core::agents",
        "arca_core::settings",
        "arca_core::resources",
        "application::mcp::catalog::write_",
        "application::agents::save_agents",
        "application::agents::update_configuration",
        "application::agents::apply_configuration",
        "ops::install(",
        "ops::disable(",
        "ops::enable(",
        "ops::delete(",
        "ops::upsert_entry(",
        "ops::forget_entry(",
        "ops::resync_entry(",
        "arca_core::scanner",
        "arca_core::network",
        "arca_core::pinned_agents",
        "arca_core::types",
        "arca_core::effective",
        "arca_core::r#override",
    ];
    for root in [repo.join("cli/src"), repo.join("desktop/src-tauri/src")] {
        for path in rust_files(&root) {
            let source = fs::read_to_string(&path).unwrap();
            for dependency in forbidden {
                assert!(
                    !source.contains(dependency),
                    "{} bypasses the application boundary through {dependency}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn application_modules_do_not_glob_reexport_resource_engines() {
    let application = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application");
    let forbidden = [
        "pub use crate::agents::*",
        "pub use crate::models::*",
        "pub use crate::skills::*",
        "pub use crate::ops::*",
        "pub use crate::registry::*",
        "pub use crate::scanner::*",
        "pub use crate::sources::*",
        "pub use crate::network::*",
        "pub use crate::update::*",
        "pub use crate::pinned_agents::*",
    ];
    for path in rust_files(&application) {
        let source = fs::read_to_string(&path).unwrap();
        for export in forbidden {
            assert!(
                !source.contains(export),
                "{} leaks a resource engine through {export}",
                path.display()
            );
        }
    }
}

#[test]
fn core_implementation_does_not_depend_on_legacy_root_aliases() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "crate::adapter",
        "crate::applier",
        "crate::codec",
        "crate::consumption",
        "crate::differ",
        "crate::disabled",
        "crate::effective",
        "crate::json_adapter",
        "crate::models",
        "crate::ops",
        "crate::r#override",
        "crate::registry",
        "crate::scanner",
        "crate::skills",
        "crate::sources",
        "crate::toml_adapter",
        "crate::toml_list_adapter",
        "crate::types",
        "crate::yaml_adapter",
    ];
    for path in rust_files(&source_root) {
        if path.file_name().and_then(|name| name.to_str()) == Some("lib.rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        for dependency in forbidden {
            assert!(
                !source.contains(dependency),
                "{} depends on legacy compatibility alias {dependency}",
                path.display()
            );
        }
    }
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files
}
