use clap::Subcommand;
use arca_core::application::network;
use serde_json::json;

use crate::command::Cli;
use crate::output::{query_output, CliError, CommandOutput};
use crate::projection::safe_url;
use crate::review::execute_direct_mutation;

#[derive(Debug, Subcommand)]
pub enum NetworkCommand {
    /// Manage the proxy shared by Arca's sources, Skills and update checks.
    Proxy {
        #[command(subcommand)]
        command: ProxyCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProxyCommand {
    /// Show the current shared proxy.
    Show,
    /// Set an HTTP/SOCKS proxy URL. Credentials in proxy URLs are unsupported.
    Set { url: String },
    /// Remove the shared proxy setting.
    Clear,
}

pub fn dispatch(cli: &Cli, command: &NetworkCommand) -> Result<CommandOutput, CliError> {
    let NetworkCommand::Proxy { command } = command;
    if matches!(command, ProxyCommand::Show) {
        cli.reject_mutation_options()?;
        let settings = network::get_proxy_settings().map_err(CliError::from_legacy)?;
        return query_output("network.proxy.show", json!({"proxy_url": settings.proxy_url.as_deref().map(safe_url)}));
    }
    let options = cli.mutation_options();
    options.validate()?;
    let (name, url) = match command {
        ProxyCommand::Set { url } => ("network.proxy.set", Some(url.clone())),
        ProxyCommand::Clear => ("network.proxy.clear", None),
        ProxyCommand::Show => unreachable!(),
    };
    let current = network::get_proxy_settings().map_err(CliError::from_legacy)?;
    let summary = json!({"proxy_url": url.as_deref().map(safe_url)});
    let review = format!("Set the shared Arca network proxy: {}", url.as_deref().map(safe_url).unwrap_or_else(|| "none".into()));
    execute_direct_mutation(name, options, summary, &review, current.proxy_url == url, || {
        network::set_proxy_url(url).map(|_| ()).map_err(CliError::from_legacy)
    })
}
