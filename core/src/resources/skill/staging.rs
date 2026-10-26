use super::{io_error, SkillError, SkillsPaths};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) const STAGING_METADATA_FILE: &str = "metadata.json";
const MAX_STAGING_METADATA_BYTES: u64 = 4096;

#[derive(Serialize)]
struct StagingMetadata<'a> {
    operation_id: &'a str,
    created_at: String,
}

#[cfg(unix)]
mod platform {
    use super::super::anchored::{
        normalize_stat_u32, normalize_stat_u64, AnchoredFileKind, AnchoredIdentity, AnchoredRoot,
    };
    use super::*;
    use rustix::fs::{
        fchmod, fstat, mkdirat, openat, renameat, statat, symlinkat, unlinkat, AtFlags, Dir,
        FileType, Mode, OFlags, CWD,
    };
    use std::ffi::{CStr, CString};
    use std::fs::{self, File};
    use std::io::{ErrorKind, Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};

    pub(crate) struct StagingRoot {
        directory: File,
        path: PathBuf,
    }

    pub(crate) struct StagingOperation {
        staging_directory: File,
        directory: File,
        operation_id: String,
        path: PathBuf,
    }

    pub(crate) struct StagingDirectory {
        directory: File,
        path: PathBuf,
    }

    impl StagingRoot {
        pub(crate) fn open_or_create(paths: &SkillsPaths) -> Result<Self, SkillError> {
            let arca = open_or_create_arca(paths.arca_dir())?;
            let staging = open_or_create_private_directory(
                &arca,
                &c("staging")?,
                &paths.arca_dir().join("staging"),
            )?;
            let skills = open_or_create_private_directory(
                &staging,
                &c("skills")?,
                &paths.staging_skills_dir(),
            )?;
            Ok(Self {
                directory: skills,
                path: paths.staging_skills_dir(),
            })
        }

        pub(crate) fn open(paths: &SkillsPaths) -> Result<Self, SkillError> {
            let arca = open_arca(paths.arca_dir())?;
            let staging =
                open_private_directory(&arca, &c("staging")?, &paths.arca_dir().join("staging"))?;
            let skills =
                open_private_directory(&staging, &c("skills")?, &paths.staging_skills_dir())?;
            Ok(Self {
                directory: skills,
                path: paths.staging_skills_dir(),
            })
        }

        pub(crate) fn create_operation(
            &self,
            operation_id: &str,
        ) -> Result<StagingOperation, SkillError> {
            super::super::transaction::validate_operation_id(operation_id)?;
            let name = c(operation_id)?;
            mkdirat(&self.directory, &name, Mode::from_raw_mode(0o700))
                .map_err(|error| io_error(&self.path, error.into()))?;
            let directory =
                open_private_directory(&self.directory, &name, &self.path.join(operation_id))?;
            let operation = StagingOperation {
                staging_directory: self
                    .directory
                    .try_clone()
                    .map_err(|error| io_error(&self.path, error))?,
                directory,
                operation_id: operation_id.to_owned(),
                path: self.path.join(operation_id),
            };
            let metadata = StagingMetadata {
                operation_id,
                created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            };
            let bytes = serde_json::to_vec(&metadata).map_err(|_| SkillError::InvalidSource {
                message: "Skills operation metadata could not be encoded safely".into(),
            })?;
            if let Err(error) = operation.write_private_atomic(
                STAGING_METADATA_FILE,
                &bytes,
                MAX_STAGING_METADATA_BYTES,
            ) {
                let _ = operation.remove();
                return Err(error);
            }
            Ok(operation)
        }

        pub(crate) fn open_operation(
            &self,
            operation_id: &str,
        ) -> Result<StagingOperation, SkillError> {
            super::super::transaction::validate_operation_id(operation_id)?;
            let name = c(operation_id)?;
            let path = self.path.join(operation_id);
            let directory = open_private_directory(&self.directory, &name, &path)?;
            Ok(StagingOperation {
                staging_directory: self
                    .directory
                    .try_clone()
                    .map_err(|error| io_error(&self.path, error))?,
                directory,
                operation_id: operation_id.to_owned(),
                path,
            })
        }

