//! Frontend-independent UI preferences.

pub use crate::pinned_agents::MAX_PINNED_AGENTS;
use crate::paths::mcp_icons_dir;
use crate::safe_write::ensure_private_file;
use crate::settings::{
    load_settings_strict, mutate_settings_checked, McpIconPreference, Settings, UiSettings,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const SUPPORTED_UI_LOCALES: [&str; 2] = ["zh-CN", "en-US"];
pub const MAX_MCP_ICON_BYTES: usize = 1_048_576;

#[derive(Debug, Serialize, Deserialize)]
pub struct McpIconDefinition { pub id: String, pub tone: String }

pub fn mcp_icon_catalog() -> &'static [McpIconDefinition] {
    static CATALOG: std::sync::LazyLock<Vec<McpIconDefinition>> = std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("../../../data/mcp-icons.json"))
            .expect("built-in MCP icon catalog must be valid")
    });
    &CATALOG
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpIconPreferenceView {
    pub kind: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub fn get_pinned_agents() -> Result<Vec<String>, String> {
    super::gate::read(crate::pinned_agents::get_pinned_agents)
}

pub fn set_pinned_agents(ids: Vec<String>) -> Result<Vec<String>, String> {
    super::gate::write_independent(|| crate::pinned_agents::set_pinned_agents(ids))
}

pub fn get_ui_locale() -> Result<Option<String>, String> {
    super::gate::read(|| {
        load_settings_strict()
            .map(|settings| settings.ui.and_then(|ui| ui.locale))
            .map_err(|error| error.to_string())
    })
}

pub fn set_ui_locale(locale: Option<String>) -> Result<Option<String>, String> {
    super::gate::write_independent(|| {
        let locale = normalize_locale(locale)?;
        mutate_settings_checked(move |settings| {
            settings.ui.get_or_insert_with(UiSettings::default).locale = locale.clone();
            Ok(locale)
        })
        .map_err(|error| error.to_string())
    })
}

pub fn list_mcp_icon_preferences(
) -> Result<BTreeMap<String, McpIconPreferenceView>, String> {
    super::gate::read(|| {
        let settings = load_settings_strict().map_err(|error| error.to_string())?;
        Ok(icon_views(&settings))
    })
}

pub fn set_mcp_builtin_icon(
    asset_key: String,
    icon_id: String,
) -> Result<BTreeMap<String, McpIconPreferenceView>, String> {
    super::gate::write_independent(|| {
        require_asset(&asset_key)?;
        let icon_id = normalize_builtin_icon_id(&icon_id)?;
        mutate_settings_checked(move |settings| {
            settings
                .ui
                .get_or_insert_with(UiSettings::default)
                .mcp_icons
                .insert(
                    asset_key,
                    McpIconPreference {
                        kind: "builtin".into(),
                        value: icon_id,
                        extra: BTreeMap::new(),
                    },
                );
            Ok(())
        })
        .map_err(|error| error.to_string())
    })?;
    list_mcp_icon_preferences()
}

pub fn import_mcp_icon(
    asset_key: String,
    source_path: PathBuf,
) -> Result<BTreeMap<String, McpIconPreferenceView>, String> {
    super::gate::write_independent(|| {
        require_asset(&asset_key)?;
        let bytes = read_icon_source(&source_path)?;
        let extension = image_extension(&bytes)?;
        let digest = hex::encode(Sha256::digest(&bytes));
        let filename = format!("{digest}.{extension}");
        ensure_private_file(&mcp_icons_dir().join(&filename), &bytes)?;
        mutate_settings_checked(move |settings| {
            settings
                .ui
                .get_or_insert_with(UiSettings::default)
                .mcp_icons
                .insert(
                    asset_key,
                    McpIconPreference {
                        kind: "custom".into(),
                        value: filename,
                        extra: BTreeMap::new(),
                    },
                );
            Ok(())
        })
        .map_err(|error| error.to_string())
    })?;
    list_mcp_icon_preferences()
}

fn read_icon_source(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{open, Mode, OFlags};
        let file = open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(fs::File::from)
        .map_err(|error| format!("failed to open selected icon safely: {error}"))?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("failed to inspect selected icon: {error}"))?;
        if !metadata.is_file() {
            return Err("selected icon must be a regular file".into());
        }
        if metadata.len() > MAX_MCP_ICON_BYTES as u64 {
            return Err("selected icon exceeds the 1 MiB limit".into());
        }
        file
    };
    #[cfg(not(unix))]
    let file = {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("failed to inspect selected icon: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("selected icon must be a regular file".into());
        }
        if metadata.len() > MAX_MCP_ICON_BYTES as u64 {
            return Err("selected icon exceeds the 1 MiB limit".into());
        }
        fs::File::open(path)
            .map_err(|error| format!("failed to read selected icon: {error}"))?
    };
    let mut bytes = Vec::new();
    file.take((MAX_MCP_ICON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read selected icon: {error}"))?;
    if bytes.len() > MAX_MCP_ICON_BYTES {
        return Err("selected icon exceeds the 1 MiB limit".into());
    }
    Ok(bytes)
}

pub fn reset_mcp_icon(
    asset_key: String,
) -> Result<BTreeMap<String, McpIconPreferenceView>, String> {
    super::gate::write_independent(|| {
        require_asset(&asset_key)?;
        mutate_settings_checked(move |settings| {
            if let Some(ui) = settings.ui.as_mut() {
                ui.mcp_icons.remove(&asset_key);
            }
            Ok(())
        })
        .map_err(|error| error.to_string())
    })?;
    list_mcp_icon_preferences()
}

fn require_asset(asset_key: &str) -> Result<(), String> {
    let exists = crate::resources::mcp::registry::read_registry()
        .iter()
        .any(|entry| entry.key() == asset_key);
    if exists {
        Ok(())
    } else {
        Err(format!("unknown MCP asset: {asset_key}"))
    }
}

fn normalize_builtin_icon_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || !mcp_icon_catalog().iter().any(|icon| icon.id == value)
        || value.len() > 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("invalid built-in MCP icon id".into());
    }
    Ok(value.to_string())
}

