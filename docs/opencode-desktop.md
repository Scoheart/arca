# OpenCode integration

Verified on 2026-09-26 against the official `anomalyco/opencode` release `v1.18.32` (tag object `545f51d26cc39a907d2867492d498d9607ea5fa4`).

## Identity and discovery

- Arca identity: `opencode-desktop`, displayed as **OpenCode**, category `desktop`.
- Existing `opencode` remains the CLI identity. Both use the OpenCode brand icon with distinct CLI/Desktop badges.
- Official production application: `OpenCode.app`, bundle ID `ai.opencode.desktop`. Arca discovers `/Applications/OpenCode.app` or `~/Applications/OpenCode.app` and opens the installed app through the existing macOS launcher.
- Download: https://opencode.ai/download. OpenCode Beta/Dev are separate upstream channels and are not automatically selected.

## Local resource contract

| Resource | Default path | Behavior |
|---|---|---|
| MCP | `~/.config/opencode/opencode.json` | Root `mcp`; existing OpenCode local/remote codec, command arrays and environment map |
| Models | `~/.config/opencode/opencode.json` | Existing OpenCode provider/models writer; multiple profiles and one global default |
| Native credentials | `~/.local/share/opencode/auth.json` | Existing reviewed OpenCode credential delivery, with separate Desktop provider IDs |
| Skills | `~/.config/opencode/skills` | Shared `opencode-user` physical target; also reads `~/.claude/skills` and `~/.agents/skills` |

The official Electron desktop bundles the OpenCode server and sets `OPENCODE_CLIENT=desktop`. It redirects the state directory, not the default config/data directories. These resources therefore belong to the local OpenCode server and are shared with the CLI. Arca does not edit Electron preferences, sessions, remote servers, project files, or shell initialization files.

Desktop-generated model provider IDs use `arca_desktop_<profile-id-hex>`; CLI entries keep their existing IDs. Removing a Desktop profile preserves CLI providers, other models and any current pointer that references a different entry. Importing a Desktop external model creates only a central asset; explicitly adding it then creates the separate Desktop entry. The physical global `model` pointer is shared, so changing the global default affects both clients; individual desktop conversations can select their own model. Clearing the entire model registry is rejected while the sibling Agent still has model consumers in the same configured file. Explicitly separated paths can be cleared independently.

MCP and Skills use the existing shared-physical-target planner, including its conflict handling; they are not independent copies. A desktop connection to a remote server needs Arca on that server. Custom `XDG_CONFIG_HOME`, `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR` and `opencode.jsonc` locations require corresponding explicit Arca path configuration. Upstream loads JSONC after JSON, so configure the actual effective file when both exist. Credential-store routing retains the existing default XDG data path contract. Restart the local OpenCode service after changes if an existing session does not refresh.

## Evidence

All source links are pinned to the audited release:

- [Production application identity](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/desktop/electron-builder.config.ts)
- [Bundled server entry](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/desktop/electron.vite.config.ts)
- [Desktop environment and local server](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/desktop/src/main/server.ts)
- [Global XDG paths](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/core/src/global.ts)
- [Configuration loading](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/config/config.ts)
- [Native MCP schema](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/core/src/v1/config/mcp.ts)
- [Skills discovery, including symlinks](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/skill/index.ts)
- [Credential store](https://github.com/anomalyco/opencode/blob/v1.18.32/packages/opencode/src/auth/index.ts)

Regression coverage uses the existing OpenCode fixture and isolated HOME: lossless MCP updates, malformed-config rejection, CLI/Desktop model removal isolation, shared clear protection, credential routing, app discovery without a CLI install, and icon surface distinction. Tests are retained but not executed under the repository's current fast-delivery policy. No live OpenCode inference call is part of this integration.
