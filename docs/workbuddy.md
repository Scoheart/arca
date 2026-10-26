# WorkBuddy editions

Arca exposes two independent desktop Agents. Both support user-level MCP under
`mcpServers`, native MCP pause, and central Skill symlinks. Models use each
edition's native setup guide.

| Edition | Arca ID | App | MCP | Skills | Official site |
| --- | --- | --- | --- | --- | --- |
| WorkBuddy AI | `workbuddy` | `WorkBuddy AI.app` | `~/.workbuddy-ai/mcp.json` | `~/.workbuddy-ai/skills` | https://www.workbuddy.ai/ |
| WorkBuddy | `workbuddy-cn` | `WorkBuddy.app` | `~/.workbuddy/mcp.json` | `~/.workbuddy/skills` | https://www.workbuddy.cn/ |

The existing `workbuddy` ID and `workbuddy-ai-user` Skill target retain all
overseas assignments. China uses the new `workbuddy-cn` ID and
`workbuddy-cn-user` target. Neither edition aliases the other's directories or
launcher. Adding China does not copy assignments, import external assets, or
change either app's configuration.

## Evidence (2026-09-27)

- Official documentation: https://www.workbuddy.ai/docs/workbuddy/Overview
- Connector guide: https://www.workbuddy.ai/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Connector
- China documentation: https://www.workbuddy.cn/docs/workbuddy/Overview
- China connector guide: https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Connector
- Both official 5.6.2 apps were inspected locally. China declares bundle ID
  `com.tencent.workbuddy.mac`, `dataFolderName: .workbuddy`, and endpoint
  `https://www.workbuddy.cn`; overseas declares bundle ID
  `com.workbuddy.workbuddy-ai`, `.workbuddy-ai`, and `https://www.workbuddy.ai`.
  The two packages implement the same `UserMcpFile`, `toTransport`, Skills root,
  and directory-symlink scanning contract, so they share the codec only.
- User-provided WorkBuddy AI 5.6.2 configuration editor shows the user-level path
  and an empty `mcpServers` object. The file may not exist until first save.
- Read-only inspection of the installed official app's `app.asar` confirms
  `UserMcpFile` in `main/server.js` joins the product config directory with
  `mcp.json`, and returns an empty object for a missing file.
- `resolveWorkbuddyConfigDir` in `main/code-cache.js` resolves the product's
  `dataFolderName`; the international app's `cli/product.json` declares
  `.workbuddy-ai`. Explicit custom Arca paths remain supported after initial
  promotion from the old read-only definition.
- `parseMcpServerEntry` / `toTransport` in `main/tar.js` accept `command`, `args`,
  `env`, and `cwd` for stdio; `url` and `headers` for HTTP; `type: sse` for SSE.
  `UserMcpFile.toEntry` omits type for stdio/HTTP and emits it for SSE.

## Write boundary

The WorkBuddy codec changes only connection fields. It preserves timeout,
description, deferred loading, disabled tools, unknown fields, and sibling
servers. Paused entries remain visible as installed and disabled. The enable/disable
actions change only the native `disabled` boolean, preserving the complete
entry in place. Connection updates preserve its paused state; explicit desired
state reconciliation applies the requested enable flag. Older Arca off-disk
snapshots can still be restored when no live entry exists. Invalid disabled flags,
malformed JSON, duplicate keys, and invalid MCP containers fail closed.

Existing files use the shared JSON adapter and in-place safe writer, preserving
the watched inode. Missing files use the existing create/backup/CAS path.
The fixture and round-trip coverage are in `core/tests/agent_formats.rs`.

## Skills

`createSkillsService` in the official app's `main/server.js` sets the user Skills
root to `join(configDir, "skills")`. `scanSkillsDirectory` in
`main/log-acl-guard.js` follows directory symlinks with `stat` and looks for
`SKILL.md`. This supports Arca's central-copy plus per-skill symlink contract.
Arca owns only its assigned links and leaves marketplace metadata, external
Skills, and project-level directories untouched.
