# Kimi Code integration

Verified on 2026-09-17 against the official Desktop 1.0.1 macOS arm64 distribution.
`kimi-code-desktop` is independent from the existing `kimi-code` CLI identity.
Both use the shared Rust capability graph, so CLI and Desktop Arca expose the same integration.

| Surface | Contract |
| --- | --- |
| Application | `Kimi Code.app`, bundle ID `com.kimi.code.desktop`; system and user Applications folders |
| MCP | `~/.kimi-code/mcp.json`, JSON `mcpServers`; existing Kimi codec |
| Transport | `command` for stdio, `url` without transport for HTTP, `transport: sse` for SSE |
| Skills | `~/.kimi-code/skills/<name>/SKILL.md`; shared target ID `kimi-code-user`; also reads `~/.agents/skills` |
| Models | Guided; native `~/.kimi-code/config.toml`, Desktop Settings → Providers or CLI `/provider` |
| Icon | Official Desktop distribution's `Contents/Resources/build/icon.png` |

## Evidence

- [Desktop quick start](https://www.kimi.com/code/docs/en/kimi-code-desktop/getting-started.html)
- [MCP wire format](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/mcp.html)
- [Skill directories](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/skills.html)
- [Provider credentials](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/overrides.html)
- [Official installer](https://code.kimi.com/kimi-code/desktop/download/KimiCode-mac-arm64.dmg)

Installer SHA-256: `a4fbecb10cda6518feb5a8d1e857e9f926629a1f66088e20f478d1319469ae0a`.
`Info.plist` and packaged `package.json` both report 1.0.1. The package was mounted
read-only for inspection, without launching or installing Kimi or reading user credentials.

The shipped `app.asar` bundle `out/screenshot-BrrrwBa-.cjs` contains the shared
`agent-core-v2` implementation: `resolveKimiHome` resolves `KIMI_CODE_HOME` or
`~/.kimi-code`, `resolveMcpJsonPaths` selects `mcp.json`, and
`features/skill/catalog/skillRoots.ts` selects `skills` and `.agents/skills`.
Skill discovery follows directory symlinks. MCP schemas accept `enabled`,
`deferred`, timeout/tool policies and `bearerTokenEnvVar` in addition to connection fields.

## Ownership and limitations

Desktop and CLI share physical MCP and Skill targets. Existing core target
conflict/closure handling is reused; there is no duplicate frontend writer.
Removing or disabling an MCP still owned by the sibling is rejected. Skills
use one shared target identity, not two competing copies of a symlink.

The Kimi codec preserves native policy fields, and now excludes `enabled: false`
servers from effective scans and refuses overwriting them as active entries.
Existing guarded, lossless writes retain unrelated entries. New MCP servers are
registered in new Kimi sessions; existing sessions do not automatically gain them.

If `KIMI_CODE_HOME` is customized, configure matching MCP and Skills paths in Arca.
The default integration does not infer another application's shell environment.
Project overlays, plugin-owned MCP/Skills, browser control and OAuth credentials
remain managed by Kimi itself.

Models are guided deliberately: `api_key` and `[providers.<name>.env]` contain
literal credentials; the latter is not a shell environment reference. The shipped
`hasConfiguredApiKey` uses the provider value or its TOML env map. No native
credential helper/reference was verified. Writing `${VAR}` would send literal
text as the key, and exporting Keychain data requires a separate explicit delivery
policy and audited writer. This integration advertises neither capability.

Regression cases cover both identities, HTTP/SSE/stdio parsing, retained policies,
disabled/malformed entries, shared-target removals, and guided Model boundaries.
The repository's fast delivery mode retains these tests without running them by default.
