//! Arca-owned entries in an existing KeePassXC database.

use super::{process, secret_text, CreateFailure, CredentialReference, Store, MAX_KEY_BYTES, UNOWNED_REVISION};
use crate::safe_write::{self, AnchoredPathState, ParentDirectorySnapshot, PathIdentity};
use process::{Diagnostics, Failure};
use sha2::{Digest, Sha256};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

const CLI: &str = "keepassxc-cli";
const DEADLINE: Duration = Duration::from_secs(15);
const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LIST_BYTES: usize = 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 4096;
const KDBX_MAGIC: [u8; 8] = [0x03, 0xd9, 0xa2, 0x9a, 0x67, 0xfb, 0x4b, 0xb5];
const MISSING_CLI: &str = "credential_store_keepassxc_missing: Install KeePassXC or configure the absolute path to keepassxc-cli.";
const MISSING_PASSWORD: &str = "credential_store_keepassxc_password: Set the configured KeePassXC database password environment variable to a nonempty single-line value.";
const INVALID_DATABASE: &str = "credential_store_keepassxc_database: Choose an existing regular KeePassXC database no larger than 64 MiB.";
const INVALID_KEY_FILE: &str = "credential_store_keepassxc_key_file: Choose an existing, readable KeePassXC key file.";
const SCRATCH_FAILED: &str = "credential_store_keepassxc_scratch: Could not prepare the encrypted KeePassXC working copy.";
const RESPONSE: &str = "credential_store_keepassxc_response: KeePassXC CLI returned an unexpected response.";
const AMBIGUOUS: &str = "credential_store_ambiguous: More than one KeePassXC entry claims this credential revision.";
const REVISION_EXISTS: &str = "credential_store_revision_exists: This KeePassXC credential revision already exists.";
const REVISION_MISSING: &str = "credential_store_missing: The active KeePassXC credential revision could not be found.";
const STALE: &str = "credential_store_keepassxc_stale: The KeePassXC database changed during the edit. Retry the save.";
const BACKUP_FAILED: &str = "credential_store_keepassxc_backup: Could not save the encrypted KeePassXC rollback copy.";
const WRITE_DENIED: &str = "credential_store_keepassxc_write: Could not open the KeePassXC database for writing. Check its file permissions.";
const RECOVERY_REQUIRED: &str = "credential_store_keepassxc_recovery: Arca could not verify the KeePassXC file write. The encrypted rollback copy is under Arca backups.";

const DIAGNOSTICS: Diagnostics = Diagnostics {
    start: "credential_store_keepassxc_start: Could not start KeePassXC CLI. Check the executable path and permissions.",
    rejected: "credential_store_keepassxc_failed: KeePassXC CLI rejected the request. Check the database, password, and key file.",
    io: "credential_store_keepassxc_io: Could not safely exchange data with KeePassXC CLI.",
    timeout: "credential_store_keepassxc_timeout: KeePassXC CLI did not finish in time.",
    output_limit: "credential_store_keepassxc_output_limit: KeePassXC CLI exceeded its output limit.",
};

pub(super) struct KeePassXc<'a> {
    pub(super) command: &'a str,
    pub(super) database_path: &'a str,
    pub(super) password_env: &'a str,
    pub(super) key_file: Option<&'a str>,
}

struct Session<'a> {
    executable: PathBuf,
    key_file: Option<PathBuf>,
    password_env: &'a str,
    unlock_input: Zeroizing<Vec<u8>>,
}

struct Scratch {
    directory: PathBuf,
    database: PathBuf,
}

struct Snapshot {
    path: PathBuf,
    parent: ParentDirectorySnapshot,
    bytes: Zeroizing<Vec<u8>>,
    mode: Option<u32>,
    identity: PathIdentity,
}

struct Entry {
    id: String,
    encoding: Encoding,
}

#[derive(Clone, Copy)]
enum Encoding {
    Literal,
    Hex,
}

