//! Isolated read-path benchmark. Never reads real Agent files or credentials.
//! cargo run -p arca-core --example perf_reads
use arca_core::{agents, application, registry, settings, sources, testenv::TestHome};
use serde_json::json;
use std::{fs, hint::black_box, time::Instant};

fn measure(name: &str, mut work: impl FnMut()) {
    work();
    let mut samples = Vec::new();
    for _ in 0..15 {
        let start = Instant::now();
        work();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!("{}", json!({"scenario": name, "median_ms": samples[7], "p95_ms": samples[14]}));
}

fn main() {
    let home = TestHome::new("perfread");
    let mut defs = Vec::new();
    for source in 0..12 {
        let def = serde_json::from_value(json!({
            "id": format!("perf-{source}"), "kind": "local", "name": format!("Source {source}"),
            "format": "json", "key": "mcpServers", "enabled": true
        })).unwrap();
        let path = sources::cached_path(&def).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let entries: Vec<_> = (0..80).map(|entry| json!({
            "name": format!("server-{entry}"), "description": "Synthetic MCP for read profiling",
            "tags": ["benchmark"], "config": {"stdio": {"command": "echo", "args": ["fixture"]}}
        })).collect();
        fs::write(path, serde_json::to_vec(&entries).unwrap()).unwrap();
        defs.push(def);
    }
    settings::save_settings(&settings::Settings { sources: Some(defs), ..Default::default() }).unwrap();
    fs::create_dir_all(home.home.join(".config/opencode")).unwrap();
    for skill in 0..120 {
        let dir = home.home.join(format!(".agents/skills/perf-{skill}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("---\nname: perf-{skill}\ndescription: Synthetic performance fixture\n---\n# Benchmark\n")).unwrap();
    }
    assert_eq!(registry::read_registry().len(), 80);
    assert_eq!(registry::read_registry_all().len(), 960);
    assert!(application::skills::list_inventory().unwrap().items.len() >= 120);
    measure("builtin_agents_100", || { for _ in 0..100 { black_box(agents::builtin_agents()); } });
    measure("registry_refresh", || {
        black_box(registry::read_registry_snapshot());
    });
    measure("registry_with_sources_legacy", || {
        black_box(registry::read_registry());
        black_box(registry::read_registry_all());
        black_box(registry::user_override_keys());
        black_box(sources::list_views());
    });
    measure("skills_and_relationships", || {
        let observation = application::assets::observe_resources();
        black_box(observation.skills.unwrap());
        black_box(observation.relationships.unwrap());
    });
    let snapshot = registry::read_registry_snapshot();
    assert_eq!(serde_json::to_value(&snapshot.entries).unwrap(), serde_json::to_value(registry::read_registry()).unwrap());
    assert_eq!(serde_json::to_value(&snapshot.catalog).unwrap(), serde_json::to_value(registry::read_registry_all()).unwrap());
    assert_eq!(snapshot.custom_keys, registry::user_override_keys());
    assert_eq!(serde_json::to_value(&snapshot.sources).unwrap(), serde_json::to_value(sources::list_views()).unwrap());
    // Keep manual precedence and disabled-source override semantics unchanged.
    let mut manual = registry::read_registry().remove(0);
    manual.description = "Manual winner".into();
    registry::write_manual_entry(&manual).unwrap();
    for enabled in [true, false] {
        let mut settings = settings::load_settings();
        settings.sources.as_mut().unwrap().iter_mut()
            .find(|source| source.id == registry::MANUAL_ID).unwrap().enabled = enabled;
        settings::save_settings(&settings).unwrap();
        let snapshot = registry::read_registry_snapshot();
        assert_eq!(serde_json::to_value(&snapshot.entries).unwrap(), serde_json::to_value(registry::read_registry()).unwrap());
        assert_eq!(serde_json::to_value(&snapshot.catalog).unwrap(), serde_json::to_value(registry::read_registry_all()).unwrap());
        assert_eq!(snapshot.custom_keys, registry::user_override_keys());
        assert_eq!(serde_json::to_value(&snapshot.sources).unwrap(), serde_json::to_value(sources::list_views()).unwrap());
        assert_eq!(snapshot.custom_keys, [manual.key()]);
    }
    println!("{}", json!({"scenario": "registry_precedence_and_disabled_manual", "result": "pass"}));
}
