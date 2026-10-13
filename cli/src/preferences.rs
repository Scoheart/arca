//! Shared preferences; window theme and editor launch remain host presentation.
use clap::Subcommand;
use arca_core::application::{terminals, ui};
use serde_json::json;
use crate::{command::Cli, output::{query_output, CliError, CommandOutput}, review::execute_direct_mutation};

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// Read preferences shared with Desktop.
    Show,
    /// Without an ID, list detected terminals. Otherwise set the CLI launch terminal.
    Terminal { id: Option<String> },
    /// Read or change Desktop's locale: system, zh-CN or en-US.
    Locale { #[arg(value_parser = ["system", "zh-CN", "en-US"])] value: Option<String> },
    /// Read pinned Agents, replace the list with IDs, or clear with --clear.
    Pins { #[arg(conflicts_with = "clear")] agents: Vec<String>, #[arg(long)] clear: bool },
}

pub fn dispatch(cli: &Cli, command: &SettingsCommand) -> Result<CommandOutput, CliError> {
    match command {
        SettingsCommand::Show => {
            cli.reject_mutation_options()?;
            query_output("settings.show", json!({
                "terminal": terminals::settings().map_err(CliError::from_legacy)?,
                "locale": ui::get_ui_locale().map_err(CliError::from_legacy)?,
                "pinned_agents": ui::get_pinned_agents().map_err(CliError::from_legacy)?,
            }))
        }
        SettingsCommand::Terminal { id: None } => {
            cli.reject_mutation_options()?;
            query_output("settings.terminal", json!(terminals::settings().map_err(CliError::from_legacy)?))
        }
        SettingsCommand::Terminal { id: Some(id) } => {
            let current = terminals::settings().map_err(CliError::from_legacy)?;
            if !current.options.iter().any(|option| option.id == *id && option.installed) {
                return Err(CliError::new("terminal_unavailable", "choose an installed terminal listed by arca settings terminal"));
            }
            execute_direct_mutation("settings.terminal", cli.mutation_options(), json!({"terminal": id}),
                &format!("Use {id} for Agent CLI launches."), current.selected == *id,
                || terminals::configure(id).map(|_| ()).map_err(CliError::from_legacy))
        }
        SettingsCommand::Locale { value: None } => {
            cli.reject_mutation_options()?;
            query_output("settings.locale", json!({"locale": ui::get_ui_locale().map_err(CliError::from_legacy)?}))
        }
        SettingsCommand::Locale { value: Some(value) } => {
            let locale = (value != "system").then(|| value.clone());
            let current = ui::get_ui_locale().map_err(CliError::from_legacy)?;
            execute_direct_mutation("settings.locale", cli.mutation_options(), json!({"locale": locale}),
                &format!("Use {value} as the Arca interface language."), current == locale,
                || ui::set_ui_locale(locale).map(|_| ()).map_err(CliError::from_legacy))
        }
        SettingsCommand::Pins { agents, clear } if agents.is_empty() && !clear => {
            cli.reject_mutation_options()?;
            query_output("settings.pins", json!({"agents": ui::get_pinned_agents().map_err(CliError::from_legacy)?}))
        }
        SettingsCommand::Pins { agents, .. } => {
            let current = ui::get_pinned_agents().map_err(CliError::from_legacy)?;
            execute_direct_mutation("settings.pins", cli.mutation_options(), json!({"agents": agents}),
                "Replace the pinned Agent list shared with Desktop.", current == *agents,
                || ui::set_pinned_agents(agents.clone()).map(|_| ()).map_err(CliError::from_legacy))
        }
    }
}