impl KeePassXc<'_> {
    fn connect(&self) -> Result<Session<'_>, String> {
        let executable = process::executable(self.command, CLI)
            .or_else(|| {
                if cfg!(target_os = "macos") && self.command == CLI {
                    let path = crate::paths::system_probe_path("/Applications/KeePassXC.app/Contents/MacOS/keepassxc-cli");
                    process::executable(path.to_str()?, CLI)
                } else {
                    None
                }
            })
            .and_then(|path| process::scoped_path(&path))
            .ok_or(MISSING_CLI)?;
        let key_file = self.key_file.map(|path| {
            regular_path(Path::new(path)).ok_or(INVALID_KEY_FILE)
        }).transpose()?;
        let password = Zeroizing::new(std::env::var(self.password_env).map_err(|_| MISSING_PASSWORD)?);
        secret_text(password.as_bytes()).map_err(|_| MISSING_PASSWORD)?;
        let mut unlock_input = Zeroizing::new(Vec::with_capacity(password.len() + 4));
        // QTextStream consumes an initial UTF-8 byte order mark.
        if password.starts_with('\u{feff}') {
            unlock_input.extend_from_slice(b"\xef\xbb\xbf");
        }
        unlock_input.extend_from_slice(password.as_bytes());
        unlock_input.push(b'\n');
        Ok(Session { executable, key_file, password_env: self.password_env, unlock_input })
    }

    fn database(&self) -> Result<PathBuf, String> {
        regular_path(Path::new(self.database_path)).ok_or_else(|| INVALID_DATABASE.into())
    }

    fn open(&self) -> Result<(Session<'_>, Scratch), String> {
        let session = self.connect()?;
        let snapshot = Snapshot::read(self.database()?)?;
        Ok((session, Scratch::new(&snapshot.bytes)?))
    }

    fn mutate<T>(
        &self, operation: impl FnOnce(&Session<'_>, &Snapshot, &Scratch) -> Result<T, CreateFailure>,
    ) -> Result<T, CreateFailure> {
        let session = self.connect().map_err(CreateFailure::NotStarted)?;
        let database = self.database().map_err(CreateFailure::NotStarted)?;
        let scope = crate::paths::arca_dir().join("keepassxc-edits").join(path_hash(&database));
        require_isolated_scope(&scope).map_err(CreateFailure::NotStarted)?;
        let _lock = safe_write::acquire_settings_lock(&scope.join("database")).map_err(|_| {
            CreateFailure::NotStarted("credential_store_keepassxc_busy: Arca could not lock the KeePassXC edit scope.".into())
        })?;
        safe_write::with_file_edit_claims(&scope.join("claims"), || {
            let snapshot = Snapshot::read(database).map_err(CreateFailure::NotStarted);
            Ok(snapshot.and_then(|snapshot| {
                let scratch = Scratch::new(&snapshot.bytes).map_err(CreateFailure::NotStarted)?;
                operation(&session, &snapshot, &scratch)
            }))
        }).map_err(|_| CreateFailure::NotStarted(RECOVERY_REQUIRED.into()))?
    }

    fn publish(&self, snapshot: &Snapshot, scratch: &Scratch, revision: &str) -> Result<(), CreateFailure> {
        let candidate = Snapshot::read(scratch.database.clone())
            .map_err(|_| CreateFailure::NotStarted(SCRATCH_FAILED.into()))?;
        if self.database().as_ref() != Ok(&snapshot.path) || !snapshot.is_current() {
            return Err(CreateFailure::NotStarted(STALE.into()));
        }
        let original_hash = hex::encode(Sha256::digest(snapshot.bytes.as_slice()));
        let backup = crate::paths::backups_dir().join("keepassxc").join(path_hash(&snapshot.path))
            .join(format!("{revision}-{original_hash}.kdbx"));
        require_isolated_scope(&backup).map_err(CreateFailure::NotStarted)?;
        safe_write::ensure_private_file(&backup, &snapshot.bytes)
            .map_err(|_| CreateFailure::NotStarted(BACKUP_FAILED.into()))?;
        if self.database().as_ref() != Ok(&snapshot.path) {
            return Err(CreateFailure::NotStarted(STALE.into()));
        }
        // This writer verifies the reviewed parent, inode, mode, and ciphertext
        // immediately before the in-place write and records durable evidence.
        safe_write::write_bytes_if_hash_unchanged_in_parent(
            &snapshot.path,
            &snapshot.parent,
            Some((&original_hash, snapshot.mode, snapshot.identity)),
            &candidate.bytes,
            Some(0o600),
        ).map_err(|error| {
            let stale = error.starts_with("asset_operation_stale:")
                || (error.starts_with("refusing to access ")
                    && error.ends_with("during rollback: hash/mode/identity changed"))
                || (error.starts_with("refusing to restore ")
                    && error.ends_with("file changed before rollback"))
                || (error.starts_with("refusing to modify ")
                    && error.ends_with("hash/mode/identity changed before rollback"));
            if stale {
                CreateFailure::NotStarted(STALE.into())
            } else if error.starts_with(&format!("failed to open {} for in-place edit: ", snapshot.path.display())) {
                CreateFailure::NotStarted(WRITE_DENIED.into())
            } else {
                CreateFailure::Unconfirmed(RECOVERY_REQUIRED.into())
            }
        })
    }
}

