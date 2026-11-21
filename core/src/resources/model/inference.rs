//! Text inference using an exact, saved Model Profile.
mod stream;
mod wire;

use std::{io::Read, time::{Duration, Instant}};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    domain::types::{AuthRequirement, ModelProfile, ModelProtocol, ModelProviderConfig},
    settings::{load_settings_strict, Settings},
};

#[derive(Serialize)]
pub(crate) struct Target {
    pub id: String,
    pub name: String,
    pub model: String,
    pub protocol: ModelProtocol,
    pub provider: String,
    pub origin: String,
    pub revision: String,
    pub streaming_available: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    profile_id: String,
    revision: String,
    prompt: String,
    #[serde(default)]
    instructions: String,
    temperature: Option<f64>,
    top_p: Option<f64>,
    max_output_tokens: Option<u32>,
    reasoning_effort: Option<String>,
    #[serde(default)]
    streaming: bool,
    #[serde(default)]
    extra: Map<String, Value>,
}

#[derive(Serialize)]
pub(crate) struct Failure {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameter: Option<String>,
    #[serde(skip)]
    pub status: u16,
}

impl Failure {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into(), upstream_status: None, parameter: None }
    }

    fn bad(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_request", message)
    }

    fn from_upstream(status: u16, body: Option<Value>) -> Self {
        let (code, message) = match status {
            200..=299 => ("provider_error", "Provider 返回了错误。"),
            300..=399 => ("provider_redirect", "Provider 返回重定向，请核对连接地址。"),
            400 | 422 => ("provider_parameters", "Provider 拒绝了请求。请检查该模型支持的参数。"),
            401 => ("provider_unauthorized", "Provider 拒绝了凭据。请检查连接的 API Key。"),
            402 => ("provider_balance", "Provider 拒绝了本次计费。请检查余额或用量限制。"),
            403 => ("provider_forbidden", "该凭据没有访问模型的权限。"),
            404 => ("provider_not_found", "Provider 找不到端点或模型。请检查连接和 Model ID。"),
            429 => ("provider_rate_limited", "请求触发限流或额度限制。"),
            _ => ("provider_error", "Provider 未能完成请求。"),
        };
        let mut error = Self::new(502, code, message);
        error.upstream_status = Some(status);
        // Provider bodies can echo arbitrary headers or message contents. Only
        // reflect known parameter names, never the upstream diagnostic body.
        error.parameter = body.as_ref()
            .and_then(|value| value.pointer("/error/param"))
            .and_then(Value::as_str)
            .filter(|name| wire::PUBLIC_PARAMETERS.contains(name))
            .map(str::to_owned);
        error
    }
}

#[derive(Serialize)]
pub(crate) struct Completed {
    text: String,
    usage: wire::Usage,
    finish: Option<&'static str>,
    elapsed_ms: u64,
    first_token_ms: Option<u64>,
    streamed: bool,
    upstream_status: u16,
}

pub(crate) struct Prepared {
    target: Target,
    url: url::Url,
    format: stream::Format,
    provider: ModelProviderConfig,
    body: Value,
}

#[derive(Serialize)]
pub(crate) struct Preview<'a> {
    method: &'static str,
    url: &'a str,
    accept: &'static str,
    authentication_header: Option<&'static str>,
    body: &'a Value,
}

struct Route<'a> {
    profile: &'a ModelProfile,
    provider: &'a ModelProviderConfig,
    url: String,
}

impl<'a> Route<'a> {
    fn at_revision(settings: &'a Settings, id: &str, revision: &str) -> Result<(Self, Target), Failure> {
        let profile = settings.model_profiles.as_ref().and_then(|rows| rows.get(id))
            .ok_or_else(|| Failure::new(404, "model_not_found", "这个模型已移除，请刷新模型列表。"))?;
        let route = Self::from_profile(settings, profile)?;
        let target = route.target()?;
        if revision != target.revision {
            return Err(Failure::new(409, "model_changed", "模型或连接已修改，请刷新列表后重试。"));
        }
        Ok((route, target))
    }

