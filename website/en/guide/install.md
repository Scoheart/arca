# Installation

Arca has two entry points — the desktop app and the CLI / TUI — sharing `~/.arca/`. The desktop app already bundles the CLI, so you usually don't need to install it separately.

## Desktop app (macOS)

1. Open the [latest stable Release](https://github.com/Scoheart/arca/releases/latest) and pick the asset labeled **Desktop installer · Apple Silicon** (its filename looks like `Arca-Desktop-Installer-*-macOS-Apple-Silicon.dmg`).
2. Open the dmg and drag **Arca.app** into `/Applications`.
3. Launch it for the first time.

### Getting "Arca is damaged and can't be opened"?

The current release is not notarized with an Apple Developer ID, so macOS may block the first launch because of the quarantine attribute — it does not mean the file is damaged. Once you've confirmed the download came from this project's Release, you can clear the quarantine attribute:

```bash
xattr -dr com.apple.quarantine /Applications/Arca.app
```

Then open it normally. (Alternatively: right-click the app → Open → click "Open" again in the dialog.)

## CLI / TUI (`arca`)

`arca` is a native Rust binary that shares the same core as the desktop app.

### Option 1: install with the desktop app (recommended)

After the stable app launches, it symlinks the bundled CLI to `~/.local/bin/arca`. If your terminal can't find `arca`, add that directory to your `PATH`:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
arca --version
```

This symlink is auto-repaired by the app; when the app updates, the CLI updates with it.

### Option 2: download the prebuilt binary separately

In the stable Release, pick the asset labeled **Command-line tool · Apple Silicon**. Its filename is `arca_v<version>_aarch64-apple-darwin.tar.gz`:

```bash
# After downloading from Releases:
tar xzf arca_v*_aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/bin
mv arca ~/.local/bin/arca
arca --version
```

A standalone CLI can run `arca upgrade` to follow the latest stable release; the CLI bundled with the desktop app will instead tell you it updates with the app.

### Option 3: install from source (requires Rust)

```bash
git clone https://github.com/Scoheart/arca
cd arca
cargo install --path cli       # installs to ~/.cargo/bin/arca
```

### Usage

Run with no arguments to enter the **interactive TUI**:

```bash
arca
```

Or use subcommands for scripting (set `ARCA_NO_TUI=1` to print help instead of entering the TUI when run with no arguments):

```bash
arca mcp list
arca mcp assign github::stdio --agent claude-code --yes
arca model use work --agent pi --yes
arca skill assign review-changes --agent codex --yes
arca mcp export --out mcp.json --yes
arca agent list
arca upgrade --yes
```

Agent-relationship mutations require exact asset IDs and an explicit `--agent <id>`. Scripts can use the global `--json`, `--yes`, `--dry-run`, and `--no-color` options; `--yes` and `--dry-run` are mutually exclusive.

See [CLI / TUI](/en/guide/cli) for details.

## Where data lives

Desktop and CLI share `~/.arca/`. Central MCP sources, Model metadata, and Skill content live under `assets/`; `settings.json` stores preferences and relationships. Reviews, recovery progress, and backups use dedicated directories. Central keys live in the system Keychain. See [Data layout](/en/guide/concepts#data-layout).

Next → [Core concepts](/en/guide/concepts)