fn image_extension(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok("png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok("jpg");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("webp");
    }
    Err("unsupported icon image; choose PNG, JPEG, or WebP".into())
}

fn icon_views(settings: &Settings) -> BTreeMap<String, McpIconPreferenceView> {
    let current_keys: BTreeSet<String> = crate::resources::mcp::registry::read_registry()
        .into_iter()
        .map(|entry| entry.key())
        .collect();
    let mut views = BTreeMap::new();
    let Some(ui) = settings.ui.as_ref() else {
        return views;
    };
    for (asset_key, preference) in &ui.mcp_icons {
        if !current_keys.contains(asset_key) {
            continue;
        }
        match preference.kind.as_str() {
            "builtin" if normalize_builtin_icon_id(&preference.value).is_ok() => {
                views.insert(
                    asset_key.clone(),
                    McpIconPreferenceView {
                        kind: "builtin".into(),
                        value: preference.value.clone(),
                        path: None,
                    },
                );
            }
            "custom" => {
                let Some(filename) = safe_custom_filename(&preference.value) else {
                    continue;
                };
                let path = mcp_icons_dir().join(filename);
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    continue;
                }
                views.insert(
                    asset_key.clone(),
                    McpIconPreferenceView {
                        kind: "custom".into(),
                        value: preference.value.clone(),
                        path: Some(path.display().to_string()),
                    },
                );
            }
            _ => {}
        }
    }
    views
}

fn safe_custom_filename(value: &str) -> Option<&str> {
    let path = Path::new(value);
    let mut components = path.components();
    let Component::Normal(name) = components.next()? else {
        return None;
    };
    if components.next().is_some() || name.to_str()? != value {
        return None;
    }
    let extension = path.extension()?.to_str()?;
    if !matches!(extension, "png" | "jpg" | "webp") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    if stem.len() != 64 || !stem.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(value)
}

