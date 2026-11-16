//! Encode the six configured protocols and keep only visible text and usage.
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::{Failure, Request};
use crate::domain::types::{ModelProfile, ModelProtocol};

pub(super) const PUBLIC_PARAMETERS: &[&str] = &[
    "model", "input", "messages", "contents", "instructions", "system", "system_instruction",
    "temperature", "top_p", "topP", "max_tokens", "max_output_tokens", "max_completion_tokens",
    "maxTokens", "maxOutputTokens", "reasoning", "reasoning.effort", "reasoning_effort",
    "thinking", "thinkingConfig", "generationConfig", "generation_config", "inferenceConfig",
];

const RESERVED: &[&str] = &[
    "model", "input", "messages", "contents", "instructions", "system", "system_instruction",
    "systemInstruction", "store", "stream", "stream_options", "background", "tools", "toolConfig",
    "tool_choice", "tool_config", "functions", "function_call", "parallel_tool_calls", "agent",
    "agent_config", "environment", "previous_response_id", "previous_interaction_id", "conversation",
    "prompt", "container", "webhook_config", "web_search_options", "plugins", "modalities",
    "response_modalities", "responseModalities", "output_modalities", "audio", "image", "video",
    "cachedContent", "continuationToken", "authorization", "api_key", "apiKey", "headers", "n",
];