impl Store for KeePassXc<'_> {
    fn check(&self) -> Result<(), String> {
        let (session, scratch) = self.open()?;
        session.run(&scratch, "db-info", &[], None, None, MAX_ENTRY_BYTES).map(|_| ())
    }

    fn preflight_create(&self, revision: &str) -> Result<(), String> {
        if self.recover(revision)?.is_some() {
            return Err(REVISION_EXISTS.into());
        }
        Ok(())
    }

    fn create(&self, revision: &str, provider_id: &str, key: &str) -> Result<String, CreateFailure> {
        self.mutate(|session, snapshot, scratch| {
            let entry_path = entry_path(revision);
            if session.entry(scratch, revision, &entry_path).map_err(CreateFailure::NotStarted)?.is_some() {
                return Err(CreateFailure::NotStarted(REVISION_EXISTS.into()));
            }
            let (encoding, encoded_key) = Encoding::encode(key);
            let notes = encoding.notes(revision);
            let password = encoded_key.as_ref().map_or(key.as_bytes(), |encoded| encoded.as_slice());
            session.run(
                scratch, "add", &["--password-prompt", "--username", provider_id, "--notes", &notes],
                Some(&entry_path), Some(password), MAX_ENTRY_BYTES,
            ).map_err(CreateFailure::NotStarted)?;
            let entry = session.entry(scratch, revision, &entry_path).map_err(CreateFailure::NotStarted)?
                .ok_or_else(|| CreateFailure::NotStarted(RESPONSE.into()))?;
            let stored = session.key(scratch, &entry_path, entry.encoding).map_err(CreateFailure::NotStarted)?;
            if stored.as_slice() != key.as_bytes() {
                return Err(CreateFailure::NotStarted(RESPONSE.into()));
            }
            self.publish(snapshot, scratch, revision)?;
            Ok(entry.id)
        })
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        let (session, scratch) = self.open()?;
        Ok(session.entry(&scratch, revision, &entry_path(revision))?.map(|entry| entry.id))
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        let (session, scratch) = self.open()?;
        let path = entry_path(&reference.revision);
        let entry = session.entry(&scratch, &reference.revision, &path)?.ok_or(REVISION_MISSING)?;
        if reference.locator != entry.id {
            return Err(UNOWNED_REVISION.into());
        }
        session.key(&scratch, &path, entry.encoding)
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        self.mutate(|session, snapshot, scratch| {
            let path = entry_path(&reference.revision);
            let Some(entry) = session.entry(scratch, &reference.revision, &path)
                .map_err(CreateFailure::NotStarted)? else { return Ok(()) };
            if reference.locator != entry.id {
                return Err(CreateFailure::NotStarted(UNOWNED_REVISION.into()));
            }
            session.run(scratch, "rm", &[], Some(&path), None, MAX_ENTRY_BYTES)
                .map_err(CreateFailure::NotStarted)?;
            if session.entry(scratch, &reference.revision, &path).map_err(CreateFailure::NotStarted)?.is_some() {
                return Err(CreateFailure::NotStarted(RESPONSE.into()));
            }
            self.publish(snapshot, scratch, &reference.revision)
        }).map_err(|failure| match failure {
            CreateFailure::NotStarted(message) | CreateFailure::Unconfirmed(message) => message,
        })
    }
}