    fn from_profile(settings: &'a Settings, profile: &'a ModelProfile) -> Result<Self, Failure> {
        let config_error = || Failure::new(409, "model_route_invalid", "请在 Arca 中检查这个模型的连接和协议。");
        let provider = super::credential_service::provider_for_profile(settings, profile)
            .ok_or_else(config_error)?;
        let protocol = provider.protocols.get(&profile.protocol).ok_or_else(config_error)?;
        if matches!(profile.model.trim(), "" | "." | "..") {
            return Err(config_error());
        }
        let url = super::full_request_url(&provider.base_url, &protocol.endpoint_path)
            .map_err(|_| config_error())?
            .replace("{model}", &super::curl::url_component(&profile.model));
        Ok(Self { profile, provider, url })
    }

    fn target(&self) -> Result<Target, Failure> {
        let bytes = serde_json::to_vec(&(self.profile, self.provider)).map_err(|_| invalid_catalog())?;
        let url = url::Url::parse(&self.url).map_err(|_| invalid_catalog())?;
        Ok(Target {
            id: self.profile.id.clone(),
            name: self.profile.name.clone(),
            model: self.profile.model.clone(),
            protocol: self.profile.protocol.clone(),
            provider: self.provider.name.clone(),
            origin: url.origin().ascii_serialization(),
            revision: hex::encode(Sha256::digest(bytes)),
            streaming_available: stream::Format::route(&self.profile.protocol, &url).is_some(),
        })
    }
}

fn invalid_catalog() -> Failure {
    Failure::new(503, "model_catalog_unavailable", "无法读取 Arca 的模型资产，请检查 Models 页面。")
}

pub(crate) fn targets() -> Result<Vec<Target>, Failure> {
    let settings = load_settings_strict().map_err(|_| invalid_catalog())?;
    let mut targets: Vec<Target> = settings.model_profiles.iter().flat_map(|profiles| profiles.values())
        .filter_map(|profile| Route::from_profile(&settings, profile).and_then(|route| route.target()).ok())
        .collect();
    targets.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    Ok(targets)
}

pub(crate) fn prepare(request: Request) -> Result<Prepared, Failure> {
    if request.prompt.trim().is_empty() || request.prompt.len() > 65_536 || request.instructions.len() > 16_384 {
        return Err(Failure::bad("请输入提示词。提示词最多 64 KiB，系统指令最多 16 KiB。"));
    }
    let settings = load_settings_strict().map_err(|_| invalid_catalog())?;
    let (route, target) = Route::at_revision(&settings, &request.profile_id, &request.revision)?;
    let profile = route.profile;
    let url = url::Url::parse(&route.url).map_err(|_| invalid_catalog())?;
    let (format, url) = if request.streaming {
        stream::Format::route(&profile.protocol, &url).ok_or_else(|| {
            Failure::bad("这个自定义端点没有对应的流式地址。请选择完整返回。")
        })?
    } else {
        (stream::Format::Json, url)
    };
    let body = wire::encode(profile, &route.provider.provider, request)?;
    Ok(Prepared { target, body, url, format, provider: route.provider.clone() })
}

