use super::{agents_home_dir, default_profile, home_dir, read_text};
use crate::domain::plugin::NativeFileSkillObservation;
use crate::resources::{mcp::scanner::collapse_home, skill::frontmatter_between_delimiters};
use serde_yaml::Value;
use std::{collections::BTreeSet, fs, path::Path};

const MAX_ENTRIES: usize = 4096;
const MAX_FILES: usize = 256;
const MAX_BYTES: usize = 8 * 1024 * 1024;

/// DSH reads standalone Markdown as well as Skill directories. Only report
/// declarations from its two native user roots. Session policy and resolution
/// order belong to Harness, and these files are never Arca Skill targets.
pub(crate) fn observe_file_skills(agent_id: &str) -> Vec<NativeFileSkillObservation> {
    if default_profile(agent_id).is_none() { return Vec::new(); }
    let mut roots = BTreeSet::new();
    let mut rows = Vec::new();
    let mut budget = MAX_BYTES;
    let mut files = 0;
    for root in [home_dir().join("skills"), agents_home_dir().join("skills")] {
        if !roots.insert(root.clone()) { continue; }
        let Ok(entries) = fs::read_dir(root) else { continue; };
        for entry in entries.take(MAX_ENTRIES).flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md")
                || !entry.file_type().is_ok_and(|kind| kind.is_file()) { continue; }
            if files >= MAX_FILES || budget == 0 { break; }
            files += 1;
            let Ok(Some(text)) = read_text(&path) else { continue; };
            budget = budget.saturating_sub(text.len());
            if let Some(row) = declaration(&path, &text) { rows.push(row); }
        }
    }
    rows.sort_by(|a, b| (&a.name, &a.source_path).cmp(&(&b.name, &b.source_path)));
    rows
}

fn declaration(path: &Path, text: &str) -> Option<NativeFileSkillObservation> {
    let yaml = frontmatter_between_delimiters(text)?;
    let metadata = serde_yaml::from_str::<Value>(yaml).ok()?;
    let name = metadata.get("name")?.as_str()?;
    let description = metadata.get("description")?.as_str()?;
    if name.len() > 256 || !name.split('-').all(|part| !part.is_empty()
        && part.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()))
        || description.trim().is_empty()
        || ["disableModelInvocation", "modelInvocable", "userInvocable"]
            .iter().any(|key| metadata.get(*key).is_some())
    {
        return None;
    }
    Some(NativeFileSkillObservation {
        name: name.into(),
        description: description.chars().take(1024).collect(),
        source_path: collapse_home(path.to_str()?),
        model_invocable: !boolean(metadata.get("disable-model-invocation"), false)?,
        user_invocable: boolean(metadata.get("user-invocable"), true)?,
    })
}

fn boolean(value: Option<&Value>, default: bool) -> Option<bool> {
    let Some(value) = value else { return Some(default); };
    if let Some(value) = value.as_bool() { return Some(value); }
    match value.as_str().map(str::to_ascii_lowercase).as_deref() {
        Some("true" | "yes" | "on" | "1") => Some(true),
        Some("false" | "no" | "off" | "0") => Some(false),
        _ => match value.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
    }
}