        pub(crate) fn remove_operation_if_exists(
            &self,
            operation_id: &str,
        ) -> Result<bool, SkillError> {
            super::super::transaction::validate_operation_id(operation_id)?;
            let name = c(operation_id)?;
            match statat(&self.directory, &name, AtFlags::SYMLINK_NOFOLLOW) {
                Err(error) if error == rustix::io::Errno::NOENT => Ok(false),
                Err(error) => Err(io_error(&self.path, error.into())),
                Ok(stat) if FileType::from_raw_mode(stat.st_mode as _) != FileType::Directory => {
                    Err(unremovable_operation())
                }
                Ok(_) => {
                    let operation = self.open_operation(operation_id).map_err(|error| {
                        if matches!(
                            error,
                            SkillError::UnsafePath { .. } | SkillError::Conflict { .. }
                        ) {
                            unremovable_operation()
                        } else {
                            error
                        }
                    })?;
                    operation.remove()?;
                    Ok(true)
                }
            }
        }
    }

    impl StagingOperation {
        pub(crate) fn path(&self) -> &Path {
            &self.path
        }

        pub(crate) fn root_directory(&self) -> Result<StagingDirectory, SkillError> {
            Ok(StagingDirectory {
                directory: self
                    .directory
                    .try_clone()
                    .map_err(|error| io_error(&self.path, error))?,
                path: self.path.clone(),
            })
        }

        pub(crate) fn create_private_directory(
            &self,
            name: &str,
        ) -> Result<StagingDirectory, SkillError> {
            let name_c = safe_component(name)?;
            mkdirat(&self.directory, &name_c, Mode::from_raw_mode(0o700))
                .map_err(|error| io_error(&self.path, error.into()))?;
            let path = self.path.join(name);
            let directory = open_private_directory(&self.directory, &name_c, &path)?;
            self.sync()?;
            Ok(StagingDirectory { directory, path })
        }

        pub(crate) fn remove_private_directory(&self, name: &str) -> Result<(), SkillError> {
            let name_c = safe_component(name)?;
            let path = self.path.join(name);
            let directory = open_private_directory(&self.directory, &name_c, &path)?;
            remove_directory_contents(&directory, &path)?;
            unlinkat(&self.directory, &name_c, AtFlags::REMOVEDIR)
                .map_err(|error| io_error(&path, error.into()))?;
            self.sync()
        }

        pub(crate) fn write_private_atomic(
            &self,
            name: &str,
            bytes: &[u8],
            maximum: u64,
        ) -> Result<(), SkillError> {
            if bytes.len() as u64 > maximum {
                return Err(SkillError::LimitExceeded {
                    limit: "operation_metadata".into(),
                    actual: bytes.len() as u64,
                    allowed: maximum,
                });
            }
            let destination = safe_component(name)?;
            validate_replaceable_entry(&self.directory, &destination, &self.path.join(name))?;
            let temporary_name = format!(".{name}.tmp");
            let temporary = safe_component(&temporary_name)?;
            remove_private_temporary(
                &self.directory,
                &temporary,
                &self.path.join(&temporary_name),
            )?;
            let descriptor = openat(
                &self.directory,
                &temporary,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|error| io_error(&self.path, error.into()))?;
            let mut file = File::from(descriptor);
            let result = (|| {
                fchmod(&file, Mode::from_raw_mode(0o600))
                    .map_err(|error| io_error(&self.path, error.into()))?;
                file.write_all(bytes)
                    .map_err(|error| io_error(&self.path, error))?;
                file.sync_all()
                    .map_err(|error| io_error(&self.path, error))?;
                renameat(&self.directory, &temporary, &self.directory, &destination)
                    .map_err(|error| io_error(&self.path, error.into()))?;
                self.sync()
            })();
            drop(file);
            if result.is_err() {
                let _ = unlinkat(&self.directory, &temporary, AtFlags::empty());
            }
            result
        }

