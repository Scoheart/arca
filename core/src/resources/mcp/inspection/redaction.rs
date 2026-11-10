//! Conservative display projection; values never leave Core before redaction.
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use regex::Regex;

const HIDDEN: &str = "[已隐藏]";
// Match labels only: consuming a non-secret value here could swallow a later
// credential flag in the same shell argument. All readers share one predicate.
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"["']?([\w-]+)["']?\s*[=:]\s*"#).expect("assignment label pattern"));
static FLAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(--[\w-]+|-H)\s+").expect("flag label pattern"));
static BEARER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:Bearer|Basic)\s+([A-Za-z0-9._~+/-]+=*)").expect("HTTP authentication pattern"));
static URLS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s"'<>]+"#).expect("HTTP URL pattern"));

fn private_field(key: &str) -> bool {
    let key = key.chars().filter(|character| character.is_ascii_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    matches!(key.as_str(), "key" | "env" | "envs" | "environment" | "headers" | "httpheaders" | "oauth" | "auth" | "authorization" | "credentials" | "cookie" | "setcookie")
        || ["password", "passwd", "passphrase", "token", "secret", "apikey", "privatekey", "secretkey", "secretaccesskey"].iter().any(|suffix| key.ends_with(suffix))
}

fn private_label(label: &str) -> bool {
    private_field(label) || matches!(label, "-H" | "--header")
}

/// A deliberately limited credential value reader, never a shell evaluator.
/// Ambiguous quoting fails the whole display projection rather than publishing
/// a partially learned secret. The source file is never changed.
fn credential_value(source: &str) -> Result<(&str, String), ()> {
    let first = source.chars().next().ok_or(())?;
    if matches!(first, '\'' | '"') {
        let mut decoded = String::new();
        let mut characters = source.char_indices().skip(1);
        while let Some((index, character)) = characters.next() {
            if character == first {
                let end = index + character.len_utf8();
                if source[end..].chars().next().is_some_and(|next|
                    !next.is_whitespace() && !matches!(next, ',' | ';' | '&' | '|')) { return Err(()); }
                return Ok((&source[..end], decoded));
            }
            if character == '\\' {
                let (_, escaped) = characters.next().ok_or(())?;
                if escaped != first && escaped != '\\' { return Err(()); }
                decoded.push(escaped);
            } else { decoded.push(character); }
        }
        Err(())
    } else {
        let end = source.find(|character: char| character.is_whitespace()
            || matches!(character, ',' | ';' | '&' | '|')).unwrap_or(source.len());
        let value = &source[..end];
        if value.is_empty() || value.contains(['\'', '"', '\\']) { return Err(()); }
        Ok((value, value.to_string()))
    }
}

fn learn_argument(value: &str, secrets: &mut BTreeSet<String>) -> Result<(), ()> {
    insert(secrets, value)?;
    let value = value.trim();
    if value.starts_with(['\'', '"']) {
        let (raw, decoded) = credential_value(value)?;
        if !value[raw.len()..].trim().is_empty() { return Err(()); }
        insert(secrets, &decoded)?;
    }
    Ok(())
}

fn learn_header(value: &str, secrets: &mut BTreeSet<String>) -> Result<(), ()> {
    let value = value.trim();
    let decoded;
    let header = if value.starts_with(['\'', '"']) {
        let (raw, text) = credential_value(value)?;
        if !value[raw.len()..].trim().is_empty() { return Err(()); }
        decoded = text;
        decoded.as_str()
    } else { value };
    if let Some((_, field_value)) = header.split_once(':') {
        learn_argument(field_value.trim(), secrets)?;
    }
    Ok(())
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
            for value in values.iter().filter_map(Value::as_str) {
                if let Some((label, value)) = value.split_once('=') {
                    if label.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                        && private_label(label) {
                        learn_argument(value, secrets)?;
                        if matches!(label, "-H" | "--header") { learn_header(value, secrets)?; }
                    }
                }
            }
            for pair in values.windows(2) {
                if let Some(flag) = pair[0].as_str().filter(|flag| flag.starts_with('-')
                    && !flag.contains(['=', ':']) && private_label(flag)) {
                    learn(&pair[1], true, depth + 1, secrets)?;
                    if let Some(value) = pair[1].as_str() {
                        learn_argument(value, secrets)?;
                        if matches!(flag, "-H" | "--header") { learn_header(value, secrets)?; }
                    }
                }
            }
            for value in values { learn(value, opaque, depth + 1, secrets)?; }
        }
        Value::String(text) => {
            if opaque { insert(secrets, text)?; }
            for pattern in [&*ASSIGNMENT, &*FLAG] {
                for capture in pattern.captures_iter(text) {
                    let label = capture.get(1).ok_or(())?;
                    if private_label(label.as_str()) {
                        let rest = &text[capture.get(0).ok_or(())?.end()..];
                        let (raw, decoded) = credential_value(rest)?;
                        insert(secrets, raw)?;
                        insert(secrets, &decoded)?;
                        if matches!(label.as_str(), "-H" | "--header") { learn_header(&decoded, secrets)?; }
                    }
                }
            }
            for capture in BEARER.captures_iter(text) {
                if let Some(value) = capture.get(1) { insert(secrets, value.as_str())?; }
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
    fn compound_credential_fields_and_arguments_use_the_same_name_policy() {
        for value in [
            json!({"aws_secret_access_key": "fixture-compound-value", "echo": "fixture-compound-value"}),
            json!({"args": ["--aws-secret-access-key", "fixture-compound-value"], "echo": "fixture-compound-value"}),
            json!({"args": ["--aws-secret-access-key=fixture-compound-value", "public-file"], "echo": "fixture-compound-value"}),
            json!({"args": ["--private-key=fixture-compound-value", "public-file"], "echo": "fixture-compound-value"}),
            json!({"secret_key": "fixture-compound-value", "echo": "fixture-compound-value"}),
            json!({"args": ["--secret-key=fixture-compound-value", "public-file"], "echo": "fixture-compound-value"}),
            json!({"args": ["--passwd=fixture-compound-value", "public-file"], "echo": "fixture-compound-value"}),
            json!({"args": ["--key=fixture-compound-value", "public-file"], "echo": "fixture-compound-value"}),
        ] {
            let rendered = redact(&value).unwrap().to_string();
            assert!(!rendered.contains("fixture-compound-value"));
            if value["args"].as_array().is_some_and(|args| args.len() == 2 && args[0].as_str().unwrap().contains('=')) {
                assert!(rendered.contains("public-file"), "an inline value must not consume the following argument");
            }
        }
        let value = json!({"args": ["--api-key=fixture-token", "public-file"], "echo": "fixture-token"});
        let rendered = redact(&value).unwrap();
        assert_eq!(rendered["args"][1], "public-file");
        assert!(!rendered.to_string().contains("fixture-token"));
    }

    #[test]
    fn credential_quotes_and_shell_separators_do_not_leave_echo_disclosures() {
        for (index, text) in [
            "tool --verbose --aws-secret-access-key fixture-compound-value",
            "tool --mode public --aws-secret-access-key='fixture-compound-value'",
            "tool --aws-secret-access-key=\"fixture-compound-value\"",
            "AWS_SECRET_ACCESS_KEY=fixture-compound-value tool",
            "tool --token=fixture-compound-value&&next",
            "tool --token fixture-compound-value|next",
            "tool -H 'Authorization: Bearer fixture-compound-value'",
            "tool --header 'X-Custom: fixture-compound-value'",
            "tool -H 'Authorization: Basic fixture-compound-value'",
        ].into_iter().enumerate() {
            let value = json!({"args": [text], "echo": "fixture-compound-value"});
            assert!(!redact(&value).unwrap().to_string().contains("fixture-compound-value"), "credential form {index} must be hidden");
        }
        for quoted in ["'fixture quoted value'", "\"fixture quoted value\""] {
            let value = json!({"args": ["--token", quoted], "echo": "fixture quoted value"});
            assert!(!redact(&value).unwrap().to_string().contains("fixture quoted value"));
        }
        let value = json!({"args": ["--token=literal|native-value"], "echo": "literal|native-value"});
        assert!(!redact(&value).unwrap().to_string().contains("native-value"), "argv inline values are not shell-split");
    }

    #[test]
    fn escaped_credential_quotes_are_complete_and_ambiguous_values_fail_closed() {
        let value = json!({"args": [r#"--token="fixture\"quoted""#], "echo": "fixture\"quoted"});
        assert!(!redact(&value).unwrap().to_string().contains("quoted"));
        for text in [r#"--token="unterminated"#, r#"--token="value"suffix"#, r#"--token=value\ with-space"#] {
            assert!(redact(&json!({"args": [text], "echo": "do not return partially redacted data"})).is_err());
        }
    }

    #[test]
    fn public_token_budgets_and_secret_less_policy_are_preserved() {
        let value = json!({"token_budget": 4096, "secret-less": true,
            "args": ["tool --token-budget=4096 --secret-less=public", "public-file"]});
        assert_eq!(redact(&value).unwrap(), value);
    }

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
