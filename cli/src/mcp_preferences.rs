use std::path::PathBuf;
use clap::Subcommand;
use arca_core::application::ui;
use serde_json::json;
use crate::{command::Cli, output::{query_output, CliError, CommandOutput}, projection::safe_path, review::execute_direct_mutation};

#[derive(Debug, Subcommand)]
pub enum IconCommand {
    /// Show customized MCP icons.
    List,
    /// Select a built-in icon, using the same icon ID as Desktop.
    Set { key: String, icon: String },
    /// Import a PNG, JPEG or WebP icon (maximum 1 MiB).
    Import { key: String, #[arg(long)] file: PathBuf },
    /// Restore automatic icon selection.
    Reset { key: String },
}

pub fn dispatch(cli: &Cli, command: &IconCommand) -> Result<CommandOutput, CliError> {
    if matches!(command, IconCommand::List) {
        cli.reject_mutation_options()?;
        let mut icons = ui::list_mcp_icon_preferences().map_err(CliError::from_legacy)?;
        for icon in icons.values_mut() { icon.path = icon.path.as_deref().map(safe_path); }
        return query_output("mcp.icon.list", json!({"icons": icons, "available": ui::mcp_icon_catalog()}));
    }
    let (key, summary) = match command {
        IconCommand::Set { key, icon } => (key, json!({"key": key, "icon": icon})),
        IconCommand::Import { key, file } => (key, json!({"key": key, "file": safe_path(&file.to_string_lossy())})),
        IconCommand::Reset { key } => (key, json!({"key": key, "automatic": true})),
        IconCommand::List => unreachable!(),
    };
    execute_direct_mutation("mcp.icon.configure", cli.mutation_options(), summary,
        &format!("Update the icon for {key}."), false, || {
            match command {
                IconCommand::Set { key, icon } => ui::set_mcp_builtin_icon(key.clone(), icon.clone()),
                IconCommand::Import { key, file } => ui::import_mcp_icon(key.clone(), file.clone()),
                IconCommand::Reset { key } => ui::reset_mcp_icon(key.clone()),
                IconCommand::List => unreachable!(),
            }.map(|_| ()).map_err(CliError::from_legacy)
        })
}
