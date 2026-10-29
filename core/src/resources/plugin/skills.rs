//! Pure projection of already observed Skill metadata. No scanning, body reads,
//! target execution, imports or asset lifecycle operations belong in this path.
use super::*;

pub(super) fn observations(plugins: &[NativePluginObservation]) -> Vec<NativeSkillObservation> {
    let resource_groups: BTreeSet<_> = hosts().iter().flat_map(|host| {
        host.resource_groups.iter().map(move |group| (host.agent_id.as_str(), group.as_str()))
    }).collect();
    let mut rows = BTreeMap::new();
    for plugin in plugins {
        if resource_groups.contains(&(plugin.agent_id.as_str(), plugin.plugin_id.as_str())) { continue; }
        for component in &plugin.components {
            if component.kind != PluginComponentKind::Skill { continue; }
            let identity = serde_json::to_vec(&(
                &plugin.agent_id, &plugin.plugin_id, &plugin.version, &plugin.revision,
                &plugin.presence, &plugin.source_paths, &plugin.source_fingerprint,
                &component.name, &component.source_path, &component.native_id,
            )).expect("native Skill identity contains only serializable metadata");
            let id = format!("native-skill:{}", hex::encode(Sha256::digest(identity)));
            rows.entry(id.clone()).or_insert_with(|| NativeSkillObservation {
                id, agent_id: plugin.agent_id.clone(), name: component.name.clone(),
                plugin_id: plugin.plugin_id.clone(), plugin_name: plugin.name.clone(),
                plugin_version: plugin.version.clone(), plugin_revision: plugin.revision.clone(),
                plugin_enabled: plugin.enabled, presence: plugin.presence.clone(),
                source_path: component.source_path.clone(), native_id: component.native_id.clone(),
                source_fingerprint: plugin.source_fingerprint.clone(), docs_url: plugin.docs_url.clone(),
            });
        }
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by(|a, b| (&a.name, &a.plugin_name, &a.id).cmp(&(&b.name, &b.plugin_name, &b.id)));
    rows
}

#[cfg(test)]
mod tests;
