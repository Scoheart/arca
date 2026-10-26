//! Capture adapts the shared network context to its supported transport.
use crate::network;
use std::path::PathBuf;
pub use crate::network::ResolvedProxy as CaptureProxy;

pub(super) fn normalize_proxy(value: &str) -> Result<String, String> {
    let value = network::canonical_proxy_url(value)?;
    if !value.starts_with("http://") {
        return Err("当前抓包传输需要无认证 HTTP 上游；所选代理不支持，未切换直连。".into());
    }
    Ok(value)
}

pub fn capture_proxy(agent_id: &str) -> Result<CaptureProxy, String> {
    let mut route = crate::application::network::resolve_agent_proxy(agent_id)?;
    if let Some(value) = &route.proxy_url { route.proxy_url = Some(normalize_proxy(value)?); }
    Ok(route)
}

/// Generate only the public certificate entry point. Trust is a separate,
/// explicit action performed by the user in macOS Keychain Access.
pub fn prepare_certificate() -> Result<PathBuf, String> {
    if super::tool().is_none() { super::install_engine()?; }
    let path = dirs::home_dir().ok_or("无法找到用户目录。")?.join(".mitmproxy/mitmproxy-ca-cert.pem");
    super::launch::initialize_ca(&path)?;
    let content = super::read_bytes(&path, 2 * 1024 * 1024)?;
    if !String::from_utf8_lossy(&content).contains("-----BEGIN CERTIFICATE-----") {
        return Err("抓包证书无效，请检查 mitmproxy 的证书文件。".into());
    }
    Ok(path)
}