        pub(crate) fn read_private(&self, name: &str, maximum: u64) -> Result<Vec<u8>, SkillError> {
            let name_c = safe_component(name)?;
            let path = self.path.join(name);
            let before = statat(&self.directory, &name_c, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|_| unavailable_private_file())?;
            validate_private_regular(&before, maximum)?;
            let descriptor = openat(
                &self.directory,
                &name_c,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| unavailable_private_file())?;
            let mut file = File::from(descriptor);
            let after = fstat(&file).map_err(|_| unavailable_private_file())?;
            if before.st_dev != after.st_dev || before.st_ino != after.st_ino {
                return Err(SkillError::Conflict {
                    message: "private Skills operation metadata changed while opening".into(),
                    path: String::new(),
                });
            }
            validate_private_regular(&after, maximum)?;
            let mut bytes = Vec::with_capacity(u64::try_from(after.st_size).unwrap_or(0) as usize);
            Read::by_ref(&mut file)
                .take(maximum + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| io_error(&path, error))?;
            if bytes.len() as u64 > maximum {
                return Err(SkillError::LimitExceeded {
                    limit: "operation_metadata".into(),
                    actual: bytes.len() as u64,
                    allowed: maximum,
                });
            }
            Ok(bytes)
        }

        pub(crate) fn list_private_directory(&self, name: &str) -> Result<Vec<String>, SkillError> {
            let name_c = safe_component(name)?;
            let path = self.path.join(name);
            let directory = open_private_directory(&self.directory, &name_c, &path)?;
            let mut names = Vec::new();
            let entries =
                Dir::read_from(&directory).map_err(|error| io_error(&path, error.into()))?;
            for entry in entries {
                let entry = entry.map_err(|error| io_error(&path, error.into()))?;
                let raw = entry.file_name().to_bytes();
                if raw == b"." || raw == b".." {
                    continue;
                }
                let value = std::str::from_utf8(raw)
                    .map_err(|_| SkillError::InvalidSource {
                        message: "a staged Skill candidate name is not UTF-8".into(),
                    })?
                    .to_owned();
                safe_component(&value)?;
                names.push(value);
            }
            names.sort();
            Ok(names)
        }

        pub(crate) fn remove(&self) -> Result<(), SkillError> {
            let name = c(&self.operation_id)?;
            remove_directory_contents(&self.directory, &self.path)?;
            unlinkat(&self.staging_directory, &name, AtFlags::REMOVEDIR)
                .map_err(|error| io_error(&self.path, error.into()))?;
            self.staging_directory
                .sync_all()
                .map_err(|error| io_error(&self.path, error))
        }

        fn sync(&self) -> Result<(), SkillError> {
            self.directory
                .sync_all()
                .map_err(|error| io_error(&self.path, error))
        }
    }

    impl StagingDirectory {
        pub(crate) fn path(&self) -> &Path {
            &self.path
        }

        pub(crate) fn anchored_root(&self) -> Result<AnchoredRoot, SkillError> {
            let stat =
                fstat(&self.directory).map_err(|error| io_error(&self.path, error.into()))?;
            let expected = anchored_identity(&stat);
            AnchoredRoot::from_open_directory(
                self.directory
                    .try_clone()
                    .map_err(|error| io_error(&self.path, error))?,
                self.path.clone(),
                &expected,
            )
        }

        pub(crate) fn open_directory(
            &self,
            relative: &str,
        ) -> Result<StagingDirectory, SkillError> {
            let components = relative_components(relative, true)?;
            let mut directory = self
                .directory
                .try_clone()
                .map_err(|error| io_error(&self.path, error))?;
            let mut path = self.path.clone();
            for component in components {
                path.push(component.to_string_lossy().as_ref());
                directory = open_private_directory(&directory, &component, &path)?;
            }
            Ok(StagingDirectory { directory, path })
        }

