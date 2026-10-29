use super::*;
use crate::testenv::TestHome;
use std::fs;

// This target-specific regression catches the generic --version execution
// fallback. Shared artifact-runtime tests cover digest, alias CAS and budgets.
#[test]
fn unknown_grok_launcher_stays_unknown_without_executing_or_borrowing_a_label() {
    let home = TestHome::new("grok-passive-version");
    assert_eq!(crate::application::agent_launch::runtime_version("grok-build").unwrap(), None);
    let selected = home.home.join("bin/grok");
    fs::write(&selected, b"#!/bin/sh\nprintf executed > \"$HOME/grok-version-executed\"\nprintf 'Grok 1.0.46\\n'\n").unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::create_dir_all(home.home.join(".grok/downloads")).unwrap();
    fs::write(home.home.join(".grok/current_version"), b"1.0.46\n").unwrap();
    fs::write(home.home.join(".grok/downloads/grok-1.0.46-macos-aarch64"), b"unverified sibling").unwrap();
    assert_eq!(runtime_version(&selected), None);
    assert_eq!(super::super::passive_runtime_version("grok-build", &selected), Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("grok-build").unwrap(), None);
    assert!(!home.home.join("grok-version-executed").exists());
}
