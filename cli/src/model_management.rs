//! Model lifecycle adapters. Validation, credentials and target writes stay in Core.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use arca_core::application::assets::{
    AssetRef, CentralAssetDraft, PlanDeleteCentralAssetRequest, PlanModelAdoptionRequest,
    PlanUpdateCentralAssetRequest,
};
use arca_core::application::models::{self, ApiKeyDelivery, ModelProviderInstanceView};
use arca_core::application::operations::{OperationPlan, PlanOperationRequest};
use arca_core::application::ArcaCore;
use arca_core::domain::types::{ApiKeySource, ModelProfile, ModelProviderConfig};
use serde_json::{json, Value};

use crate::command::{parse_identity, Cli};
use crate::input::{json_file, provider_credential};
use crate::output::{query_output, CliError, CommandOutput};
use crate::projection::{safe_path, safe_url};
use crate::review::{execute_operation, MutationOptions};

#[derive(Debug, Subcommand)]
pub enum ModelManagementCommand {
    /// Print a ready-to-edit cURL command. Secret export requires --include-api-key.
    Curl {
        #[arg(value_parser = parse_identity)]
        profile_id: String,
        #[arg(long)]
        include_api_key: bool,
    },
    /// Create a central Model Profile, or edit one with --id, from a ModelProfile JSON file.
    Save {
        /// Model Profile JSON. The id field can be omitted.
        #[arg(long)]
        file: PathBuf,
        /// Exact existing Profile ID to edit. Omit to create.
        #[arg(long, value_parser = parse_identity)]
        id: Option<String>,
    },
    /// Delete a central Model Profile and clean up its managed consumers.
    Delete {
        #[arg(value_parser = parse_identity)]
        profile_id: String,
    },
    /// Explicitly import an external candidate shown by `arca discover model`.
    Import {
        #[arg(value_parser = parse_identity)]
        candidate_id: String,
    },
    /// Configure Provider connections and look up available models.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
}

impl ModelManagementCommand {
    fn is_mutation(&self) -> bool {
        !matches!(self, Self::Curl { .. } | Self::Provider { command: ProviderCommand::List
            | ProviderCommand::Show { .. } | ProviderCommand::Templates
            | ProviderCommand::Catalog { .. } | ProviderCommand::Docs { .. } })
    }
}