        pub(crate) fn create_directory(
            &self,
            relative: &str,
        ) -> Result<StagingDirectory, SkillError> {
            let (parent, name, path) = self.open_relative_parent(relative)?;
            mkdirat(&parent, &name, Mode::from_raw_mode(0o700))
                .map_err(|error| io_error(&path, error.into()))?;
            let directory = open_private_directory(&parent, &name, &path)?;
            Ok(StagingDirectory { directory, path })
        }

        pub(crate) fn ensure_directory(
            &self,
            relative: &str,
        ) -> Result<StagingDirectory, SkillError> {
            let components = relative_components(relative, true)?;
            let mut directory = self
                .directory
                .try_clone()
                .map_err(|error| io_error(&self.path, error))?;
            let mut path = self.path.clone();
            for component in components {
                path.push(component.to_string_lossy().as_ref());
                match statat(&directory, &component, AtFlags::SYMLINK_NOFOLLOW) {
                    Err(error) if error == rustix::io::Errno::NOENT => {
                        mkdirat(&directory, &component, Mode::from_raw_mode(0o700))
                            .map_err(|error| io_error(&path, error.into()))?;
                    }
                    Err(error) => return Err(io_error(&path, error.into())),
                    Ok(_) => {}
                }
                directory = open_private_directory(&directory, &component, &path)?;
            }
            Ok(StagingDirectory { directory, path })
        }

        pub(crate) fn create_file(
            &self,
            relative: &str,
            executable_bits: u32,
        ) -> Result<File, SkillError> {
            let (parent, name, path) = self.open_relative_parent(relative)?;
            let mode = 0o600 | (executable_bits & 0o111);
            let descriptor = openat(
                &parent,
                &name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(mode as _),
            )
            .map_err(|error| io_error(&path, error.into()))?;
            let file = File::from(descriptor);
            fchmod(&file, Mode::from_raw_mode(mode as _))
                .map_err(|error| io_error(&path, error.into()))?;
            Ok(file)
        }

        pub(crate) fn open_file(&self, relative: &str) -> Result<File, SkillError> {
            let (parent, name, path) = self.open_relative_parent(relative)?;
            let before = statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|error| io_error(&path, error.into()))?;
            validate_private_regular(&before, u64::MAX)?;
            let descriptor = openat(
                &parent,
                &name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| io_error(&path, error.into()))?;
            let file = File::from(descriptor);
            let after = fstat(&file).map_err(|error| io_error(&path, error.into()))?;
            validate_private_regular(&after, u64::MAX)?;
            if before.st_dev != after.st_dev || before.st_ino != after.st_ino {
                return Err(SkillError::Conflict {
                    message: "a private Skills staging file changed while opening".into(),
                    path: String::new(),
                });
            }
            Ok(file)
        }

