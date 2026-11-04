//! On-demand, read-only configuration projection for a single discovered MCP.

use crate::domain::error::{CoreError, CoreResult};
use crate::domain::mcp::{ConfiguredMcpObservation, McpConfigurationDocument, McpConfigurationView, McpInspectionRequest, McpInspectionSource};
use crate::domain::types::AgentDefinition;
use super::adapter::{get_agent_adapter_for, Adapter};
use super::json_adapter::JsonAdapter;
use super::scanner::{collapse_home, expand_tilde};
use std::fs::File;
use std::io::Read;
use std::path::Path;

mod redaction;

const MAX_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn inspect(request: McpInspectionRequest) -> CoreResult<McpConfigurationView> {
    let agents = crate::agents::load_agents();
    let definition = agents.get(&request.agent_id)
        .ok_or_else(|| CoreError::new("unknown_agent", "未找到此 Agent。"))?;
    let (server_id, documents) = match request.source {
        McpInspectionSource::Configured { key } => {
            crate::domain::assets::validate_mcp_asset_key(&key)
                .map_err(|_| CoreError::new("invalid_mcp_identity", "MCP 标识无效。"))?;
            let (name, _) = key.rsplit_once("::").expect("validated MCP identity");
            (name.to_string(), vec![configured_document(definition, &request.agent_id, name)?])
        }
        McpInspectionSource::ConfiguredName { name } => {
            if !valid_name(&name) { return Err(CoreError::new("invalid_mcp_identity", "MCP 名称无效。")); }
            let document = configured_document(definition, &request.agent_id, &name)?;
            (name, vec![document])
        }
        McpInspectionSource::Host { source_fingerprint } => {
            if source_fingerprint.len() != 64 || !source_fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(stale());
            }
            // This read is independent of the model credential/workspace gate.
            let resources = crate::assets::inventory::list_native_resources(&request.agent_id);
            let observed = resources.host_mcps.iter().find(|item| item.source_fingerprint == source_fingerprint)
                .ok_or_else(stale)?;
            if observed.source_paths.len() > 32 { return Err(CoreError::new("mcp_configuration_limit", "MCP 来源数量超过查看限制。")); }
            let mut documents = Vec::new();
            for source in &observed.source_paths {
                let path = expand_tilde(source);
                let supported = matches!(path.extension().and_then(|extension| extension.to_str()), Some("json" | "jsonc" | "toml" | "yaml" | "yml"));
                if !supported {
                    documents.push(unavailable(&path, "此来源由宿主代码生成连接，没有可独立读取的 MCP 声明配置。"));
                    continue;
                }
                let text = crate::resources::plugin::open_declared_mcp_configuration(&path)
                    .and_then(|file| read_bounded(file, &path));
                documents.push(match text {
                    Ok(text) => document(&path, host_entry(definition, &request.agent_id, &path, &text, &observed.name)),
                    Err(()) => unavailable(&path, "此来源没有可安全读取的独立 MCP 声明配置。"),
                });
            }
            if documents.is_empty() { documents.push(unavailable(Path::new(""), "此 MCP 由宿主管理，没有独立的声明配置文件。")); }
            (observed.server_id.clone(), documents)
        }
    };
    Ok(McpConfigurationView { agent_id: request.agent_id, server_id, documents })
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 240 && !name.chars().any(char::is_control)
}

fn configured_document(definition: &AgentDefinition, agent_id: &str, name: &str) -> CoreResult<McpConfigurationDocument> {
    let target = definition.global.as_deref()
        .ok_or_else(|| CoreError::new("mcp_configuration_unavailable", "此 Agent 没有已核验的用户级 MCP 配置入口。"))?;
    let path = expand_tilde(target);
    Ok(match open_configuration(&path).and_then(|file| read_bounded(file, &path)) {
        Ok(text) => document(&path, get_agent_adapter_for(definition, agent_id).inspect_text(&text, name)),
        Err(()) => unavailable(&path, "配置文件不存在、不可读或已变化，未返回配置内容。"),
    })
}

