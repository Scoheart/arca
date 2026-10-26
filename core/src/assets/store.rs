//! Unified, typed access to Arca-owned asset state.
//!
//! MCP, Model, and Skill payloads deliberately keep their native persistence
//! backends. This store unifies how planners bind the central assets, desired
//! relationships, Agent configuration, and operational target graph they
//! actually reviewed, without copying those payloads into a second manifest.

use crate::domain::assets::{AssetCapability, AssetRef};
use crate::resources::mcp::registry::{read_registry_all_for_settings, CatalogItem};
use crate::resources::model::provider_credential_present;
use crate::resources::skill::{
    hash_tree, list_inventory_for_settings, SkillsInventory, SkillsPaths,
};
use crate::settings::{load_settings_strict, Settings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// One semantic state unit reviewed by an Asset operation.
///
/// Subjects contain stable identities only. Configuration values and secrets
/// are reduced to a SHA-256 revision before an operation is persisted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "subject", rename_all = "kebab-case")]
pub(crate) enum StateSubject {
    CentralAsset {
        asset: AssetRef,
    },
    AssetConsumers {
        asset: AssetRef,
    },
    ModelCatalog,
    ModelProviderNames,
    AgentConsumption {
        capability: AssetCapability,
        agent_id: String,
    },
    AgentConfiguration {
        capability: AssetCapability,
        agent_id: String,
    },
    SkillTargetGraph,
    CredentialPresence {
        profile_id: String,
    },
    ProviderCredentialPresence {
        provider_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StatePrecondition {
    pub subject: StateSubject,
    pub revision: String,
}

/// A single strict settings snapshot shared by all semantic fingerprints in
/// one planning or verification pass.
pub(crate) struct AssetStateStore {
    settings: Settings,
    mcp_catalog: OnceCell<Vec<CatalogItem>>,
    skill_inventory: OnceCell<Result<SkillsInventory, String>>,
}

impl AssetStateStore {
    pub(crate) fn load() -> Result<Self, String> {
        Ok(Self {
            settings: load_settings_strict().map_err(|error| error.to_string())?,
            mcp_catalog: OnceCell::new(),
            skill_inventory: OnceCell::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_settings(settings: Settings) -> Self {
        Self {
            settings,
            mcp_catalog: OnceCell::new(),
            skill_inventory: OnceCell::new(),
        }
    }

    pub(crate) fn capture(
        &self,
        subjects: impl IntoIterator<Item = StateSubject>,
    ) -> Result<Vec<StatePrecondition>, String> {
        let subjects = subjects.into_iter().collect::<BTreeSet<_>>();
        subjects
            .into_iter()
            .map(|subject| {
                let revision = self.fingerprint(&subject)?;
                Ok(StatePrecondition { subject, revision })
            })
            .collect()
    }

    pub(crate) fn verify(&self, expected: &[StatePrecondition]) -> Result<(), String> {
        for precondition in expected {
            if self.fingerprint(&precondition.subject)? != precondition.revision {
                return Err(format!(
                    "asset_operation_stale: {} changed after review",
                    subject_label(&precondition.subject)
                ));
            }
        }
        Ok(())
    }

    fn fingerprint(&self, subject: &StateSubject) -> Result<String, String> {
        let value = match subject {
            StateSubject::CentralAsset { asset } => self.central_asset(asset)?,
            StateSubject::AssetConsumers { asset } => self.asset_consumers(asset)?,
            StateSubject::ModelCatalog => serde_json::to_value((
                &self.settings.model_providers,
                &self.settings.model_profiles,
            ))
            .map_err(|error| error.to_string())?,
            StateSubject::ModelProviderNames => serde_json::to_value(
                self.settings
                    .model_providers
                    .as_ref()
                    .into_iter()
                    .flat_map(|providers| providers.iter())
                    .map(|(id, provider)| (id, provider.name.to_ascii_lowercase()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .map_err(|error| error.to_string())?,
            StateSubject::AgentConsumption {
                capability,
                agent_id,
            } => self.agent_consumption(*capability, agent_id)?,
            StateSubject::AgentConfiguration {
                capability,
                agent_id,
            } => serde_json::to_value((
                capability,
                self.settings
                    .agents
                    .as_ref()
                    .and_then(|agents| agents.get(agent_id)),
                self.settings
                    .agent_config_paths
                    .as_ref()
                    .and_then(|paths| paths.get(agent_id)),
            ))
            .map_err(|error| error.to_string())?,
            StateSubject::SkillTargetGraph => skill_target_graph(self.skill_inventory()?)?,
            StateSubject::CredentialPresence { profile_id } => {
                Value::Bool(crate::resources::model::credential_present(profile_id))
            }
            StateSubject::ProviderCredentialPresence { provider_id } => {
                Value::Bool(provider_credential_present(provider_id))
            }
        };
        hash_value(value)
    }

    fn central_asset(&self, asset: &AssetRef) -> Result<Value, String> {
        match asset {
            AssetRef::Mcp { key } => {
                let copies = self
                    .mcp_catalog()
                    .iter()
                    .filter(|copy| copy.entry.key() == *key)
                    .collect::<Vec<_>>();
                serde_json::to_value(copies).map_err(|error| error.to_string())
            }
            AssetRef::Model { profile_id } => {
                let profile = self
                    .settings
                    .model_profiles
                    .as_ref()
                    .and_then(|profiles| profiles.get(profile_id));
                let provider = profile
                    .and_then(|profile| profile.provider_id.as_deref())
                    .and_then(|provider_id| {
                        self.settings
                            .model_providers
                            .as_ref()
                            .and_then(|providers| providers.get(provider_id))
                    });
                serde_json::to_value((profile, provider)).map_err(|error| error.to_string())
            }
            AssetRef::ModelProvider { provider_id } => {
                let provider = self
                    .settings
                    .model_providers
                    .as_ref()
                    .and_then(|providers| providers.get(provider_id));
                let profiles = self
                    .settings
                    .model_profiles
                    .as_ref()
                    .into_iter()
                    .flat_map(|profiles| profiles.values())
                    .filter(|profile| profile.provider_id.as_deref() == Some(provider_id))
                    .collect::<Vec<_>>();
                serde_json::to_value((provider, profiles)).map_err(|error| error.to_string())
            }
            AssetRef::Skill { name } => {
                let record = self
                    .settings
                    .managed_skills
                    .as_ref()
                    .and_then(|skills| skills.get(name));
                let paths =
                    SkillsPaths::resolve_from_env().map_err(|error| format!("{error:?}"))?;
                let path = paths.central_skill(name);
                let content_hash = match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.file_type().is_dir() => {
                        Some(hash_tree(&path).map_err(|error| format!("{error:?}"))?)
                    }
                    Ok(_) => Some("invalid".into()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.to_string()),
                };
                serde_json::to_value((record, content_hash)).map_err(|error| error.to_string())
            }
        }
    }

    fn asset_consumers(&self, asset: &AssetRef) -> Result<Value, String> {
        match asset {
            AssetRef::Mcp { key } => serde_json::to_value(
                self.settings
                    .mcp_consumptions
                    .as_ref()
                    .into_iter()
                    .flat_map(|consumptions| consumptions.iter())
                    .filter_map(|(agent_id, records)| {
                        records
                            .get(key)
                            .map(|record| (agent_id.clone(), record.clone()))
                    })
                    .collect::<BTreeMap<_, _>>(),
            )
            .map_err(|error| error.to_string()),
            AssetRef::Model { profile_id } => {
                serde_json::to_value(self.model_consumers(profile_id))
                    .map_err(|error| error.to_string())
            }
            AssetRef::ModelProvider { provider_id } => {
                let consumers = self
                    .settings
                    .model_profiles
                    .as_ref()
                    .into_iter()
                    .flat_map(|profiles| profiles.iter())
                    .filter(|(_, profile)| profile.provider_id.as_deref() == Some(provider_id))
                    .map(|(profile_id, _)| (profile_id.clone(), self.model_consumers(profile_id)))
                    .collect::<BTreeMap<_, _>>();
                serde_json::to_value(consumers).map_err(|error| error.to_string())
            }
            AssetRef::Skill { name } => serde_json::to_value((
                self.settings
                    .skill_assignments
                    .as_ref()
                    .and_then(|assignments| assignments.get(name)),
                self.settings
                    .skill_consumptions
                    .as_ref()
                    .and_then(|consumptions| consumptions.get(name)),
            ))
            .map_err(|error| error.to_string()),
        }
    }

    fn model_consumers(
        &self,
        profile_id: &str,
    ) -> BTreeMap<String, (crate::domain::assets::ModelConsumptionRecord, bool)> {
        let agent_ids = self
            .settings
            .model_consumptions
            .as_ref()
            .into_iter()
            .flat_map(|consumptions| consumptions.keys())
            .chain(
                self.settings
                    .model_assignments
                    .as_ref()
                    .into_iter()
                    .flat_map(|assignments| assignments.keys()),
            )
            .collect::<BTreeSet<_>>();
        agent_ids
            .into_iter()
            .filter_map(|agent_id| {
                let selection = self.settings.model_selection(agent_id);
                selection.profiles.get(profile_id).cloned().map(|record| {
                    (
                        agent_id.clone(),
                        (
                            record,
                            selection.active_profile_id.as_deref() == Some(profile_id),
                        ),
                    )
                })
            })
            .collect()
    }

    fn mcp_catalog(&self) -> &Vec<CatalogItem> {
        self.mcp_catalog
            .get_or_init(|| read_registry_all_for_settings(&self.settings))
    }

    fn skill_inventory(&self) -> Result<&SkillsInventory, String> {
        match self.skill_inventory.get_or_init(|| {
            list_inventory_for_settings(&self.settings).map_err(|error| format!("{error:?}"))
        }) {
            Ok(inventory) => Ok(inventory),
            Err(error) => Err(error.clone()),
        }
    }

    fn agent_consumption(
        &self,
        capability: AssetCapability,
        agent_id: &str,
    ) -> Result<Value, String> {
        match capability {
            AssetCapability::Mcp => serde_json::to_value((
                self.settings
                    .mcp_consumptions
                    .as_ref()
                    .and_then(|all| all.get(agent_id)),
                self.settings
                    .disabled
                    .as_ref()
                    .and_then(|all| all.get(agent_id)),
            ))
            .map_err(|error| error.to_string()),
            AssetCapability::Model => serde_json::to_value(self.settings.model_selection(agent_id))
                .map_err(|error| error.to_string()),
            AssetCapability::Skill => {
                let inventory = self.skill_inventory()?;
                let target_ids = inventory
                    .targets
                    .iter()
                    .filter(|target| target.affected_agent_ids.iter().any(|id| id == agent_id))
                    .map(|target| target.target_id.clone())
                    .collect::<BTreeSet<_>>();
                let assignments = self
                    .settings
                    .skill_assignments
                    .as_ref()
                    .into_iter()
                    .flat_map(|all| all.iter())
                    .filter_map(|(name, targets)| {
                        let selected = targets
                            .intersection(&target_ids)
                            .cloned()
                            .collect::<BTreeSet<_>>();
                        (!selected.is_empty()).then_some((name.clone(), selected))
                    })
                    .collect::<BTreeMap<_, _>>();
                let consumptions = self
                    .settings
                    .skill_consumptions
                    .as_ref()
                    .into_iter()
                    .flat_map(|all| all.iter())
                    .filter_map(|(name, records)| {
                        let selected = records
                            .iter()
                            .filter(|(target_id, _)| target_ids.contains(*target_id))
                            .map(|(target_id, record)| (target_id.clone(), record.clone()))
                            .collect::<BTreeMap<_, _>>();
                        (!selected.is_empty()).then_some((name.clone(), selected))
                    })
                    .collect::<BTreeMap<_, _>>();
                serde_json::to_value((target_ids, assignments, consumptions))
                    .map_err(|error| error.to_string())
            }
        }
    }
}

fn skill_target_graph(inventory: &SkillsInventory) -> Result<Value, String> {
    let mut targets = inventory
        .targets
        .iter()
        .map(|target| {
            (
                target.target_id.clone(),
                target.global_dir.clone(),
                target
                    .affected_agent_ids
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<Vec<_>>();
    targets.sort();
    serde_json::to_value(targets).map_err(|error| error.to_string())
}

fn hash_value(value: Value) -> Result<String, String> {
    // serde_json's map representation is key ordered in this build, including
    // maps that originated as MCP header/environment HashMaps.
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn subject_label(subject: &StateSubject) -> &'static str {
    match subject {
        StateSubject::CentralAsset {
            asset: AssetRef::Mcp { .. },
        } => "central MCP catalog",
        StateSubject::CentralAsset {
            asset: AssetRef::Model { .. } | AssetRef::ModelProvider { .. },
        } => "central Model asset",
        StateSubject::CentralAsset {
            asset: AssetRef::Skill { .. },
        } => "central Skill asset",
        StateSubject::AssetConsumers { .. } => "central asset consumers",
        StateSubject::ModelCatalog => "the Model catalog",
        StateSubject::ModelProviderNames => "the Model Provider name index",
        StateSubject::AgentConsumption { .. } => "an Agent consumption",
        StateSubject::AgentConfiguration { .. } => "an Agent configuration",
        StateSubject::SkillTargetGraph => "Skill target graph",
        StateSubject::CredentialPresence { .. } => "Model credential state",
        StateSubject::ProviderCredentialPresence { .. } => "Model Provider credential state",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assets::{McpConsumptionRecord, ModelConsumptionRecord};
    use crate::domain::mcp::OverridePatch;
    use crate::domain::types::{RegistryConfig, RegistryEntry, StdioConfig};
    use crate::resources::mcp::registry::write_manual_entry;
    use crate::settings::{NetworkSettings, UiSettings};
    use crate::testenv::TestHome;
    use std::collections::HashMap;

    fn mcp_subject(agent_id: &str) -> StateSubject {
        StateSubject::AgentConsumption {
            capability: AssetCapability::Mcp,
            agent_id: agent_id.into(),
        }
    }

    #[test]
    fn unrelated_preferences_do_not_change_asset_revisions() {
        let mut settings = Settings::default();
        let before = AssetStateStore::from_settings(settings.clone())
            .capture([mcp_subject("codex")])
            .unwrap();

        settings.ui = Some(UiSettings {
            pinned_agents: vec!["codex".into()],
            ..Default::default()
        });
        settings.network = Some(NetworkSettings {
            proxy_url: Some("http://127.0.0.1:7890".into()),
            ..Default::default()
        });

        AssetStateStore::from_settings(settings)
            .verify(&before)
            .unwrap();
    }

    #[test]
    fn unrelated_agent_consumption_does_not_change_reviewed_agent_revision() {
        let mut settings = Settings::default();
        let before = AssetStateStore::from_settings(settings.clone())
            .capture([mcp_subject("codex")])
            .unwrap();
        settings
            .mcp_consumptions
            .get_or_insert_default()
            .entry("claude-code".into())
            .or_default()
            .insert(
                "server::stdio".into(),
                McpConsumptionRecord {
                    asset_key: "server::stdio".into(),
                    enabled: true,
                    overrides: OverridePatch::default(),
                },
            );

        AssetStateStore::from_settings(settings)
            .verify(&before)
            .unwrap();
    }

    #[test]
    fn relevant_consumption_change_invalidates_revision() {
        let mut settings = Settings::default();
        let before = AssetStateStore::from_settings(settings.clone())
            .capture([mcp_subject("codex")])
            .unwrap();
        settings
            .mcp_consumptions
            .get_or_insert_default()
            .entry("codex".into())
            .or_default()
            .insert(
                "server::stdio".into(),
                McpConsumptionRecord {
                    asset_key: "server::stdio".into(),
                    enabled: true,
                    overrides: OverridePatch::default(),
                },
            );

        let error = AssetStateStore::from_settings(settings)
            .verify(&before)
            .unwrap_err();
        assert!(error.contains("Agent consumption"));
    }

    #[test]
    fn reverse_consumer_revision_is_exact_to_one_central_asset() {
        let mut settings = Settings::default();
        let subject = StateSubject::AssetConsumers {
            asset: AssetRef::Mcp {
                key: "alpha::stdio".into(),
            },
        };
        let before = AssetStateStore::from_settings(settings.clone())
            .capture([subject.clone()])
            .unwrap();

        settings
            .mcp_consumptions
            .get_or_insert_default()
            .entry("claude-code".into())
            .or_default()
            .insert(
                "beta::stdio".into(),
                McpConsumptionRecord {
                    asset_key: "beta::stdio".into(),
                    enabled: true,
                    overrides: OverridePatch::default(),
                },
            );
        AssetStateStore::from_settings(settings.clone())
            .verify(&before)
            .unwrap();

        settings
            .mcp_consumptions
            .as_mut()
            .unwrap()
            .get_mut("claude-code")
            .unwrap()
            .insert(
                "alpha::stdio".into(),
                McpConsumptionRecord {
                    asset_key: "alpha::stdio".into(),
                    enabled: true,
                    overrides: OverridePatch::default(),
                },
            );
        let error = AssetStateStore::from_settings(settings)
            .verify(&before)
            .unwrap_err();
        assert!(error.contains("central asset consumers"));
    }

    #[test]
    fn model_legacy_and_canonical_consumption_share_one_projection() {
        let mut legacy = Settings::default();
        legacy
            .model_assignments
            .get_or_insert_default()
            .insert("codex".into(), "work".into());
        let mut canonical = Settings::default();
        canonical
            .model_consumptions
            .get_or_insert_default()
            .entry("codex".into())
            .or_default()
            .insert(
                "work".into(),
                ModelConsumptionRecord {
                    profile_id: "work".into(),
                    enabled: true,
                    credential: crate::domain::assets::ModelCredentialPolicy {
                        source_override: None,
                        delivery: crate::domain::assets::ApiKeyDelivery::Plaintext,
                    },
                    last_selected_at: None,
                },
            );
        canonical
            .model_assignments
            .get_or_insert_default()
            .insert("codex".into(), "work".into());
        let subject = StateSubject::AgentConsumption {
            capability: AssetCapability::Model,
            agent_id: "codex".into(),
        };

        let left = AssetStateStore::from_settings(legacy)
            .capture([subject.clone()])
            .unwrap();
        let right = AssetStateStore::from_settings(canonical)
            .capture([subject])
            .unwrap();

        assert_eq!(left, right);
    }

    #[test]
    fn persisted_preconditions_contain_hashes_not_mcp_secrets() {
        let _home = TestHome::new("asset-store-secret-free");
        write_manual_entry(&RegistryEntry {
            name: "private".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "private-server".into(),
                    args: None,
                    env: Some(HashMap::from([(
                        "TOKEN".into(),
                        "never-persist-this-value".into(),
                    )])),
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();

        let preconditions = AssetStateStore::load()
            .unwrap()
            .capture([StateSubject::CentralAsset {
                asset: AssetRef::Mcp {
                    key: "private::stdio".into(),
                },
            }])
            .unwrap();
        let persisted = serde_json::to_string(&preconditions).unwrap();

        assert!(!persisted.contains("never-persist-this-value"));
        assert!(!persisted.contains("private-server"));
    }
}
