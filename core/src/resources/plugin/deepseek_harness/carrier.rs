//! Read the published CLI installation or Desktop carrier without loading DSH.
use super::*;
use crate::resources::plugin::electron_package::{app_info, AsarArchive};

const CLI: &str = "@deepseek-ai/dsh";
const BASE: &str = "@deepseek-ai/dsh-base";
pub(super) const WEB: &str = "@deepseek-ai/dsh-web-app";
pub(super) const OFFICE: &str = "@deepseek-ai/dsh-skill-office";
const DESKTOP_HOST: &str = "@deepseek-ai/dsh-desktop-host";
const REPO: &str = "git+https://github.com/deepseek-ai/deepseek-harness.git";
const ASAR_MODULES: &str = "dsh/node_modules";
const CLI_BUNDLES: &[(&str, &str)] = &[
    (BASE, "packages/bundle/base"),
    (WEB, "packages/bundle/web-app"),
    ("@deepseek-ai/dsh-headless", "packages/bundle/headless"),
    ("@deepseek-ai/dsh-acp-app", "packages/bundle/acp-app"),
    ("@deepseek-ai/dsh-sdk-app", "packages/bundle/sdk-app"),
    ("@deepseek-ai/dsh-sdk-minimal", "packages/bundle/sdk-minimal"),
];

pub(super) struct Package {
    pub version: String,
    pub source: String,
    pub revision: String,
    pub skills: Vec<PluginComponent>,
}

fn verify(metadata: &Value, name: &str, directory: &str, version: &str) -> Result<(), ()> {
    if metadata.get("name").and_then(Value::as_str) != Some(name)
        || metadata.get("version").and_then(Value::as_str) != Some(version)
        || metadata.pointer("/repository/url").and_then(Value::as_str) != Some(REPO)
        || metadata.pointer("/repository/directory").and_then(Value::as_str) != Some(directory)
    {
        return Err(());
    }
    Ok(())
}

fn version(metadata: &Value) -> Result<&str, ()> {
    metadata.get("version").and_then(Value::as_str)
        .filter(|version| PRESET_VERSIONS.contains(version)).ok_or(())
}

fn depends_on(metadata: &Value, name: &str, version: &str) -> bool {
    metadata.get("dependencies").and_then(|deps| deps.get(name)).and_then(Value::as_str) == Some(version)
}

fn cli_entry() -> Result<Option<(PathBuf, PathBuf)>, ()> {
    let Some(command) = crate::application::agent_launch::find_command("dsh") else { return Ok(None); };
    let path = fs::canonicalize(&command).map_err(|_| ())?;
    if !path.ends_with("node_modules/@deepseek-ai/dsh/lib/bin.js") { return Ok(None); }
    Ok(Some((command, path)))
}

fn cli_package(
    root: &Path, pool: &Path, name: &str, budget: &mut ScanBudget,
) -> Result<(PathBuf, PathBuf, BoundText, Value), ()> {
    // Resolve from the selected installation's anchor, stopping at its
    // outer module directory. Never pick a later dsh command or global slot.
    for directory in root.ancestors().take_while(|path| path.starts_with(pool)) {
        let modules = if directory.file_name().is_some_and(|name| name == "node_modules") {
            directory.to_path_buf()
        } else {
            directory.join("node_modules")
        };
        if let Some(package) = installed_within(&modules, pool, name, budget)? { return Ok(package); }
    }
    Err(())
}

fn cli(selected: &[&str], budget: &mut ScanBudget) -> Result<BTreeMap<String, Package>, ()> {
    if !CLI_BUNDLES.iter().any(|(name, _)| selected.contains(name)) {
        return Ok(BTreeMap::new());
    }
    let Some((command, entry)) = cli_entry()? else { return Ok(BTreeMap::new()); };
    let root = entry.parent().and_then(Path::parent).ok_or(())?;
    let pool = root.ancestors().filter(|dir| dir.file_name().is_some_and(|n| n == "node_modules")).last().ok_or(())?;
    let manifest = BoundText::read(&root.join("package.json"), budget)?.ok_or(())?;
    let metadata = manifest.json()?;
    let version = version(&metadata)?;
    verify(&metadata, CLI, "apps/cli", version)?;
    if metadata.pointer("/bin/dsh").and_then(Value::as_str) != Some("lib/bin.js") {
        return Err(());
    }
    let mut packages = BTreeMap::new();
    let mut anchors = Vec::new();
    for &(name, directory) in CLI_BUNDLES.iter().filter(|(name, _)| selected.contains(name)) {
        if !depends_on(&metadata, name, version) { return Err(()); }
        let (slot, package_root, file, bundle) = cli_package(root, pool, name, budget)?;
        verify(&bundle, name, directory, version)?;
        if name == WEB {
            if !depends_on(&bundle, PRESET, version) { return Err(()); }
            let (slot, root, file, preset) = cli_package(&package_root, pool, PRESET, budget)?;
            verify(&preset, PRESET, "packages/preset/agent-preset", version)?;
            check_preset(&preset)?;
            packages.insert(PRESET.into(), Package {
                version: version.into(), source: collapse_home(&file.path),
                revision: format!("manifest-sha256:{}", file.digest),
                skills: skill_files(&root.join("skills"), budget)?,
            });
            anchors.push((slot, root, file));
        }
        packages.insert(name.into(), Package {
            version: version.into(), source: collapse_home(&file.path),
            revision: format!("manifest-sha256:{}", file.digest), skills: Vec::new(),
        });
        anchors.push((slot, package_root, file));
    }
    manifest.recheck(budget)?;
    for (slot, root, file) in anchors {
        if fs::canonicalize(slot).map_err(|_| ())? != root { return Err(()); }
        file.recheck(budget)?;
    }
    if cli_entry()? != Some((command, entry)) { return Err(()); }
    Ok(packages)
}

