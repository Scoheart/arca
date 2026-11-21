use serde::Serialize;
use std::path::Path;

#[cfg(target_os = "macos")]
const OPEN_AFTER_EXIT: &str = r#"
pid=$1
shift
attempt=0
while /bin/kill -0 "$pid" 2>/dev/null; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 600 ]; then
        exit 1
    fi
    /bin/sleep 0.1
done
exec /usr/bin/open "$@"
"#;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEnvironment {
    pub can_self_update: bool,
    pub reason: Option<&'static str>,
}

#[tauri::command]
pub fn update_environment() -> UpdateEnvironment {
    let Ok(executable) = std::env::current_exe() else {
        return UpdateEnvironment {
            can_self_update: true,
            reason: None,
        };
    };

    environment_for_path(&executable, filesystem_is_read_only(&executable))
}

#[tauri::command]
pub fn restart_after_update(app: tauri::AppHandle) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        use tauri::Manager;

        let env = app.env();
        let executable = tauri::process::current_binary(&env).map_err(|error| error.to_string())?;
        let bundle = executable
            .ancestors()
            .nth(3)
            .filter(|bundle| {
                bundle.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
                    && executable.parent() == Some(bundle.join("Contents/MacOS").as_path())
            })
            .ok_or("Cannot locate the installed Arca application bundle.")?;

        // Let LaunchServices open the updated bundle normally once the old process exits.
        // A forced new instance can be registered on a second Dock tile during the handoff.
        Command::new("/bin/sh")
            .args(["-c", OPEN_AFTER_EXIT, "arca-update-restart"])
            .arg(std::process::id().to_string())
            .arg(bundle)
            .arg("--args")
            .args(env.args_os.iter().skip(1))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|error| error.to_string())?;
        app.exit(0);
        Ok(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(false)
    }
}

fn environment_for_path(path: &Path, read_only: bool) -> UpdateEnvironment {
    let translocated = path
        .components()
        .any(|component| component.as_os_str() == "AppTranslocation");

    let reason = if translocated {
        Some("app-translocation")
    } else if read_only && path.starts_with("/Volumes") {
        Some("disk-image")
    } else if read_only {
        Some("read-only-volume")
    } else {
        None
    };

    UpdateEnvironment {
        can_self_update: reason.is_none(),
        reason,
    }
}

#[cfg(target_os = "macos")]
fn filesystem_is_read_only(path: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    let result = unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return false;
    }

    let stats = unsafe { stats.assume_init() };
    stats.f_flags & libc::MNT_RDONLY as u32 != 0
}

#[cfg(not(target_os = "macos"))]
fn filesystem_is_read_only(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_apps_running_from_a_read_only_disk_image() {
        let env = environment_for_path(Path::new("/Volumes/Arca/Arca.app/Contents/MacOS/arca"), true);
        assert!(!env.can_self_update);
        assert_eq!(env.reason, Some("disk-image"));
    }

    #[test]
    fn blocks_macos_app_translocation() {
        let env = environment_for_path(
            Path::new("/private/var/folders/xy/AppTranslocation/ABC/d/Arca.app/Contents/MacOS/arca"),
            true,
        );
        assert!(!env.can_self_update);
        assert_eq!(env.reason, Some("app-translocation"));
    }

    #[test]
    fn allows_an_installed_application() {
        let env =
            environment_for_path(Path::new("/Applications/Arca.app/Contents/MacOS/arca"), false);
        assert!(env.can_self_update);
        assert_eq!(env.reason, None);
    }

    #[test]
    fn does_not_block_a_writable_external_volume() {
        let env = environment_for_path(
            Path::new("/Volumes/External/Applications/Arca.app/Contents/MacOS/arca"),
            false,
        );
        assert!(env.can_self_update);
        assert_eq!(env.reason, None);
    }
}
