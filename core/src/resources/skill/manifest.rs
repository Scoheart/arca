use super::{capped_message, normalized_error_path, SkillError, SkillManifest};
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Debug, Deserialize)]
struct RawManifest {
    name: String,
    description: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(rename = "allowed-tools", default)]
    allowed_tools: Option<String>,
}

static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap());

fn frontmatter_between_delimiters(text: &str) -> Option<&str> {
    let first_newline = text.find('\n')?;
    if text[..first_newline].trim_end_matches('\r') != "---" {
        return None;
    }
    let rest = &text[first_newline + 1..];
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(&['\r', '\n'][..]) == "---" {
            return Some(&rest[..offset]);
        }
        offset += line.len();
    }
    None
}

fn invalid(root: &Path, message: impl AsRef<str>) -> SkillError {
    SkillError::InvalidManifest {
        message: capped_message(message),
        path: normalized_error_path(root),
    }
}

pub fn parse_manifest(root: &Path, text: &str) -> Result<SkillManifest, SkillError> {
    let yaml = frontmatter_between_delimiters(text)
        .ok_or_else(|| invalid(root, "SKILL.md must start with YAML frontmatter"))?;
    let raw: RawManifest = serde_yaml::from_str(yaml)
        .map_err(|error| invalid(root, format!("invalid YAML frontmatter: {error}")))?;
    if !(1..=64).contains(&raw.name.len()) || !NAME.is_match(&raw.name) {
        return Err(invalid(
            root,
            "name must match ^[a-z0-9]+(?:-[a-z0-9]+)*$ and be at most 64 characters",
        ));
    }
    if raw.description.trim().is_empty() || raw.description.chars().count() > 1024 {
        return Err(invalid(
            root,
            "description must contain 1 to 1024 characters",
        ));
    }
    if raw
        .compatibility
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value.chars().count() > 500)
    {
        return Err(invalid(
            root,
            "compatibility must contain 1 to 500 characters when provided",
        ));
    }
    if root.file_name().and_then(OsStr::to_str) != Some(raw.name.as_str()) {
        return Err(invalid(root, "name must match the parent directory"));
    }
    Ok(SkillManifest {
        name: raw.name,
        description: raw.description,
        license: raw.license,
        compatibility: raw.compatibility,
        metadata: raw.metadata,
        allowed_tools: raw.allowed_tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::skill::{validate_candidate, SkillContentKind};
    use crate::testenv::TestHome;
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/skills")
            .join(name)
    }

    #[cfg(unix)]
    #[test]
    fn parses_a_valid_skill_and_requires_directory_name() {
        let root = fixture("safe");
        let parsed = validate_candidate(&root).unwrap();
        assert_eq!(parsed.manifest.name, "safe");
        assert_eq!(parsed.manifest.description, "Safe fixture for Arca tests");
        assert_eq!(parsed.content_kind, SkillContentKind::Reference);

        let th = TestHome::new("skill-name-mismatch");
        let renamed = th.home.join("different");
        fs::create_dir_all(&renamed).unwrap();
        fs::copy(root.join("SKILL.md"), renamed.join("SKILL.md")).unwrap();
        assert!(matches!(
            validate_candidate(&renamed),
            Err(SkillError::InvalidManifest { .. })
        ));
    }

    #[test]
    fn enforces_description_and_compatibility_boundaries() {
        let th = TestHome::new("skill-manifest-boundaries");
        let root = th.home.join("boundary");
        fs::create_dir_all(&root).unwrap();
        let manifest = |description: &str, compatibility: &str| {
            format!(
                "---\nname: boundary\ndescription: {description}\ncompatibility: {compatibility}\n---\nbody\n"
            )
        };
        assert!(parse_manifest(&root, &manifest(&"d".repeat(1024), &"c".repeat(500))).is_ok());
        assert!(matches!(
            parse_manifest(&root, &manifest(&"d".repeat(1025), "macOS")),
            Err(SkillError::InvalidManifest { .. })
        ));
        assert!(matches!(
            parse_manifest(&root, &manifest("description", &"c".repeat(501))),
            Err(SkillError::InvalidManifest { .. })
        ));
        assert!(matches!(
            parse_manifest(&root, &manifest("   ", "macOS")),
            Err(SkillError::InvalidManifest { .. })
        ));
    }

    #[test]
    fn parses_exact_agent_skills_fields_and_rejects_invalid_frontmatter() {
        let th = TestHome::new("skill-manifest-fields");
        let root = th.home.join("field-test");
        fs::create_dir_all(&root).unwrap();
        let text = "---\nname: field-test\ndescription: Field test\nlicense: MIT\ncompatibility: macOS\nmetadata:\n  owner: arca\nallowed-tools: Read Write\n---\nbody\n";
        let parsed = parse_manifest(&root, text).unwrap();
        assert_eq!(parsed.license.as_deref(), Some("MIT"));
        assert_eq!(parsed.compatibility.as_deref(), Some("macOS"));
        assert_eq!(
            parsed.metadata.get("owner").map(String::as_str),
            Some("arca")
        );
        assert_eq!(parsed.allowed_tools.as_deref(), Some("Read Write"));

        assert!(matches!(
            parse_manifest(&root, "name: field-test\ndescription: missing delimiters\n"),
            Err(SkillError::InvalidManifest { .. })
        ));
        assert!(matches!(
            parse_manifest(
                &root,
                "---\nname: Field_Test\ndescription: invalid name\n---\n"
            ),
            Err(SkillError::InvalidManifest { .. })
        ));
        assert!(matches!(
            parse_manifest(
                &root,
                "---\nname: field-test\ndescription: invalid metadata\nmetadata:\n  count: [1]\n---\n"
            ),
            Err(SkillError::InvalidManifest { .. })
        ));
    }
}
