//! Arca CLI — a thin clap/TUI front-end over `arca-core`.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};

use clap::{FromArgMatches, Parser};
use serde_json::json;

mod agent_management;
mod command;
mod diagnostics;
mod help;
mod input;
mod model_management;
mod mcp_preferences;
mod network_management;
mod preferences;
mod playground;
mod output;
mod operation_session;
mod projection;
mod review;
mod skill_management;
mod source_management;
mod tui;

use command::{dispatch, Cli, CredentialExecArgs, CredentialResolveArgs};
use output::{render_error, render_success, CliError};

fn main() {
    arca_core::logging::init_session("cli");
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.get(1).is_some_and(|argument| argument == "credential") {
        if let Err(message) = run_credential_helper(&args) {
            let _ = writeln!(io::stderr().lock(), "{message}");
            std::process::exit(1);
        }
        return;
    }

    let json_mode = requested_json_mode(&args);
    let no_color = json_mode || args.iter().any(|argument| argument == "--no-color");
    let cli = match parse(args, json_mode) {
        Ok(ParseOutcome::Run(cli)) => cli,
        Ok(ParseOutcome::Early(output)) => {
            if let Err(error) = render_success(&output, true) {
                render_error(&error, true, true);
                std::process::exit(1);
            }
            return;
        }
        Err(error) => {
            render_error(&error, json_mode, no_color);
            std::process::exit(2);
        }
    };
    let (json_mode, no_color) = runtime_output_modes(&cli);
    if let Err(error) = run(cli) {
        arca_core::logging::LogEvent::error("cli.error", &error.message)
            .code(&error.code)
            .fields(error.details.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .emit();
        render_error(&error, json_mode, no_color);
        std::process::exit(1);
    }
}

fn run_credential_helper(args: &[OsString]) -> Result<(), &'static str> {
    match args.get(2).and_then(|argument| argument.to_str()) {
        Some("resolve") => resolve_credential(&args[2..]),
        Some("exec") => {
            let args = CredentialExecArgs::try_parse_from(&args[2..])
                .map_err(|_| "credential_launch_invalid: Start the Agent from Arca")?;
            arca_core::application::agent_launch::exec_with_launch_credentials(
                &args.agent,
                &args.program,
                &args.profiles_revision,
                args.startup_agent.as_deref(),
            )
        }
        _ => Err("usage: arca credential resolve --agent <agent_id> --profile <profile_id>"),
    }
}

fn resolve_credential(args: &[OsString]) -> Result<(), &'static str> {
    const USAGE: &str = "usage: arca credential resolve --agent <agent_id> --profile <profile_id>";
    let args = CredentialResolveArgs::try_parse_from(args).map_err(|_| USAGE)?;
    let mut stdout = credential_pipe()?;
    // The helper is a direct Core read. Standard CLI startup and output
    // handling would mix diagnostics or machine envelopes into this stream.
    let credential =
        arca_core::application::models::resolve_agent_credential(&args.agent, &args.profile)
            .map_err(|_| "credential_unavailable: Could not resolve the Agent credential")?;
    stdout
        .write_all(credential.as_slice())
        .map_err(|_| "credential_output_failed: Could not write to the output pipe")
}

fn credential_pipe() -> Result<File, &'static str> {
    const PIPE_REQUIRED: &str = "credential_pipe_required: Standard output must be a pipe or an anonymous local socket";
    #[cfg(unix)]
    {
        use std::os::{
            fd::{AsFd, OwnedFd},
            unix::{fs::FileTypeExt, net::UnixStream},
        };

        let output = io::stdout()
            .as_fd()
            .try_clone_to_owned()
            .map(File::from)
            .map_err(|_| PIPE_REQUIRED)?;
        let kind = output.metadata().map_err(|_| PIPE_REQUIRED)?.file_type();
        if kind.is_fifo() {
            return Ok(output);
        }
        // Node uses anonymous Unix socket pairs to capture child stdout.
        if kind.is_socket() {
            let socket = UnixStream::from(OwnedFd::from(output));
            if socket.local_addr().is_ok_and(|addr| addr.is_unnamed())
                && socket.peer_addr().is_ok_and(|addr| addr.is_unnamed())
            {
                return Ok(File::from(OwnedFd::from(socket)));
            }
        }
    }
    Err(PIPE_REQUIRED)
}

