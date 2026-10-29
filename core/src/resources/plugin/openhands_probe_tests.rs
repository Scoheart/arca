use super::*;
use crate::testenv::TestHome;

#[cfg(unix)]
#[test]
fn openhands_default_launcher_does_not_bootstrap_a_target_to_obtain_its_version() {
    use std::os::unix::fs::PermissionsExt;
    let home = TestHome::new("openhands-passive-probe");
    let selected = home.home.join("bin/openhands");
    fs::write(&selected, b"#!/bin/sh\nprintf executed > \"$HOME/openhands-bootstrap-marker\"\n").unwrap();
    fs::set_permissions(&selected, fs::Permissions::from_mode(0o755)).unwrap();
    let info = crate::application::agent_launch::info("openhands").unwrap();
    assert_eq!(info.available, cfg!(target_os = "macos"));
    assert_eq!(info.installed, Some(true));
    let Some(crate::domain::agents::LaunchTarget::Cli { command, args, env }) = info.resolved_target else {
        panic!("OpenHands CLI must resolve its own console entry");
    };
    assert_eq!(Path::new(&command), selected.as_path());
    assert!(args.is_empty() && env.is_empty());
    assert_eq!(passive_runtime_version("openhands", &selected), Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("openhands").unwrap(), None);
    assert!(!home.home.join("openhands-bootstrap-marker").exists());
}
