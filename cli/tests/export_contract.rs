use std::fs;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_home() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_nanos();
    let temp_root = fs::canonicalize(std::env::temp_dir()).expect("canonical temporary root");
    temp_root.join(format!(
        "arca-export-contract-{}-{nonce}",
        std::process::id()
    ))
}

fn run(home: &std::path::Path, args: &[&std::ffi::OsStr]) -> Output {
    let arca_home = home.join(".arca");
    let probe_root = home.join("probe");
    let mut command = Command::new(env!("CARGO_BIN_EXE_arca"));
    command
        .args(args)
        .env("HOME", home)
        .env("ARCA_HOME", &arca_home)
        .env("ARCA_TEST_PROBE_ROOT", &probe_root)
        .env("ARCA_NO_UPDATE_CHECK", "1");
    command.output().expect("run isolated arca command")
}

#[test]
fn file_export_requires_consent_is_private_and_never_echoes_or_overwrites_secrets() {
    let home = temporary_home();
    let arca_home = home.join(".arca");
    fs::create_dir_all(&arca_home).expect("create isolated Arca home");
    fs::create_dir_all(home.join("probe")).expect("create isolated probe root");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&arca_home, fs::Permissions::from_mode(0o700))
            .expect("secure isolated Arca home");
    }

    let secret = "EXPORT_SECRET_SENTINEL";
    let add = run(
        &home,
        &[
            "--json".as_ref(),
            "mcp".as_ref(),
            "add".as_ref(),
            "secret::stdio".as_ref(),
            "--command".as_ref(),
            "npx".as_ref(),
            "--arg".as_ref(),
            secret.as_ref(),
            "--yes".as_ref(),
        ],
    );
    assert!(
        add.status.success(),
        "seed MCP failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(!String::from_utf8_lossy(&add.stdout).contains(secret));

    let target = home.join("mcp.json");
    let target_arg = target.as_os_str();
    let denied = run(
        &home,
        &[
            "--json".as_ref(),
            "mcp".as_ref(),
            "export".as_ref(),
            "--out".as_ref(),
            target_arg,
        ],
    );
    assert_eq!(denied.status.code(), Some(1));
    assert!(!target.exists());
    assert!(!String::from_utf8_lossy(&denied.stderr).contains(secret));

    let dry_run = run(
        &home,
        &[
            "--json".as_ref(),
            "mcp".as_ref(),
            "export".as_ref(),
            "--out".as_ref(),
            target_arg,
            "--dry-run".as_ref(),
        ],
    );
    assert!(
        dry_run.status.success(),
        "dry-run export failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    assert!(!target.exists());
    assert!(!String::from_utf8_lossy(&dry_run.stdout).contains(secret));

    let exported = run(
        &home,
        &[
            "--json".as_ref(),
            "mcp".as_ref(),
            "export".as_ref(),
            "--out".as_ref(),
            target_arg,
            "--yes".as_ref(),
        ],
    );
    assert!(
        exported.status.success(),
        "confirmed export failed: {}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(fs::read_to_string(&target)
        .expect("read private export")
        .contains(secret));
    assert!(!String::from_utf8_lossy(&exported.stdout).contains(secret));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target)
            .expect("stat private export")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let original = fs::read(&target).expect("read first export bytes");
    let overwrite = run(
        &home,
        &[
            "--json".as_ref(),
            "mcp".as_ref(),
            "export".as_ref(),
            "--out".as_ref(),
            target_arg,
            "--yes".as_ref(),
        ],
    );
    assert_eq!(overwrite.status.code(), Some(1));
    assert_eq!(fs::read(&target).expect("read unchanged export"), original);
    assert!(!String::from_utf8_lossy(&overwrite.stderr).contains(secret));

    fs::remove_dir_all(home).expect("remove isolated export home");
}

#[test]
fn json_errors_do_not_echo_invalid_configuration_or_absolute_paths() {
    let home = temporary_home();
    let arca_home = home.join(".arca");
    fs::create_dir_all(&arca_home).expect("create isolated Arca home");
    fs::create_dir_all(home.join("probe")).expect("create isolated probe root");
    fs::write(
        arca_home.join("settings.json"),
        "{ INVALID_SETTINGS_SECRET_SENTINEL",
    )
    .expect("seed invalid settings");

    let output = run(&home, &["--json".as_ref(), "workspace".as_ref()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("parse JSON error envelope");
    assert_eq!(envelope["ok"], false);
    assert!(envelope["error"]["code"]
        .as_str()
        .is_some_and(|code| !code.is_empty()));
    assert!(!stderr.contains("INVALID_SETTINGS_SECRET_SENTINEL"));
    assert!(!stderr.contains(&home.to_string_lossy().to_string()));

    fs::remove_dir_all(home).expect("remove isolated invalid-settings home");
}
