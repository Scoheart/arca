use super::*;
use crate::testenv::TestHome;

fn plugin(id: &str) -> NativePluginObservation {
    NativePluginObservation {
        agent_id: "cursor".into(), plugin_id: id.into(), name: "Example plugin".into(),
        version: "1.0.0".into(), revision: "revision".into(), presence: PluginPresence::Installed,
        enabled: None, source_paths: vec!["~/plugins/example/plugin.json".into()],
        components: vec![PluginComponent {
            kind: PluginComponentKind::Skill, name: "review".into(),
            source_path: "~/plugins/example/skills/review/SKILL.md".into(), native_id: None,
        }], docs_url: "https://example.invalid/plugin".into(), source_fingerprint: "fingerprint".into(),
    }
}

#[test]
fn only_skill_components_become_readonly_rows_with_their_actual_owner() {
    let mut item = plugin("example");
    for kind in [PluginComponentKind::Rule, PluginComponentKind::Command, PluginComponentKind::McpReference] {
        item.components.push(PluginComponent { kind, name: "not-a-skill".into(), source_path: "metadata-only".into(), native_id: None });
    }
    let rows = observations(&[item]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "review");
    assert_eq!(rows[0].plugin_id, "example");
    assert_eq!(rows[0].plugin_name, "Example plugin");
    assert_eq!(rows[0].plugin_version, "1.0.0");
    assert_eq!(rows[0].source_path, "~/plugins/example/skills/review/SKILL.md");
    assert_eq!(rows[0].plugin_enabled, None);
}

#[test]
fn same_names_from_other_plugins_versions_and_physical_paths_remain_distinct() {
    let first = plugin("first");
    let second = plugin("second");
    let mut version = first.clone(); version.version = "2.0.0".into();
    let mut physical = first.clone(); physical.components[0].source_path = "~/other/skills/review/SKILL.md".into();
    let rows = observations(&[first, second, version, physical]);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows.iter().map(|row| &row.id).collect::<BTreeSet<_>>().len(), 4);
}

#[test]
fn identical_component_labels_from_distinct_plugin_snapshots_keep_their_sources() {
    let first = plugin("example");
    let mut second = first.clone();
    second.source_paths = vec!["~/second-market/example/plugin.json".into()];
    second.source_fingerprint = "other-physical-source".into();
    let rows = observations(&[first, second]);
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].id, rows[1].id);
}

#[test]
fn cache_declarations_and_plugin_enablement_never_turn_into_skill_activation() {
    let mut cached = plugin("cached"); cached.presence = PluginPresence::CacheOnly;
    let mut declared = plugin("declared"); declared.presence = PluginPresence::Declared; declared.enabled = Some(false);
    let rows = observations(&[cached, declared]);
    assert!(rows.iter().any(|row| row.presence == PluginPresence::CacheOnly && row.plugin_enabled.is_none()));
    assert!(rows.iter().any(|row| row.presence == PluginPresence::Declared && row.plugin_enabled == Some(false)));
    let json = serde_json::to_value(&rows).unwrap();
    assert!(json.as_array().unwrap().iter().all(|row| row.get("asset").is_none() && row.get("enabled").is_none()));
}

#[test]
fn declared_global_resource_groups_are_not_attributed_to_plugins() {
    let mut global = plugin("@global/resources"); global.agent_id = "hermes".into();
    let mut other = global.clone(); other.agent_id = "pi".into();
    let mut local_plugin = plugin("ordinary-local-plugin"); local_plugin.presence = PluginPresence::Local;
    let rows = observations(&[global, other, local_plugin]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].plugin_id, "ordinary-local-plugin");
}

#[test]
fn builtin_skill_groups_remain_visible_and_exact_duplicates_are_removed() {
    let mut bundled = plugin("@builtin/skills"); bundled.agent_id = "claude-desktop".into(); bundled.presence = PluginPresence::Bundled;
    let rows = observations(&[bundled.clone(), bundled.clone()]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].presence, PluginPresence::Bundled);
    assert_eq!(observations(&[plugin("b"), plugin("a")]), observations(&[plugin("a"), plugin("b")]));
}

#[test]
fn projection_uses_observed_metadata_without_reading_skill_bodies_or_resolving_paths() {
    let home = TestHome::new("native-skill-projection");
    let body = home.home.join("SKILL.md"); std::fs::write(&body, "PRIVATE_SKILL_BODY").unwrap();
    let mut item = plugin("metadata"); item.components[0].source_path = body.to_string_lossy().into_owned();
    let rows = observations(&[item]);
    assert_eq!(rows.len(), 1);
    assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_SKILL_BODY"));
}
