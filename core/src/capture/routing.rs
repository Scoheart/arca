//! Resolve ordinary capture defaults in Core, shared by Desktop and CLI.
use std::{collections::BTreeMap, path::PathBuf};

pub(super) fn automatic_proxy() -> Result<Option<String>, String> {
    if let Some(value) = crate::network::configured_proxy_url()? { return Ok(Some(value)); }
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() { return Ok(Some(value)); }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let settings = super::output("/usr/sbin/scutil", &["--proxy"])
            .ok_or("无法读取系统网络出口，请在抓包设置中选择出口后重试。")?;
        let fields: BTreeMap<_, _> = settings.lines().filter_map(|line| line.trim().split_once(" : ")).collect();
        for scheme in ["HTTPS", "HTTP"] {
            if fields.get(format!("{scheme}Enable").as_str()) == Some(&"1") {
                let host = fields.get(format!("{scheme}Proxy").as_str()).ok_or("系统代理缺少主机地址。")?;
                let port = fields.get(format!("{scheme}Port").as_str()).ok_or("系统代理缺少端口。")?;
                let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
                return Ok(Some(format!("http://{host}:{port}")));
            }
        }
        if ["ProxyAutoConfigEnable", "ProxyAutoDiscoveryEnable", "SOCKSEnable"].iter().any(|key| fields.get(key) == Some(&"1")) {
            return Err("当前网络使用自动代理或 SOCKS，请在抓包设置中选择可用的 HTTP 代理；未切换直连。".into());
        }
    }
    Ok(None)
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
