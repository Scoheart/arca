use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_home() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "arca-version-metadata-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn version_does_not_bootstrap_or_rewrite_user_data() {
    let home = temporary_home();
    let arca_home = home.join(".arca");
    let settings = arca_home.join("settings.json");
    fs::create_dir_all(&arca_home).expect("create isolated Arca home");
    fs::write(&settings, b"{ intentionally invalid settings").expect("seed invalid settings");

    let output = Command::new(env!("CARGO_BIN_EXE_arca"))
        .arg("--version")
        .env("HOME", &home)
        .env("ARCA_HOME", &arca_home)
        .output()
        .expect("run arca --version");

    assert!(
        output.status.success(),
        "arca --version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("arca "));
    assert_eq!(
        fs::read(&settings).expect("read isolated settings"),
        b"{ intentionally invalid settings"
    );

    fs::remove_dir_all(home).expect("remove isolated Arca home");
}
