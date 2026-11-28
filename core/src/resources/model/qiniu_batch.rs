//! Qiniu's asynchronous batch API uses the saved Qiniu Provider credential.

use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;
use zeroize::Zeroizing;

use super::credential_service;
use crate::settings::load_settings_strict;

const BASE: &str = "https://api.qnaigc.com/v1";
const JOBS: &str = "https://api.qnaigc.com/v1/batchjob/inference";

#[derive(Serialize)]
pub struct QiniuBatchRequest<'a> {
    pub name: &'a str,
    pub model: &'a str,
    pub input_files_url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
}

#[derive(Deserialize, Serialize)]
pub struct QiniuBatchJob {
    pub id: String,
    pub status: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub status_message: Option<String>,
    #[serde(skip_serializing)]
    output_files_url: Option<String>,
}

#[derive(Deserialize)]
struct CreatedJob {
    id: String,
}

pub fn submit(provider_id: &str, request: QiniuBatchRequest<'_>) -> Result<String, String> {
    let valid_text = |text: &str, limit| !text.trim().is_empty() && text.chars().count() <= limit;
    if !valid_text(request.name, 100)
        || request.model.trim().is_empty()
        || request.description.is_some_and(|text| text.chars().count() > 255)
        || !valid_text(request.input_files_url, 1000)
        || http_url(request.input_files_url).is_none()
    {
        return Err("qiniu_batch_invalid: Enter a name, a model ID, and a valid public JSONL URL".into());
    }
    let credential = authorization(provider_id)?;
    let body = serde_json::to_vec(&request)
        .map_err(|_| "qiniu_batch_invalid: Could not encode the job request")?;
    let response = agent(Duration::from_secs(30))?.post(JOBS)
        .header("Authorization", credential.as_str())
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send(body.as_slice())
        .map_err(|_| "qiniu_batch_network: Could not submit the Qiniu job")?;
    let job: CreatedJob = decode_json(response)?;
    Ok(job.id)
}

pub fn status(provider_id: &str, job_id: &str) -> Result<QiniuBatchJob, String> {
    if !job_id.starts_with("bat-") || job_id.len() > 128
        || !job_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("qiniu_batch_invalid: Enter a valid Qiniu batch job ID".into());
    }
    let credential = authorization(provider_id)?;
    let response = agent(Duration::from_secs(30))?.get(format!("{JOBS}/{job_id}"))
        .header("Authorization", credential.as_str())
        .header("Accept", "application/json")
        .call()
        .map_err(|_| "qiniu_batch_network: Could not reach the Qiniu jobs endpoint")?;
    decode_json(response)
}

pub fn download(provider_id: &str, job_id: &str, output: &Path) -> Result<(), String> {
    let job = status(provider_id, job_id)?;
    let url = job.output_files_url.as_deref().and_then(http_url)
        .filter(|url| url.scheme() == "https")
        .filter(|_| job.status == "Completed")
        .ok_or("qiniu_batch_not_ready: No downloadable result is available for this job")?;
    let directory = output.parent().filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| "qiniu_batch_output: Could not create a temporary result file")?;
    // The result URL is presigned. Never forward the Provider key or print the URL.
    let mut response = agent(Duration::from_secs(300))?.get(url.as_str()).call()
        .map_err(|_| "qiniu_batch_network: Could not download the Qiniu result")?;
    require_success(&response)?;
    io::copy(&mut response.body_mut().as_reader(), &mut file)
        .map_err(|_| "qiniu_batch_output: Could not write the complete result file")?;
    file.as_file().sync_all().map_err(|_| "qiniu_batch_output: Could not flush the result file")?;
    file.persist_noclobber(output).map_err(|_| "qiniu_batch_output: Could not save the result to a new file")?;
    Ok(())
}

fn http_url(input: &str) -> Option<Url> {
    Url::parse(input).ok().filter(|url| {
        matches!(url.scheme(), "https" | "http") && url.host_str().is_some()
            && url.username().is_empty() && url.password().is_none() && url.fragment().is_none()
    })
}

fn authorization(provider_id: &str) -> Result<Zeroizing<String>, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let provider = settings.model_providers.as_ref().and_then(|providers| providers.get(provider_id))
        .filter(|provider| provider.provider == "qiniu-ai" && provider.base_url.trim_end_matches('/') == BASE)
        .ok_or("qiniu_batch_provider: Select a saved Qiniu AI Provider with its official Base URL")?;
    let key = credential_service::read_provider(&settings, provider)?
        .ok_or("credential_missing: Save a Qiniu AI API key for this Provider")?;
    let key = std::str::from_utf8(&key).ok()
        .filter(|key| !key.is_empty() && !key.contains(['\r', '\n', '\0']))
        .ok_or("credential_invalid: API Key must be one non-empty line")?;
    Ok(Zeroizing::new(format!("Bearer {key}")))
}

fn agent(timeout: Duration) -> Result<ureq::Agent, String> {
    crate::network::build_ureq_agent(
        ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(timeout)),
    )
}

fn require_success(response: &ureq::http::Response<ureq::Body>) -> Result<(), String> {
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("qiniu_batch_http: Qiniu returned HTTP {status}"))
    }
}

fn decode_json<T: serde::de::DeserializeOwned>(mut response: ureq::http::Response<ureq::Body>) -> Result<T, String> {
    require_success(&response)?;
    const LIMIT: u64 = 1024 * 1024;
    let mut data = Vec::new();
    response.body_mut().as_reader().take(LIMIT + 1).read_to_end(&mut data)
        .map_err(|_| "qiniu_batch_response: Could not read the job response")?;
    if data.len() as u64 > LIMIT {
        return Err("qiniu_batch_response: Qiniu returned an oversized job response".into());
    }
    serde_json::from_slice(&data)
        .map_err(|_| "qiniu_batch_response: Qiniu returned an unexpected job response".into())
}
