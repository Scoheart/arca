//! Shared network preferences and Agent runtime contexts.
use crate::network::{self, ProxyConfigSource, ProxySource, ResolvedProxy, PROXY_ENVIRONMENT_KEYS};
use std::{collections::BTreeMap, sync::LazyLock};

pub use crate::settings::NetworkSettings;

pub fn get_proxy_settings() -> Result<NetworkSettings, String> {
    super::gate::read(crate::network::get_proxy_settings)
}

pub fn set_proxy_url(proxy_url: Option<String>) -> Result<NetworkSettings, String> {
    super::gate::write_independent(|| crate::network::set_proxy_url(proxy_url))
}

static NATIVE_SOURCES: LazyLock<BTreeMap<String, Vec<ProxyConfigSource>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/agent-network-sources.json"))
        .expect("embedded network source descriptors must be valid")
});

pub fn resolve_agent_proxy(agent_id: &str) -> Result<ResolvedProxy, String> {
    super::gate::read(|| resolve_agent_proxy_unlocked(agent_id))
}

fn resolve_agent_proxy_unlocked(agent_id: &str) -> Result<ResolvedProxy, String> {
    let info = super::agent_launch::info_unlocked(agent_id)?;
    let variables = match info.resolved_target {
        Some(crate::domain::agents::LaunchTarget::Cli { env, .. } | crate::domain::agents::LaunchTarget::App { env, .. }) => env,
        _ => BTreeMap::new(),
    };
    let descriptors = NATIVE_SOURCES.get(agent_id).cloned().unwrap_or_default();
    let inherited: BTreeMap<_, _> = PROXY_ENVIRONMENT_KEYS.into_iter()
        .chain(descriptors.iter().filter_map(|source| source.directory_environment.as_deref()))
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value))).collect();
    let mut native_variables = inherited.clone();
    native_variables.extend(variables.clone());

    let mut sources = vec![
        ProxySource::new("Arca 代理", network::configured_proxy_url),
        ProxySource::new("Agent 启动环境", || Ok(network::environment_proxy(&variables))),
        ProxySource::new("Arca 进程环境", || Ok(network::environment_proxy(&inherited))),
        ProxySource::new("终端环境", network::shell_proxy),
    ];
    for descriptor in descriptors {
        let context = &native_variables;
        sources.push(ProxySource::new("Agent 原生网络配置", move || network::config_proxy(&descriptor, context)));
    }
    sources.push(ProxySource::new("macOS 系统代理", network::system_proxy));
    network::resolve_proxy(sources)
}