fn normalize_locale(locale: Option<String>) -> Result<Option<String>, String> {
    let Some(locale) = locale else {
        return Ok(None);
    };
    let locale = locale.trim();
    if SUPPORTED_UI_LOCALES.contains(&locale) {
        Ok(Some(locale.to_string()))
    } else {
        Err(format!("unsupported UI locale: {locale}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{RegistryConfig, RegistryEntry, RegistryOrigin, StdioConfig};
    use crate::testenv::TestHome;
    use serde_json::Value;
    use std::fs;

    fn register_mcp(name: &str) -> String {
        let entry = RegistryEntry {
            name: name.into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "npx".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: Some(RegistryOrigin {
                kind: "manual".into(),
                agent: None,
                scope: None,
                source: None,
            }),
            repo: None,
        };
        let key = entry.key();
        crate::registry::write_manual_entry(&entry).unwrap();
        key
    }

    #[test]
    fn locale_roundtrips_and_preserves_unknown_ui_fields() {
        let home = TestHome::new("ui-locale-roundtrip");
        let path = home.home.join(".arca/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"ui":{"future_ui_key":{"keep":true}},"future_section":{"keep":true}}"#,
        )
        .unwrap();

        assert_eq!(get_ui_locale().unwrap(), None);
        assert_eq!(
            set_ui_locale(Some("en-US".into())).unwrap().as_deref(),
            Some("en-US")
        );
        assert_eq!(get_ui_locale().unwrap().as_deref(), Some("en-US"));

        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["ui"]["future_ui_key"]["keep"], true);
        assert_eq!(value["future_section"]["keep"], true);
        assert_eq!(value["ui"]["locale"], "en-US");

        assert_eq!(set_ui_locale(None).unwrap(), None);
        let value: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert!(value["ui"].get("locale").is_none());
    }

    #[test]
    fn locale_rejects_unknown_values_without_writing() {
        let _home = TestHome::new("ui-locale-invalid");
        assert!(set_ui_locale(Some("fr-FR".into())).is_err());
        assert_eq!(get_ui_locale().unwrap(), None);
    }

    #[test]
    fn mcp_builtin_icon_roundtrips_without_clobbering_unknown_settings() {
        let home = TestHome::new("ui-mcp-icon-builtin");
        let path = home.home.join(".arca/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"ui":{"future_ui_key":{"keep":true}},"future_section":{"keep":true}}"#,
        )
        .unwrap();
        let key = register_mcp("brave-search");

        let icons = set_mcp_builtin_icon(key.clone(), "search".into()).unwrap();
        assert_eq!(icons[&key].kind, "builtin");
        assert_eq!(icons[&key].value, "search");
        assert_eq!(icons[&key].path, None);
        assert_eq!(list_mcp_icon_preferences().unwrap(), icons);

        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["ui"]["future_ui_key"]["keep"], true);
        assert_eq!(value["future_section"]["keep"], true);
        assert_eq!(value["ui"]["mcp_icons"][key.as_str()]["kind"], "builtin");
        assert_eq!(value["ui"]["mcp_icons"][key.as_str()]["value"], "search");

        assert!(set_mcp_builtin_icon(key.clone(), "Not Safe".into()).is_err());
        assert_eq!(list_mcp_icon_preferences().unwrap(), icons);
        assert!(set_mcp_builtin_icon("missing::stdio".into(), "search".into()).is_err());
    }

    #[test]
    fn mcp_custom_icon_is_content_addressed_private_and_resettable() {
        let home = TestHome::new("ui-mcp-icon-custom");
        let key = register_mcp("team-docs");
        let source = home.home.join("icon-input.bin");
        fs::write(&source, b"\x89PNG\r\n\x1a\ncustom-icon").unwrap();

        let icons = import_mcp_icon(key.clone(), source).unwrap();
        let icon = &icons[&key];
        assert_eq!(icon.kind, "custom");
        assert!(icon.value.ends_with(".png"));
        let copied = std::path::PathBuf::from(icon.path.as_ref().unwrap());
        assert!(copied.starts_with(home.home.join(".arca/assets/mcp-icons")));
        assert_eq!(fs::read(&copied).unwrap(), b"\x89PNG\r\n\x1a\ncustom-icon");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&copied).unwrap().permissions().mode() & 0o777, 0o600);
        }

        let jpeg = home.home.join("icon.jpg");
        fs::write(&jpeg, [0xff_u8, 0xd8, 0xff, 0xe0, 0x00]).unwrap();
        assert!(import_mcp_icon(key.clone(), jpeg).unwrap()[&key].value.ends_with(".jpg"));
        let webp = home.home.join("icon.webp");
        fs::write(&webp, b"RIFF\x04\x00\x00\x00WEBPdata").unwrap();
        assert!(import_mcp_icon(key.clone(), webp).unwrap()[&key].value.ends_with(".webp"));

        let reset = reset_mcp_icon(key.clone()).unwrap();
        assert!(!reset.contains_key(&key));
        assert!(!list_mcp_icon_preferences().unwrap().contains_key(&key));
    }

    #[test]
    fn mcp_custom_icon_rejects_unsupported_and_oversized_files() {
        let home = TestHome::new("ui-mcp-icon-invalid");
        let key = register_mcp("safe-icon-target");
        let svg = home.home.join("icon.svg");
        fs::write(&svg, b"<svg xmlns='http://www.w3.org/2000/svg' />").unwrap();
        assert!(import_mcp_icon(key.clone(), svg).is_err());

        let oversized = home.home.join("oversized.png");
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(MAX_MCP_ICON_BYTES + 1, 0);
        fs::write(&oversized, bytes).unwrap();
        assert!(import_mcp_icon(key.clone(), oversized).is_err());
        assert!(!list_mcp_icon_preferences().unwrap().contains_key(&key));

        mutate_settings_checked(|settings| {
            settings
                .ui
                .get_or_insert_with(UiSettings::default)
                .mcp_icons
                .insert(
                    key.clone(),
                    McpIconPreference {
                        kind: "custom".into(),
                        value: "../outside.png".into(),
                        extra: BTreeMap::new(),
                    },
                );
            Ok(())
        })
        .unwrap();
        assert!(!list_mcp_icon_preferences().unwrap().contains_key(&key));
    }
}
