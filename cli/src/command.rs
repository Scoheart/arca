use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use clap::{Parser, Subcommand, ValueEnum};
use arca_core::application::assets::{
    AgentConsumptionSelection, AssetRef, CentralAssetDraft, ConvergenceAction, McpAdoptionStatus,
    ModelAdoptionStatus, PlanClearAgentMcpRequest, PlanClearAgentModelsRequest, PlanConvergeConsumptionRequest,
    PlanDeleteCentralAssetRequest, PlanEnsureAgentConsumptionRequest,
    PlanRemoveAgentConsumptionRequest, PlanSetActiveModelRequest, PlanSetAgentConsumptionRequest,
    PlanSetMcpEnabledRequest, PlanSetModelEnabledRequest, PlanSetSkillEnabledRequest,
    PlanUpdateCentralAssetRequest,
};
use arca_core::application::mcp::catalog::read_registry;
use arca_core::application::mcp::operations as mcp_operations;
use arca_core::application::operations::PlanOperationRequest;
use arca_core::application::skills::{InventoryState, SkillLocation};
use arca_core::application::ArcaCore;
use arca_core::domain::types::{
    HttpConfig, RegistryConfig, RegistryEntry, RegistryOrigin, StdioConfig,
};
use serde_json::{json, Value};

use crate::output::{CliError, CommandOutput, Palette};
use crate::projection::{
    safe_agent_view, safe_consumption_inventory, safe_consumption_view, safe_model_candidate,
    safe_path, safe_skill_inventory, safe_skill_item, safe_url,
};
use crate::review::{execute_direct_mutation, execute_operation, MutationOptions};

#[derive(Debug, Parser)]
#[command(
    name = "arca",
    version,
    about = "Arca — central MCP, Model, and Skill assets for AI Agents",
    color = clap::ColorChoice::Never
)]
pub struct Cli {
    /// Emit one stable JSON envelope. Mutations also require --yes or --dry-run.
    #[arg(long, global = true)]
    pub json: bool,
    /// Confirm a reviewed mutation without prompting.
    #[arg(long, global = true)]
    pub yes: bool,
    /// Plan and review a mutation, then cancel it without writing.
    #[arg(long, global = true)]
    pub dry_run: bool,
    /// Explicitly accept the risk findings of the reviewed Skill plan.
    #[arg(long, global = true)]
    pub accept_risk: bool,
    /// Disable ANSI styling in human-readable output.
    #[arg(long, global = true)]
    pub no_color: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    pub fn mutation_options(&self) -> MutationOptions {
        MutationOptions {
            json: self.json,
            yes: self.yes,
            dry_run: self.dry_run,
            accept_risk: self.accept_risk,
            no_color: self.no_color || self.json,
        }
    }