pub(super) fn encode(profile: &ModelProfile, provider: &str, request: Request) -> Result<Value, Failure> {
    if request.temperature.is_some_and(|value| !(0.0..=2.0).contains(&value)) {
        return Err(Failure::bad("Temperature 必须在 0 到 2 之间。"));
    }
    if request.top_p.is_some_and(|value| !(0.0..=1.0).contains(&value)) {
        return Err(Failure::bad("Top P 必须在 0 到 1 之间。"));
    }
    if request.max_output_tokens.is_some_and(|value| !(1..=131_072).contains(&value)) {
        return Err(Failure::bad("最大输出 Token 必须在 1 到 131072 之间。"));
    }
    let mut body = request.extra;
    if let Some(key) = RESERVED.iter().find(|key| body.contains_key(**key)) {
        return Err(Failure::bad(format!("高级 JSON 不能设置 {key}。请使用面板对应的控件。")));
    }
    let effort = request.reasoning_effort.as_deref().filter(|value| !value.is_empty());
    if effort.is_some_and(|value| {
        value.len() > 24 || !value.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    }) {
        return Err(Failure::bad("推理强度只能使用小写英文和下划线。"));
    }
    let tokens = request.max_output_tokens.map(u64::from);
    let has_system = !request.instructions.trim().is_empty();
    let model = &profile.model;

    use ModelProtocol::*;
    match profile.protocol {
        OpenaiCompletions => {
            let mut messages = Vec::new();
            if has_system {
                let role = if matches!(provider, "openai" | "azure-openai") { "developer" } else { "system" };
                messages.push(json!({"role": role, "content": request.instructions}));
            }
            messages.push(json!({"role": "user", "content": request.prompt}));
            body.insert("messages".into(), json!(messages));
            body.insert("model".into(), json!(model));
            body.insert("stream".into(), json!(false));
            if matches!(provider, "openai" | "azure-openai") {
                body.insert("store".into(), json!(false));
            }
            if tokens.is_some() && (body.contains_key("max_tokens") || body.contains_key("max_completion_tokens")) {
                return Err(duplicate("max_tokens / max_completion_tokens"));
            }
            let limit_field = if matches!(provider, "openai" | "azure-openai") {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            params(&mut body, &["temperature", "top_p", limit_field], request.temperature, request.top_p, tokens)?;
            field(&mut body, "reasoning_effort", effort)?;
        }
        OpenaiResponses => {
            body.insert("model".into(), json!(model));
            body.insert("input".into(), json!(request.prompt));
            body.insert("stream".into(), json!(false));
            body.insert("store".into(), json!(false));
            if has_system { body.insert("instructions".into(), json!(request.instructions)); }
            params(&mut body, &["temperature", "top_p", "max_output_tokens"], request.temperature, request.top_p, tokens)?;
            if let Some(effort) = effort {
                field(nested(&mut body, "reasoning")?, "effort", Some(effort))?;
            }
        }
        AnthropicMessages => {
            require_native_effort(effort)?;
            body.insert("model".into(), json!(model));
            body.insert("messages".into(), json!([{"role": "user", "content": request.prompt}]));
            body.insert("stream".into(), json!(false));
            if has_system { body.insert("system".into(), json!(request.instructions)); }
            params(&mut body, &["temperature", "top_p", "max_tokens"], request.temperature, request.top_p, tokens)?;
            body.entry("max_tokens").or_insert_with(|| json!(profile.max_output_tokens.unwrap_or(4096)));
        }
        GeminiInteractions => {
            require_native_effort(effort)?;
            if request.temperature.is_some() || request.top_p.is_some() {
                return Err(Failure::bad("Gemini Interactions 不提供 Temperature 和 Top P 参数。"));
            }
            body.insert("model".into(), json!(model));
            body.insert("input".into(), json!(request.prompt));
            body.insert("stream".into(), json!(false));
            body.insert("store".into(), json!(false));
            if has_system { body.insert("system_instruction".into(), json!(request.instructions)); }
            if tokens.is_some() {
                field(nested(&mut body, "generation_config")?, "max_output_tokens", tokens)?;
            }
        }
        GeminiGenerateContent => {
            require_native_effort(effort)?;
            body.insert("contents".into(), json!([{"role":"user","parts":[{"text":request.prompt}]}]));
            if has_system {
                body.insert("systemInstruction".into(), json!({"parts":[{"text":request.instructions}]}));
            }
            if request.temperature.is_some() || request.top_p.is_some() || tokens.is_some() || body.contains_key("generationConfig") {
                let config = nested(&mut body, "generationConfig")?;
                if ["responseModalities", "response_modalities"].iter().any(|name| config.contains_key(*name)) {
                    return Err(Failure::bad("模型试验台只发送文本请求。"));
                }
                params(config, &["temperature", "topP", "maxOutputTokens"], request.temperature, request.top_p, tokens)?;
            }
        }
        AwsBedrockConverse => {
            require_native_effort(effort)?;
            body.insert("messages".into(), json!([{"role":"user","content":[{"text":request.prompt}]}]));
            if has_system { body.insert("system".into(), json!([{"text":request.instructions}])); }
            if request.temperature.is_some() || request.top_p.is_some() || tokens.is_some() {
                params(nested(&mut body, "inferenceConfig")?, &["temperature", "topP", "maxTokens"],
                    request.temperature, request.top_p, tokens)?;
            }
            if model.starts_with("arn:") && model.contains(":prompt/")
                && ["system", "inferenceConfig", "additionalModelRequestFields"].iter().any(|name| body.contains_key(*name))
            {
                return Err(Failure::bad("Bedrock Prompt ARN 的系统指令和推理参数由保存的 Prompt 管理。"));
            }
        }
    }
    Ok(Value::Object(body))
}

fn require_native_effort(effort: Option<&str>) -> Result<(), Failure> {
    if effort.is_some() {
        Err(Failure::bad("这个协议的推理参数请填在高级 JSON 中。"))
    } else {
        Ok(())
    }
}

fn duplicate(field: &str) -> Failure {
    Failure::bad(format!("{field} 同时出现在参数控件和高级 JSON 中，请保留一个。"))
}

fn nested<'a>(body: &'a mut Map<String, Value>, key: &str) -> Result<&'a mut Map<String, Value>, Failure> {
    body.entry(key).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| Failure::bad(format!("{key} 必须是 JSON 对象。")))
}

fn field<T: Serialize>(body: &mut Map<String, Value>, key: &str, value: Option<T>) -> Result<(), Failure> {
    if let Some(value) = value {
        if body.contains_key(key) {
            return Err(duplicate(key));
        }
        body.insert(key.into(), json!(value));
    }
    Ok(())
}

fn params(
    body: &mut Map<String, Value>, fields: &[&str; 3],
    temperature: Option<f64>, top_p: Option<f64>, tokens: Option<u64>,
) -> Result<(), Failure> {
    field(body, fields[0], temperature)?;
    field(body, fields[1], top_p)?;
    field(body, fields[2], tokens)
}

