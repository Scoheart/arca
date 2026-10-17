//! Bind capture to the authoritative launcher, including Node/Bun CLI scripts.
//! No command line or credential is returned to clients or persisted.
use super::{expand_pids, CaptureTarget, Process};
use crate::domain::agents::LaunchTarget;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, sync::LazyLock};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureCaScope { Process, Mixed, System }

#[derive(Deserialize)]
pub(super) struct CaptureProfile {
    pub ca_environment: Vec<String>,
    #[serde(default)] pub explicit_proxy: bool,
    #[serde(default)] pub chromium: bool,
    #[serde(default)] pub process_title: Option<String>,
    #[serde(default)] pub npm_package: Option<String>,
    pub ca_scope: CaptureCaScope,
}

pub(super) static PROFILES: LazyLock<BTreeMap<String, CaptureProfile>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/capture-agent-profiles.json"))
        .expect("capture profiles must be valid")
});

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentCaptureSupport {
    pub agent_id: String,
    pub available: bool,
    pub kind: Option<String>,
    pub target_ids: Vec<String>,
    pub ca_environment: Vec<String>,
    pub chromium: bool,
    pub ca_scope: CaptureCaScope,
    pub explicit_proxy: bool,
}

fn canonical(path: &str) -> String {
    fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf()).to_string_lossy().into_owned()
}

pub(super) fn chromium(target: &LaunchTarget) -> bool {
    match target {
        LaunchTarget::App { path, .. } => ["Electron Framework.framework", "Codex Framework.framework", "Chromium Embedded Framework.framework"]
            .iter().any(|framework| Path::new(path).join("Contents/Frameworks").join(framework).exists()),
        _ => false,
    }
}

