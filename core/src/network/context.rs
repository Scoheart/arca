//! Ordered proxy sources shared by network consumers. Source readers describe
//! configuration; resolution never depends on an Agent name or a port number.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::{Path, PathBuf}};

pub const PROXY_ENVIRONMENT_KEYS: [&str; 6] = [
    "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY", "http_proxy", "HTTP_PROXY",
];

#[derive(Clone, Serialize)]
pub struct ResolvedProxy { pub proxy_url: Option<String>, pub source: String }

/// Sources are evaluated lazily in caller-supplied order. A missing source
/// permits the next reader; a broken or unsupported source is an error.
pub struct ProxySource<'a> {
    name: String,
    read: Box<dyn Fn() -> Result<Option<String>, String> + Send + Sync + 'a>,
}
impl<'a> ProxySource<'a> {
    pub fn new(name: impl Into<String>, read: impl Fn() -> Result<Option<String>, String> + Send + Sync + 'a) -> Self {
        Self { name: name.into(), read: Box::new(read) }
    }
}

pub fn resolve_proxy<'a>(sources: impl IntoIterator<Item = ProxySource<'a>>) -> Result<ResolvedProxy, String> {
    for source in sources {
        if let Some(value) = (source.read)()? {
            return Ok(ResolvedProxy { proxy_url: Some(super::canonical_proxy_url(&value)?), source: source.name });
        }
    }
    Ok(ResolvedProxy { proxy_url: None, source: "未配置上游代理，将直连".into() })
}

pub fn environment_proxy(values: &BTreeMap<String, String>) -> Option<String> {
    PROXY_ENVIRONMENT_KEYS.into_iter()
        .find_map(|key| values.get(key).filter(|value| !value.trim().is_empty()).cloned())
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyConfigFormat { Json, Toml, Yaml }

/// Audited native configuration locations. New consumers can provide the same
/// descriptor without adding another branch to the resolver or capture layer.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyConfigSource {
    pub default_directory: String,
    pub directory_environment: Option<String>,
    pub file: String,
    pub format: ProxyConfigFormat,
    pub pointer: String,
}

fn expand_path(value: &str) -> Result<PathBuf, String> {
    let path = if value == "~" || value.starts_with("~/") {
        dirs::home_dir().ok_or("无法找到网络配置目录。")?.join(value.strip_prefix("~/").unwrap_or(""))
    } else { PathBuf::from(value) };
    if !path.is_absolute() { return Err("网络配置目录必须是绝对路径。".into()); }
    Ok(path)
}

pub(super) fn read_bounded(path: &Path, maximum: u64) -> Result<Option<Vec<u8>>, String> {
    let mut options = fs::OpenOptions::new(); options.read(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法安全读取网络配置，未切换直连。".into()),
    };
    let metadata = file.metadata().map_err(|_| "无法读取网络配置。")?;
    if !metadata.is_file() || metadata.len() > maximum { return Err("网络配置不是普通文件或超过读取上限。".into()); }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes).map_err(|_| "无法读取网络配置。")?;
    if bytes.len() as u64 > maximum { return Err("网络配置超过读取上限。".into()); }
    Ok(Some(bytes))
}

pub fn config_proxy(source: &ProxyConfigSource, variables: &BTreeMap<String, String>) -> Result<Option<String>, String> {
    let directory = source.directory_environment.as_ref().and_then(|key| variables.get(key))
        .unwrap_or(&source.default_directory);
    let relative = Path::new(&source.file);
    if relative.is_absolute() || relative.components().any(|part| !matches!(part, std::path::Component::Normal(_)))
        || !source.pointer.starts_with('/') {
        return Err("网络配置来源描述无效。".into());
    }
    let Some(bytes) = read_bounded(&expand_path(directory)?.join(relative), 1024 * 1024)? else { return Ok(None); };
    let invalid = |_| "网络配置无法解析，未切换直连；请检查设置或指定上游代理。".to_owned();
    let document: serde_json::Value = match source.format {
        ProxyConfigFormat::Json => serde_json::from_slice(&bytes).map_err(invalid)?,
        ProxyConfigFormat::Toml => {
            let text = std::str::from_utf8(&bytes).map_err(|_| "网络配置必须是 UTF-8 文本。")?;
            let value: toml::Value = toml::from_str(text).map_err(|_| "网络配置无法解析，未切换直连。")?;
            serde_json::to_value(value).map_err(invalid)?
        },
        ProxyConfigFormat::Yaml => serde_yaml::from_slice(&bytes).map_err(|_| "网络配置无法解析，未切换直连。")?,
    };
    match document.pointer(&source.pointer) {
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(serde_json::Value::Null | serde_json::Value::String(_)) | None => Ok(None),
        _ => Err("网络代理配置必须是地址文本。".into()),
    }
}