fn desktop_paths() -> [PathBuf; 2] {
    [
        crate::paths::system_probe_path("/Applications/DeepSeek Harness.app"),
        expand_tilde("~/Applications/DeepSeek Harness.app"),
    ]
}

fn desktop(use_web: bool, budget: &mut ScanBudget) -> Result<BTreeMap<String, Package>, ()> {
    let Some(app) = desktop_paths().into_iter().find(|path| {
        !fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }) else { return Ok(BTreeMap::new()); };
    let info = app_info(&app, "com.deepseek.dsh", "DeepSeek Harness", budget)?;
    let version = info.get("CFBundleShortVersionString").and_then(plist::Value::as_string)
        .filter(|version| PRESET_VERSIONS.contains(version)).ok_or(())?;
    let integrity = info.get("ElectronAsarIntegrity").and_then(plist::Value::as_dictionary)
        .and_then(|entries| entries.get("Resources/app.asar")).and_then(plist::Value::as_dictionary).ok_or(())?;
    let hash = integrity.get("hash").and_then(plist::Value::as_string).ok_or(())?;
    if integrity.get("algorithm").and_then(plist::Value::as_string) != Some("SHA256") { return Err(()); }
    let path = app.join("Contents/Resources/app.asar");
    let mut archive = AsarArchive::open(&path, Some(hash), budget)?;
    let source = collapse_home(&path);
    let revision = format!("asar-header-sha256:{hash}");
    let mut packages = BTreeMap::new();
    let mut add = |name: &str, directory, archive: &mut AsarArchive, budget: &mut ScanBudget| {
        let member = format!("{ASAR_MODULES}/{name}/package.json");
        let metadata = archive.json(&member, budget)?;
        verify(&metadata, name, directory, version)?;
        packages.insert(name.into(), Package {
            version: version.into(), source: format!("{source}!{member}"),
            revision: revision.clone(), skills: Vec::new(),
        });
        Ok::<Value, ()>(metadata)
    };
    if use_web {
        add(BASE, "packages/bundle/base", &mut archive, budget)?;
        let web = add(WEB, "packages/bundle/web-app", &mut archive, budget)?;
        if !depends_on(&web, PRESET, version) { return Err(()); }
        check_preset(&add(PRESET, "packages/preset/agent-preset", &mut archive, budget)?)?;
    }
    let host = archive.json(&format!("{ASAR_MODULES}/{DESKTOP_HOST}/package.json"), budget)?;
    if host.get("name").and_then(Value::as_str) != Some(DESKTOP_HOST)
        || host.get("version").and_then(Value::as_str) != Some(version)
        || !depends_on(&host, OFFICE, version)
    {
        return Err(());
    }
    add(OFFICE, "packages/skill/skill-office", &mut archive, budget)?;
    if let Some(preset) = packages.get_mut(PRESET) {
        let skills = format!("{ASAR_MODULES}/{PRESET}/skills");
        for name in archive.children(&skills)?.keys() {
            budget.visit()?;
            let Some(name) = metadata_name(name).filter(|name| !name.contains(['/', '\\']) && !matches!(name.as_str(), "." | "..")) else { return Err(()); };
            let member = format!("{skills}/{name}/SKILL.md");
            if archive.has_file(&member) {
                preset.skills.push(PluginComponent {
                    kind: PluginComponentKind::Skill, name,
                    source_path: format!("{source}!{member}"), native_id: None,
                });
                if preset.skills.len() > MAX_COMPONENTS { return Err(()); }
            }
        }
    }
    packages.get_mut(OFFICE).ok_or(())?.skills =
        skill_files(&app.join("Contents/Resources/runtime/office-skills"), budget)?;
    archive.recheck()?;
    Ok(packages)
}

pub(super) fn observe(profile: &str, selected: &[&str], budget: &mut ScanBudget) -> Result<BTreeMap<String, Package>, ()> {
    if profile == "desktop" {
        desktop(selected.contains(&WEB), budget)
    } else {
        cli(selected, budget)
    }
}

pub(super) fn watch_paths(profile: &str) -> Vec<PathBuf> {
    if profile != "desktop" {
        return cli_entry().ok().flatten().into_iter().flat_map(|(command, entry)| {
            let root = entry.parent().and_then(Path::parent).expect("validated dsh entry");
            let pool = root.parent().and_then(Path::parent).expect("validated dsh npm root");
            [command, root.join("package.json"), pool.join(WEB).join("package.json"), pool.join(PRESET).join("skills")]
        }).collect();
    }
    desktop_paths().into_iter().flat_map(|app| [
        app.join("Contents/Info.plist"),
        app.join("Contents/Resources/app.asar"),
        app.join("Contents/Resources/runtime/office-skills"),
    ]).collect()
}