enum Matcher { App(String), Cli { executable: String, arguments: Option<regex::Regex>, generic: bool }, Unsupported }
impl Matcher {
    fn new(target: &LaunchTarget) -> Self {
        match target {
            LaunchTarget::App { path, .. } => Self::App(format!("{}/Contents/", canonical(path))),
            LaunchTarget::Cli { command, args, .. } => {
                let executable = canonical(command);
                let generic = Path::new(&executable).file_name().is_some_and(|name| matches!(name.to_str(), Some("node" | "bun" | "python" | "python3" | "bash" | "zsh" | "sh" | "npx" | "uv")));
                let paths = if generic { args.join(" ") } else { format!("{}|{}", regex::escape(command), regex::escape(&executable)) };
                let expression = if generic { regex::escape(&paths) } else { paths };
                let arguments = if expression.is_empty() { None } else {
                    regex::Regex::new(&format!(r#"(?:^|[\s"'])(?:{})(?:$|[\s"'])"#, expression)).ok()
                };
                Self::Cli { executable, arguments, generic }
            }
            LaunchTarget::Web { .. } => Self::Unsupported,
        }
    }
    fn matches(&self, row: &Process) -> bool {
        match self {
            Self::App(prefix) => row.command.starts_with(prefix),
            Self::Cli { executable, arguments, generic } => {
                let argument_match = arguments.as_ref().is_some_and(|pattern| pattern.is_match(&row.arguments));
                if *generic { row.command == *executable && argument_match }
                else { row.command == *executable || argument_match }
            }
            Self::Unsupported => false,
        }
    }
}

pub(super) fn bind_launchers(rows: &[Process]) -> Vec<CaptureTarget> {
    bind(rows, None)
}
pub(super) fn bind_agent(rows: &[Process], id: &str) -> Vec<CaptureTarget> {
    bind(rows, Some(id))
}
fn bind(rows: &[Process], selected: Option<&str>) -> Vec<CaptureTarget> {
    let mut targets: BTreeMap<String, CaptureTarget> = BTreeMap::new();
    for id in PROFILES.keys().filter(|id| selected.is_none_or(|selected| selected == id.as_str())) {
        let Ok(info) = crate::application::agent_launch::info_unlocked(id) else { continue; };
        let Some(launcher) = info.resolved_target else { continue; };
        let matcher = Matcher::new(&launcher);
        let profile = &PROFILES[id];
        let script = match &launcher { LaunchTarget::Cli { command, .. } =>
            Path::new(&canonical(command)).extension().is_some_and(|ext| matches!(ext.to_str(), Some("js" | "mjs" | "cjs"))), _ => false };
        let package_prefix = profile.npm_package.as_ref().map(|package| format!("/node_modules/{package}/"));
        let matched: Vec<_> = rows.iter().filter(|row| {
            if matcher.matches(row) { return true; }
            // Pi replaces its argv with process.title after boot. The title
            // is verified from its shipped entry point; restrict this fallback
            // to its installed JS launcher and an actual Node/Bun executable.
            let interpreter = Path::new(&row.command).file_name().is_some_and(|name| matches!(name.to_str(), Some("node" | "bun")));
            if interpreter && package_prefix.as_ref().is_some_and(|prefix| row.arguments.split_whitespace().any(|argument| {
                let path = Path::new(argument.trim_matches(['\'', '"']));
                path.is_absolute() && argument.contains(prefix) && path.is_file()
                    && canonical(&path.to_string_lossy()).contains(prefix)
            })) { return true; }
            script && profile.process_title.as_deref().is_some_and(|title| row.arguments.trim() == title)
                && interpreter
        }).collect();
        match launcher {
            LaunchTarget::App { path, .. } if !matched.is_empty() => {
                let id_key = format!("app:{}", canonical(&path));
                let roots: Vec<_> = matched.iter().map(|row| row.pid).collect();
                let entry = targets.entry(id_key.clone()).or_insert_with(|| CaptureTarget {
                    id: id_key, name: info.name.clone(), pids: expand_pids(rows, &roots), agent_ids: vec![],
                });
                entry.agent_ids.push(id.clone());
                // Top-level instances can be selected independently. This
                // avoids mixing a fresh isolated window with existing ones.
                for row in matched.iter().filter(|row| !matched.iter().any(|parent| parent.pid == row.parent)) {
                    let key = format!("pid:{}:{}", row.pid, row.started_at);
                    let instance = targets.entry(key.clone()).or_insert_with(|| CaptureTarget {
                        id: key, name: format!("{} · PID {}", info.name, row.pid),
                        pids: expand_pids(rows, &[row.pid]), agent_ids: vec![],
                    });
                    instance.agent_ids.push(id.clone());
                }
            }
            LaunchTarget::Cli { .. } => {
                for row in matched {
                    // PID alone can be recycled for an unrelated process.
                    let key = format!("pid:{}:{}", row.pid, row.started_at);
                    let entry = targets.entry(key.clone()).or_insert_with(|| CaptureTarget {
                        id: key, name: format!("{} · PID {}", info.name, row.pid),
                        pids: expand_pids(rows, &[row.pid]), agent_ids: vec![],
                    });
                    entry.agent_ids.push(id.clone());
                }
            }
            _ => {},
        }
    }
    targets.into_values().collect()
}

pub(super) fn support(targets: &[CaptureTarget]) -> Vec<AgentCaptureSupport> {
    PROFILES.iter().filter_map(|(id, profile)| {
        let info = crate::application::agent_launch::info_unlocked(id).ok()?;
        let explicit_proxy = profile.explicit_proxy && info.kind.as_deref() == Some("cli");
        Some(AgentCaptureSupport {
            agent_id: id.clone(), available: info.available, kind: info.kind,
            target_ids: targets.iter().filter(|target| target.agent_ids.contains(id)).map(|target| target.id.clone()).collect(),
            ca_environment: profile.ca_environment.clone(), chromium: profile.chromium && info.resolved_target.as_ref().is_some_and(chromium),
            ca_scope: profile.ca_scope,
            explicit_proxy,
        })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_arguments_bind_the_agent_without_capturing_every_node_process() {
        let target = LaunchTarget::Cli { command: "/tmp/pi/dist/cli.js".into(), args: vec![], env: BTreeMap::new() };
        let row = |arguments: &str| Process { pid: 12, parent: 1, command: "/usr/bin/node".into(), arguments: arguments.into(), started_at: "Mon Oct 12 00:00:00 2026".into() };
        let matcher = Matcher::new(&target);
        assert!(matcher.matches(&row("node /tmp/pi/dist/cli.js --api-key hidden")));
        assert!(!matcher.matches(&row("node /tmp/other/dist/cli.js")));
        assert!(!matcher.matches(&row("node /tmp/pi/dist/cli.js.unrelated")));
        let bare = Matcher::new(&LaunchTarget::Cli { command: "/usr/bin/node".into(), args: vec![], env: BTreeMap::new() });
        assert!(!bare.matches(&row("node /tmp/other/dist/cli.js")));
    }
}
