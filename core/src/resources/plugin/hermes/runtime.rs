//! Passive binding to one reviewed editable installation. A returned version is
//! its registered source declaration; future Python import resolution is unknown.
//! No interpreter, other .pth, site hook, environment/config or plugin body runs.
use super::super::*;
use super::super::electron_package::Guard;

const COMMIT: &str = "ff6a86cb529a372198b4b80d5e022e32a4a3f2cc";
const DIST: &str = "hermes_agent-0.7.0.dist-info";
const FINDER: &str = "__editable___hermes_agent_0_7_0_finder";

pub(super) struct BoundSource {
    pub root: PathBuf,
    pub site_packages: PathBuf,
    pub source_version: String,
    pub packaging_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Template { text: String, bytes: u64, sha256: String }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    schema: String,
    agent_id: String,
    command: String,
    source_commit: String,
    source_version: String,
    packaging_version: String,
    site_packages: String,
    metadata: Guard,
    entry_points: Guard,
    direct_url_path: String,
    pth_path: String,
    finder_path: String,
    finder_module: String,
    finder_placeholder_prefix: String,
    entry: Template,
    pth: Template,
    finder: Template,
    direct_url: Template,
    guards: Vec<Guard>,
}

fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| serde_json::from_str(include_str!("../../../../../data/native-plugin-hermes-runtime.json"))
        .expect("audited Hermes editable source contract"))
}

// Only the observed simple absolute-path spelling is supported. In particular,
// values cannot inject Python quoting, URL escaping or normalization placeholders.
fn safe_absolute(path: &Path) -> Result<&str, ()> {
    let value = path.to_str().ok_or(())?;
    if !path.is_absolute() || value.len() > 4096
        || !value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
        || path.components().any(|c| !matches!(c, Component::RootDir | Component::Normal(_))) {
        return Err(());
    }
    let normalized: PathBuf = path.components().collect();
    if normalized.as_os_str() != path.as_os_str() { return Err(()); }
    Ok(value)
}

struct Snapshot { path: PathBuf, metadata: fs::Metadata }

fn same_identity(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    let (Ok(before_modified), Ok(after_modified)) = (before.modified(), after.modified()) else { return false; };
    if before.file_type() != after.file_type() || before.len() != after.len()
        || before_modified != after_modified { return false; }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino()
            || before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() { return false; }
    }
    true
}

fn recheck(snapshots: &[Snapshot]) -> Result<(), ()> {
    for snapshot in snapshots {
        let parent = snapshot.path.parent().ok_or(())?;
        checked_root(parent)?;
        if fs::canonicalize(parent).map_err(|_| ())? != parent { return Err(()); }
        if !same_identity(&snapshot.metadata, &fs::symlink_metadata(&snapshot.path).map_err(|_| ())?) { return Err(()); }
    }
    Ok(())
}

fn selected_script(selected: &Path, budget: &mut ScanBudget, snapshots: &mut Vec<Snapshot>) -> Result<PathBuf, ()> {
    safe_absolute(selected)?;
    let mut path = selected.to_path_buf();
    let mut seen = BTreeSet::new();
    for _ in 0..8 {
        budget.visit()?;
        if !seen.insert(path.clone()) { return Err(()); }
        let parent = path.parent().ok_or(())?;
        checked_root(parent)?;
        if fs::canonicalize(parent).map_err(|_| ())? != parent { return Err(()); }
        let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
        let is_link = metadata.file_type().is_symlink();
        let is_file = metadata.file_type().is_file();
        snapshots.push(Snapshot { path: path.clone(), metadata });
        if is_file { return Ok(path); }
        if !is_link { return Err(()); }
        // The reviewed selection has an absolute final-component symlink.
        // Ancestor links and unfamiliar relative wrappers stay unsupported.
        path = fs::read_link(&path).map_err(|_| ())?;
        safe_absolute(&path)?;
    }
    Err(())
}

fn read_bounded(path: &Path, limit: u64, budget: &mut ScanBudget, snapshots: &mut Vec<Snapshot>) -> Result<Vec<u8>, ()> {
    checked_root(path)?;
    budget.visit()?;
    let before = fs::symlink_metadata(path).map_err(|_| ())?;
    if !before.file_type().is_file() || before.len() > limit
        || before.len() > MAX_READ_BYTES.saturating_sub(budget.bytes) as u64 { return Err(()); }
    let mut file = open_confined_file(path)?;
    if !same_identity(&before, &file.metadata().map_err(|_| ())?) { return Err(()); }
    let mut bytes = Vec::new();
    let read = (&mut file).take(limit + 1).read_to_end(&mut bytes);
    budget.bytes += bytes.len();
    read.map_err(|_| ())?;
    if bytes.len() as u64 != before.len() || !same_identity(&before, &file.metadata().map_err(|_| ())?)
        || !same_identity(&before, &fs::symlink_metadata(path).map_err(|_| ())?) { return Err(()); }
    snapshots.push(Snapshot { path: path.to_path_buf(), metadata: before });
    Ok(bytes)
}

fn template_matches(bytes: &[u8], template: &Template, bindings: &[(&str, &str)]) -> Result<(), ()> {
    let mut expected = template.text.clone();
    for (token, value) in bindings { expected = expected.replace(token, value); }
    if bytes != expected.as_bytes() { return Err(()); }
    let mut normalized = std::str::from_utf8(bytes).map_err(|_| ())?.to_owned();
    for (token, value) in bindings { normalized = normalized.replace(value, token); }
    if normalized.len() as u64 != template.bytes
        || hex::encode(Sha256::digest(normalized.as_bytes())) != template.sha256 { return Err(()); }
    Ok(())
}