impl Prepared {
    pub(crate) fn preview(&self) -> Preview<'_> {
        Preview {
            method: "POST",
            url: self.url.as_str(),
            accept: self.format.accept(),
            authentication_header: (self.provider.auth_requirement != AuthRequirement::None).then(|| match self.target.protocol {
                ModelProtocol::AnthropicMessages => "x-api-key",
                ModelProtocol::GeminiGenerateContent | ModelProtocol::GeminiInteractions => "x-goog-api-key",
                _ => "Authorization",
            }),
            body: &self.body,
        }
    }

    fn credential(&self) -> Result<Option<Zeroizing<String>>, Failure> {
        let bad_credential = |message: String| {
            let (code, display) = match message.split(':').next() {
                Some("credential_store_bw_locked") =>
                    ("credential_locked", "请解锁 Bitwarden CLI 并检查 Arca 的凭据存储连接。"),
                Some("credential_missing") =>
                    ("credential_missing", "此连接的凭据缺失。请在 Arca 的 Provider 设置中检查凭据来源。"),
                _ => ("credential_unavailable", "无法读取连接凭据，请检查 Arca 的 Provider 设置。"),
            };
            Failure::new(409, code, display)
        };
        let settings = load_settings_strict().map_err(|_| invalid_catalog())?;
        let Some(key) = super::credential_service::read_provider(&settings, &self.provider).map_err(bad_credential)? else {
            return Ok(None);
        };
        let token = std::str::from_utf8(&key)
            .map_err(|_| Failure::bad("API Key 必须是单行文本。"))?;
        if token.is_empty() || token.contains(['\r', '\n', '\0']) {
            return Err(Failure::bad("API Key 必须是单行文本。"));
        }
        Ok(Some(Zeroizing::new(token.to_owned())))
    }

    pub(crate) fn execute(self) -> Result<Completed, Failure> {
        self.execute_with(|_| Ok(()))
    }

    pub(crate) fn execute_with(
        self,
        mut emit: impl FnMut(&str) -> Result<(), Failure>,
    ) -> Result<Completed, Failure> {
        let access = self.credential()?;
        // Keychain and credential helpers may wait. Check the selected route
        // again after obtaining the key so a concurrent Provider edit cannot
        // send the newly resolved key or the prompt to the old endpoint.
        crate::application::gate::read(|| {
            let settings = load_settings_strict().map_err(|_| invalid_catalog())?;
            Route::at_revision(&settings, &self.target.id, &self.target.revision).map(|_| ())
        })?;
        let config = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(10))).timeout_global(Some(Duration::from_secs(120)));
        // A local model server should be reachable even with an Arca proxy set.
        let local = match self.url.host() {
            Some(url::Host::Ipv4(host)) => host.is_loopback(),
            Some(url::Host::Ipv6(host)) => host.is_loopback(),
            Some(url::Host::Domain("localhost")) => true,
            _ => false,
        };
        let agent = if local {
            config.proxy(None).tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            ).build().new_agent()
        } else {
            crate::network::build_ureq_agent(config)
                .map_err(|_| Failure::new(503, "proxy_unavailable", "请检查 Arca 的网络代理。"))?
        };
        let mut request = agent.post(self.url.as_str())
            .header("Accept", self.format.accept())
            .header("Content-Type", "application/json");
        if self.target.protocol == ModelProtocol::AnthropicMessages {
            request = request.header("anthropic-version", "2023-06-01");
        }
        let bearer;
        if let Some(token) = access.as_deref() {
            request = match self.target.protocol {
                ModelProtocol::AnthropicMessages => request.header("x-api-key", token.as_str()),
                ModelProtocol::GeminiGenerateContent | ModelProtocol::GeminiInteractions =>
                    request.header("x-goog-api-key", token.as_str()),
                _ => {
                    bearer = Zeroizing::new(format!("Bearer {token}"));
                    request.header("Authorization", bearer.as_str())
                }
            };
        }
        let started = Instant::now();
        let mut response = request.send(self.body.to_string()).map_err(network_failure)?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let mut bytes = Vec::new();
            let body = response.body_mut().as_reader().take(32_768).read_to_end(&mut bytes)
                .ok().and_then(|_| serde_json::from_slice(&bytes).ok());
            return Err(Failure::from_upstream(status, body));
        }
        let streamed = self.format.matches(response.headers().get("content-type").and_then(|value| value.to_str().ok()));
        let mut first_token_ms = None;
        let (text, usage, finish) = if streamed {
            let reader = response.body_mut().with_config().limit(stream::MAX_WIRE_BYTES).reader();
            stream::decode(&self.target.protocol, self.format, reader, access.as_deref().map(String::as_str), status, &mut |delta| {
                if first_token_ms.is_none() && !delta.trim().is_empty() {
                    first_token_ms = Some(started.elapsed().as_millis() as u64);
                }
                emit(delta)
            })?
        } else {
            let body = response.body_mut().with_config().limit(2_097_152).read_to_string()
                .map_err(network_failure)?;
            let result: Value = serde_json::from_str(&body)
                .map_err(|_| Failure::new(502, "invalid_response", "Provider 未返回可读取的响应。请检查协议与端点。"))?;
            if result.get("error").is_some_and(|error| !error.is_null()) {
                return Err(Failure::from_upstream(status, Some(result)));
            }
            let (mut text, usage, finish) = wire::decode(&self.target.protocol, &result)?;
            if let Some(token) = access.as_deref() {
                text = text.replace(token.as_str(), "[凭据已隐藏]");
            }
            (text, usage, finish)
        };
        Ok(Completed {
            text, usage, finish, first_token_ms, streamed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            upstream_status: status,
        })
    }
}

fn network_failure(error: ureq::Error) -> Failure {
    if matches!(error, ureq::Error::Timeout(_)) {
        Failure::new(504, "provider_timeout", "Provider 未在时限内完成请求。")
    } else {
        Failure::new(502, "provider_connection", "无法读取 Provider 的响应。请检查地址、网络或代理。")
    }
}