#[derive(Serialize)]
pub(super) struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
}

pub(super) fn decode(protocol: &ModelProtocol, value: &Value) -> Result<(String, Usage, Option<&'static str>), Failure> {
    use ModelProtocol::*;
    let mut chunks = Vec::new();
    let (stop, counts, input, output, total, reasoning) = match protocol {
        OpenaiCompletions => {
            let message = &value["choices"][0]["message"];
            if let Some(text) = message.get("content").and_then(Value::as_str).or_else(|| message["refusal"].as_str()) {
                chunks.push(text);
            } else {
                pick_text(&message["content"], &mut chunks);
            }
            (value["choices"][0]["finish_reason"].as_str(), &value["usage"],
                "prompt_tokens", "completion_tokens", "total_tokens", "/completion_tokens_details/reasoning_tokens")
        }
        OpenaiResponses => {
            for item in items(&value["output"]).filter(|item| item["type"] == "message") {
                pick_text(&item["content"], &mut chunks);
            }
            if chunks.is_empty() {
                if let Some(text) = value["output_text"].as_str() { chunks.push(text); }
            }
            (value["incomplete_details"]["reason"].as_str().or_else(|| value["status"].as_str()), &value["usage"],
                "input_tokens", "output_tokens", "total_tokens", "/output_tokens_details/reasoning_tokens")
        }
        AnthropicMessages => {
            pick_text(&value["content"], &mut chunks);
            (value["stop_reason"].as_str(), &value["usage"], "input_tokens", "output_tokens", "total_tokens", "/reasoning_tokens")
        }
        GeminiInteractions => {
            for step in items(&value["steps"]).filter(|step| step["type"] == "model_output") {
                pick_text(&step["content"], &mut chunks);
            }
            if chunks.is_empty() { pick_text(&value["outputs"], &mut chunks); }
            (value["status"].as_str(), &value["usage"], "total_input_tokens", "total_output_tokens", "total_tokens", "/total_thought_tokens")
        }
        GeminiGenerateContent => {
            let candidate = &value["candidates"][0];
            pick_text(&candidate["content"]["parts"], &mut chunks);
            (candidate["finishReason"].as_str().or_else(|| value["promptFeedback"]["blockReason"].as_str()),
                &value["usageMetadata"], "promptTokenCount", "candidatesTokenCount", "totalTokenCount", "/thoughtsTokenCount")
        }
        AwsBedrockConverse => {
            pick_text(&value["output"]["message"]["content"], &mut chunks);
            (value["stopReason"].as_str(), &value["usage"], "inputTokens", "outputTokens", "totalTokens", "/reasoningTokens")
        }
    };
    let finish = match stop {
        Some("length" | "max_tokens" | "max_output_tokens" | "MAX_TOKENS") => Some("length"),
        Some("content_filter" | "content_filtered" | "SAFETY" | "RECITATION" | "guardrail_intervened") => Some("filtered"),
        Some("incomplete" | "cancelled") => Some("incomplete"),
        _ => None,
    };
    let text = chunks.concat();
    if text.trim().is_empty() {
        let explanation = match finish {
            Some("length") => "模型用完了输出 Token，未生成可见文本。请提高输出上限后重试。",
            Some("filtered") => "Provider 过滤了这个回答，未返回可见文本。",
            _ => "Provider 未返回可见文本。请检查模型类型、协议或输出上限。",
        };
        return Err(Failure::new(502, "empty_text", explanation));
    }
    let usage = Usage { input_tokens: counts[input].as_u64(), output_tokens: counts[output].as_u64(),
        total_tokens: counts[total].as_u64(), reasoning_tokens: counts.pointer(reasoning).and_then(Value::as_u64) };
    Ok((text, usage, finish))
}

fn items(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

fn pick_text<'a>(value: &'a Value, chunks: &mut Vec<&'a str>) {
    for item in items(value) {
        if item["thought"] == true { continue; }
        match item["type"].as_str() {
            None | Some("text" | "output_text") => {
                if let Some(text) = item["text"].as_str() { chunks.push(text); }
            }
            Some("refusal") => {
                if let Some(text) = item["refusal"].as_str() { chunks.push(text); }
            }
            _ => {}
        }
    }
}