impl Session<'_> {
    fn run(
        &self, scratch: &Scratch, subcommand: &str, options: &[&str], entry_path: Option<&str>,
        entry_password: Option<&[u8]>, limit: usize,
    ) -> Result<Zeroizing<Vec<u8>>, String> {
        if process::scoped_path(&self.executable).as_ref() != Some(&self.executable) {
            return Err(MISSING_CLI.into());
        }
        let mut command = process::command(&self.executable, self.password_env);
        command.current_dir(&scratch.directory)
            .env("HOME", &scratch.directory)
            .env("XDG_CONFIG_HOME", &scratch.directory)
            .env("XDG_DATA_HOME", &scratch.directory)
            .env("XDG_CACHE_HOME", &scratch.directory)
            .env("TMPDIR", &scratch.directory)
            .env("LC_CTYPE", if cfg!(target_os = "macos") { "UTF-8" } else { "C.UTF-8" })
            .args([subcommand, "--quiet"]);
        if let Some(key_file) = &self.key_file {
            if regular_path(key_file).as_ref() != Some(key_file) {
                return Err(INVALID_KEY_FILE.into());
            }
            command.arg("--key-file").arg(key_file);
        }
        command.args(options).arg(&scratch.database);
        if let Some(entry_path) = entry_path {
            command.arg(entry_path);
        }
        let payload = entry_password.map(|password| {
            let mut input = Zeroizing::new(Vec::with_capacity(self.unlock_input.len() + password.len() + 1));
            input.extend_from_slice(&self.unlock_input);
            input.extend_from_slice(password);
            input.push(b'\n');
            input
        });
        let input = payload.as_ref().map_or(self.unlock_input.as_slice(), |input| input.as_slice());
        process::run(command, Some(input), limit, DEADLINE, &DIAGNOSTICS)
            .map_err(|failure: Failure| failure.message().into())
    }

    fn entry(&self, scratch: &Scratch, revision: &str, path: &str) -> Result<Option<Entry>, String> {
        let title = path.strip_prefix('/').ok_or(RESPONSE)?;
        let listing = self.run(scratch, "ls", &[], None, None, MAX_LIST_BYTES)?;
        let names = listing.strip_suffix(b"\n").ok_or(RESPONSE)?;
        let matches = names.split(|byte| *byte == b'\n').filter(|name| *name == title.as_bytes()).count();
        match matches {
            0 => return Ok(None),
            1 => {},
            _ => return Err(AMBIGUOUS.into()),
        }
        // `show` uses the first matching path. The root listing above rejects
        // duplicate names before `show` or `rm` can select an ambiguous entry.
        let attributes = self.run(
            scratch, "show", &["-a", "Title", "-a", "Notes", "-a", "Uuid"], Some(path), None, MAX_ENTRY_BYTES,
        )?;
        let mut lines = std::str::from_utf8(attributes.strip_suffix(b"\n").ok_or(RESPONSE)?)
            .map_err(|_| RESPONSE)?.split('\n');
        let (Some(name), Some(notes), Some(id), None) = (lines.next(), lines.next(), lines.next(), lines.next()) else {
            return Err(UNOWNED_REVISION.into());
        };
        if name != title {
            return Err(UNOWNED_REVISION.into());
        }
        let encoding = if notes == Encoding::Literal.notes(revision) {
            Encoding::Literal
        } else if notes == Encoding::Hex.notes(revision) {
            Encoding::Hex
        } else {
            return Err(UNOWNED_REVISION.into());
        };
        let id = Uuid::parse_str(id).ok().filter(|id| !id.is_nil()).ok_or(RESPONSE)?;
        Ok(Some(Entry { id: id.hyphenated().to_string(), encoding }))
    }

    fn key(&self, scratch: &Scratch, path: &str, encoding: Encoding) -> Result<Zeroizing<Vec<u8>>, String> {
        let stream = self.run(scratch, "show", &["-a", "Password"], Some(path), None, MAX_KEY_BYTES * 2 + 1)?;
        let raw = stream.strip_suffix(b"\n").ok_or(RESPONSE)?;
        let key = match encoding {
            Encoding::Literal => Zeroizing::new(raw.to_vec()),
            Encoding::Hex => {
                if raw.len() % 2 != 0 || raw.len() > MAX_KEY_BYTES * 2 {
                    return Err(RESPONSE.into());
                }
                let mut decoded = Zeroizing::new(vec![0; raw.len() / 2]);
                hex::decode_to_slice(raw, &mut *decoded).map_err(|_| RESPONSE)?;
                decoded
            }
        };
        secret_text(&key).map_err(|_| RESPONSE)?;
        Ok(key)
    }
}

