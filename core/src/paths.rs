use std::path::PathBuf;

/// System installation probes use the same sandbox root as TestHome. Production
/// builds always inspect the actual system paths.
pub(crate) fn system_probe_path(path: &str) -> PathBuf {
    #[cfg(any(test, debug_assertions))]
    if let Some(root) = crate::brand::env_os("ARCA_TEST_PROBE_ROOT") {
        return PathBuf::from(root).join(path.trim_start_matches('/'));
    }
    PathBuf::from(path)
}

/// `~/.arca` —— 与 CLI 共用的数据目录。
///
/// `ARCA_HOME` 环境变量可整体重定向该目录（值即数据目录本身，类似
/// `CARGO_HOME`）。除了给用户/CI 挪数据目录，它也是测试隔离的关键防线：
/// 即使测试对 `HOME` 的操纵发生竞态，只要 `ARCA_HOME` 指向临时目录，
/// 真实 `~/.arca` 就不会被写脏（2026-07-08 曾因 HOME 竞态污染真实缓存）。
pub fn arca_dir() -> PathBuf {
    crate::brand::data_directory()
}

pub fn backups_dir() -> PathBuf {
    arca_dir().join("backups")
}

/// `~/.arca/assets` —— MCP、Model 与 Skill 中央资产的统一物理根目录。
pub fn assets_dir() -> PathBuf {
    arca_dir().join("assets")
}

pub fn mcp_assets_dir() -> PathBuf {
    assets_dir().join("mcps")
}

pub fn mcp_sources_dir() -> PathBuf {
    mcp_assets_dir().join("sources")
}

pub fn mcp_icons_dir() -> PathBuf {
    assets_dir().join("mcp-icons")
}

pub fn model_assets_dir() -> PathBuf {
    assets_dir().join("models")
}

pub fn skill_assets_dir() -> PathBuf {
    assets_dir().join("skills")
}

pub fn skill_contents_dir() -> PathBuf {
    skill_assets_dir().join("items")
}

pub fn mcp_catalog_file() -> PathBuf {
    mcp_assets_dir().join("catalog.json")
}

pub fn model_catalog_file() -> PathBuf {
    model_assets_dir().join("catalog.json")
}

pub fn skill_catalog_file() -> PathBuf {
    skill_assets_dir().join("catalog.json")
}

pub(crate) fn legacy_sources_dir() -> PathBuf {
    arca_dir().join("sources")
}

/// MCP source payload root. Before the startup migration completes, readers
/// transparently resolve the legacy `~/.arca/sources` location so recovery and
/// upgrades never observe an empty catalog merely because the binary changed.
pub fn sources_dir() -> PathBuf {
    let current = mcp_sources_dir();
    let legacy = legacy_sources_dir();
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}

/// `~/.arca/sources/remote` —— 订阅(远程 URL)抓取后的缓存副本
pub fn remote_sources_dir() -> PathBuf {
    sources_dir().join("remote")
}

/// `~/.arca/sources/local` —— 本地添加的配置文件副本
pub fn local_sources_dir() -> PathBuf {
    sources_dir().join("local")
}

/// `~/.arca/settings.json` —— 所有用户数据(registry/agents/disabled/state…)的单一文件
pub fn settings_file() -> PathBuf {
    arca_dir().join("settings.json")
}

/// `~/.arca/registry` —— legacy 自定义条目目录(仅迁移时读取)
pub fn registry_dir() -> PathBuf {
    arca_dir().join("registry")
}

pub fn user_agents_file() -> PathBuf {
    arca_dir().join("agents.json")
}

fn migrate_directory(legacy: &std::path::Path, current: &std::path::Path) -> std::io::Result<bool> {
    match (legacy.exists(), current.exists()) {
        (false, _) => Ok(false),
        (true, false) => {
            if let Some(parent) = current.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(legacy, current)?;
            Ok(true)
        }
        (true, true) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "refusing to merge legacy asset directory {} into existing {}",
                legacy.display(),
                current.display()
            ),
        )),
    }
}

/// Move the legacy MCP source directory under `~/.arca/assets`.
/// The settings lock must be held by the caller.
pub(crate) fn migrate_legacy_mcp_sources() -> std::io::Result<bool> {
    migrate_directory(&legacy_sources_dir(), &mcp_sources_dir())
}

/// Filename-safe local timestamp (`%Y-%m-%dT%H-%M-%S`) used for backup artifacts.
pub fn backup_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[test]
    fn arca_dir_ends_with_dot_arca() {
        let _home = TestHome::new("paths-arca-dir");
        assert!(arca_dir().ends_with(".arca"));
    }

    #[test]
    fn backups_under_arca_dir() {
        let _home = TestHome::new("paths-backups");
        assert!(backups_dir().starts_with(arca_dir()));
    }
}