fn verify_member(root: &Path, guard: &Guard, budget: &mut ScanBudget, snapshots: &mut Vec<Snapshot>) -> Result<(), ()> {
    let path = relative(root, &guard.path)?;
    checked_root(&path)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
    verify_artifact(&path, guard.bytes, &guard.sha256, budget)?;
    snapshots.push(Snapshot { path, metadata });
    Ok(())
}

fn reviewed_contract(c: &Contract) -> Result<(), ()> {
    let paths: BTreeSet<_> = c.guards.iter().map(|g| g.path.as_str()).collect();
    if c.schema != "hermes-passive-source-binding/1" || c.agent_id != "hermes" || c.command != "hermes"
        || c.source_commit != COMMIT || c.source_version != "0.8.0" || c.packaging_version != "0.7.0"
        || c.site_packages != "lib/python3.11/site-packages" || c.finder_module != FINDER
        || c.finder_placeholder_prefix != "__editable__.hermes_agent-0.7.0.finder"
        || c.metadata.path != format!("{DIST}/METADATA") || c.entry_points.path != format!("{DIST}/entry_points.txt")
        || c.direct_url_path != format!("{DIST}/direct_url.json") || c.pth_path != "__editable__.hermes_agent-0.7.0.pth"
        || c.finder_path != format!("{FINDER}.py") || c.guards.len() > 64 || paths.len() != c.guards.len()
        || !["hermes_cli/__init__.py", "pyproject.toml", "hermes_cli/main.py"].iter().all(|p| paths.contains(p)) { return Err(()); }
    Ok(())
}

fn bind_with_contract(selected: &Path, c: &Contract) -> Result<BoundSource, ()> {
    reviewed_contract(c)?;
    // Binding owns a finite budget; it never consumes a parent observer's budget.
    let mut budget = ScanBudget::default();
    let mut snapshots = Vec::new();
    let script = selected_script(selected, &mut budget, &mut snapshots)?;
    let venv = script.parent().and_then(Path::parent).ok_or(())?;
    let root = venv.parent().ok_or(())?;
    if script != root.join("venv/bin/hermes") { return Err(()); }
    let root_text = safe_absolute(root)?;
    let venv_text = safe_absolute(venv)?;
    let site = relative(venv, &c.site_packages)?;
    let dist = site.join(DIST);
    for path in [root, venv, site.as_path(), dist.as_path()] {
        checked_root(path)?;
        let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
        if !metadata.file_type().is_dir() { return Err(()); }
        snapshots.push(Snapshot { path: path.to_path_buf(), metadata });
    }
    template_matches(&read_bounded(&script, 16_384, &mut budget, &mut snapshots)?, &c.entry, &[("${VENV_ROOT}", venv_text)])?;
    verify_member(&site, &c.metadata, &mut budget, &mut snapshots)?;
    verify_member(&site, &c.entry_points, &mut budget, &mut snapshots)?;
    template_matches(&read_bounded(&relative(&site, &c.direct_url_path)?, 16_384, &mut budget, &mut snapshots)?,
        &c.direct_url, &[("${REGISTERED_SOURCE_ROOT}", root_text)])?;
    template_matches(&read_bounded(&relative(&site, &c.pth_path)?, 4096, &mut budget, &mut snapshots)?,
        &c.pth, &[("${FINDER_MODULE}", &c.finder_module)])?;
    template_matches(&read_bounded(&relative(&site, &c.finder_path)?, 131_072, &mut budget, &mut snapshots)?,
        &c.finder, &[("${REGISTERED_SOURCE_ROOT}", root_text), ("${FINDER_PLACEHOLDER_PREFIX}", &c.finder_placeholder_prefix)])?;
    for guard in &c.guards { verify_member(root, guard, &mut budget, &mut snapshots)?; }
    recheck(&snapshots)?;
    Ok(BoundSource { root: root.to_path_buf(), site_packages: site, source_version: c.source_version.clone(), packaging_version: c.packaging_version.clone() })
}

pub(super) fn bind(selected: &Path) -> Result<BoundSource, ()> { bind_with_contract(selected, contract()) }

pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    // Installation-source metadata label only. Unknown never invokes a target.
    bind(selected).ok().map(|bound| bound.source_version)
}

fn watch_paths_with_contract(selected: &Path, c: &Contract) -> Vec<PathBuf> {
    let mut paths = BTreeSet::from([selected.to_path_buf()]);
    if let Ok(script) = selected_script(selected, &mut ScanBudget::default(), &mut Vec::new()) { paths.insert(script); }
    if let Ok(bound) = bind_with_contract(selected, c) {
        paths.insert(bound.root.join("venv/bin/hermes"));
        for path in [&c.metadata.path, &c.entry_points.path, &c.direct_url_path, &c.pth_path, &c.finder_path] {
            if let Ok(path) = relative(&bound.site_packages, path) { paths.insert(path); }
        }
        for guard in &c.guards { if let Ok(path) = relative(&bound.root, &guard.path) { paths.insert(path); } }
    }
    paths.into_iter().collect()
}

pub(super) fn watch_paths(selected: &Path) -> Vec<PathBuf> { watch_paths_with_contract(selected, contract()) }

#[cfg(test)]
mod tests;