        pub(crate) fn remove_file(&self, relative: &str) -> Result<(), SkillError> {
            let (parent, name, path) = self.open_relative_parent(relative)?;
            let stat = statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|error| io_error(&path, error.into()))?;
            validate_private_regular(&stat, u64::MAX)?;
            unlinkat(&parent, &name, AtFlags::empty())
                .map_err(|error| io_error(&path, error.into()))
        }

        pub(crate) fn create_symlink(
            &self,
            relative: &str,
            target: &str,
        ) -> Result<(), SkillError> {
            let (parent, name, _path) = self.open_relative_parent(relative)?;
            let target = CString::new(target).map_err(|_| unsafe_staging())?;
            symlinkat(&target, &parent, &name).map_err(|_| SkillError::InvalidSource {
                message: "archive symlink entries collide after normalization".into(),
            })
        }

        pub(crate) fn is_empty(&self) -> Result<bool, SkillError> {
            let entries = Dir::read_from(&self.directory)
                .map_err(|error| io_error(&self.path, error.into()))?;
            for entry in entries {
                let entry = entry.map_err(|error| io_error(&self.path, error.into()))?;
                if !matches!(entry.file_name().to_bytes(), b"." | b"..") {
                    return Ok(false);
                }
            }
            Ok(true)
        }

        fn open_relative_parent(
            &self,
            relative: &str,
        ) -> Result<(File, CString, PathBuf), SkillError> {
            let mut components = relative_components(relative, false)?;
            let name = components
                .pop()
                .expect("non-empty relative paths have a name");
            let mut parent = self
                .directory
                .try_clone()
                .map_err(|error| io_error(&self.path, error))?;
            let mut path = self.path.clone();
            for component in components {
                path.push(component.to_string_lossy().as_ref());
                parent = open_private_directory(&parent, &component, &path)?;
            }
            path.push(name.to_string_lossy().as_ref());
            Ok((parent, name, path))
        }
    }

    fn open_or_create_arca(path: &Path) -> Result<File, SkillError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(unsafe_staging()),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder
                    .mode(0o700)
                    .create(path)
                    .map_err(|error| io_error(path, error))?;
            }
            Err(error) => return Err(io_error(path, error)),
        }
        open_arca(path)
    }

    fn open_arca(path: &Path) -> Result<File, SkillError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
        if !metadata.file_type().is_dir() {
            return Err(unsafe_staging());
        }
        let descriptor = openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| unsafe_staging())?;
        let file = File::from(descriptor);
        let opened = file.metadata().map_err(|error| io_error(path, error))?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(SkillError::Conflict {
                message: "the Arca root changed while opening Skills staging".into(),
                path: String::new(),
            });
        }
        Ok(file)
    }

    fn open_or_create_private_directory(
        parent: &File,
        name: &CStr,
        path: &Path,
    ) -> Result<File, SkillError> {
        match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => {}
            Err(error) if error == rustix::io::Errno::NOENT => {
                mkdirat(parent, name, Mode::from_raw_mode(0o700))
                    .map_err(|error| io_error(path, error.into()))?;
                parent.sync_all().map_err(|error| io_error(path, error))?;
            }
            Err(error) => return Err(io_error(path, error.into())),
        }
        let file = open_directory(parent, name, path, false)?;
        fchmod(&file, Mode::from_raw_mode(0o700)).map_err(|error| io_error(path, error.into()))?;
        let secured = fstat(&file).map_err(|error| io_error(path, error.into()))?;
        validate_private_directory(&secured)?;
        Ok(file)
    }

    fn open_private_directory(parent: &File, name: &CStr, path: &Path) -> Result<File, SkillError> {
        open_directory(parent, name, path, true)
    }

    fn open_directory(
        parent: &File,
        name: &CStr,
        path: &Path,
        require_private: bool,
    ) -> Result<File, SkillError> {
        let before = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| io_error(path, error.into()))?;
        validate_directory_type(&before)?;
        if require_private {
            validate_private_directory(&before)?;
        }
        let descriptor = openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| unsafe_staging())?;
        let file = File::from(descriptor);
        let after = fstat(&file).map_err(|error| io_error(path, error.into()))?;
        validate_directory_type(&after)?;
        if require_private {
            validate_private_directory(&after)?;
        }
        if before.st_dev != after.st_dev || before.st_ino != after.st_ino {
            return Err(SkillError::Conflict {
                message: "a private Skills staging directory changed while opening".into(),
                path: String::new(),
            });
        }
        Ok(file)
    }

    fn validate_directory_type(stat: &rustix::fs::Stat) -> Result<(), SkillError> {
        if FileType::from_raw_mode(stat.st_mode as _) != FileType::Directory {
            return Err(unsafe_staging());
        }
        Ok(())
    }

    fn validate_private_directory(stat: &rustix::fs::Stat) -> Result<(), SkillError> {
        validate_directory_type(stat)?;
        if stat.st_mode & 0o077 != 0 {
            return Err(unsafe_staging());
        }
        Ok(())
    }

    fn validate_private_regular(stat: &rustix::fs::Stat, maximum: u64) -> Result<(), SkillError> {
        let size = u64::try_from(stat.st_size).unwrap_or(u64::MAX);
        if FileType::from_raw_mode(stat.st_mode as _) != FileType::RegularFile
            || stat.st_mode & 0o077 != 0
            || stat.st_nlink != 1
            || size > maximum
        {
            return Err(SkillError::InvalidSource {
                message: "private Skills operation metadata is not a bounded private file".into(),
            });
        }
        Ok(())
    }

    fn validate_replaceable_entry(
        parent: &File,
        name: &CStr,
        path: &Path,
    ) -> Result<(), SkillError> {
        match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
            Err(error) => Err(io_error(path, error.into())),
            Ok(stat) => validate_private_regular(&stat, u64::MAX),
        }
    }

    fn remove_private_temporary(parent: &File, name: &CStr, path: &Path) -> Result<(), SkillError> {
        match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
            Err(error) => Err(io_error(path, error.into())),
            Ok(stat) => {
                validate_private_regular(&stat, u64::MAX)?;
                unlinkat(parent, name, AtFlags::empty())
                    .map_err(|error| io_error(path, error.into()))
            }
        }
    }

    fn remove_directory_contents(directory: &File, path: &Path) -> Result<(), SkillError> {
        let entries = Dir::read_from(directory).map_err(|error| io_error(path, error.into()))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| io_error(path, error.into()))?;
            let name = entry.file_name();
            if name.to_bytes() != b"." && name.to_bytes() != b".." {
                names.push(name.to_owned());
            }
        }
        for name in names {
            let child_path = path.join(name.to_string_lossy().as_ref());
            let stat = statat(directory, &name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|error| io_error(&child_path, error.into()))?;
            if FileType::from_raw_mode(stat.st_mode as _) == FileType::Directory {
                let child = open_private_directory(directory, &name, &child_path)?;
                remove_directory_contents(&child, &child_path)?;
                unlinkat(directory, &name, AtFlags::REMOVEDIR)
                    .map_err(|error| io_error(&child_path, error.into()))?;
            } else {
                unlinkat(directory, &name, AtFlags::empty())
                    .map_err(|error| io_error(&child_path, error.into()))?;
            }
        }
        Ok(())
    }

    fn safe_component(value: &str) -> Result<CString, SkillError> {
        if value.is_empty()
            || matches!(value, "." | "..")
            || value.contains(['/', '\0'])
            || value.len() > 255
        {
            return Err(unsafe_staging());
        }
        c(value)
    }

    fn relative_components(value: &str, allow_empty: bool) -> Result<Vec<CString>, SkillError> {
        if value.is_empty() {
            return if allow_empty {
                Ok(Vec::new())
            } else {
                Err(unsafe_staging())
            };
        }
        if value.starts_with('/') || value.contains(['\\', '\0']) {
            return Err(unsafe_staging());
        }
        value.split('/').map(safe_component).collect()
    }

    fn anchored_identity(stat: &rustix::fs::Stat) -> AnchoredIdentity {
        let kind = match FileType::from_raw_mode(stat.st_mode as _) {
            FileType::RegularFile => AnchoredFileKind::Regular,
            FileType::Directory => AnchoredFileKind::Directory,
            FileType::Symlink => AnchoredFileKind::Symlink,
            _ => AnchoredFileKind::Other,
        };
        AnchoredIdentity {
            kind,
            device: normalize_stat_u64(stat.st_dev),
            inode: stat.st_ino,
            links: normalize_stat_u64(stat.st_nlink),
            size: u64::try_from(stat.st_size).unwrap_or(u64::MAX),
            mode: normalize_stat_u32(stat.st_mode),
        }
    }

    fn c(value: &str) -> Result<CString, SkillError> {
        CString::new(value).map_err(|_| unsafe_staging())
    }

    fn unsafe_staging() -> SkillError {
        SkillError::UnsafePath {
            message: "private Skills staging traversal is unsafe".into(),
            path: String::new(),
        }
    }

    fn unremovable_operation() -> SkillError {
        SkillError::RecoveryRequired {
            message: "the named Skills staging operation is not a removable directory".into(),
        }
    }

    fn unavailable_private_file() -> SkillError {
        SkillError::InvalidSource {
            message: "private Skills operation metadata is unavailable".into(),
        }
    }
}

