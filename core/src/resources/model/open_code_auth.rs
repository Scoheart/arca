use serde_json::{Map, Value};
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub struct PreparedAuthFile {
    pub path: PathBuf,
    pub original: Option<Zeroizing<String>>,
    pub content: Option<Zeroizing<String>>,
    pub sensitive: bool,
}

impl fmt::Debug for PreparedAuthFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedAuthFile")
            .field("path", &self.path)
            .field("had_original", &self.original.is_some())
            .field("has_content", &self.content.is_some())
            .field("sensitive", &self.sensitive)
            .finish()
    }
}

impl PreparedAuthFile {
    pub fn from_model_file(file: super::adapters::PreparedModelFile) -> Self {
        Self {
            path: file.path,
            original: file.original.map(Zeroizing::new),
            content: file.content.map(Zeroizing::new),
            sensitive: true,
        }
    }
}

fn read_root(path: &Path) -> Result<(Option<Zeroizing<String>>, Map<String, Value>), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(
                    "agent_store_conflicted: OpenCode auth.json must be a regular non-symlink file"
                        .into(),
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                if metadata.permissions().mode() & 0o777 != 0o600
                    || metadata.uid() != rustix::process::getuid().as_raw()
                {
                    return Err(
                        "plaintext_target_insecure: OpenCode auth.json must be owned by the current user with mode 0600"
                            .into(),
                    );
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => {
            return Err("agent_store_conflicted: OpenCode auth.json cannot be inspected".into())
        }
    }
    let original = match fs::read_to_string(path) {
        Ok(value) => Some(Zeroizing::new(value)),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(_) => return Err("agent_store_conflicted: OpenCode auth.json cannot be read".into()),
    };
    let root = match original.as_deref() {
        Some(content) => serde_json::from_str::<Value>(content)
            .map_err(|_| "agent_store_conflicted: OpenCode auth.json is invalid JSON".to_string())?
            .as_object()
            .cloned()
            .ok_or_else(|| {
                "agent_store_conflicted: OpenCode auth.json root must be an object".to_string()
            })?,
        None => Map::new(),
    };
    Ok((original, root))
}

pub fn prepare_auth(
    path: &Path,
    provider_id: &str,
    credential: &[u8],
) -> Result<PreparedAuthFile, String> {
    if provider_id.trim().is_empty()
        || credential.is_empty()
        || credential.contains(&b'\n')
        || credential.contains(&b'\r')
        || credential.contains(&0)
    {
        return Err("agent_store_conflicted: invalid OpenCode credential input".into());
    }
    let credential = std::str::from_utf8(credential).map_err(|_| {
        "agent_store_conflicted: OpenCode API credentials must be UTF-8".to_string()
    })?;
    let (original, mut root) = read_root(path)?;
    let mut entry = match root.remove(provider_id) {
        None => Map::new(),
        Some(Value::Object(entry)) => entry,
        Some(_) => {
            return Err(format!(
                "agent_store_conflicted: OpenCode auth entry '{provider_id}' is not an object"
            ))
        }
    };
    if entry
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "api")
    {
        return Err(format!(
            "agent_store_conflicted: OpenCode auth entry '{provider_id}' is owned by a non-API login"
        ));
    }
    entry.insert("type".into(), Value::String("api".into()));
    entry.insert("key".into(), Value::String(credential.into()));
    root.insert(provider_id.into(), Value::Object(entry));
    let content = serde_json::to_string_pretty(&Value::Object(root)).map_err(|_| {
        "agent_store_conflicted: OpenCode auth candidate could not be encoded".to_string()
    })?;
    Ok(PreparedAuthFile {
        path: path.to_path_buf(),
        original,
        content: Some(Zeroizing::new(format!("{content}\n"))),
        sensitive: true,
    })
}

pub fn prepare_remove_auth(path: &Path, provider_id: &str) -> Result<PreparedAuthFile, String> {
    let (original, mut root) = read_root(path)?;
    root.remove(provider_id);
    let content = if original.is_none() && root.is_empty() {
        None
    } else {
        Some(Zeroizing::new(format!(
            "{}\n",
            serde_json::to_string_pretty(&Value::Object(root)).map_err(|_| {
                "agent_store_conflicted: OpenCode auth candidate could not be encoded".to_string()
            })?
        )))
    };
    Ok(PreparedAuthFile {
        path: path.to_path_buf(),
        original,
        content,
        sensitive: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn isolated_file() -> PathBuf {
        let root = std::env::temp_dir().join(format!("arca-opencode-auth-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root.join("auth.json")
    }

    fn write_private(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn merges_one_api_credential_and_preserves_unrelated_auth() {
        let path = isolated_file();
        write_private(
            &path,
            r#"{
  "unrelated": { "type": "oauth", "access": "opaque" },
  "arca_existing": { "type": "api", "key": "old", "futureField": "preserved" }
}"#,
        );

        let prepared = prepare_auth(&path, "arca_existing", b"new-secret").unwrap();
        assert!(prepared.sensitive);
        let value: serde_json::Value =
            serde_json::from_str(prepared.content.as_deref().unwrap()).unwrap();
        assert_eq!(value["arca_existing"]["type"], "api");
        assert_eq!(value["arca_existing"]["key"], "new-secret");
        assert_eq!(value["arca_existing"]["futureField"], "preserved");
        assert_eq!(value["unrelated"]["access"], "opaque");
        assert!(!format!("{prepared:?}").contains("new-secret"));
    }

    #[test]
    fn refuses_to_replace_an_existing_non_api_auth_entry() {
        let path = isolated_file();
        write_private(&path, r#"{"arca_work":{"type":"oauth","access":"opaque"}}"#);
        let error = prepare_auth(&path, "arca_work", b"new-secret").unwrap_err();
        assert!(error.starts_with("agent_store_conflicted:"));
        assert!(!error.contains("new-secret"));
    }
}
