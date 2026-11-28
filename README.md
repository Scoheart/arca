<img src="desktop/src-tauri/icons/icon.png" width="72" align="right" alt="Arca icon">

# Arca

Manage model providers, MCP servers, and skills for your AI agents from one place.

**[Download for macOS](https://github.com/Scoheart/arca/releases/latest)** · [Documentation](https://mux.scoheart.com/en/guide/what-is-arca) · [中文文档](https://mux.scoheart.com/guide/what-is-arca) · [Arca Web](https://arca-web-delta.vercel.app/)

## What you can do

- **Models** — Set up providers and reusable model profiles. Store API keys in macOS Keychain, 1Password, Bitwarden, or another supported store.
- **MCPs and Skills** — Import them once. Choose which agents use them from each agent’s workspace.
- **Agents** — Configure and launch Codex, Claude Code, Cursor, OpenCode, and other supported agents. Browse local sessions with Trace.

## Get started

Download the Apple silicon installer and drag **Arca.app** to **Applications**. Open an agent in Arca, then choose the models, MCP servers, and skills you want it to use.

The desktop app bundles the `arca` CLI. You can also download it separately from the release page. Run `arca --help` for commands or `arca` to open the interactive MCP workspace.

Use **Arca Web** to send a single model request or start a chat in your browser.

[Models and credential stores](website/en/guide/models.md) · [Supported agents](website/en/guide/agents.md) · [CLI guide](website/en/guide/cli.md)

## Development

```sh
cd desktop
npm ci
npm run tauri dev
```

Build the standalone CLI from the repository root with `cargo build --release -p arca-cli`.

[MIT License](LICENSE) · © Scoheart