fn runtime_output_modes(cli: &Cli) -> (bool, bool) {
    (cli.json, cli.json || cli.no_color)
}

#[derive(Debug)]
enum ParseOutcome {
    Run(Cli),
    Early(output::CommandOutput),
}

fn requested_json_mode(args: &[OsString]) -> bool {
    let mut argument_value = false;
    for argument in args.iter().skip(1) {
        if argument_value {
            argument_value = false;
            continue;
        }
        if argument == "--" {
            break;
        }
        if argument == "--arg" {
            argument_value = true;
            continue;
        }
        if argument == "--json" {
            return true;
        }
    }
    false
}

fn parse(args: Vec<OsString>, json_mode: bool) -> Result<ParseOutcome, CliError> {
    let parsed = help::command()
        .try_get_matches_from(args)
        .and_then(|mut matches| Cli::from_arg_matches_mut(&mut matches));
    match parsed {
        Ok(cli) => Ok(ParseOutcome::Run(cli)),
        Err(error) if error.use_stderr() => {
            if json_mode {
                Err(CliError::private("invalid_arguments", error.to_string()))
            } else {
                error.exit()
            }
        }
        Err(error) if json_mode => {
            let kind = error.kind();
            let (command, data) = if kind == clap::error::ErrorKind::DisplayVersion {
                (
                    "version",
                    json!({"name": "arca", "version": env!("CARGO_PKG_VERSION")}),
                )
            } else {
                ("help", json!({"text": error.to_string()}))
            };
            Ok(ParseOutcome::Early(output::CommandOutput::new(
                command, false, data, "",
            )))
        }
        Err(error) => {
            let _ = error.print();
            std::process::exit(0);
        }
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    if cli.command.is_none() {
        if cli.json || cli.yes || cli.dry_run || cli.accept_risk || cli.no_color {
            return Err(CliError::new(
                "command_required",
                "global flags require an explicit command",
            ));
        }
        if arca_core::brand::env_os("ARCA_NO_TUI").is_some() {
            help::command()
                .print_help()
                .map_err(|error| CliError::private("help_output_failed", error.to_string()))?;
            println!();
            return Ok(());
        }
        arca_core::logging::init_session("tui");
        bootstrap(false);
        return tui::run().map_err(|error| CliError::private("tui_failed", error.to_string()));
    }

    // Core gates each actual use case; the CLI must not maintain a second
    // capability-blocker policy based on command names.
    let backend = bootstrap(cli.json);

    let is_upgrade = matches!(cli.command, Some(command::Command::Upgrade));
    let output = dispatch(&cli)?.with_backend(&backend);
    render_success(&output, cli.json)?;

    if !cli.json && !is_upgrade {
        if let Some(notice) =
            arca_core::application::update::passive_check_notice(env!("CARGO_PKG_VERSION"))
        {
            eprintln!("\n{notice}");
        }
    }
    Ok(())
}

fn bootstrap(json_mode: bool) -> arca_core::application::bootstrap::BootstrapReport {
    let outcome = arca_core::application::ArcaCore::bootstrap();
    if !json_mode {
        for warning in &outcome.warnings {
            eprintln!("Arca startup warning: {}", warning.message);
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_parse_error_becomes_structured_cli_error() {
        let error = parse(vec!["arca".into(), "--json".into(), "list".into()], true).unwrap_err();
        assert_eq!(error.code, "invalid_arguments");
    }

    #[test]
    fn json_looking_server_arg_does_not_select_json_output() {
        let ParseOutcome::Run(cli) = parse(
            vec![
                "arca".into(),
                "mcp".into(),
                "add".into(),
                "edge::stdio".into(),
                "--command".into(),
                "npx".into(),
                "--arg".into(),
                "--json".into(),
            ],
            true,
        )
        .unwrap() else {
            panic!("expected parsed CLI");
        };
        assert_eq!(runtime_output_modes(&cli), (false, false));
    }

    #[test]
    fn option_aware_json_detection_skips_server_argument_values() {
        let args = vec![
            "arca".into(),
            "mcp".into(),
            "add".into(),
            "invalid".into(),
            "--arg".into(),
            "--json".into(),
        ];
        assert!(!requested_json_mode(&args));
        assert!(requested_json_mode(&[
            "arca".into(),
            "--json".into(),
            "mcp".into(),
            "list".into(),
        ]));
    }
}
