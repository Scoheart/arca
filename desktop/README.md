# Arca Desktop

Tauri + React desktop app for MCP configuration management.

## Development

```bash
cd desktop
npm install
npm run tauri dev   # Opens the Arca window (requires a display)
```

## Features (Plan 2)

- Browse and search the built-in MCP registry (40+ servers)
- Install MCPs into global agent configs
- Per-agent env overrides
- Preview each target path and its effective connection config before applying
- Per-Agent backup of existing config files; failed backups abort the write
- Atomic config replacement with concurrent-edit detection

## Data

- Server/agent definitions: repo root `data/registry.json` / `data/agents.json` (shared with CLI)
- Overrides: `~/.arca/overrides.json`

## Manual Smoke Test (requires display — cannot run in headless CI)

This step MUST be run by a human on a machine with a graphical display:

```bash
cd desktop
npm run tauri dev
```

Verify the following:

1. Window opens and shows the registry grid with 40+ server cards.
2. Typing "git" in the search box filters the cards correctly.
3. Clicking a card opens the install dialog.
4. Open an Agent and click "添加 MCP" to install an entry into its global config.
5. Confirm with `cat ~/.claude.json` (or the agent's config file) that the server was added to `mcpServers`.
6. Confirm `ls ~/.arca/backups/` shows a backup file.
7. Close and reopen the app — the Agent page still lists the installed server.

Record results in the PR description or commit message.

## Build (headless / CI)

```bash
# TypeScript + Vite compile check (no display needed)
cd desktop && npm run build

# Rust compile check (no display needed)
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --manifest-path desktop/src-tauri/Cargo.toml

# Unit tests (no display needed)
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```
