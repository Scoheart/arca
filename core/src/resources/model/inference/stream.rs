//! Read Provider streams without forwarding private metadata or reasoning.
mod bedrock;

use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Read},
};

use serde_json::Value;
use zeroize::Zeroizing;

use super::{wire, Failure};
use crate::domain::types::ModelProtocol;

const MAX_EVENT_BYTES: usize = 512 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_WIRE_BYTES: u64 = 16 * 1024 * 1024;
const HIDDEN_CREDENTIAL: &str = "[凭据已隐藏]";

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Format {
    Json,
    Sse,
    Bedrock,
}

impl Format {
    pub(super) fn route(protocol: &ModelProtocol, url: &url::Url) -> Option<(Self, url::Url)> {
        use ModelProtocol::*;
        let (format, suffix, streaming_suffix) = match protocol {
            GeminiGenerateContent => (Self::Sse, ":generateContent", ":streamGenerateContent"),
            AwsBedrockConverse => (Self::Bedrock, "/converse", "/converse-stream"),
            _ => return Some((Self::Sse, url.clone())),
        };
        let mut target = url.clone();
        let prefix = target.path().strip_suffix(suffix)?;
        let path = format!("{prefix}{streaming_suffix}");
        target.set_path(&path);
        if *protocol == GeminiGenerateContent {
            target.query_pairs_mut().append_pair("alt", "sse");
        }
        Some((format, target))
    }

    pub(super) fn accept(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Sse => "text/event-stream",
            Self::Bedrock => "application/vnd.amazon.eventstream",
        }
    }

    pub(super) fn matches(self, content_type: Option<&str>) -> bool {
        self != Self::Json && content_type
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case(self.accept()))
    }
}

type Output = (String, wire::Usage, Option<&'static str>);
type Emit<'a> = dyn FnMut(&str) -> Result<(), Failure> + 'a;

pub(super) fn decode(
    protocol: &ModelProtocol,
    format: Format,
    reader: impl Read,
    credential: Option<&str>,
    status: u16,
    emit: &mut Emit<'_>,
) -> Result<Output, Failure> {
    let mut output = Accumulator::new(credential);
    let mut reader = BufReader::new(reader);
    let mut frame = Vec::new();
    let mut data = Vec::new();
    let mut event = String::new();

    loop {
        let end = match format {
            Format::Bedrock => match bedrock::next(&mut reader)? {
                Some((name, value)) => output.consume(protocol, &name, &value, status, emit)?,
                None => break,
            },
            Format::Sse => match next_sse(&mut reader, &mut frame, &mut data, &mut event)? {
                false => break,
                true if data == b"[DONE]" => {
                    output.terminal = true;
                    true
                }
                true => {
                    let value: Value = serde_json::from_slice(&data).map_err(|_| invalid_stream())?;
                    let name = value["type"].as_str()
                        .or_else(|| value["event_type"].as_str())
                        .unwrap_or(&event);
                    output.consume(protocol, name, &value, status, emit)?
                }
            },
            Format::Json => return Err(invalid_stream()),
        };
        if end {
            break;
        }
    }
    if !output.terminal {
        return Err(Failure::new(502, "stream_incomplete", "连接在 Provider 完成回答前断开，可以单独重试。"));
    }
    output.finish(emit)
}

fn next_sse(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    data: &mut Vec<u8>,
    event: &mut String,
) -> Result<bool, Failure> {
    data.clear();
    event.clear();
    loop {
        line.clear();
        let read = (&mut *reader).take((MAX_EVENT_BYTES + 1) as u64)
            .read_until(b'\n', line)
            .map_err(read_failure)?;
        if read == 0 {
            return Ok(!data.is_empty());
        }
        if read > MAX_EVENT_BYTES {
            return Err(invalid_stream());
        }
        let field = line.strip_suffix(b"\n").unwrap_or(line);
        let field = field.strip_suffix(b"\r").unwrap_or(field);
        if field.is_empty() {
            if !data.is_empty() {
                return Ok(true);
            }
            event.clear();
            continue;
        }
        if let Some(value) = field.strip_prefix(b"data:") {
            if data.len() + value.len() + 1 > MAX_EVENT_BYTES {
                return Err(invalid_stream());
            }
            if !data.is_empty() {
                data.push(b'\n');
            }
            data.extend_from_slice(value.strip_prefix(b" ").unwrap_or(value));
        } else if let Some(value) = field.strip_prefix(b"event:") {
            let value = std::str::from_utf8(value).map_err(|_| invalid_stream())?;
            event.clear();
            event.push_str(value.trim());
        }
    }
}

pub(super) fn invalid_stream() -> Failure {
    Failure::new(502, "invalid_stream", "Provider 返回了无法解析的流式响应。请检查端点，或选择完整返回。")
}

fn read_failure(error: std::io::Error) -> Failure {
    if error.kind() == std::io::ErrorKind::TimedOut {
        Failure::new(504, "provider_timeout", "Provider 未在时限内完成请求。")
    } else {
        Failure::new(502, "stream_read_failed", "读取 Provider 流式响应时中断，可以单独重试。")
    }
}

struct Accumulator<'a> {
    text: String,
    pending: Zeroizing<String>,
    credential: Option<&'a str>,
    usage: wire::Usage,
    interaction_outputs: BTreeSet<u64>,
    stop: Option<&'static str>,
    terminal: bool,
}

