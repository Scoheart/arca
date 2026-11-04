//! Conservative display projection; values never leave Core before redaction.
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use regex::Regex;

const HIDDEN: &str = "[已隐藏]";
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(?:^|[\s;&])(?:--)?[\w-]*(?:api[_-]?key|token|password|secret)(?:\s*[=:]\s*|\s+)(?:"([^"]*)"|'([^']*)'|([^\s,;]+))"#).expect("credential assignment pattern"));
static BEARER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bBearer\s+([A-Za-z0-9._~+/-]+=*)").expect("bearer pattern"));
static URLS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s"'<>]+"#).expect("HTTP URL pattern"));

fn private_field(key: &str) -> bool {
    let key = key.chars().filter(|character| character.is_ascii_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    matches!(key.as_str(), "key" | "env" | "envs" | "environment" | "headers" | "httpheaders" | "oauth" | "auth" | "authorization" | "credentials" | "cookie" | "setcookie")
        || ["password", "passwd", "passphrase", "token", "secret", "apikey", "privatekey"].iter().any(|suffix| key.ends_with(suffix))
}

fn insert(secrets: &mut BTreeSet<String>, value: &str) -> Result<(), ()> {
    if !value.is_empty() { secrets.insert(value.to_string()); }
    if secrets.len() > 1024 { return Err(()); }
    Ok(())
}

fn learn(value: &Value, opaque: bool, depth: usize, secrets: &mut BTreeSet<String>) -> Result<(), ()> {
    if depth > 64 { return Err(()); }
    match value {
        Value::Object(fields) => for (key, value) in fields { learn(value, opaque || private_field(key), depth + 1, secrets)?; },
        Value::Array(values) => {
            for pair in values.windows(2) {
                if pair[0].as_str().is_some_and(|flag| flag.starts_with('-') && (private_field(flag.trim_start_matches('-')) || matches!(flag, "-H" | "--header"))) {
                    learn(&pair[1], true, depth + 1, secrets)?;
                }
            }
            for value in values { learn(value, opaque, depth + 1, secrets)?; }
        }
        Value::String(text) => {
            if opaque { insert(secrets, text)?; }
            for pattern in [&*ASSIGNMENT, &*BEARER] {
                for capture in pattern.captures_iter(text) {
                    for value in capture.iter().skip(1).flatten() { insert(secrets, value.as_str())?; }
                }
            }
            for found in URLS.find_iter(text) {
                if let Ok(url) = url::Url::parse(found.as_str()) {
                    insert(secrets, url.username())?;
                    if let Some(password) = url.password() { insert(secrets, password)?; }
                    for (_, value) in url.query_pairs() { insert(secrets, &value)?; }
                    if let Some(fragment) = url.fragment() { insert(secrets, fragment)?; }
                }
            }
        }
        Value::Number(number) if opaque => insert(secrets, &number.to_string())?,
        _ => {},
    }
    Ok(())
}

fn text(value: &str, secrets: &[String]) -> String {
    let mut result = URLS.replace_all(value, |found: &regex::Captures<'_>| {
        if let Ok(mut url) = url::Url::parse(&found[0]) {
            let _ = url.set_username(""); let _ = url.set_password(None);
            let keys = url.query_pairs().map(|(key, _)| key.to_string()).collect::<Vec<_>>();
            if !keys.is_empty() { url.query_pairs_mut().clear().extend_pairs(keys.iter().map(|key| (key.as_str(), HIDDEN))); }
            url.set_fragment(None);
            url.to_string()
        } else { found[0].to_string() }
    }).into_owned();
    for secret in secrets { result = result.replace(secret, HIDDEN); }
    result
}

fn display(value: &Value, opaque: bool, secrets: &[String]) -> Value {
    match value {
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, value)| (text(key, secrets), display(value, opaque || private_field(key), secrets))).collect()),
        Value::Array(values) => Value::Array(values.iter().map(|value| display(value, opaque, secrets)).collect()),
        Value::String(value) => Value::String(if opaque { HIDDEN.into() } else { text(value, secrets) }),
        Value::Number(number) if opaque || secrets.contains(&number.to_string()) => Value::String(HIDDEN.into()),
        _ => value.clone(),
    }
}

pub(super) fn redact(value: &Value) -> Result<Value, ()> {
    let mut secrets = BTreeSet::new();
    learn(value, false, 0, &mut secrets)?;
    let mut secrets = secrets.into_iter().collect::<Vec<_>>();
    secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    Ok(display(value, false, &secrets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quoted_command_credentials_and_embedded_url_values_cannot_escape_through_sibling_fields() {
        let source = json!({
            "command": "fixture-command",
            "args": ["--api-key=\"quoted-private-value\"", "--password='private value with spaces'",
                "tool --token shell-private-value", "--url=https://user:private-pass@example.test/mcp?api_key=query-private-value#fragment-private-value"],
            "echo": "quoted-private-value private value with spaces shell-private-value private-pass query-private-value fragment-private-value",
            "key": "generic-key-private-value",
        });
        let rendered = redact(&source).unwrap().to_string();
        for secret in ["quoted-private-value", "private value with spaces", "shell-private-value", "private-pass", "query-private-value", "fragment-private-value", "generic-key-private-value"] {
            assert!(!rendered.contains(secret), "credential leaked: {secret}");
        }
        assert!(rendered.contains("fixture-command"));
        assert!(rendered.contains("example.test/mcp"));
    }
}