impl Encoding {
    fn encode(key: &str) -> (Self, Option<Zeroizing<Vec<u8>>>) {
        // KeePassXC `show` resolves braces as placeholders. Encoding also
        // preserves nonprintable characters that are unsafe in its XML data.
        if key.bytes().all(|byte| (b' '..=b'~').contains(&byte) && !matches!(byte, b'{' | b'}')) {
            return (Self::Literal, None);
        }
        let mut encoded = Zeroizing::new(vec![0; key.len() * 2]);
        const HEX: &[u8] = b"0123456789abcdef";
        for (byte, digits) in key.bytes().zip(encoded.chunks_exact_mut(2)) {
            digits[0] = HEX[usize::from(byte >> 4)];
            digits[1] = HEX[usize::from(byte & 0xf)];
        }
        (Self::Hex, Some(encoded))
    }

    fn notes(self, revision: &str) -> String {
        match self {
            Self::Literal => format!("arca-provider-credential-v1:{revision}"),
            Self::Hex => format!("arca-provider-credential-v1:{revision}:hex"),
        }
    }
}

impl Scratch {
    fn new(ciphertext: &[u8]) -> Result<Self, String> {
        let parent = scratch_root().ok_or(SCRATCH_FAILED)?;
        let directory = parent.join(format!(".arca-keepassxc-{}", Uuid::new_v4()));
        let mut builder = DirBuilder::new();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            builder.mode(0o700);
            options.mode(0o600);
        }
        builder.create(&directory).map_err(|_| SCRATCH_FAILED)?;
        let scratch = Self { database: directory.join("working.kdbx"), directory };
        options.open(&scratch.database).and_then(|mut file| file.write_all(ciphertext))
            .map_err(|_| SCRATCH_FAILED)?;
        Ok(scratch)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

impl Snapshot {
    fn read(path: PathBuf) -> Result<Self, String> {
        let parent = safe_write::capture_parent_directory(&path).map_err(|_| INVALID_DATABASE)?;
        let AnchoredPathState::File { bytes, mode, identity } =
            safe_write::read_path_state_anchored_limited(&path, &parent, MAX_DATABASE_BYTES)
                .map_err(|_| INVALID_DATABASE)?
        else { return Err(INVALID_DATABASE.into()) };
        if !bytes.starts_with(&KDBX_MAGIC) || !identity.is_exact() {
            return Err(INVALID_DATABASE.into());
        }
        Ok(Self { path, parent, bytes, mode, identity })
    }

    fn is_current(&self) -> bool {
        matches!(safe_write::read_path_state_anchored_limited(&self.path, &self.parent, MAX_DATABASE_BYTES),
            Ok(AnchoredPathState::File { bytes, mode, identity })
                if bytes == self.bytes && mode == self.mode && identity == self.identity)
    }
}

fn entry_path(revision: &str) -> String {
    format!("/Arca Provider {revision}")
}

fn path_hash(path: &Path) -> String {
    hex::encode(Sha256::digest(path.as_os_str().as_encoded_bytes()))
}

fn regular_path(path: &Path) -> Option<PathBuf> {
    let canonical = process::scoped_path(path)?;
    let metadata = fs::symlink_metadata(path).ok()?;
    let resolved_metadata = fs::symlink_metadata(&canonical).ok()?;
    if !metadata.is_file() || !resolved_metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != resolved_metadata.dev() || metadata.ino() != resolved_metadata.ino()
            || resolved_metadata.nlink() != 1
        {
            return None;
        }
    }
    Some(canonical)
}

fn scratch_root() -> Option<PathBuf> {
    #[cfg(any(test, debug_assertions))]
    if let Some(root) = crate::brand::env_os("ARCA_TEST_PROBE_ROOT") {
        return process::scoped_path(Path::new(&root));
    }
    process::scoped_path(&std::env::temp_dir())
}

fn require_isolated_scope(path: &Path) -> Result<(), String> {
    #[cfg(any(test, debug_assertions))]
    if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some()
        && path.ancestors().find(|parent| fs::symlink_metadata(parent).is_ok())
            .and_then(process::scoped_path).is_none()
    {
        return Err(SCRATCH_FAILED.into());
    }
    let _ = path;
    Ok(())
}