impl<'a> Accumulator<'a> {
    fn new(credential: Option<&'a str>) -> Self {
        Self {
            text: String::new(),
            pending: Zeroizing::new(String::new()),
            credential,
            usage: wire::Usage::default(),
            interaction_outputs: BTreeSet::new(),
            stop: None,
            terminal: false,
        }
    }

    fn append(&mut self, text: &str, emit: &mut Emit<'_>) -> Result<(), Failure> {
        if text.is_empty() {
            return Ok(());
        }
        if self.text.len() + self.pending.len() + text.len() > MAX_TEXT_BYTES {
            return Err(Failure::new(502, "answer_too_large", "回答超过了试验台的长度上限，可以降低最大输出 Token。"));
        }
        let Some(secret) = self.credential else {
            self.text.push_str(text);
            return emit(text);
        };
        self.pending.push_str(text);
        // Keep only a possible credential prefix until the next delta. A
        // credential echoed in several deltas must never reach the browser.
        let last_match = self.pending.match_indices(secret).last()
            .map_or(0, |(offset, matched)| offset + matched.len());
        let earliest = self.pending.len().saturating_sub(secret.len().saturating_sub(1)).max(last_match);
        let mut suffix_start = earliest;
        while !self.pending.is_char_boundary(suffix_start) {
            suffix_start += 1;
        }
        let split = self.pending[suffix_start..].char_indices()
            .map(|(index, _)| suffix_start + index)
            .find(|&index| secret.starts_with(&self.pending[index..]))
            .unwrap_or(self.pending.len());
        if split > 0 {
            let visible = self.pending[..split].replace(secret, HIDDEN_CREDENTIAL);
            self.pending.drain(..split);
            if self.text.len() + visible.len() > MAX_TEXT_BYTES {
                return Err(Failure::new(502, "answer_too_large", "回答超过了试验台的长度上限。"));
            }
            self.text.push_str(&visible);
            emit(&visible)?;
        }
        Ok(())
    }

    fn mark_stop(&mut self, cause: Option<&str>) {
        if let Some(cause) = cause.filter(|cause| !cause.is_empty()) {
            self.stop = wire::finish_reason(Some(cause));
            self.terminal = true;
        }
    }

