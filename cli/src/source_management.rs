//! MCP source adapters. Source ownership and catalog impact are enforced by Core.

use clap::Subcommand;
use arca_core::application::mcp::sources::{self, SourceView};
use serde_json::{json, Value};

use crate::command::{parse_identity, Cli};
use crate::output::{query_output, CliError, CommandOutput};
use crate::projection::{safe_path, safe_url};
use crate::review::execute_direct_mutation;

#[derive(Debug, Subcommand)]
pub enum SourceCommand {
    /// List sources, sync status and whether Arca maintains them automatically.
    List,
    /// Subscribe to a remote MCP catalog.
    Subscribe {
        #[arg(long)]
        url: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Add a local MCP catalog file as a refreshable source.
    AddLocal {
        #[arg(long)]
        path: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Add Arca's built-in curated catalog.
    AddBuiltin,
    /// Refresh one subscribed or local source.
    Refresh {
        #[arg(value_parser = parse_identity)]
        id: String,
    },
    /// Enable a source without changing assigned MCPs behind the review boundary.
    Enable {
        #[arg(value_parser = parse_identity)]
        id: String,
    },
    /// Disable a source, subject to Core's desired-consumer checks.
    Disable {
        #[arg(value_parser = parse_identity)]
        id: String,
    },
    /// Remove a user-managed source. Arca's automatic sources are protected.
    Remove {
        #[arg(value_parser = parse_identity)]
        id: String,
    },
}

pub fn dispatch(cli: &Cli, command: &SourceCommand) -> Result<CommandOutput, CliError> {
    if matches!(command, SourceCommand::List) {
        cli.reject_mutation_options()?;
        return query_output("mcp.source.list", json!({
            "sources": sources::list_views().iter().map(safe_source).collect::<Vec<_>>()
        }));
    }
    let options = cli.mutation_options();
    options.validate()?;
    let (name, summary) = match command {
        SourceCommand::Subscribe { url, name } => ("mcp.source.subscribe", json!({"url": safe_url(url), "name": name})),
        SourceCommand::AddLocal { path, name } => ("mcp.source.add-local", json!({"path": safe_path(path), "name": name})),
        SourceCommand::AddBuiltin => ("mcp.source.add-builtin", json!({"builtin": true})),
        SourceCommand::Refresh { id } => ("mcp.source.refresh", json!({"id": id})),
        SourceCommand::Enable { id } => ("mcp.source.enable", json!({"id": id, "enabled": true})),
        SourceCommand::Disable { id } => ("mcp.source.disable", json!({"id": id, "enabled": false})),
        SourceCommand::Remove { id } => ("mcp.source.remove", json!({"id": id})),
        SourceCommand::List => unreachable!(),
    };
    let already_applied = match command {
        SourceCommand::Enable { id } | SourceCommand::Disable { id } => sources::list_views().iter()
            .find(|source| source.id == *id)
            .is_some_and(|source| source.enabled == matches!(command, SourceCommand::Enable { .. })),
        _ => false,
    };
    let review = format!("{name}\n{}\nCore will reject changes that would alter assigned MCPs without an asset plan.",
        serde_json::to_string_pretty(&summary).unwrap_or_default());
    let mut result = None;
    let mut output = execute_direct_mutation(name, options, summary, &review, already_applied, || {
        result = match command {
            SourceCommand::Subscribe { url, name } => Some(sources::subscribe(url.clone(), name.clone()).map_err(CliError::from_legacy)?),
            SourceCommand::AddLocal { path, name } => Some(sources::add_local(path.clone(), name.clone()).map_err(CliError::from_legacy)?),
            SourceCommand::AddBuiltin => Some(sources::add_official().map_err(CliError::from_legacy)?),
            SourceCommand::Refresh { id } => Some(sources::refresh(id.clone()).map_err(CliError::from_legacy)?),
            SourceCommand::Enable { id } | SourceCommand::Disable { id } => {
                sources::set_enabled(id.clone(), matches!(command, SourceCommand::Enable { .. })).map_err(CliError::from_legacy)?;
                None
            }
            SourceCommand::Remove { id } => {
                sources::remove(id.clone()).map_err(CliError::from_legacy)?;
                None
            }
            SourceCommand::List => unreachable!(),
        };
        Ok(())
    })?;
    if let Some(source) = result {
        output.data["result"] = safe_source(&source);
        output.human = format!("Source {}: {} MCP entries.", source.id, source.server_count);
    }
    Ok(output)
}

fn safe_source(source: &SourceView) -> Value {
    json!({
        "id": source.id, "name": source.name, "kind": source.kind,
        "url": source.url.as_deref().map(safe_url), "path": source.path.as_deref().map(safe_path),
        "format": source.format, "enabled": source.enabled, "managed": source.managed,
        "server_count": source.server_count, "added_at": source.added_at, "synced_at": source.synced_at,
        "has_error": source.error.is_some(),
    })
}