    pub(crate) fn reject_mutation_options(&self) -> Result<(), CliError> {
        if self.yes || self.dry_run || self.accept_risk {
            return Err(CliError::new(
                "option_not_applicable",
                "--yes, --dry-run and --accept-risk are only valid for mutation commands",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect local conversation traces through Core's redacted readers.
    Trace {
        #[command(subcommand)]
        command: crate::diagnostics::TraceCommand,
    },
    /// Discover processes, record scoped traffic, or inspect saved redacted records.
    Capture {
        #[command(subcommand)]
        command: crate::diagnostics::CaptureCommand,
    },
    /// Query several Agents/capabilities in one scoped read. An empty scope means all.
    Status {
        #[arg(long = "agent", value_parser = parse_identity)]
        agents: Vec<String>,
        #[arg(long, value_enum)]
        capability: Option<AssetDomain>,
    },
    /// Use typed operations, including a plan/commit session for automation.
    Operation {
        #[command(subcommand)]
        command: crate::operation_session::OperationCommand,
    },
    /// Inspect or change preferences shared with Arca Desktop.
    Settings {
        #[command(subcommand)]
        command: crate::preferences::SettingsCommand,
    },
    /// Manage central MCP assets and their Agent relationships.
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Manage central Models, Providers, credentials, and Agent relationships.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Install, update, repair, and assign central Skills.
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    /// Inspect and enable or disable configured Agents.
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Inspect or configure the shared Arca network proxy.
    Network {
        #[command(subcommand)]
        command: crate::network_management::NetworkCommand,
    },
    /// Discover external configurations without changing them.
    Discover {
        #[arg(value_enum)]
        domain: Option<AssetDomain>,
    },
    /// Show the complete revisioned workspace projection.
    Workspace,
    /// Upgrade a standalone CLI to the latest Stable release.
    Upgrade,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum AssetDomain {
    Mcp,
    Model,
    Skill,
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// Customize MCP icons shared with Desktop.
    Icon {
        #[command(subcommand)]
        command: crate::mcp_preferences::IconCommand,
    },
    /// Enable every assigned MCP for one Agent in one reviewed operation.
    EnableAll { #[arg(long)] agent: String },
    /// Disable every assigned MCP for one Agent in one reviewed operation.
    DisableAll { #[arg(long)] agent: String },
    /// Manage subscribed, local and built-in MCP catalogs.
    Source {
        #[command(subcommand)]
        command: crate::source_management::SourceCommand,
    },
    /// Create or edit a central MCP from a RegistryEntry JSON document.
    Save {
        #[arg(long)]
        file: PathBuf,
        /// Exact existing name::transport key to edit. Omit to create.
        #[arg(long, value_parser = parse_mcp_key)]
        key: Option<String>,
    },
    /// List every MCP asset in the central catalog.
    List,
    /// Show one central MCP asset by its exact name::transport key.
    Show {
        #[arg(value_parser = parse_mcp_key)]
        key: String,
    },
    /// Show desired, observed, enabled, and drift state for MCP relationships.
    Status {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Add central MCP assets to one Agent without replacing its other assignments.
    Assign {
        #[arg(required = true, num_args = 1.., value_parser = parse_mcp_key)]
        keys: Vec<String>,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Remove MCP assignments from one Agent without deleting central assets.
    Unassign {
        #[arg(num_args = 1.., required_unless_present = "all", conflicts_with = "all", value_parser = parse_mcp_key)]
        keys: Vec<String>,
        /// Clear every MCP from this Agent, including external and disabled entries.
        #[arg(long)]
        all: bool,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Enable an assigned MCP for one Agent without changing its assignment.
    Enable {
        #[arg(value_parser = parse_mcp_key)]
        key: String,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Disable an assigned MCP for one Agent while retaining its assignment.
    Disable {
        #[arg(value_parser = parse_mcp_key)]
        key: String,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Converge one observed MCP relationship: adopt, restore, or detach.
    Converge {
        #[arg(value_parser = parse_mcp_key)]
        key: String,
        #[arg(long, required = true)]
        agent: String,
        #[arg(value_enum)]
        action: ConvergenceActionArg,
    },
    /// Add one manual MCP. stdio requires --command; http requires --url.
    Add {
        #[arg(value_parser = parse_mcp_key)]
        key: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long = "arg", allow_hyphen_values = true)]
        arguments: Vec<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "http")]
        http_type: String,
    },
    /// Delete one central MCP asset and clean up its managed consumers.
    Delete {
        #[arg(value_parser = parse_mcp_key)]
        key: String,
    },
    /// Export the effective MCP catalog, including its full configuration values.
    Export {
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ModelCommand {
    #[command(flatten)]
    Manage(crate::model_management::ModelManagementCommand),
    /// List every central Model Profile.
    List,
    /// Show one central Model Profile by its exact profile ID.
    Show {
        #[arg(value_parser = parse_identity)]
        profile_id: String,
    },
    /// Show desired, observed, enabled, and current state for Model relationships.
    Status {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Add Model Profiles to one Agent without replacing other Profiles by default.
    Assign {
        #[arg(required = true, num_args = 1.., value_parser = parse_identity)]
        profile_ids: Vec<String>,
        #[arg(long, required = true)]
        agent: String,
        /// Replace the Agent's complete Model selection instead of adding.
        #[arg(long)]
        replace: bool,
    },
    /// Remove Model Profile assignments without deleting central Profiles.
    Unassign {
        #[arg(num_args = 1.., required_unless_present = "all", conflicts_with = "all", value_parser = parse_identity)]
        profile_ids: Vec<String>,
        /// Clear this Agent's complete Model source, including external native Models.
        #[arg(long)]
        all: bool,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Enable an assigned Model Profile without changing its assignment.
    Enable {
        #[arg(value_parser = parse_identity)]
        profile_id: String,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Disable an assigned Model Profile while retaining its assignment.
    Disable {
        #[arg(value_parser = parse_identity)]
        profile_id: String,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Converge one observed Model relationship: adopt, restore, or detach.
    Converge {
        #[arg(value_parser = parse_identity)]
        profile_id: String,
        #[arg(long, required = true)]
        agent: String,
        #[arg(value_enum)]
        action: ConvergenceActionArg,
    },
    /// Select the current Model Profile for an Agent.
    Use {
        #[arg(value_parser = parse_identity)]
        profile_id: String,
        #[arg(long, required = true)]
        agent: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    #[command(flatten)]
    Manage(crate::skill_management::SkillManagementCommand),
    /// List every Skill in the central library.
    List,
    /// Show one central Skill by its exact name.
    Show {
        #[arg(value_parser = parse_identity)]
        name: String,
    },
    /// Show desired, observed, enabled, shared-target, and drift state for Skills.
    Status {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Add central Skills to one Agent without replacing its other assignments.
    Assign {
        #[arg(required = true, num_args = 1.., value_parser = parse_identity)]
        names: Vec<String>,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Remove Skill assignments from one Agent without deleting central Skills.
    Unassign {
        #[arg(required = true, num_args = 1.., value_parser = parse_identity)]
        names: Vec<String>,
        #[arg(long, required = true)]
        agent: String,
    },
    /// Enable an assigned Skill without changing its assignment.
    Enable {
        #[arg(value_parser = parse_identity)]
        name: String,
        #[arg(long, required = true)]
        agent: String,
        /// Physical target ID from status, required when the Skill has multiple copies.
        #[arg(long, value_parser = parse_identity)]
        target: Option<String>,
    },
    /// Disable an assigned Skill while retaining its assignment.
    Disable {
        #[arg(value_parser = parse_identity)]
        name: String,
        #[arg(long, required = true)]
        agent: String,
        /// Physical target ID from status, required when the Skill has multiple copies.
        #[arg(long, value_parser = parse_identity)]
        target: Option<String>,
    },
    /// Converge one observed Skill relationship: adopt, restore, or detach.
    Converge {
        #[arg(value_parser = parse_identity)]
        name: String,
        #[arg(long, required = true)]
        agent: String,
        #[arg(value_enum)]
        action: ConvergenceActionArg,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    #[command(flatten)]
    Manage(crate::agent_management::AgentManagementCommand),
    /// List configured Agents and their supported resource capabilities.
    List,
    /// Enable one configured Agent for Arca operations.
    Enable {
        #[arg(value_parser = parse_identity)]
        agent: String,
    },
    /// Disable one configured Agent for Arca operations.
    Disable {
        #[arg(value_parser = parse_identity)]
        agent: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ConvergenceActionArg {
    Adopt,
    Restore,
    Detach,
}

impl From<ConvergenceActionArg> for ConvergenceAction {
    fn from(value: ConvergenceActionArg) -> Self {
        match value {
            ConvergenceActionArg::Adopt => Self::AdoptObserved,
            ConvergenceActionArg::Restore => Self::RestoreDesired,
            ConvergenceActionArg::Detach => Self::Detach,
        }
    }
}

fn parse_mcp_key(raw: &str) -> Result<String, String> {
    let asset = AssetRef::Mcp {
        key: raw.to_string(),
    };
    asset.validate().map_err(|error| error.to_string())?;
    Ok(raw.to_string())
}

pub(crate) fn parse_identity(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() {
        Err("identity must not be empty".into())
    } else {
        Ok(raw.to_string())
    }
}

pub fn dispatch(cli: &Cli) -> Result<CommandOutput, CliError> {
    let command = cli.command.as_ref().ok_or_else(|| {
        CliError::new(
            "command_required",
            "a command is required when global output or mutation flags are present",
        )
    })?;
    match command {
        Command::Trace { command } => crate::diagnostics::trace(cli, command),
        Command::Capture { command } => crate::diagnostics::captured(cli, command),
        Command::Status { agents, capability } => {
            cli.reject_mutation_options()?;
            let inventory = arca_core::application::assets::list_inventory_scoped(
                arca_core::application::assets::InventoryScope {
                    capability: capability.map(core_capability),
                    agent_ids: agents.iter().cloned().collect(),
                },
            ).map_err(CliError::from_legacy)?;
            crate::output::query_output("status", safe_consumption_inventory(&inventory))
        }
        Command::Operation { command } => crate::operation_session::dispatch(cli, command),
        Command::Settings { command } => crate::preferences::dispatch(cli, command),
        Command::Mcp { command } => dispatch_mcp(cli, command),
        Command::Model { command } => dispatch_model(cli, command),
        Command::Skill { command } => dispatch_skill(cli, command),
        Command::Agent { command } => dispatch_agent(cli, command),
        Command::Network { command } => crate::network_management::dispatch(cli, command),
        Command::Discover { domain } => {
            cli.reject_mutation_options()?;
            discover(*domain, Palette::new(cli.no_color || cli.json))
        }
        Command::Workspace => {
            cli.reject_mutation_options()?;
            workspace(Palette::new(cli.no_color || cli.json))
        }
        Command::Upgrade => upgrade(cli.mutation_options()),
    }
}

fn dispatch_mcp(cli: &Cli, command: &McpCommand) -> Result<CommandOutput, CliError> {
    let palette = Palette::new(cli.no_color || cli.json);
    match command {
        McpCommand::Icon { command } => crate::mcp_preferences::dispatch(cli, command),
        McpCommand::EnableAll { agent } | McpCommand::DisableAll { agent } => {
            let enabled = matches!(command, McpCommand::EnableAll { .. });
            let options = cli.mutation_options();
            options.validate()?;
            let plan = ArcaCore::plan(PlanOperationRequest::SetAllMcpEnabled(
                arca_core::domain::assets::PlanSetAllMcpEnabledRequest { agent_id: agent.clone(), enabled },
            )).map_err(CliError::from_core)?;
            execute_operation(if enabled { "mcp.enable-all" } else { "mcp.disable-all" }, plan, options)
        }
        McpCommand::Source { command } => crate::source_management::dispatch(cli, command),
        McpCommand::Save { file, key } => {
            let options = cli.mutation_options();
            options.validate()?;
            let entry: RegistryEntry = serde_json::from_value(crate::input::json_file(file)?)
                .map_err(|error| CliError::private("invalid_mcp_entry", error.to_string()))?;
            let plan = ArcaCore::plan(PlanOperationRequest::UpdateCentralAsset(PlanUpdateCentralAssetRequest {
                draft: CentralAssetDraft::Mcp { existing_key: key.clone(), entry: Box::new(entry) },
            })).map_err(CliError::from_core)?;
            execute_operation("mcp.save", plan, options)
        }
        McpCommand::List => {
            cli.reject_mutation_options()?;
            mcp_list(palette)
        }
        McpCommand::Show { key } => {
            cli.reject_mutation_options()?;
            mcp_show(key, palette)
        }
        McpCommand::Status { agent } => {
            cli.reject_mutation_options()?;
            status(AssetDomain::Mcp, agent.as_deref(), palette)
        }
        McpCommand::Assign { keys, agent } => {
            assign(AssetDomain::Mcp, keys, agent, false, cli.mutation_options())
        }
        McpCommand::Unassign { keys, all, agent } => {
            unassign(AssetDomain::Mcp, keys, *all, agent, cli.mutation_options())
        }
        McpCommand::Enable { key, agent } => {
            set_enabled(AssetDomain::Mcp, key, agent, true, None, cli.mutation_options())
        }
        McpCommand::Disable { key, agent } => {
            set_enabled(AssetDomain::Mcp, key, agent, false, None, cli.mutation_options())
        }
        McpCommand::Add {
            key,
            description,
            tags,
            command,
            arguments,
            cwd,
            url,
            http_type,
        } => mcp_add(
            key,
            description,
            tags,
            command.as_deref(),
            arguments,
            cwd.as_deref(),
            url.as_deref(),
            http_type,
            cli.mutation_options(),
        ),
        McpCommand::Delete { key } => mcp_delete(key, cli.mutation_options()),
        McpCommand::Export { out: None } => {
            cli.reject_mutation_options()?;
            mcp_export_stdout()
        }
        McpCommand::Export { out: Some(path) } => mcp_export_file(path, cli.mutation_options()),
        McpCommand::Converge { key, agent, action } => converge(
            AssetRef::Mcp { key: key.clone() },
            agent,
            (*action).into(),
            cli.mutation_options(),
        ),
    }
}

fn dispatch_model(cli: &Cli, command: &ModelCommand) -> Result<CommandOutput, CliError> {
    let palette = Palette::new(cli.no_color || cli.json);
    match command {
        ModelCommand::Manage(command) => crate::model_management::dispatch(cli, command),
        ModelCommand::List => {
            cli.reject_mutation_options()?;
            model_list(palette)
        }
        ModelCommand::Show { profile_id } => {
            cli.reject_mutation_options()?;
            model_show(profile_id, palette)
        }
        ModelCommand::Status { agent } => {
            cli.reject_mutation_options()?;
            status(AssetDomain::Model, agent.as_deref(), palette)
        }
        ModelCommand::Assign {
            profile_ids,
            agent,
            replace,
        } => assign(
            AssetDomain::Model,
            profile_ids,
            agent,
            *replace,
            cli.mutation_options(),
        ),
        ModelCommand::Unassign { profile_ids, all, agent } => unassign(
            AssetDomain::Model,
            profile_ids,
            *all,
            agent,
            cli.mutation_options(),
        ),
        ModelCommand::Enable { profile_id, agent } => set_enabled(
            AssetDomain::Model,
            profile_id,
            agent,
            true,
            None,
            cli.mutation_options(),
        ),
        ModelCommand::Disable { profile_id, agent } => set_enabled(
            AssetDomain::Model,
            profile_id,
            agent,
            false,
            None,
            cli.mutation_options(),
        ),
        ModelCommand::Use { profile_id, agent } => {
            model_use(profile_id, agent, cli.mutation_options())
        }
        ModelCommand::Converge {
            profile_id,
            agent,
            action,
        } => converge(
            AssetRef::Model {
                profile_id: profile_id.clone(),
            },
            agent,
            (*action).into(),
            cli.mutation_options(),
        ),
    }
}

fn dispatch_skill(cli: &Cli, command: &SkillCommand) -> Result<CommandOutput, CliError> {
    let palette = Palette::new(cli.no_color || cli.json);
    match command {
        SkillCommand::Manage(command) => crate::skill_management::dispatch(cli, command),
        SkillCommand::List => {
            cli.reject_mutation_options()?;
            skill_list(palette)
        }
        SkillCommand::Show { name } => {
            cli.reject_mutation_options()?;
            skill_show(name, palette)
        }
        SkillCommand::Status { agent } => {
            cli.reject_mutation_options()?;
            status(AssetDomain::Skill, agent.as_deref(), palette)
        }
        SkillCommand::Assign { names, agent } => assign(
            AssetDomain::Skill,
            names,
            agent,
            false,
            cli.mutation_options(),
        ),
        SkillCommand::Unassign { names, agent } => unassign(
            AssetDomain::Skill,
            names,
            false,
            agent,
            cli.mutation_options(),
        ),
        SkillCommand::Enable { name, agent, target } => set_enabled(
            AssetDomain::Skill,
            name,
            agent,
            true,
            target.as_deref(),
            cli.mutation_options(),
        ),
        SkillCommand::Disable { name, agent, target } => set_enabled(
            AssetDomain::Skill,
            name,
            agent,
            false,
            target.as_deref(),
            cli.mutation_options(),
        ),
        SkillCommand::Converge {
            name,
            agent,
            action,
        } => converge(
            AssetRef::Skill { name: name.clone() },
            agent,
            (*action).into(),
            cli.mutation_options(),
        ),
    }
}

fn dispatch_agent(cli: &Cli, command: &AgentCommand) -> Result<CommandOutput, CliError> {
    match command {
        AgentCommand::Manage(command) => crate::agent_management::dispatch(cli, command),
        AgentCommand::List => {
            cli.reject_mutation_options()?;
            agent_list(Palette::new(cli.no_color || cli.json))
        }
        AgentCommand::Enable { agent } => agent_set(agent, true, cli.mutation_options()),
        AgentCommand::Disable { agent } => agent_set(agent, false, cli.mutation_options()),
    }
}

fn mcp_list(palette: Palette) -> Result<CommandOutput, CliError> {
    let mut entries = read_registry();
    entries.sort_by_key(RegistryEntry::key);
    let data = entries.iter().map(redacted_mcp_entry).collect::<Vec<_>>();
    let human = if entries.is_empty() {
        palette.dim("No central MCP assets.")
    } else {
        let mut lines = vec![palette.bold(&format!("{} central MCP assets", entries.len()))];
        for entry in &entries {
            lines.push(format!("  {}", palette.green(&entry.key())));
            if !entry.description.is_empty() {
                lines.push(format!("    {}", palette.dim(&entry.description)));
            }
        }
        lines.join("\n")
    };
    Ok(CommandOutput::new("mcp.list", false, json!(data), human))
}

fn mcp_show(key: &str, palette: Palette) -> Result<CommandOutput, CliError> {
    let entry = central_mcp(key)?;
    let human = format!(
        "{}\n  transport: {}\n  description: {}",
        palette.bold(key),
        entry.transport(),
        if entry.description.is_empty() {
            palette.dim("none")
        } else {
            entry.description.clone()
        }
    );
    Ok(CommandOutput::new(
        "mcp.show",
        false,
        redacted_mcp_entry(&entry),
        human,
    ))
}

fn model_list(palette: Palette) -> Result<CommandOutput, CliError> {
    let mut profiles = arca_core::application::models::list_profiles();
    profiles.sort_by(|left, right| left.profile.id.cmp(&right.profile.id));
    let human = if profiles.is_empty() {
        palette.dim("No central Model Profiles.")
    } else {
        let mut lines = vec![palette.bold(&format!("{} central Model Profiles", profiles.len()))];
        for profile in &profiles {
            lines.push(format!(
                "  {}  {}",
                palette.green(&profile.profile.id),
                profile.profile.model
            ));
        }
        lines.join("\n")
    };
    Ok(CommandOutput::new(
        "model.list",
        false,
        json!(profiles
            .iter()
            .map(redacted_model_profile)
            .collect::<Vec<_>>()),
        human,
    ))
}

fn model_show(profile_id: &str, palette: Palette) -> Result<CommandOutput, CliError> {
    let profile = central_model(profile_id)?;
    let human = format!(
        "{}\n  name: {}\n  model: {}\n  credential: {}",
        palette.bold(profile_id),
        profile.profile.name,
        profile.profile.model,
        if profile.credential_saved {
            "saved"
        } else {
            "not saved"
        }
    );
    Ok(CommandOutput::new(
        "model.show",
        false,
        redacted_model_profile(&profile),
        human,
    ))
}

fn skill_list(palette: Palette) -> Result<CommandOutput, CliError> {
    let inventory = skill_inventory()?;
    let mut items = inventory
        .items
        .into_iter()
        .filter(|item| matches!(item.location, SkillLocation::Central))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.name.cmp(&right.name));
    let human = if items.is_empty() {
        palette.dim("No central Skills.")
    } else {
        let mut lines = vec![palette.bold(&format!("{} central Skills", items.len()))];
        for item in &items {
            lines.push(format!("  {}", palette.green(&item.name)));
            if !item.description.is_empty() {
                lines.push(format!("    {}", palette.dim(&item.description)));
            }
        }
        lines.join("\n")
    };
    Ok(CommandOutput::new(
        "skill.list",
        false,
        json!(items.iter().map(safe_skill_item).collect::<Vec<_>>()),
        human,
    ))
}

fn skill_show(name: &str, palette: Palette) -> Result<CommandOutput, CliError> {
    let item = central_skill(name)?;
    let human = format!(
        "{}\n  description: {}\n  states: {}",
        palette.bold(name),
        if item.description.is_empty() {
            palette.dim("none")
        } else {
            item.description.clone()
        },
        item.states
            .iter()
            .map(|state| format!("{state:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(CommandOutput::new(
        "skill.show",
        false,
        safe_skill_item(&item),
        human,
    ))
}

fn status(
    domain: AssetDomain,
    agent: Option<&str>,
    palette: Palette,
) -> Result<CommandOutput, CliError> {
    if let Some(agent) = agent {
        require_agent_capability(agent, domain)?;
    }
    let inventory = arca_core::application::assets::list_inventory_scoped(
        arca_core::application::assets::InventoryScope {
            capability: Some(core_capability(domain)),
            agent_ids: agent.into_iter().map(str::to_owned).collect(),
        },
    ).map_err(CliError::from_legacy)?;
    let revision = inventory.revision.clone();
    let observed_at = inventory.observed_at.clone();
    let host_mcps = inventory.host_mcps.clone();
    let native_plugins = inventory.native_plugins.clone();
    let capability_errors = inventory
        .capability_errors
        .iter()
        .filter(|diagnostic| match domain {
            AssetDomain::Mcp => {
                diagnostic.capability == arca_core::application::assets::AssetCapability::Mcp
            }
            AssetDomain::Model => {
                diagnostic.capability == arca_core::application::assets::AssetCapability::Model
            }
            AssetDomain::Skill => {
                diagnostic.capability == arca_core::application::assets::AssetCapability::Skill
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    let (managed, external, target_incidents) = status_projection(inventory, domain, agent);
    let command = match domain {
        AssetDomain::Mcp => "mcp.status",
        AssetDomain::Model => "model.status",
        AssetDomain::Skill => "skill.status",
    };
    let mut lines = if managed.is_empty() && external.is_empty() && host_mcps.is_empty() && native_plugins.is_empty() {
        vec![
            palette.dim(&format!("Observed at {observed_at} · revision {revision}")),
            palette.dim("No matching relationships or external observations."),
        ]
    } else {
        let mut lines = vec![
            palette.dim(&format!("Observed at {observed_at} · revision {revision}")),
            palette.bold("Managed relationships"),
        ];
        for row in &managed {
            lines.push(format_status_row(row));
        }
        if !external.is_empty() {
            lines.push(String::new());
            lines.push(palette.bold("External observations"));
            for row in &external {
                lines.push(format_status_row(row));
            }
        }
        lines
    };
    if !host_mcps.is_empty() {
        lines.push(String::new());
        lines.push(palette.bold("Host MCPs (read-only; enablement and connection managed by the host)"));
        for item in &host_mcps {
            lines.push(format!("{} · {} · {} · {:?}", item.agent_id, item.server_id, item.provider, item.presence));
        }
    }
    if !native_plugins.is_empty() {
        lines.push(String::new());
        lines.push(palette.bold("Host plugins (read-only; runtime state is not inferred)"));
        for plugin in &native_plugins {
            lines.push(format!("{} · {} · {} · {} · {:?} · {} components", plugin.agent_id, plugin.name, plugin.version, plugin.revision, plugin.presence, plugin.components.len()));
        }
    }
    for incident in &target_incidents {
        lines.push(String::new());
        lines.push(palette.yellow(&format!(
            "target warning: {} ({})",
            incident.code, incident.target_path
        )));
    }
    for diagnostic in &capability_errors {
        lines.push(String::new());
        lines.push(palette.yellow(&format!("capability warning: {}", diagnostic.code)));
    }
    let human = lines.join("\n");
    Ok(CommandOutput::new(
        command,
        false,
        json!({
            "revision": revision,
            "observed_at": observed_at,
            "host_mcps": host_mcps.iter().map(crate::projection::safe_host_mcp).collect::<Vec<_>>(),
            "native_plugins": native_plugins.iter().map(crate::projection::safe_native_plugin).collect::<Vec<_>>(),
            "managed": managed.iter().map(safe_consumption_view).collect::<Vec<_>>(),
            "external": external.iter().map(safe_consumption_view).collect::<Vec<_>>(),
            "capability_errors": capability_errors,
            "target_incidents": target_incidents.iter().map(crate::projection::safe_target_incident).collect::<Vec<_>>(),
        }),
        human,
    ))
}

fn core_capability(domain: AssetDomain) -> arca_core::application::assets::AssetCapability {
    use arca_core::application::assets::AssetCapability;
    match domain {
        AssetDomain::Mcp => AssetCapability::Mcp,
        AssetDomain::Model => AssetCapability::Model,
        AssetDomain::Skill => AssetCapability::Skill,
    }
}

fn status_projection(
    inventory: arca_core::application::assets::ConsumptionInventory,
    domain: AssetDomain,
    agent: Option<&str>,
) -> (
    Vec<arca_core::application::assets::ConsumptionView>,
    Vec<arca_core::application::assets::ConsumptionView>,
    Vec<arca_core::application::assets::TargetIncident>,
) {
    let capability = match domain {
        AssetDomain::Mcp => arca_core::application::assets::AssetCapability::Mcp,
        AssetDomain::Model => arca_core::application::assets::AssetCapability::Model,
        AssetDomain::Skill => arca_core::application::assets::AssetCapability::Skill,
    };
    let mut target_incidents = inventory
        .target_incidents
        .into_iter()
        .filter(|incident| incident.capability == capability)
        .filter(|incident| {
            agent.is_none_or(|agent| incident.affected_agent_ids.iter().any(|id| id == agent))
        })
        .collect::<Vec<_>>();
    let mut managed = inventory
        .consumptions
        .into_iter()
        .filter(|row| asset_matches_domain(&row.asset, domain))
        .filter(|row| agent.is_none_or(|agent| row.agent_id == agent))
        .collect::<Vec<_>>();
    let mut external = inventory
        .external
        .into_iter()
        .filter(|row| asset_matches_domain(&row.asset, domain))
        .filter(|row| agent.is_none_or(|agent| row.agent_id == agent))
        .collect::<Vec<_>>();
    managed
        .sort_by(|left, right| (&left.agent_id, &left.asset).cmp(&(&right.agent_id, &right.asset)));
    external
        .sort_by(|left, right| (&left.agent_id, &left.asset).cmp(&(&right.agent_id, &right.asset)));
    target_incidents.sort_by(|left, right| left.id.cmp(&right.id));
    (managed, external, target_incidents)
}

fn converge(
    asset: AssetRef,
    agent_id: &str,
    action: ConvergenceAction,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    options.validate()?;
    let inventory =
        arca_core::application::assets::list_inventory().map_err(CliError::from_legacy)?;
    let row = inventory
        .consumptions
        .iter()
        .chain(inventory.external.iter())
        .find(|row| row.agent_id == agent_id && row.asset == asset)
        .ok_or_else(|| {
            CliError::new(
                "observation_missing",
                "no current observation matches that Agent and asset identity; run status again",
            )
        })?;
    if !row.available_actions.contains(&action) {
        return Err(CliError::new(
            "convergence_action_unavailable",
            format!("available actions: {:?}", row.available_actions),
        ));
    }
    let plan = ArcaCore::plan(PlanOperationRequest::ConvergeConsumption(
        PlanConvergeConsumptionRequest {
            agent_id: agent_id.to_string(),
            asset,
            action,
            observed_revision: inventory.revision,
        },
    ))
    .map_err(CliError::from_core)?;
    execute_operation("converge", plan, options)
}

fn assign(
    domain: AssetDomain,
    identities: &[String],
    agent: &str,
    replace: bool,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    options.validate()?;
    require_enabled_agent_capability(agent, domain)?;
    require_central_assets(domain, identities)?;
    let selection = selection(domain, identities.to_vec());
    let request = if domain == AssetDomain::Model && replace {
        PlanOperationRequest::SetAgentConsumption(PlanSetAgentConsumptionRequest {
            agent_id: agent.to_string(),
            selection,
        })
    } else {
        PlanOperationRequest::EnsureAgentConsumption(PlanEnsureAgentConsumptionRequest {
            agent_id: agent.to_string(),
            selection,
        })
    };
    let plan = ArcaCore::plan(request).map_err(CliError::from_core)?;
    execute_operation(
        domain_command(domain, "assign"),
        plan,
        options,
    )
}

fn unassign(
    domain: AssetDomain,
    identities: &[String],
    clear_all: bool,
    agent: &str,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    options.validate()?;
    require_enabled_agent_capability(agent, domain)?;
    let request = if domain == AssetDomain::Mcp && clear_all {
        PlanOperationRequest::ClearAgentMcp(PlanClearAgentMcpRequest {
            agent_id: agent.to_string(),
        })
    } else if domain == AssetDomain::Model && clear_all {
        PlanOperationRequest::ClearAgentModels(PlanClearAgentModelsRequest {
            agent_id: agent.to_string(),
        })
    } else {
        PlanOperationRequest::RemoveAgentConsumption(PlanRemoveAgentConsumptionRequest {
            agent_id: agent.to_string(),
            selection: selection(domain, identities.to_vec()),
        })
    };
    let plan = ArcaCore::plan(request).map_err(CliError::from_core)?;
    execute_operation(
        domain_command(domain, "unassign"),
        plan,
        options,
    )
}

fn set_enabled(
    domain: AssetDomain,
    identity: &str,
    agent: &str,
    enabled: bool,
    target_id: Option<&str>,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    options.validate()?;
    require_enabled_agent_capability(agent, domain)?;
    require_central_assets(domain, &[identity.to_string()])?;
    let command = domain_command(domain, if enabled { "enable" } else { "disable" });
    let request = match domain {
        AssetDomain::Mcp => PlanOperationRequest::SetMcpEnabled(PlanSetMcpEnabledRequest {
            agent_id: agent.to_string(),
            asset_key: identity.to_string(),
            enabled,
        }),
        AssetDomain::Model => PlanOperationRequest::SetModelEnabled(PlanSetModelEnabledRequest {
            agent_id: agent.to_string(),
            profile_id: identity.to_string(),
            enabled,
        }),
        AssetDomain::Skill => PlanOperationRequest::SetSkillEnabled(PlanSetSkillEnabledRequest {
            agent_id: agent.to_string(),
            name: identity.to_string(),
            enabled,
            target_id: target_id.map(str::to_string),
        }),
    };
    let plan = ArcaCore::plan(request).map_err(CliError::from_core)?;
    execute_operation(command, plan, options)
}

fn model_use(
    profile_id: &str,
    agent: &str,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    options.validate()?;
    require_enabled_agent_capability(agent, AssetDomain::Model)?;
    require_central_assets(AssetDomain::Model, &[profile_id.to_string()])?;
    let plan = ArcaCore::plan(PlanOperationRequest::SetActiveModel(
        PlanSetActiveModelRequest {
            agent_id: agent.to_string(),
            profile_id: profile_id.to_string(),
        },
    ))
    .map_err(CliError::from_core)?;
    execute_operation("model.use", plan, options)
}

#[allow(clippy::too_many_arguments)]
fn mcp_add(
    key: &str,
    description: &str,
    tags: &[String],
    command: Option<&str>,
    arguments: &[String],
    cwd: Option<&str>,
    url: Option<&str>,
    http_type: &str,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    options.validate()?;
    if read_registry().iter().any(|entry| entry.key() == key) {
        return Err(CliError::new(
            "asset_already_exists",
            format!("central MCP asset {key} already exists"),
        ));
    }
    let (name, transport) = key
        .rsplit_once("::")
        .expect("MCP key was validated by clap");
    let config = match transport {
        "stdio" => {
            if url.is_some() {
                return Err(CliError::new(
                    "option_not_applicable",
                    "--url is only valid for an http MCP",
                ));
            }
            let command = command
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    CliError::new("missing_stdio_command", "stdio MCP add requires --command")
                })?;
            RegistryConfig {
                stdio: Some(StdioConfig {
                    command: command.to_string(),
                    args: (!arguments.is_empty()).then(|| arguments.to_vec()),
                    env: None,
                    cwd: cwd.map(str::to_string),
                }),
                http: None,
            }
        }
        "http" => {
            if command.is_some() || !arguments.is_empty() || cwd.is_some() {
                return Err(CliError::new(
                    "option_not_applicable",
                    "--command, --arg, and --cwd are only valid for a stdio MCP",
                ));
            }
            let url = url
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| CliError::new("missing_http_url", "http MCP add requires --url"))?;
            RegistryConfig {
                stdio: None,
                http: Some(HttpConfig {
                    kind: http_type.to_string(),
                    url: url.to_string(),
                    headers: None,
                }),
            }
        }
        _ => unreachable!("MCP transport was validated by clap"),
    };
    let entry = RegistryEntry {
        name: name.to_string(),
        description: description.to_string(),
        tags: tags.to_vec(),
        config,
        origin: Some(RegistryOrigin {
            kind: "manual".into(),
            agent: None,
            scope: None,
            source: Some("manual".into()),
        }),
        repo: None,
    };
    let plan = ArcaCore::plan(PlanOperationRequest::UpdateCentralAsset(
        PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: None,
                entry: Box::new(entry),
            },
        },
    ))
    .map_err(CliError::from_core)?;
    execute_operation("mcp.add", plan, options)
}

fn mcp_delete(key: &str, options: MutationOptions) -> Result<CommandOutput, CliError> {
    options.validate()?;
    let entry = central_mcp(key)?;
    let source_id = entry
        .origin
        .as_ref()
        .and_then(|origin| origin.source.clone())
        .or_else(|| entry.origin.as_ref().map(|origin| origin.kind.clone()));
    let plan = ArcaCore::plan(PlanOperationRequest::DeleteCentralAsset(
        PlanDeleteCentralAssetRequest {
            asset: AssetRef::Mcp {
                key: key.to_string(),
            },
            source_id,
        },
    ))
    .map_err(CliError::from_core)?;
    execute_operation("mcp.delete", plan, options)
}

fn mcp_export_stdout() -> Result<CommandOutput, CliError> {
    let content = mcp_operations::export_effective().map_err(CliError::from_legacy)?;
    let catalog = serde_json::from_str::<Value>(&content).unwrap_or_else(|_| json!(content));
    Ok(CommandOutput::new(
        "mcp.export",
        false,
        json!({"catalog": catalog}),
        content,
    ))
}

fn mcp_export_file(path: &Path, options: MutationOptions) -> Result<CommandOutput, CliError> {
    options.validate()?;
    ensure_export_target_absent(path)?;
    let content = mcp_operations::export_effective().map_err(CliError::from_legacy)?;
    let safe_target = safe_path(&path.to_string_lossy());
    let summary = json!({
        "path": safe_target,
        "create_new": true,
        "permissions": "0600",
    });
    let review = format!(
        "Review change\n  create private MCP export: {}\n  overwrite: never",
        path.display()
    );
    let outcome = execute_direct_mutation("mcp.export", options, summary, &review, false, || {
        write_new_private_export(path, &content)
    })?;
    if outcome.changed {
        Ok(CommandOutput::new(
            "mcp.export",
            true,
            json!({"path": safe_target, "permissions": "0600"}),
            format!("Exported effective MCP catalog to {}", path.display()),
        ))
    } else {
        Ok(outcome)
    }
}

fn ensure_export_target_absent(path: &Path) -> Result<(), CliError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(CliError::new(
            "export_target_exists",
            "export target already exists; refusing to overwrite it",
        )
        .with_detail("path", safe_path(&path.to_string_lossy()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(
            CliError::private("export_target_unavailable", error.to_string())
                .with_detail("path", safe_path(&path.to_string_lossy())),
        ),
    }
}

fn write_new_private_export(path: &Path, content: &str) -> Result<(), CliError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(path).map_err(|error| {
        let code = if error.kind() == std::io::ErrorKind::AlreadyExists {
            "export_target_exists"
        } else {
            "export_write_failed"
        };
        CliError::private(code, error.to_string())
            .with_detail("path", safe_path(&path.to_string_lossy()))
    })?;
    if let Err(error) = file
        .write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
    {
        // The path may be renamed and replaced by another process after this
        // handle was opened. Blind pathname cleanup could then delete content
        // Arca never created. Leave the still-private partial file in place and
        // fail closed; create_new prevents a later export from overwriting it.
        return Err(CliError::private("export_write_failed", error.to_string())
            .with_detail("path", safe_path(&path.to_string_lossy())));
    }
    Ok(())
}

fn agent_list(palette: Palette) -> Result<CommandOutput, CliError> {
    let agents = arca_core::application::agents::list_capabilities().map_err(CliError::from_core)?;
    let human = if agents.is_empty() {
        palette.dim("No configured Agents.")
    } else {
        let mut lines = vec![palette.bold(&format!("{} Agents", agents.len()))];
        for agent in &agents {
            let mut capabilities = Vec::new();
            if agent.capabilities.mcp.is_some() {
                capabilities.push("MCP");
            }
            if agent.capabilities.model.is_some() {
                capabilities.push("Model");
            }
            if agent.capabilities.skill.is_some() {
                capabilities.push("Skill");
            }
            lines.push(format!(
                "  {}  [{}]  {}",
                agent.identity.id,
                if agent.identity.enabled {
                    palette.green("enabled")
                } else {
                    palette.dim("disabled")
                },
                capabilities.join(" · ")
            ));
        }
        lines.join("\n")
    };
    Ok(CommandOutput::new(
        "agent.list",
        false,
        json!(agents.iter().map(safe_agent_view).collect::<Vec<_>>()),
        human,
    ))
}

fn agent_set(
    agent: &str,
    enabled: bool,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    options.validate()?;
    let agents = arca_core::application::agents::load_agents();
    let definition = agents.get(agent).ok_or_else(|| {
        CliError::new(
            "unknown_agent",
            format!("unknown configured Agent: {agent}"),
        )
    })?;
    let command = if enabled {
        "agent.enable"
    } else {
        "agent.disable"
    };
    let summary = json!({"agent": agent, "enabled": enabled});
    let review = format!(
        "Review change\n  Agent: {agent}\n  enabled: {} -> {enabled}",
        definition.enabled
    );
    execute_direct_mutation(
        command,
        options,
        summary,
        &review,
        definition.enabled == enabled,
        || {
            arca_core::application::agents::set_enabled(agent, enabled)
                .map_err(CliError::from_legacy)
        },
    )
}

fn discover(domain: Option<AssetDomain>, palette: Palette) -> Result<CommandOutput, CliError> {
    let mcps = if domain.is_none() || domain == Some(AssetDomain::Mcp) {
        arca_core::application::assets::list_mcp_adoption_candidates()
            .map_err(CliError::from_legacy)?
    } else {
        Vec::new()
    };
    let models = if domain.is_none() || domain == Some(AssetDomain::Model) {
        arca_core::application::assets::list_model_adoption_candidates()
            .map_err(CliError::from_legacy)?
    } else {
        Vec::new()
    };
    let skills = if domain.is_none() || domain == Some(AssetDomain::Skill) {
        arca_core::application::skills::list_migration_candidates()
            .map_err(skill_error)?
            .into_iter()
            .filter(is_external_skill)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut lines = vec![palette.bold("Detected external configurations")];
    if domain.is_none() || domain == Some(AssetDomain::Mcp) {
        lines.push(format!("\nMCP ({})", mcps.len()));
        for candidate in &mcps {
            lines.push(format!(
                "  {}  --agent {}  [{}]",
                candidate.asset_key,
                candidate.agent_id,
                mcp_status_label(&candidate.status)
            ));
        }
    }
    if domain.is_none() || domain == Some(AssetDomain::Model) {
        lines.push(format!("\nModels ({})", models.len()));
        for candidate in &models {
            lines.push(format!(
                "  {}  {} · {}  [{}]",
                candidate.candidate_id,
                candidate.agent_id,
                candidate.model,
                model_status_label(&candidate.status)
            ));
        }
    }
    if domain.is_none() || domain == Some(AssetDomain::Skill) {
        lines.push(format!("\nSkills ({})", skills.len()));
        for candidate in &skills {
            lines.push(format!("  {}  {}", candidate.identity, candidate.name));
        }
    }
    Ok(CommandOutput::new(
        "discover",
        false,
        json!({
            "mcp": mcps,
            "model": models.iter().map(safe_model_candidate).collect::<Vec<_>>(),
            "skill": skills.iter().map(safe_skill_item).collect::<Vec<_>>(),
        }),
        lines.join("\n"),
    ))
}

fn workspace(palette: Palette) -> Result<CommandOutput, CliError> {
    let snapshot = ArcaCore::snapshot().map_err(CliError::from_core)?;
    let central_skills = snapshot
        .assets
        .skills
        .items
        .iter()
        .filter(|item| matches!(item.location, SkillLocation::Central))
        .count();
    let human = format!(
        "{}\n  revision: {}\n  Agents: {}\n  MCPs: {}\n  Models: {}\n  Skills: {}\n  managed relationships: {}\n  external observations: {}",
        palette.bold("Arca workspace"),
        snapshot.revision,
        snapshot.agents.len(),
        snapshot.assets.mcp.len(),
        snapshot.assets.models.len(),
        central_skills,
        snapshot.relationships.consumptions.len(),
        snapshot.relationships.external.len(),
    );
    Ok(CommandOutput::new(
        "workspace",
        false,
        json!({
            "revision": snapshot.revision,
            "agents": snapshot.agents.iter().map(safe_agent_view).collect::<Vec<_>>(),
            "assets": {
                "mcp": snapshot.assets.mcp.iter().map(redacted_mcp_entry).collect::<Vec<_>>(),
                "models": snapshot.assets.models.iter().map(redacted_model_profile).collect::<Vec<_>>(),
                "skills": safe_skill_inventory(&snapshot.assets.skills),
            },
            "relationships": safe_consumption_inventory(&snapshot.relationships),
        }),
        human,
    ))
}

fn upgrade(options: MutationOptions) -> Result<CommandOutput, CliError> {
    options.validate()?;
    let current = env!("CARGO_PKG_VERSION");
    if let Some(real) = arca_core::application::update::managed_by_desktop_app() {
        return Ok(CommandOutput::new(
            "upgrade",
            false,
            json!({
                "managed_by_desktop": true,
                "path": safe_path(&real.to_string_lossy()),
                "version": current,
            }),
            format!(
                "This CLI is provided by Arca.app ({}); it upgrades with the Desktop app.",
                real.display()
            ),
        ));
    }
    if options.dry_run {
        return Ok(CommandOutput::new(
            "upgrade",
            false,
            json!({"dry_run": true, "would_check": true, "current_version": current}),
            format!("Would check for a Stable release newer than v{current}; no changes made."),
        ));
    }
    if !options.yes {
        let summary = json!({"current_version": current, "channel": "stable"});
        return execute_upgrade_after_review(options, summary, current);
    }
    perform_upgrade(current)
}

fn execute_upgrade_after_review(
    options: MutationOptions,
    summary: Value,
    current: &str,
) -> Result<CommandOutput, CliError> {
    let mut outcome = None;
    let direct = execute_direct_mutation(
        "upgrade",
        options,
        summary,
        &format!("Review change\n  check and install Stable CLI newer than v{current}"),
        false,
        || {
            outcome = Some(
                arca_core::application::update::upgrade_cli(current)
                    .map_err(|error| CliError::private("upgrade_failed", error.to_string()))?,
            );
            Ok(())
        },
    )?;
    match outcome {
        Some(Some(upgrade)) => Ok(CommandOutput::new(
            "upgrade",
            true,
            json!({"from": upgrade.from, "to": upgrade.to}),
            format!("Upgraded from v{} to v{}.", upgrade.from, upgrade.to),
        )),
        Some(None) => Ok(CommandOutput::new(
            "upgrade",
            false,
            json!({"current_version": current, "up_to_date": true}),
            "Already on the latest Stable release.",
        )),
        None => Ok(direct),
    }
}

fn perform_upgrade(current: &str) -> Result<CommandOutput, CliError> {
    match arca_core::application::update::upgrade_cli(current)
        .map_err(|error| CliError::private("upgrade_failed", error.to_string()))?
    {
        Some(upgrade) => Ok(CommandOutput::new(
            "upgrade",
            true,
            json!({"from": upgrade.from, "to": upgrade.to}),
            format!("Upgraded from v{} to v{}.", upgrade.from, upgrade.to),
        )),
        None => Ok(CommandOutput::new(
            "upgrade",
            false,
            json!({"current_version": current, "up_to_date": true}),
            "Already on the latest Stable release.",
        )),
    }
}

fn selection(domain: AssetDomain, identities: Vec<String>) -> AgentConsumptionSelection {
    match domain {
        AssetDomain::Mcp => AgentConsumptionSelection::Mcp {
            asset_keys: identities,
        },
        AssetDomain::Model => AgentConsumptionSelection::Model {
            profile_ids: identities,
        },
        AssetDomain::Skill => AgentConsumptionSelection::Skill { names: identities },
    }
}

fn domain_command(domain: AssetDomain, action: &str) -> &'static str {
    match (domain, action) {
        (AssetDomain::Mcp, "assign") => "mcp.assign",
        (AssetDomain::Mcp, "unassign") => "mcp.unassign",
        (AssetDomain::Mcp, "enable") => "mcp.enable",
        (AssetDomain::Mcp, "disable") => "mcp.disable",
        (AssetDomain::Model, "assign") => "model.assign",
        (AssetDomain::Model, "unassign") => "model.unassign",
        (AssetDomain::Model, "enable") => "model.enable",
        (AssetDomain::Model, "disable") => "model.disable",
        (AssetDomain::Skill, "assign") => "skill.assign",
        (AssetDomain::Skill, "unassign") => "skill.unassign",
        (AssetDomain::Skill, "enable") => "skill.enable",
        (AssetDomain::Skill, "disable") => "skill.disable",
        _ => unreachable!("unsupported domain action"),
    }
}

fn asset_matches_domain(asset: &AssetRef, domain: AssetDomain) -> bool {
    matches!(
        (asset, domain),
        (AssetRef::Mcp { .. }, AssetDomain::Mcp)
            | (AssetRef::Model { .. }, AssetDomain::Model)
            | (AssetRef::Skill { .. }, AssetDomain::Skill)
    )
}

fn require_central_assets(domain: AssetDomain, identities: &[String]) -> Result<(), CliError> {
    let requested = identities.iter().collect::<BTreeSet<_>>();
    let existing = match domain {
        AssetDomain::Mcp => read_registry()
            .into_iter()
            .map(|entry| entry.key())
            .collect::<BTreeSet<_>>(),
        AssetDomain::Model => arca_core::application::models::list_profiles()
            .into_iter()
            .map(|profile| profile.profile.id)
            .collect::<BTreeSet<_>>(),
        AssetDomain::Skill => skill_inventory()?
            .items
            .into_iter()
            .filter(|item| matches!(item.location, SkillLocation::Central))
            .map(|item| item.name)
            .collect::<BTreeSet<_>>(),
    };
    let missing = requested
        .into_iter()
        .filter(|identity| !existing.contains(identity.as_str()))
        .map(|identity| identity.to_string())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CliError::new(
            "central_asset_missing",
            format!("central asset not found: {}", missing.join(", ")),
        )
        .with_detail("missing", json!(missing)))
    }
}

fn require_agent_capability(agent: &str, domain: AssetDomain) -> Result<(), CliError> {
    require_agent_capability_state(agent, domain, false)
}

fn require_enabled_agent_capability(agent: &str, domain: AssetDomain) -> Result<(), CliError> {
    require_agent_capability_state(agent, domain, true)
}

fn require_agent_capability_state(
    agent: &str,
    domain: AssetDomain,
    require_enabled: bool,
) -> Result<(), CliError> {
    let capabilities = arca_core::application::agents::list_capabilities()
        .map_err(CliError::from_core)?
        .into_iter()
        .find(|candidate| candidate.identity.id == agent)
        .ok_or_else(|| CliError::new("unknown_agent", format!("unknown Agent: {agent}")))?;
    if require_enabled && !capabilities.identity.enabled {
        return Err(CliError::new(
            "agent_disabled",
            format!("Agent {agent} is disabled in Arca"),
        ));
    }
    let supported = match domain {
        AssetDomain::Mcp => capabilities.capabilities.mcp.is_some(),
        AssetDomain::Model => capabilities.capabilities.model.is_some(),
        AssetDomain::Skill => capabilities.capabilities.skill.is_some(),
    };
    if supported {
        Ok(())
    } else {
        Err(CliError::new(
            "unsupported_agent_capability",
            format!("Agent {agent} does not support {domain:?}"),
        ))
    }
}

fn central_mcp(key: &str) -> Result<RegistryEntry, CliError> {
    read_registry()
        .into_iter()
        .find(|entry| entry.key() == key)
        .ok_or_else(|| {
            CliError::new(
                "central_asset_missing",
                format!("central MCP asset not found: {key}"),
            )
        })
}

fn central_model(
    profile_id: &str,
) -> Result<arca_core::application::models::ModelProfileView, CliError> {
    arca_core::application::models::list_profiles()
        .into_iter()
        .find(|profile| profile.profile.id == profile_id)
        .ok_or_else(|| {
            CliError::new(
                "central_asset_missing",
                format!("central Model Profile not found: {profile_id}"),
            )
        })
}

fn central_skill(
    name: &str,
) -> Result<arca_core::application::skills::SkillInventoryItem, CliError> {
    skill_inventory()?
        .items
        .into_iter()
        .find(|item| item.name == name && matches!(item.location, SkillLocation::Central))
        .ok_or_else(|| {
            CliError::new(
                "central_asset_missing",
                format!("central Skill not found: {name}"),
            )
        })
}

fn skill_inventory() -> Result<arca_core::application::skills::SkillsInventory, CliError> {
    arca_core::application::skills::list_inventory().map_err(skill_error)
}

pub(crate) fn skill_error(error: arca_core::application::skills::SkillError) -> CliError {
    let parts = error.into_command_parts();
    let mut result = CliError::private(parts.code, parts.message);
    if let Some(retry_at) = parts.retry_at {
        result = result.with_detail("retry_at", retry_at);
    }
    if let Some(findings_hash) = parts.findings_hash {
        result = result.with_detail("findings_hash", findings_hash);
    }
    result
}

fn is_external_skill(item: &arca_core::application::skills::SkillInventoryItem) -> bool {
    matches!(item.location, SkillLocation::AgentTarget { .. })
        && item.states.contains(&InventoryState::External)
}

fn format_status_row(row: &arca_core::application::assets::ConsumptionView) -> String {
    let identity = match &row.asset {
        AssetRef::Mcp { key } => key,
        AssetRef::Model { profile_id } => profile_id,
        AssetRef::Skill { name } => name,
        AssetRef::ModelProvider { provider_id } => provider_id,
    };
    let mut state = vec![
        format!("ownership={:?}", row.ownership),
        format!("desired={}", row.desired),
        format!("observed={}", row.observed),
        format!("status={:?}", row.status),
    ];
    if let Some(enabled) = row.enabled {
        state.push(format!("desired_enabled={enabled}"));
    }
    if let Some(observed_enabled) = row.observed_enabled {
        state.push(format!("observed_enabled={observed_enabled}"));
    }
    if let Some(active) = row.active {
        state.push(format!("active={active}"));
    }
    if let Some(desired_active) = row.desired_active {
        state.push(format!("desired_active={desired_active}"));
    }
    if !row.available_actions.is_empty() {
        state.push(format!("actions={:?}", row.available_actions));
    }
    format!("  {}  {}  {}", row.agent_id, identity, state.join(" "))
}

fn redacted_mcp_entry(entry: &RegistryEntry) -> Value {
    let config = if let Some(stdio) = &entry.config.stdio {
        let mut value = json!({
            "transport": "stdio",
            "command_configured": !stdio.command.trim().is_empty(),
            "arg_count": stdio.args.as_ref().map_or(0, Vec::len),
            "cwd_configured": stdio.cwd.is_some(),
        });
        if let Some(env) = &stdio.env {
            let mut keys = env.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            value["env"] = json!({"keys": keys, "redacted": true});
        }
        value
    } else if let Some(http) = &entry.config.http {
        let mut value = json!({
            "transport": "http",
            "type": http.kind,
            "url": safe_url(&http.url),
        });
        if let Some(headers) = &http.headers {
            let mut keys = headers.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            value["headers"] = json!({"keys": keys, "redacted": true});
        }
        value
    } else {
        json!({"transport": entry.transport(), "invalid": true})
    };
    json!({
        "key": entry.key(),
        "name": entry.name,
        "description": entry.description,
        "tags": entry.tags,
        "config": config,
        "origin": entry.origin.as_ref().map(|origin| json!({
            "kind": origin.kind,
            "agent": origin.agent,
            "scope": origin.scope,
            "source_configured": origin.source.is_some(),
        })),
        "repo": entry.repo.as_deref().map(safe_url),
    })
}

fn redacted_model_profile(view: &arca_core::application::models::ModelProfileView) -> Value {
    let profile = &view.profile;
    json!({
        "id": profile.id,
        "name": profile.name,
        "provider_id": profile.provider_id,
        "provider": profile.provider,
        "model_vendor": profile.model_vendor,
        "native_ids": profile.native_ids,
        "protocol": profile.protocol,
        "model": profile.model,
        "env_key": profile.env_key,
        "context_window": profile.context_window,
        "max_output_tokens": profile.max_output_tokens,
        "reasoning": profile.reasoning,
        "catalog_key": view.catalog_key,
        "credential_saved": view.credential_saved,
    })
}

fn mcp_status_label(status: &McpAdoptionStatus) -> &'static str {
    match status {
        McpAdoptionStatus::ExternalAdded => "external added",
        McpAdoptionStatus::ExternalChanged => "external changed",
    }
}

fn model_status_label(status: &ModelAdoptionStatus) -> &'static str {
    match status {
        ModelAdoptionStatus::Adoptable => "adoptable",
        ModelAdoptionStatus::NeedsCredential => "needs credential",
        ModelAdoptionStatus::Unsupported => "unsupported",
        ModelAdoptionStatus::Conflicted => "conflicted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_clear_requires_explicit_all_or_exact_profiles() {
        assert!(Cli::try_parse_from(["arca", "model", "unassign", "--agent", "qoder-cli"]).is_err());
        assert!(Cli::try_parse_from(["arca", "model", "unassign", "work", "--all", "--agent", "qoder-cli"]).is_err());
        let cli = Cli::try_parse_from(["arca", "model", "unassign", "--all", "--agent", "qoder-cli"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Model { command: ModelCommand::Unassign { all: true, .. } })));
    }

    #[test]
    fn skill_install_requires_one_source_and_explicit_names() {
        for args in [
            vec!["arca", "skill", "install", "--name", "review"],
            vec!["arca", "skill", "install", "--path", "/tmp/skills"],
            vec!["arca", "skill", "install", "--path", "/tmp/skills", "--github", "owner/repo", "--name", "review"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        let cli = Cli::try_parse_from(["arca", "skill", "install", "--path", "/tmp/skills", "--name", "review", "--yes"]).unwrap();
        assert!(!cli.accept_risk);
        let cli = Cli::try_parse_from(["arca", "skill", "install", "--path", "/tmp/skills", "--name", "review", "--accept-risk", "--yes"]).unwrap();
        assert!(cli.accept_risk);
    }

    #[test]
    fn disabled_agents_remain_queryable_but_are_rejected_for_mutations() {
        let _home = arca_core::testenv::TestHome::new("cli-disabled-query");
        arca_core::application::agents::set_enabled("codex", false).unwrap();

        require_agent_capability("codex", AssetDomain::Model).unwrap();
        let error = require_enabled_agent_capability("codex", AssetDomain::Model).unwrap_err();
        assert_eq!(error.code, "agent_disabled");
    }

    #[test]
    fn parses_complete_resource_command_tree() {
        for args in [
            vec!["arca", "mcp", "list"],
            vec!["arca", "mcp", "show", "github::stdio"],
            vec!["arca", "mcp", "status", "--agent", "codex"],
            vec![
                "arca",
                "mcp",
                "assign",
                "github::stdio",
                "gitlab::http",
                "--agent",
                "codex",
            ],
            vec![
                "arca",
                "mcp",
                "unassign",
                "github::stdio",
                "--agent",
                "codex",
            ],
            vec!["arca", "mcp", "unassign", "--all", "--agent", "codex"],
            vec!["arca", "mcp", "enable", "github::stdio", "--agent", "codex"],
            vec!["arca", "mcp", "disable", "github::stdio", "--agent", "codex"],
            vec!["arca", "mcp", "add", "github::stdio", "--command", "npx"],
            vec!["arca", "mcp", "delete", "github::stdio"],
            vec!["arca", "mcp", "export", "--out", "mcp.json"],
            vec![
                "arca",
                "mcp",
                "converge",
                "github::stdio",
                "--agent",
                "codex",
                "restore",
            ],
            vec![
                "arca",
                "mcp",
                "converge",
                "github::stdio",
                "--agent",
                "codex",
                "adopt",
            ],
            vec![
                "arca",
                "mcp",
                "converge",
                "github::stdio",
                "--agent",
                "codex",
                "detach",
            ],
            vec!["arca", "model", "list"],
            vec!["arca", "model", "show", "work"],
            vec!["arca", "model", "status", "--agent", "codex"],
            vec!["arca", "model", "assign", "work", "--agent", "codex"],
            vec!["arca", "model", "unassign", "work", "--agent", "codex"],
            vec!["arca", "model", "enable", "work", "--agent", "codex"],
            vec!["arca", "model", "disable", "work", "--agent", "codex"],
            vec!["arca", "model", "use", "work", "--agent", "codex"],
            vec![
                "arca", "model", "converge", "work", "--agent", "codex", "restore",
            ],
            vec!["arca", "skill", "list"],
            vec!["arca", "skill", "show", "review-changes"],
            vec!["arca", "skill", "status", "--agent", "codex"],
            vec![
                "arca",
                "skill",
                "assign",
                "review-changes",
                "--agent",
                "codex",
            ],
            vec![
                "arca",
                "skill",
                "unassign",
                "review-changes",
                "--agent",
                "codex",
            ],
            vec![
                "arca",
                "skill",
                "enable",
                "review-changes",
                "--agent",
                "codex",
            ],
            vec![
                "arca",
                "skill",
                "disable",
                "review-changes",
                "--agent",
                "codex",
            ],
            vec![
                "arca",
                "skill",
                "converge",
                "review-changes",
                "--agent",
                "codex",
                "restore",
            ],
            vec!["arca", "agent", "list"],
            vec!["arca", "agent", "enable", "codex"],
            vec!["arca", "agent", "disable", "codex"],
            vec!["arca", "discover"],
            vec!["arca", "discover", "skill"],
            vec!["arca", "workspace"],
            vec!["arca", "upgrade"],
        ] {
            Cli::try_parse_from(args.clone()).unwrap_or_else(|error| {
                panic!("failed to parse {args:?}: {error}");
            });
        }
        assert!(
            Cli::try_parse_from(["arca", "mcp", "converge", "github::stdio", "restore"]).is_err()
        );
        assert!(Cli::try_parse_from([
            "arca",
            "mcp",
            "converge",
            "github::stdio",
            "--agent",
            "codex",
            "invalid-action",
        ])
        .is_err());
    }

    #[test]
    fn legacy_top_level_commands_are_rejected() {
        for command in [
            "list", "status", "add", "remove", "export", "apply", "clean", "models", "skills",
            "agents", "detected", "manage", "import",
        ] {
            assert!(
                Cli::try_parse_from(["arca", command]).is_err(),
                "legacy command unexpectedly parsed: {command}"
            );
        }
    }

    #[test]
    fn relationship_mutations_require_one_explicit_agent() {
        assert!(Cli::try_parse_from(["arca", "mcp", "assign", "github::stdio"]).is_err());
        assert!(Cli::try_parse_from([
            "arca",
            "skill",
            "unassign",
            "review-changes",
            "--agent",
            "codex",
            "--agent",
            "cursor"
        ])
        .is_err());
        assert!(Cli::try_parse_from(["arca", "mcp", "unassign", "--agent", "codex"]).is_err());
        assert!(Cli::try_parse_from([
            "arca",
            "mcp",
            "unassign",
            "github::stdio",
            "--all",
            "--agent",
            "codex"
        ])
        .is_err());
        assert!(
            Cli::try_parse_from(["arca", "model", "unassign", "--all", "--agent", "codex"]).is_err()
        );
    }

    #[test]
    fn mcp_writes_require_exact_transport_identity() {
        assert!(Cli::try_parse_from(["arca", "mcp", "delete", "github"]).is_err());
        assert!(Cli::try_parse_from(["arca", "mcp", "delete", "github::sse"]).is_err());
        assert!(Cli::try_parse_from(["arca", "mcp", "delete", "github::http"]).is_ok());
    }

    #[test]
    fn model_assign_supports_explicit_replace() {
        let cli = Cli::try_parse_from([
            "arca",
            "--json",
            "model",
            "assign",
            "work",
            "--agent",
            "codex",
            "--replace",
            "--dry-run",
        ])
        .unwrap();
        assert!(cli.json);
        assert!(cli.dry_run);
        assert!(matches!(
            cli.command,
            Some(Command::Model {
                command: ModelCommand::Assign { replace: true, .. }
            })
        ));
    }

    #[test]
    fn global_flags_parse_before_or_after_subcommands() {
        for args in [
            vec!["arca", "--json", "skill", "list", "--no-color"],
            vec!["arca", "skill", "--json", "list", "--no-color"],
            vec!["arca", "skill", "list", "--json", "--no-color"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.json);
            assert!(cli.no_color);
        }
    }

    #[test]
    fn mcp_projection_redacts_config_secrets() {
        let stdio = RegistryEntry {
            name: "stdio-secret".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "COMMAND_SENTINEL".into(),
                    args: Some(vec!["--token".into(), "ARG_SENTINEL".into()]),
                    env: Some(std::collections::HashMap::from([(
                        "API_KEY".into(),
                        "ENV_SENTINEL".into(),
                    )])),
                    cwd: Some("/private/CWD_SENTINEL".into()),
                }),
                http: None,
            },
            origin: Some(RegistryOrigin {
                kind: "manual".into(),
                agent: None,
                scope: None,
                source: Some("SOURCE_SENTINEL".into()),
            }),
            repo: Some("https://user:REPO_SENTINEL@example.com/private?token=x".into()),
        };
        let encoded = redacted_mcp_entry(&stdio).to_string();
        assert!(!encoded.contains("ARG_SENTINEL"));
        assert!(!encoded.contains("COMMAND_SENTINEL"));
        assert!(!encoded.contains("ENV_SENTINEL"));
        assert!(!encoded.contains("CWD_SENTINEL"));
        assert!(!encoded.contains("SOURCE_SENTINEL"));
        assert!(!encoded.contains("REPO_SENTINEL"));
        assert!(encoded.contains("API_KEY"));
        assert!(encoded.contains("arg_count"));
        assert!(encoded.contains("command_configured"));

        let http = RegistryEntry {
            name: "http-secret".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: None,
                http: Some(HttpConfig {
                    kind: "http".into(),
                    url: "https://user:URL_SENTINEL@example.com/private/TOKEN?key=QUERY_SENTINEL#x"
                        .into(),
                    headers: Some(std::collections::HashMap::from([(
                        "Authorization".into(),
                        "HEADER_SENTINEL".into(),
                    )])),
                }),
            },
            origin: None,
            repo: None,
        };
        let encoded = redacted_mcp_entry(&http).to_string();
        assert!(!encoded.contains("URL_SENTINEL"));
        assert!(!encoded.contains("QUERY_SENTINEL"));
        assert!(!encoded.contains("TOKEN"));
        assert!(!encoded.contains("HEADER_SENTINEL"));
        assert!(encoded.contains("Authorization"));
        assert!(encoded.contains("https://example.com"));
    }

    #[test]
    fn status_projection_is_sorted_and_scopes_target_incidents() {
        use arca_core::application::assets::{
            AssetCapability, ConsumptionInventory, ConsumptionStatus, ConsumptionView,
            TargetIncident,
        };

        let row = |agent: &str, key: &str| ConsumptionView {
            agent_id: agent.into(),
            asset: AssetRef::Mcp { key: key.into() },
            ownership: arca_core::application::assets::OwnershipState::Managed,
            desired: true,
            observed: true,
            enabled: Some(true),
            observed_enabled: Some(true),
            active: None,
            desired_active: None,
            status: ConsumptionStatus::Synced,
            reason: None,
            observation_id: None,
            available_actions: Vec::new(),
            affected_agent_ids: Vec::new(),
            target: None,
        };
        let inventory = ConsumptionInventory {
            consumptions: vec![row("z-agent", "z::stdio"), row("a-agent", "z::stdio")],
            external: vec![row("b-agent", "b::http"), row("a-agent", "a::stdio")],
            target_incidents: vec![TargetIncident {
                id: "target-1".into(),
                operation_id: "operation-1".into(),
                capability: AssetCapability::Mcp,
                target_id: "target-1".into(),
                target_path: "~/.qoder/mcp.json".into(),
                affected_agent_ids: vec!["a-agent".into()],
                code: "target_recovery_required".into(),
                retryable: true,
            }],
            ..Default::default()
        };
        let (managed, external, target_incidents) =
            status_projection(inventory, AssetDomain::Mcp, None);
        assert_eq!(managed[0].agent_id, "a-agent");
        assert_eq!(managed[1].agent_id, "z-agent");
        assert_eq!(external[0].agent_id, "a-agent");
        assert_eq!(external[1].agent_id, "b-agent");
        assert_eq!(target_incidents.len(), 1);
        assert_eq!(target_incidents[0].affected_agent_ids, ["a-agent"]);
    }

    #[test]
    fn human_status_preserves_observed_and_desired_model_current_state() {
        use arca_core::application::assets::{ConsumptionStatus, ConsumptionView};

        let row = ConsumptionView {
            agent_id: "codex".into(),
            asset: AssetRef::Model {
                profile_id: "work".into(),
            },
            ownership: arca_core::application::assets::OwnershipState::Managed,
            desired: true,
            observed: true,
            enabled: None,
            observed_enabled: Some(true),
            active: Some(false),
            desired_active: Some(true),
            status: ConsumptionStatus::ExternalChanged,
            reason: Some("current model changed externally".into()),
            observation_id: None,
            available_actions: vec![
                arca_core::application::assets::ConvergenceAction::AdoptObserved,
            ],
            affected_agent_ids: Vec::new(),
            target: None,
        };
        let rendered = format_status_row(&row);
        assert!(rendered.contains("active=false"));
        assert!(rendered.contains("desired_active=true"));
    }

    #[test]
    fn export_creates_one_private_file_and_never_overwrites_it() {
        let unique = format!(
            "arca-cli-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir(&root).unwrap();
        let path = root.join("mcp.json");

        write_new_private_export(&path, "first").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        let error = write_new_private_export(&path, "second").unwrap_err();
        assert_eq!(error.code, "export_target_exists");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
