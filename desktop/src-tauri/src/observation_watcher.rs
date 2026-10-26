use arca_core::assets::{ObservationDomain, ObservationWatchTarget};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const EVENT_NAME: &str = "asset-observation-changed";

#[derive(Clone, Serialize)]
struct ObservationChange {
    domains: Vec<ObservationDomain>,
}

pub fn start(app: AppHandle) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(256);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = overflow.clone();
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<Event>| {
            if event.as_ref().is_ok_and(|event| matches!(event.kind, EventKind::Access(_))) {
                return;
            }
            if let Err(mpsc::TrySendError::Full(_)) = sender.try_send(event) {
                callback_overflow.store(true, Ordering::Release);
            }
        },
        Config::default(),
    )
    .map_err(|error| error.to_string())?;

    let mut roots = desired_roots(&arca_core::assets::observation_watch_targets());
    // A single inaccessible Agent path must not disable observation for every
    // other capability. Keep every watch that the host can establish.
    roots.retain(|root, mode| watcher.watch(root, *mode).is_ok());

    std::thread::Builder::new()
        .name("arca-observation-watcher".into())
        .spawn(move || {
            // Keep the watcher owned by this thread. Coalesce editor temp-file
            // sequences and Arca's own atomic writes into one projection refresh.
            while let Ok(first) = receiver.recv() {
                let targets = arca_core::assets::observation_watch_targets();
                let deadline = Instant::now() + Duration::from_secs(1);
                let mut domains = affected_domains(&[first], &targets);
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() { break; }
                    match receiver.recv_timeout(remaining.min(Duration::from_millis(250))) {
                        Ok(event) => domains.extend(affected_domains(&[event], &targets)),
                        Err(_) => break,
                    }
                }
                // A bounded queue may discard paths, but never the fact that
                // observation was lost. Reconcile all domains on overflow.
                if overflow.swap(false, Ordering::AcqRel) {
                    domains.insert(ObservationDomain::Central);
                }
                if domains.is_empty() {
                    continue;
                }
                // A target may not exist at startup. After its nearest watched
                // ancestor changes, move the watch closer (and switch Skill
                // directories to recursive mode) so later edits stay live.
                // Agent capability paths are themselves mutable central state.
                // Rebuild targets as well as roots so changing a configured
                // path takes effect without restarting Arca.
                let next = desired_roots(&arca_core::assets::observation_watch_targets());
                if next != roots {
                    for root in roots.keys() {
                        let _ = watcher.unwatch(root);
                    }
                    roots.clear();
                    for (root, mode) in next {
                        if watcher.watch(&root, mode).is_ok() {
                            roots.insert(root, mode);
                        }
                    }
                }
                if domains.contains(&ObservationDomain::Model)
                    || domains.contains(&ObservationDomain::Central)
                {
                    // Reconcile before notifying readers so the fresh snapshot
                    // does not label a normal native /model switch as drift.
                    if let Err(error) = arca_core::application::models::reconcile_native_selection() {
                        eprintln!("Arca native model selection reconciliation: {error}");
                    }
                }
                let _ = app.emit(
                    EVENT_NAME,
                    ObservationChange {
                        domains: domains.into_iter().collect(),
                    },
                );
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn affected_domains(
    events: &[notify::Result<Event>],
    targets: &[ObservationWatchTarget],
) -> BTreeSet<ObservationDomain> {
    let mut domains = BTreeSet::new();
    for event in events.iter().filter_map(|event| event.as_ref().ok()) {
        // Reads and metadata probes must never create a refresh feedback loop.
        if matches!(&event.kind, EventKind::Access(_)) {
            continue;
        }
        for target in targets {
            if event
                .paths
                .iter()
                .any(|path| path_affects_target(path, target))
            {
                domains.insert(target.domain.clone());
            }
        }
    }
    domains
}

fn path_affects_target(path: &Path, target: &ObservationWatchTarget) -> bool {
    if path == target.path {
        return true;
    }
    if target.recursive && path.starts_with(&target.path) {
        return true;
    }
    // Only an absent target needs ancestor events to follow directories as
    // they are created. Once the target exists, a broad parent notification is
    // not evidence that this specific Agent input changed.
    !target.path.exists() && target.path.starts_with(path)
}

fn desired_roots(
    targets: &[arca_core::assets::ObservationWatchTarget],
) -> BTreeMap<PathBuf, RecursiveMode> {
    let mut roots = BTreeMap::new();
    for target in targets {
        let Some((root, mode)) = watch_root(&target.path, target.recursive) else {
            continue;
        };
        roots
            .entry(root)
            .and_modify(|current| {
                if mode == RecursiveMode::Recursive {
                    *current = RecursiveMode::Recursive;
                }
            })
            .or_insert(mode);
    }
    roots
}

fn watch_root(path: &Path, recursive: bool) -> Option<(PathBuf, RecursiveMode)> {
    if recursive && path.is_dir() {
        return Some((path.to_path_buf(), RecursiveMode::Recursive));
    }
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    while !current.exists() {
        current = current.parent()?.to_path_buf();
    }
    Some((current, RecursiveMode::NonRecursive))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, ModifyKind};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "arca-observation-watcher-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn missing_target_moves_to_the_nearest_existing_parent() {
        let root = temporary_root("nearest-parent");
        let target = root.join("agent/config/settings.json");
        assert_eq!(
            watch_root(&target, false),
            Some((root.clone(), RecursiveMode::NonRecursive))
        );

        let agent = root.join("agent");
        fs::create_dir(&agent).unwrap();
        assert_eq!(
            watch_root(&target, false),
            Some((agent.clone(), RecursiveMode::NonRecursive))
        );

        let config = agent.join("config");
        fs::create_dir(&config).unwrap();
        assert_eq!(
            watch_root(&target, false),
            Some((config, RecursiveMode::NonRecursive))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_skill_directory_is_recursive() {
        let root = temporary_root("recursive-skill");
        let skills = root.join("skills");
        fs::create_dir(&skills).unwrap();
        assert_eq!(
            watch_root(&skills, true),
            Some((skills, RecursiveMode::Recursive))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unrelated_sibling_changes_do_not_match_a_missing_target() {
        let root = temporary_root("unrelated-sibling");
        let target = ObservationWatchTarget {
            domain: ObservationDomain::Model,
            path: root.join("agent/config.json"),
            recursive: false,
        };
        let event = Ok(
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
                .add_path(root.join("other/cache.json")),
        );

        assert!(affected_domains(&[event], &[target]).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn broad_parent_events_do_not_refresh_an_existing_file_target() {
        let root = temporary_root("existing-file-parent");
        let path = root.join("config.json");
        fs::write(&path, b"{}").unwrap();
        let target = ObservationWatchTarget {
            domain: ObservationDomain::Model,
            path: path.clone(),
            recursive: false,
        };
        let parent_event =
            Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(root.clone()));
        assert!(affected_domains(&[parent_event], std::slice::from_ref(&target)).is_empty());

        let target_event = Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path));
        assert_eq!(
            affected_domains(&[target_event], &[target]),
            BTreeSet::from([ObservationDomain::Model])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creating_a_missing_target_ancestor_moves_the_watch_forward() {
        let root = temporary_root("target-ancestor");
        let target = ObservationWatchTarget {
            domain: ObservationDomain::Mcp,
            path: root.join("agent/config/settings.json"),
            recursive: false,
        };
        let created = root.join("agent");
        let event = Ok(Event::new(EventKind::Create(CreateKind::Folder)).add_path(created));

        assert_eq!(
            affected_domains(&[event], &[target]),
            BTreeSet::from([ObservationDomain::Mcp])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursive_targets_match_descendants_but_ignore_access_events() {
        let root = temporary_root("recursive-events");
        let skills = root.join("skills");
        fs::create_dir(&skills).unwrap();
        let target = ObservationWatchTarget {
            domain: ObservationDomain::Skill,
            path: skills.clone(),
            recursive: true,
        };
        let skill_file = skills.join("example/SKILL.md");
        let modified = Ok(
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
                .add_path(skill_file.clone()),
        );
        assert_eq!(
            affected_domains(&[modified], std::slice::from_ref(&target)),
            BTreeSet::from([ObservationDomain::Skill])
        );

        let accessed = Ok(Event::new(EventKind::Access(AccessKind::Any)).add_path(skill_file));
        assert!(affected_domains(&[accessed], &[target]).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