#[cfg(unix)]
pub(crate) use platform::{StagingDirectory, StagingOperation, StagingRoot};

#[cfg(not(unix))]
pub(crate) struct StagingRoot;

#[cfg(not(unix))]
pub(crate) struct StagingOperation;

#[cfg(not(unix))]
pub(crate) struct StagingDirectory;

#[cfg(not(unix))]
impl StagingRoot {
    pub(crate) fn open_or_create(_paths: &SkillsPaths) -> Result<Self, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn open(_paths: &SkillsPaths) -> Result<Self, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn create_operation(
        &self,
        _operation_id: &str,
    ) -> Result<StagingOperation, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn open_operation(
        &self,
        _operation_id: &str,
    ) -> Result<StagingOperation, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn remove_operation_if_exists(
        &self,
        _operation_id: &str,
    ) -> Result<bool, SkillError> {
        Err(unsupported())
    }
}

#[cfg(not(unix))]
impl StagingOperation {
    pub(crate) fn path(&self) -> &Path {
        Path::new("")
    }

    pub(crate) fn root_directory(&self) -> Result<StagingDirectory, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn create_private_directory(
        &self,
        _name: &str,
    ) -> Result<StagingDirectory, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn remove_private_directory(&self, _name: &str) -> Result<(), SkillError> {
        Err(unsupported())
    }

