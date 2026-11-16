use std::{io::{self, Write}, process::Command};

use arca_core::application::playground::Playground;
use serde_json::json;

use crate::{command::Cli, output::{render_success, CliError, CommandOutput}};

pub(crate) fn run(cli: &Cli, no_open: bool) -> Result<CommandOutput, CliError> {
    cli.reject_mutation_options()?;
    let playground = Playground::start()
        .map_err(|message| CliError::new("playground_start_failed", message))?;
    let url = playground.launch_url();
    let opened = !no_open && open_browser(&url);
    let human = format!(
        "Model playground\n{url}\n{}\nPress Ctrl+C to stop the local server.",
        if opened { "Opened in your browser." } else { "Open the link in your browser." },
    );
    render_success(&CommandOutput::new(
        "model.playground", false, json!({"url": url, "opened": opened}), human,
    ), cli.json)?;
    io::stdout().flush().map_err(|_| CliError::new("output_failed", "Could not print the local URL"))?;
    loop {
        std::thread::park();
    }
}

fn open_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");

    command.arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status().is_ok_and(|status| status.success())
}