/// Configuration names have a read path separate from connection decoding and
/// workspace ownership. A transport that cannot be managed must remain visible.
pub(crate) fn configured_observations(agent_id: &str) -> (Vec<ConfiguredMcpObservation>, bool) {
    let agents = crate::agents::load_agents();
    let Some(definition) = agents.get(agent_id) else { return (Vec::new(), false); };
    let Some(target) = definition.global.as_deref() else { return (Vec::new(), false); };
    let path = expand_tilde(target);
    if std::fs::symlink_metadata(&path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
        return (Vec::new(), false);
    }
    let names = open_configuration(&path).and_then(|file| read_bounded(file, &path))
        .and_then(|text| get_agent_adapter_for(definition, agent_id).declaration_names(&text).map_err(|_| ()));
    let Ok(mut names) = names else { return (Vec::new(), true); };
    if names.len() > 4096 || names.iter().any(|name| !valid_name(name)) { return (Vec::new(), true); }
    names.sort();
    if names.windows(2).any(|pair| pair[0] == pair[1]) { return (Vec::new(), true); }
    let source_path = collapse_home(&path.to_string_lossy());
    (names.into_iter().map(|name| ConfiguredMcpObservation {
        agent_id: agent_id.into(), name, source_path: source_path.clone(),
    }).collect(), false)
}

fn stale() -> CoreError {
    CoreError::new("mcp_observation_changed", "MCP 来源已变化，请刷新后重新查看。")
}

fn unavailable(path: &Path, reason: &str) -> McpConfigurationDocument {
    McpConfigurationDocument { source_path: collapse_home(&path.to_string_lossy()), configuration: None, unavailable_reason: Some(reason.into()) }
}

fn document(path: &Path, value: Result<Option<serde_json::Value>, String>) -> McpConfigurationDocument {
    match value {
        Ok(Some(value)) => match redaction::redact(&value) {
            Ok(value) => McpConfigurationDocument { source_path: collapse_home(&path.to_string_lossy()), configuration: Some(value), unavailable_reason: None },
            Err(()) => unavailable(path, "配置超过安全查看限制，未返回配置内容。"),
        },
        Ok(None) => unavailable(path, "未在此来源中找到该 MCP 的声明配置。"),
        // Parser diagnostics can contain native source lines and credentials.
        Err(_) => unavailable(path, "配置无法安全解析或存在歧义，未返回配置内容。"),
    }
}

fn host_entry(definition: &AgentDefinition, agent_id: &str, path: &Path, text: &str, name: &str) -> Result<Option<serde_json::Value>, String> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json" | "jsonc") => {
            for key in ["mcpServers", "mcpServers.mcpServers"] {
                let adapter = JsonAdapter::with_spec_and_key_path(key, super::codec::Codec::Standard, Default::default(), key.contains('.'));
                if let Some(value) = adapter.inspect_text(text, name)? { return Ok(Some(value)); }
            }
            if definition.format == "json" {
                if let Some(value) = get_agent_adapter_for(definition, agent_id).inspect_text(text, name)? { return Ok(Some(value)); }
            }
            if matches!(path.file_name().and_then(|name| name.to_str()), Some("mcp.json" | ".mcp.json")) {
                return JsonAdapter::new("").inspect_text(text, name);
            }
            Ok(None)
        }
        Some(extension) if extension == definition.format || (extension == "yml" && definition.format == "yaml") =>
            get_agent_adapter_for(definition, agent_id).inspect_text(text, name),
        _ => Ok(None),
    }
}

fn open_configuration(path: &Path) -> Result<File, ()> {
    #[cfg(unix)] {
        use rustix::fs::{open, Mode, OFlags};
        open(path, OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK, Mode::empty()).map(File::from).map_err(|_| ())
    }
    #[cfg(not(unix))] { File::open(path).map_err(|_| ()) }
}

fn read_bounded(mut file: File, path: &Path) -> Result<String, ()> {
    let before = file.metadata().map_err(|_| ())?;
    if !before.is_file() || before.len() > MAX_BYTES { return Err(()); }
    let mut text = String::new();
    (&mut file).take(MAX_BYTES + 1).read_to_string(&mut text).map_err(|_| ())?;
    let after = file.metadata().map_err(|_| ())?;
    let bound = std::fs::metadata(path).map_err(|_| ())?;
    if text.len() as u64 != before.len() || before.len() != after.len()
        || before.modified().map_err(|_| ())? != after.modified().map_err(|_| ())? { return Err(()); }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != bound.dev() || before.ino() != bound.ino()
            || before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() { return Err(()); }
    }
    Ok(text)
}
