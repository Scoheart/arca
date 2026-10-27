//! Test-only environment isolation for `~/.arca` and agent-config paths.
//!
//! Historically every integration test hand-rolled `set_var("HOME", tmp)` +
//! `remove_var("HOME")`. That pattern had two real failure modes:
//!
//! 1. **`remove_var` is not a restore** — with `HOME` unset, `dirs::home_dir()`
//!    falls back to the passwd entry (the *real* home), so any core call after
//!    teardown silently touches real user data.
//! 2. **Parallel tests race** — env vars are process-global; two tests in one
//!    binary interleave `set_var`/`remove_var` and one of them ends up writing
//!    through the other's (or the real) HOME. This corrupted the real
//!    `~/.arca/sources/remote/*` cache on 2026-07-08.
//!
//! [`TestHome`] fixes both: a process-wide mutex serializes every user of the
//! env, `HOME`, `ARCA_HOME`, command lookup, and install-probe roots point into
//! a fresh temp dir, and `Drop` restores (not removes) the previous values.
//! Multiple tests per file are safe with this guard.
//!
//! Not intended for production use; hidden from docs.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static SEQ: AtomicU32 = AtomicU32::new(0);

/// RAII guard: fake `$HOME` + `$ARCA_HOME` in a temp dir, serialized process-wide.
pub struct TestHome {
    /// The fake home directory (agent configs live under here, `.arca` inside it).
    pub home: PathBuf,
    saved_home: Option<OsString>,
    saved_arca_home: Option<OsString>,
    saved_path: Option<OsString>,
    saved_test_probe_root: Option<OsString>,
    saved_native_plugin_homes: Vec<(&'static str, Option<OsString>)>,
    readiness: Option<crate::application::gate::TestReadyGuard>,
    _guard: MutexGuard<'static, ()>,
}

impl TestHome {
    /// Acquire the env lock, create a unique temp home, and point both
    /// `HOME` and `ARCA_HOME` (→ `<home>/.arca`) at it.
    pub fn new(tag: &str) -> Self {
        let guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let short_tag: String = tag
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .take(8)
            .collect();
        let home = std::env::temp_dir().join(format!(
            "mx-{}-{:x}-{:x}",
            short_tag,
            std::process::id(),
            seq
        ));
        std::fs::create_dir_all(&home).expect("create test home dir");
        let home = std::fs::canonicalize(&home).expect("canonicalize test home dir");
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).expect("create test bin dir");
        let saved_home = std::env::var_os("HOME");
        let saved_arca_home = std::env::var_os("ARCA_HOME");
        let saved_path = std::env::var_os("PATH");
        let saved_test_probe_root = std::env::var_os("ARCA_TEST_PROBE_ROOT");
        std::env::set_var("HOME", &home);
        std::env::set_var("ARCA_HOME", home.join(".arca"));
        std::env::set_var("PATH", &bin);
        std::env::set_var("ARCA_TEST_PROBE_ROOT", &home);
        // Native observers honor host-specific homes. Isolate those overrides
        // too, so inherited CLI variables cannot escape the temporary HOME.
        let saved_native_plugin_homes = [
            ("CODEX_HOME", home.join(".codex")),
            ("CODEX_ELECTRON_BUNDLED_PLUGINS_RESOURCES_PATH", home.join("codex-app-resources")),
            ("CLAUDE_CONFIG_DIR", home.join(".claude")),
            ("GEMINI_CLI_HOME", home.clone()),
            ("XDG_CONFIG_HOME", home.join(".config")),
            ("XDG_STATE_HOME", home.join(".local/state")),
            ("OPENCODE_CONFIG", home.join(".config/opencode/opencode.json")),
            ("OPENCODE_CONFIG_DIR", home.join(".config/opencode")),
            ("OPENCODE_PLUGIN_META_FILE", home.join(".local/state/opencode/plugin-meta.json")),
            ("OPENCODE_CONFIG_CONTENT", PathBuf::new()),
        ].into_iter().map(|(key, path)| {
            let previous = std::env::var_os(key);
            std::env::set_var(key, path);
            (key, previous)
        }).collect();
        let readiness = Some(crate::application::gate::TestReadyGuard::enter());
        TestHome {
            home,
            saved_home,
            saved_arca_home,
            saved_path,
            saved_test_probe_root,
            saved_native_plugin_homes,
            readiness,
            _guard: guard,
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // Restore lifecycle state while the same process-wide guard still owns
        // HOME/ARCA_HOME, so another isolated test cannot observe a mismatched
        // workspace and backend readiness pair.
        if let Some(readiness) = self.readiness.take() {
            readiness.restore();
        }
        // Restore, never remove-and-fall-back-to-real-home.
        match &self.saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match &self.saved_arca_home {
            Some(v) => std::env::set_var("ARCA_HOME", v),
            None => std::env::remove_var("ARCA_HOME"),
        }
        match &self.saved_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        match &self.saved_test_probe_root {
            Some(v) => std::env::set_var("ARCA_TEST_PROBE_ROOT", v),
            None => std::env::remove_var("ARCA_TEST_PROBE_ROOT"),
        }
        for (key, previous) in &self.saved_native_plugin_homes {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let _ = std::fs::remove_dir_all(&self.home);
        // _guard releases after this body — env is consistent before the next
        // holder proceeds.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn redirects_and_restores_env() {
        // Capture the original values from inside TestHome, after it owns the
        // process-wide lock. Reading before acquisition races another test's
        // temporary HOME even though both mutations themselves are serialized.
        let th = TestHome::new("selftest");
        let before_home = th.saved_home.clone();
        let before_arca = th.saved_arca_home.clone();
        let before_path = th.saved_path.clone();
        let before_probe_root = th.saved_test_probe_root.clone();
        assert_eq!(std::env::var_os("HOME"), Some(th.home.clone().into()));
        assert_eq!(
            std::env::var_os("PATH"),
            Some(th.home.join("bin").into()),
            "PATH redirected into the disposable home"
        );
        assert_eq!(
            std::env::var_os("ARCA_TEST_PROBE_ROOT"),
            Some(th.home.clone().into()),
            "probe roots redirected into the disposable home"
        );
        assert_eq!(
            crate::paths::arca_dir(),
            th.home.join(".arca"),
            "arca_dir must resolve into the fake home"
        );
        drop(th);
        assert_eq!(std::env::var_os("HOME"), before_home, "HOME restored");
        assert_eq!(
            std::env::var_os("ARCA_HOME"),
            before_arca,
            "ARCA_HOME restored"
        );
        assert_eq!(std::env::var_os("PATH"), before_path, "PATH restored");
        assert_eq!(
            std::env::var_os("ARCA_TEST_PROBE_ROOT"),
            before_probe_root,
            "probe root restored"
        );
    }

    #[test]
    fn restores_every_redirected_variable_during_panic_unwind() {
        type SavedEnv = (
            Option<OsString>,
            Option<OsString>,
            Option<OsString>,
            Option<OsString>,
        );
        let saved = Arc::new(Mutex::new(None::<SavedEnv>));
        let captured = Arc::clone(&saved);
        let result = std::panic::catch_unwind(move || {
            let th = TestHome::new("panic-unwind");
            *captured.lock().unwrap() = Some((
                th.saved_home.clone(),
                th.saved_arca_home.clone(),
                th.saved_path.clone(),
                th.saved_test_probe_root.clone(),
            ));
            panic!("exercise TestHome panic cleanup");
        });
        assert!(result.is_err());

        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (home, arca_home, path, probe_root) = saved.lock().unwrap().clone().unwrap();
        assert_eq!(std::env::var_os("HOME"), home);
        assert_eq!(std::env::var_os("ARCA_HOME"), arca_home);
        assert_eq!(std::env::var_os("PATH"), path);
        assert_eq!(std::env::var_os("ARCA_TEST_PROBE_ROOT"), probe_root);
    }
}