#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// Show the official documentation URL for a template or configured Provider.
    Docs { id: String },
    /// List Provider connections already saved in Arca.
    List,
    /// Show one configured Provider, with private connection details redacted.
    Show {
        #[arg(value_parser = parse_identity)]
        id: String,
    },
    /// List built-in Provider templates and supported protocols.
    Templates,
    /// List models available from one saved Provider connection.
    #[command(alias = "models")]
    Catalog {
        /// Provider ID from `arca model provider list`.
        #[arg(value_parser = parse_identity)]
        id: String,
    },
    /// Save a ModelProviderConfig JSON document. Credentials are retained unless explicitly changed.
    Save {
        /// Provider JSON. The id field can be omitted.
        #[arg(long)]
        file: PathBuf,
        /// Exact existing Provider ID to edit. Omit to create.
        #[arg(long, value_parser = parse_identity)]
        id: Option<String>,
        /// Read a new Keychain credential from piped stdin, never from argv.
        #[arg(long, conflicts_with = "clear_credential")]
        credential_stdin: bool,
        /// Explicitly remove the Provider's stored credential.
        #[arg(long)]
        clear_credential: bool,
    },
    /// Delete a Provider through Core's reviewed lifecycle plan.
    Delete {
        #[arg(value_parser = parse_identity)]
        id: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DeliveryArg {
    Auto,
    Env,
    Command,
    AgentStore,
    Plaintext,
}

impl From<DeliveryArg> for ApiKeyDelivery {
    fn from(value: DeliveryArg) -> Self {
        match value {
            DeliveryArg::Auto => Self::Auto,
            DeliveryArg::Env => Self::Env,
            DeliveryArg::Command => Self::Command,
            DeliveryArg::AgentStore => Self::AgentStore,
            DeliveryArg::Plaintext => Self::Plaintext,
        }
    }
}

pub fn dispatch(cli: &Cli, command: &ModelManagementCommand) -> Result<CommandOutput, CliError> {
    let options = cli.mutation_options();
    if command.is_mutation() {
        options.validate()?;
    } else {
        cli.reject_mutation_options()?;
    }
    match command {
        ModelManagementCommand::Curl { profile_id, include_api_key } => {
            let command = models::export_curl(profile_id, *include_api_key).map_err(CliError::from_legacy)?;
            Ok(CommandOutput::new("model.curl", false, json!({"profile_id": profile_id, "curl": command}), command))
        }
        ModelManagementCommand::Save { file, id } => {
            let profile: ModelProfile = serde_json::from_value(model_draft_json(file)?)
                .map_err(|error| CliError::private("invalid_model_profile", error.to_string()))?;
            apply("model.save", PlanOperationRequest::UpdateCentralAsset(PlanUpdateCentralAssetRequest {
                draft: CentralAssetDraft::Model {
                    existing_id: id.clone(), profile: Box::new(profile), credential: None,
                },
            }), options)
        }
        ModelManagementCommand::Delete { profile_id } => delete(
            "model.delete", AssetRef::Model { profile_id: profile_id.clone() }, options,
        ),
        ModelManagementCommand::Import { candidate_id } => {
            let candidate = arca_core::application::assets::list_model_adoption_candidates()
                .map_err(CliError::from_legacy)?.into_iter()
                .find(|candidate| candidate.candidate_id == *candidate_id)
                .ok_or_else(|| CliError::new("candidate_not_found", "run arca discover model to obtain a current candidate ID"))?;
            let plan = arca_core::application::assets::plan_model_adoption(PlanModelAdoptionRequest {
                candidate_fingerprints: BTreeMap::from([(candidate.candidate_id, candidate.fingerprint)]),
            }).map_err(CliError::from_legacy)?;
            execute_operation("model.import", OperationPlan::Asset { plan: Box::new(plan) }, options)
        }
        ModelManagementCommand::Provider { command } => provider(command, options),
    }
}

fn apply(command: &'static str, request: PlanOperationRequest, options: MutationOptions) -> Result<CommandOutput, CliError> {
    options.validate()?;
    let plan = ArcaCore::plan(request).map_err(CliError::from_core)?;
    execute_operation(command, plan, options)
}

fn delete(command: &'static str, asset: AssetRef, options: MutationOptions) -> Result<CommandOutput, CliError> {
    apply(command, PlanOperationRequest::DeleteCentralAsset(PlanDeleteCentralAssetRequest {
        asset, source_id: None,
    }), options)
}

fn provider(command: &ProviderCommand, options: MutationOptions) -> Result<CommandOutput, CliError> {
    match command {
        ProviderCommand::Docs { id } => {
            let url = models::provider_documentation(id).map_err(CliError::from_legacy)?
                .ok_or_else(|| CliError::new("provider_docs_unavailable", "no official documentation is registered for this Provider"))?;
            Ok(CommandOutput::new("model.provider.docs", false, json!({"provider": id, "url": url}), url))
        }
        ProviderCommand::List => query_output("model.provider.list", json!({
            "providers": models::list_provider_instances().iter().map(safe_provider).collect::<Vec<_>>()
        })),
        ProviderCommand::Show { id } => {
            let view = models::list_provider_instances().into_iter().find(|view| view.provider.id == *id)
                .ok_or_else(|| CliError::new("provider_not_found", "no configured Provider has that ID"))?;
            query_output("model.provider.show", safe_provider(&view))
        }
        ProviderCommand::Templates => query_output("model.provider.templates", json!({"providers": models::list_providers()})),
        ProviderCommand::Catalog { id } => {
            let models = models::discover_provider_models(id).map_err(CliError::from_legacy)?;
            // Keep the existing JSON command identifier for scripts.
            query_output("model.provider.models", json!({"provider_id": id, "models": models}))
        }
        ProviderCommand::Save { file, id, credential_stdin, clear_credential } => {
            // Reject competing stdin consumers before opening either input.
            let credential = provider_credential(file, *credential_stdin, *clear_credential)?;
            let provider: ModelProviderConfig = serde_json::from_value(model_draft_json(file)?)
                .map_err(|error| CliError::private("invalid_model_provider", error.to_string()))?;
            apply("model.provider.save", PlanOperationRequest::UpdateCentralAsset(PlanUpdateCentralAssetRequest {
                draft: CentralAssetDraft::ModelProvider {
                    existing_id: id.clone(), provider: Box::new(provider), credential,
                },
            }), options)
        }
        ProviderCommand::Delete { id } => delete("model.provider.delete", AssetRef::ModelProvider { provider_id: id.clone() }, options),
    }
}

fn model_draft_json(file: &Path) -> Result<Value, CliError> {
    let mut draft = json_file(file)?;
    // The empty identity is Core's draft convention. Leave explicit IDs for
    // Core to validate and keep persisted records subject to their own schema.
    if let Some(fields) = draft.as_object_mut() {
        fields.entry("id").or_insert_with(|| Value::String(String::new()));
    }
    Ok(draft)
}

fn safe_provider(view: &ModelProviderInstanceView) -> Value {
    let provider = &view.provider;
    let source = provider.api_key_source.as_ref().map(|source| match source {
        ApiKeySource::ArcaStore => json!({"kind": "arca-store"}),
        ApiKeySource::Env { name } => json!({"kind": "env", "name": name}),
        ApiKeySource::File { path } => json!({"kind": "file", "path": safe_path(path)}),
        ApiKeySource::Helper { .. } => json!({"kind": "helper", "command_redacted": true}),
    });
    json!({
        "id": provider.id, "name": provider.name, "provider": provider.provider,
        "base_url": safe_url(&provider.base_url),
        "model_catalog_url": provider.model_catalog_url.as_deref().map(safe_url),
        "protocols": provider.protocols.keys().collect::<Vec<_>>(),
        "auth_requirement": provider.auth_requirement, "api_key_source": source,
        "credential_saved": view.credential_saved, "model_count": view.model_count,
        "model_discovery_supported": view.model_discovery_supported,
        "docs_url": models::provider_docs_url(&provider.provider),
        "portal": &view.portal,
    })
}
