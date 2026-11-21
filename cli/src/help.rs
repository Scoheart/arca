//! Organize the public Clap command tree around common tasks.

use std::fmt::Write;

use clap::{Command, CommandFactory, Subcommand};

use crate::command::{Cli, McpRelationshipCommand, ModelRelationshipCommand, SkillRelationshipCommand};

pub(crate) fn command() -> Command {
    let root = grouped(
        Cli::command(),
        &[
            ("Asset libraries", &["mcp", "model", "skill"]),
            ("Agent management", &["agent"]),
            (
                "Inspection and diagnostics",
                &["status", "discover", "workspace", "trace", "capture", "logs"],
            ),
            ("Settings and updates", &["settings", "network", "upgrade"]),
            ("Automation", &["operation"]),
        ],
        &["arca model list", "arca agent list", "arca agent status codex"],
    );
    root.mut_subcommand("mcp", |command| {
        grouped(
            hide_legacy_agent_paths::<McpRelationshipCommand>(command),
            &[
                ("MCP library", &["list", "show", "add", "save", "delete", "export"]),
                ("Sources and icons", &["source", "icon"]),
            ],
            &["arca mcp list", "arca agent mcp --help"],
        )
    })
    .mut_subcommand("model", |command| {
        grouped(
            hide_legacy_agent_paths::<ModelRelationshipCommand>(command),
            &[
                ("Saved Model Profiles", &["list", "show", "save", "import", "delete"]),
                ("Providers and credential stores", &["provider", "store"]),
                ("API requests", &["playground", "curl"]),
            ],
            &[
                "arca model provider list",
                "arca model provider catalog PROVIDER_ID",
                "arca model store list",
                "arca model playground",
                "arca agent model --help",
            ],
        )
        .mut_subcommand("provider", |command| {
            grouped(
                command,
                &[
                    ("Saved Provider connections", &["list", "show", "save", "delete"]),
                    ("Templates and available models", &["templates", "catalog", "docs"]),
                ],
                &[
                    "arca model provider save --file provider.json --dry-run",
                    "arca model provider catalog PROVIDER_ID",
                ],
            )
        })
    })
    .mut_subcommand("skill", |command| {
        grouped(
            hide_legacy_agent_paths::<SkillRelationshipCommand>(command),
            &[
                (
                    "Skill library",
                    &["list", "show", "inspect-source", "install", "import", "check-updates", "update", "repair", "remove"],
                ),
            ],
            &["arca skill list", "arca agent skill --help"],
        )
    })
    .mut_subcommand("agent", |command| {
        grouped(
            command,
            &[
                ("Agent overview", &["list", "status"]),
                ("Agent assets", &["mcp", "model", "skill"]),
                ("Agent configuration", &["save", "configure", "enable", "disable"]),
                ("Launching Agents", &["run", "launch"]),
            ],
            &["arca agent status codex", "arca agent model status --agent codex"],
        )
        .mut_subcommand("mcp", |command| {
            grouped(
                command,
                &[
                    ("Inspection", &["status", "show"]),
                    ("Assignments", &["assign", "unassign", "enable", "disable", "enable-all", "disable-all"]),
                    ("External changes", &["converge"]),
                ],
                &[
                    "arca agent mcp status --agent codex",
                    "arca agent mcp assign 'docs::http' --agent codex --dry-run",
                ],
            )
        })
        .mut_subcommand("model", |command| {
            grouped(
                command,
                &[
                    ("Inspection", &["status"]),
                    ("Assignments and current model", &["assign", "unassign", "enable", "disable", "use"]),
                    ("Credential delivery", &["delivery"]),
                    ("External changes", &["converge"]),
                ],
                &["arca agent model status --agent pi", "arca agent model use PROFILE_ID --agent pi --dry-run"],
            )
        })
        .mut_subcommand("skill", |command| {
            grouped(
                command,
                &[
                    ("Inspection", &["status"]),
                    ("Assignments", &["assign", "unassign", "enable", "disable"]),
                    ("External changes", &["converge"]),
                ],
                &["arca agent skill status --agent codex", "arca agent skill assign SKILL_NAME --agent codex --dry-run"],
            )
        })
    })
}

fn hide_legacy_agent_paths<R: Subcommand>(command: Command) -> Command {
    // Keep the old grammar for existing scripts without advertising two paths.
    command.mut_subcommands(|child| {
        if child.get_name() == "status" || R::has_subcommand(child.get_name()) {
            child.hide(true)
        } else {
            child
        }
    })
}

fn grouped(command: Command, sections: &[(&str, &[&str])], examples: &[&str]) -> Command {
    let subcommands: Vec<_> = command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set() && subcommand.get_name() != "help")
        .collect();
    let width = subcommands
        .iter()
        .map(|subcommand| subcommand.get_name().len())
        .max()
        .unwrap_or(0);

    // Descriptions and accepted names come from Clap. Leave new commands visible
    // even if they have not been assigned a help section yet.
    let mut listing = String::new();
    let mut ungrouped = subcommands;
    for (heading, names) in sections {
        let rows = names.iter().filter_map(|name| {
            let index = ungrouped.iter().position(|subcommand| subcommand.get_name() == *name)?;
            Some(ungrouped.remove(index))
        });
        append_section(&mut listing, heading, rows, width);
    }
    append_section(&mut listing, "Other commands", ungrouped, width);

    let mut footer = String::from("Examples:");
    for example in examples {
        write!(&mut footer, "\n  {example}").unwrap();
    }
    footer.push_str("\n\nRun a subcommand with --help for its arguments and options.");

    let template = format!(
        "{{about-with-newline}}\n{{usage-heading}} {{usage}}\n{listing}\nOptions:\n{{options}}{{after-help}}"
    );
    command.help_template(template).after_help(footer)
}

fn append_section<'a>(
    listing: &mut String,
    heading: &str,
    rows: impl IntoIterator<Item = &'a Command>,
    width: usize,
) {
    let mut rows = rows.into_iter().peekable();
    if rows.peek().is_none() {
        return;
    }
    write!(listing, "\n{heading}:\n").unwrap();
    for subcommand in rows {
        let name = subcommand.get_name();
        match subcommand.get_about() {
            Some(about) => writeln!(listing, "  {name:width$}  {about}").unwrap(),
            None => writeln!(listing, "  {name}").unwrap(),
        }
    }
}
