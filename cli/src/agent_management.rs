//! Agent definition and configuration adapters; Core owns supported capabilities.

use std::path::PathBuf;

use clap::Subcommand;
use arca_core::application::agents::{self, AgentConfigurationPatch, AgentDefinition};
use arca_core::application::assets::PlanUpdateAgentCapabilitiesRequest;
use arca_core::application::operations::PlanOperationRequest;
use arca_core::application::ArcaCore;
use arca_core::application::agent_launch::{self, LaunchInfo, LaunchTarget};
use serde_json::json;

use crate::command::{parse_identity, Cli};
use crate::input::json_file;
use crate::output::{query_output, CliError, CommandOutput};
use crate::projection::{safe_path, safe_url};
use crate::review::{execute_direct_mutation, execute_operation};

#[derive(Debug, Subcommand)]
pub enum AgentManagementCommand {
    /// Open an Agent using the same application/terminal settings as Desktop.
    Run {
        #[arg(value_parser = parse_identity)]
        agent: String,
        #[arg(long)]
        directory: Option<String>,
    },
    /// Inspect or configure an Agent's shared launcher.
    Launch {
        #[command(subcommand)]
        command: LaunchCommand,
    },
    /// Save an AgentDefinition JSON document. Updating an existing ID requires --replace.
    Save {
        #[arg(value_parser = parse_identity)]
        id: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        replace: bool,
    },
    /// Apply an AgentConfigurationPatch JSON document through a reviewed capability plan.
    Configure {
        #[arg(value_parser = parse_identity)]
        agent: String,
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum LaunchCommand {
    Show { agent: String },
    /// Configure a LaunchTarget JSON file and/or the default directory. Empty directory clears it.
    Configure {
        agent: String,
        #[arg(long, required_unless_present = "default_directory")]
        file: Option<PathBuf>,
        #[arg(long)]
        default_directory: Option<String>,
    },
    /// Restore automatic launch detection and clear the configured default directory.
    Reset { agent: String },
}

fn safe_target(target: &LaunchTarget) -> serde_json::Value {
    match target {
        LaunchTarget::App { path, args, env, new_instance } => json!({
            "kind": "app", "path": safe_path(path), "argument_count": args.len(),
            "environment_names": env.keys().collect::<Vec<_>>(), "new_instance": new_instance,
        }),
        LaunchTarget::Cli { command, args, env } => json!({
            "kind": "cli", "command": safe_path(command), "argument_count": args.len(),
            "environment_names": env.keys().collect::<Vec<_>>(),
        }),
        LaunchTarget::Web { url } => json!({"kind": "web", "url": safe_url(url)}),
    }
}

fn safe_launch(info: &LaunchInfo) -> serde_json::Value {
    json!({"agent_id": info.agent_id, "name": info.name, "kind": info.kind,
        "supported": info.supported, "available": info.available, "install_url": info.install_url,
        "version": agent_launch::runtime_version(&info.agent_id).ok().flatten(),
        "directory": info.directory.as_deref().map(safe_path),
        "default_directory": info.default_directory.as_deref().map(safe_path),
        "directory_exists": info.directory_exists,
        "configured_target": info.configured_target.as_ref().map(safe_target),
        "resolved_target": info.resolved_target.as_ref().map(safe_target)})
}

fn launch_settings(cli: &Cli, command: &LaunchCommand) -> Result<CommandOutput, CliError> {
    if let LaunchCommand::Show { agent } = command {
        cli.reject_mutation_options()?;
        return query_output("agent.launch.show", safe_launch(&agent_launch::info(agent).map_err(CliError::from_legacy)?));
    }
    let (agent, target, directory) = match command {
        LaunchCommand::Configure { agent, file, default_directory } => {
            let target = if let Some(file) = file {
                Some(serde_json::from_value::<LaunchTarget>(json_file(file)?)
                    .map_err(|error| CliError::private("invalid_launch_target", error.to_string()))?)
            } else { agent_launch::info(agent).map_err(CliError::from_legacy)?.configured_target };
            (agent, target, default_directory.clone())
        }
        LaunchCommand::Reset { agent } => (agent, None, Some(String::new())),
        LaunchCommand::Show { .. } => unreachable!(),
    };
    execute_direct_mutation("agent.launch.configure", cli.mutation_options(),
        json!({"agent_id": agent, "target": target.as_ref().map(safe_target), "default_directory": directory.as_deref().map(safe_path)}),
        &format!("Update shared launch settings for {agent}. Environment values are never printed."), false,
        || agent_launch::configure_with_directory(agent, target, directory).map(|_| ()).map_err(CliError::from_legacy))
}

pub fn dispatch(cli: &Cli, command: &AgentManagementCommand) -> Result<CommandOutput, CliError> {
    let options = cli.mutation_options();
    if let AgentManagementCommand::Launch { command: LaunchCommand::Show { .. } } = command {
        cli.reject_mutation_options()?;
    } else { options.validate()?; }
    match command {
        AgentManagementCommand::Run { agent, directory } => {
            let info = agent_launch::info(agent).map_err(CliError::from_legacy)?;
            if !info.available { return Err(CliError::new("agent_launch_unavailable", "install the Agent or configure its launcher first")); }
            let mut directory_saved = true;
            let mut output = execute_direct_mutation("agent.run", options,
                json!({"agent_id": agent, "kind": info.kind, "directory": directory.as_deref().or(info.directory.as_deref()).map(safe_path)}),
                &format!("Open {} using its shared Arca launcher.", info.name), false,
                || {
                    directory_saved = agent_launch::launch(agent, directory.clone()).map_err(CliError::from_legacy)?.directory_saved;
                    Ok(())
                })?;
            if output.changed {
                output.data["directory_saved"] = json!(directory_saved);
                output.human = if directory_saved { "Agent launch request sent." }
                    else { "Agent launch request sent, but its working directory could not be remembered." }.into();
            }
            Ok(output)
        }
        AgentManagementCommand::Launch { command } => launch_settings(cli, command),
        AgentManagementCommand::Save { id, file, replace } => {
            let definition: AgentDefinition = serde_json::from_value(json_file(file)?)
                .map_err(|error| CliError::private("invalid_agent_definition", error.to_string()))?;
            execute_direct_mutation("agent.save", options,
                json!({"id": id, "name": definition.name, "replace": replace}),
                &format!("Save Agent {} ({id}); replace existing: {replace}. Core validates its global capability contract.", definition.name.as_deref().unwrap_or(id)),
                false, || agents::put(id.clone(), definition, *replace).map_err(CliError::from_legacy))
        }
        AgentManagementCommand::Configure { agent, file } => {
            let patch: AgentConfigurationPatch = serde_json::from_value(json_file(file)?)
                .map_err(|error| CliError::private("invalid_agent_configuration", error.to_string()))?;
            let plan = ArcaCore::plan(PlanOperationRequest::UpdateAgentCapabilities(PlanUpdateAgentCapabilitiesRequest {
                agent_id: agent.clone(), patch,
            })).map_err(CliError::from_core)?;
            execute_operation("agent.configure", plan, options)
        }
    }
}