    fn consume(
        &mut self,
        protocol: &ModelProtocol,
        event: &str,
        value: &Value,
        status: u16,
        emit: &mut Emit<'_>,
    ) -> Result<bool, Failure> {
        if matches!(event, "error" | "response.failed" | "interaction.failed" | "bedrock.exception")
            || value["error"].is_object()
        {
            return Err(Failure::from_upstream(status, Some(value.clone())));
        }
        use ModelProtocol::*;
        match protocol {
            OpenaiCompletions => {
                let choice = &value["choices"][0];
                let delta = &choice["delta"];
                if let Some(text) = delta["content"].as_str().or_else(|| delta["refusal"].as_str()) {
                    self.append(text, emit)?;
                }
                self.usage.observe(protocol, &value["usage"]);
                self.mark_stop(choice["finish_reason"].as_str());
            }
            OpenaiResponses => match event {
                "response.output_text.delta" | "response.refusal.delta" => self.append_pointer(value, "/delta", emit)?,
                "response.completed" | "response.incomplete" => {
                    let response = &value["response"];
                    self.usage.observe(protocol, &response["usage"]);
                    let stop = response["incomplete_details"]["reason"].as_str()
                        .or_else(|| response["status"].as_str())
                        .unwrap_or(if event == "response.incomplete" { "incomplete" } else { "completed" });
                    self.mark_stop(Some(stop));
                    if self.text.is_empty() && self.pending.is_empty() {
                        let (text, _, _) = wire::decode(protocol, response)?;
                        self.append(&text, emit)?;
                    }
                    return Ok(true);
                }
                _ => {}
            },
            AnthropicMessages => match event {
                "message_start" => self.usage.observe(protocol, &value["message"]["usage"]),
                "content_block_start" if value["content_block"]["type"] == "text" => self.append_pointer(value, "/content_block/text", emit)?,
                "content_block_delta" if value["delta"]["type"] == "text_delta" => self.append_pointer(value, "/delta/text", emit)?,
                "message_delta" => {
                    self.usage.observe(protocol, &value["usage"]);
                    self.stop = wire::finish_reason(value["delta"]["stop_reason"].as_str());
                }
                "message_stop" => {
                    self.terminal = true;
                    return Ok(true);
                }
                _ => {}
            },
            GeminiInteractions => match event {
                "step.start" if value["step"]["type"] == "model_output" => {
                    if let Some(index) = value["index"].as_u64() {
                        self.interaction_outputs.insert(index);
                    }
                }
                "step.stop" => {
                    if let Some(index) = value["index"].as_u64() {
                        self.interaction_outputs.remove(&index);
                    }
                }
                "step.delta" if value["delta"]["type"] == "text"
                    && value["index"].as_u64().is_some_and(|index| self.interaction_outputs.contains(&index)) =>
                {
                    self.append_pointer(value, "/delta/text", emit)?;
                }
                "content.delta" if value["delta"]["type"] == "text" => {
                    self.append_pointer(value, "/delta/text", emit)?;
                }
                "interaction.completed" | "interaction.complete" => {
                    let interaction = value.get("interaction").unwrap_or(value);
                    self.usage.observe(protocol, &interaction["usage"]);
                    let status = interaction["status"].as_str().unwrap_or("completed");
                    if status == "failed" {
                        return Err(Failure::new(502, "provider_error", "Provider 未能完成请求。"));
                    }
                    self.mark_stop(Some(status));
                    return Ok(true);
                }
                _ => {}
            },
            GeminiGenerateContent => {
                let candidate = &value["candidates"][0];
                let mut parts = Vec::new();
                wire::pick_text(&candidate["content"]["parts"], &mut parts);
                for text in parts {
                    self.append(text, emit)?;
                }
                self.usage.observe(protocol, &value["usageMetadata"]);
                self.mark_stop(candidate["finishReason"].as_str().or_else(|| value["promptFeedback"]["blockReason"].as_str()));
            }
            AwsBedrockConverse => match event {
                "contentBlockDelta" => self.append_pointer(value, "/delta/text", emit)?,
                "messageStop" => self.mark_stop(value["stopReason"].as_str().or(Some(event))),
                "metadata" => {
                    self.usage.observe(protocol, &value["usage"]);
                    return Ok(self.terminal);
                }
                _ => {}
            },
        };
        Ok(false)
    }

    fn append_pointer(&mut self, value: &Value, path: &str, emit: &mut Emit<'_>) -> Result<(), Failure> {
        if let Some(text) = value.pointer(path).and_then(Value::as_str) {
            self.append(text, emit)?;
        }
        Ok(())
    }

    fn finish(mut self, emit: &mut Emit<'_>) -> Result<Output, Failure> {
        if !self.pending.is_empty() {
            self.text.push_str(&self.pending);
            emit(&self.pending)?;
        }
        Ok((wire::checked_text(self.text, self.stop)?, self.usage, self.stop))
    }
}