    pub(crate) fn write_private_atomic(
        &self,
        _name: &str,
        _bytes: &[u8],
        _maximum: u64,
    ) -> Result<(), SkillError> {
        Err(unsupported())
    }

    pub(crate) fn read_private(&self, _name: &str, _maximum: u64) -> Result<Vec<u8>, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn list_private_directory(&self, _name: &str) -> Result<Vec<String>, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn remove(&self) -> Result<(), SkillError> {
        Err(unsupported())
    }
}

#[cfg(not(unix))]
impl StagingDirectory {
    pub(crate) fn path(&self) -> &Path {
        Path::new("")
    }

    pub(crate) fn anchored_root(&self) -> Result<super::anchored::AnchoredRoot, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn open_directory(&self, _relative: &str) -> Result<Self, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn create_directory(&self, _relative: &str) -> Result<Self, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn ensure_directory(&self, _relative: &str) -> Result<Self, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn create_file(
        &self,
        _relative: &str,
        _executable_bits: u32,
    ) -> Result<std::fs::File, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn open_file(&self, _relative: &str) -> Result<std::fs::File, SkillError> {
        Err(unsupported())
    }

    pub(crate) fn remove_file(&self, _relative: &str) -> Result<(), SkillError> {
        Err(unsupported())
    }

    pub(crate) fn create_symlink(&self, _relative: &str, _target: &str) -> Result<(), SkillError> {
        Err(unsupported())
    }

    pub(crate) fn is_empty(&self) -> Result<bool, SkillError> {
        Err(unsupported())
    }
}

#[cfg(not(unix))]
fn unsupported() -> SkillError {
    SkillError::InvalidSource {
        message: "secure Skills staging is unavailable on this platform".into(),
    }
}
