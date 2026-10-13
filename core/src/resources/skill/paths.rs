use super::{anchored::AnchoredRoot, SkillError};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillsPaths {
    arca_dir: PathBuf,
    user_home: PathBuf,
}

impl SkillsPaths {
    /// Resolve the Skills roots without creating or chmod-ing anything.
    ///
    /// Read-only inventory callers must use this constructor. Lifecycle
    /// mutation paths use [`Self::from_env`] so their private roots exist.
    pub(crate) fn resolve_from_env() -> Result<Self, SkillError> {
        let arca_dir = crate::paths::arca_dir();
        if !arca_dir.is_absolute() {
            return Err(SkillError::InvalidSource {
                message: "ARCA_HOME must resolve to an absolute path".into(),
            });
        }
        let user_home = dirs::home_dir()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| SkillError::InvalidSource {
                message: "the user home directory is unavailable".into(),
            })?;
        Ok(Self { arca_dir, user_home })
    }

    pub fn from_env() -> Result<Self, SkillError> {
        let paths = Self::resolve_from_env()?;
        paths.ensure_arca_root()?;
        paths.ensure_transaction_roots()?;
        Ok(paths)
    }

    pub(crate) fn ensure_arca_root(&self) -> Result<(), SkillError> {
        create_private_root(&self.arca_dir)
    }

    pub(crate) fn ensure_transaction_roots(&self) -> Result<(), SkillError> {
        for root in [
            self.skills_dir(),
            self.staging_skills_dir(),
            self.backups_skills_dir(),
            self.journals_skills_dir(),
        ] {
            create_private_root(&root)?;
        }
        Ok(())
    }

    pub fn arca_dir(&self) -> &Path {
        &self.arca_dir
    }

    pub fn user_home(&self) -> &Path {
        &self.user_home
    }

    pub fn skills_dir(&self) -> PathBuf {
        crate::paths::skill_contents_dir()
    }

    pub fn staging_skills_dir(&self) -> PathBuf {
        self.arca_dir.join("staging/skills")
    }

    pub fn backups_skills_dir(&self) -> PathBuf {
        self.arca_dir.join("backups/skills")
    }

    pub fn journals_skills_dir(&self) -> PathBuf {
        self.arca_dir.join("journals/skills")
    }

    /// Compatibility alias retained for the transaction fixture contract.
    /// Both names resolve to the single journal root.
    pub fn journals_dir(&self) -> PathBuf {
        self.journals_skills_dir()
    }

    pub fn skills_lock(&self) -> PathBuf {
        self.arca_dir.join("skills.lock")
    }

    pub fn central_skill(&self, name: &str) -> PathBuf {
        self.skills_dir().join(name)
    }

    pub fn expand_user(&self, value: &str) -> Option<PathBuf> {
        if value == "~" {
            return Some(self.user_home.clone());
        }
        if let Some(relative) = value.strip_prefix("~/") {
            let relative = Path::new(relative);
            if relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
            {
                return Some(crate::brand::translate_legacy_path(self.user_home.join(relative)));
            }
            return None;
        }
        if value.starts_with('~') {
            return None;
        }
        Some(crate::brand::translate_legacy_path(PathBuf::from(value)))
    }
}

#[cfg(unix)]
fn create_private_root(path: &Path) -> Result<(), SkillError> {
    AnchoredRoot::open_or_create_private_absolute(path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_root(_path: &Path) -> Result<(), SkillError> {
    Err(SkillError::InvalidSource {
        message: "secure Skill transaction roots are unavailable on this platform".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[test]
    fn old_skill_root_is_ignored_and_no_alias_is_created() {
        let th = TestHome::new("skill-canonical-root");
        let old_root = th.home.join(".arca/skills");
        std::fs::create_dir_all(&old_root).unwrap();
        std::fs::write(old_root.join("sentinel"), "untouched").unwrap();

        let paths = SkillsPaths::resolve_from_env().unwrap();
        assert_eq!(paths.skills_dir(), crate::paths::skill_contents_dir());
        assert!(!paths.skills_dir().exists());
        SkillsPaths::from_env().unwrap();
        assert!(paths.skills_dir().is_dir());
        assert!(std::fs::symlink_metadata(&old_root).unwrap().is_dir());
        assert_eq!(std::fs::read_to_string(old_root.join("sentinel")).unwrap(), "untouched");

        std::fs::remove_file(old_root.join("sentinel")).unwrap();
        std::fs::remove_dir(&old_root).unwrap();
        crate::settings::migrate_asset_layout_if_needed().unwrap();
        assert!(std::fs::symlink_metadata(&old_root).is_err());
    }

    #[test]
    fn user_expansion_rejects_paths_that_escape_the_user_home() {
        let _th = TestHome::new("skill-user-path-expansion");
        let paths = SkillsPaths::from_env().unwrap();
        assert!(paths.expand_user("~//outside").is_none());
        assert!(paths.expand_user("~/../outside").is_none());
        assert_eq!(
            paths.expand_user("~/.agent/skills").unwrap(),
            paths.user_home().join(".agent/skills")
        );
    }
}
