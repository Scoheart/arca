# MCP Apps client coverage

Reviewed 2026-10-16. Arca manages Agent assets; it does not render MCP Apps.
MCP tool compatibility, HTML App rendering, and safe local asset assignment
are separate capabilities. Client names in the picker do not promise all three.

## Coverage added

| Entry | Change | Arca behavior |
|---|---|---|
| LibreChat | New | Web client, official deployment configuration guide |
| MCP Inspector by mcp-use | New | Browser client, official Inspector guide |
| CopilotKit | New | Development framework, documentation only |
| AG-UI | New | Protocol/framework, documentation only |
| Claude.ai | Restored visibility | Web client, account connector guide |
| ChatGPT | Restored visibility | Web client, developer-mode guide |
| Open WebUI | Restored visibility | Web client, deployment/account guide |
| Postman | Restored visibility | Native App launch/install entry and MCP request guide |
| MCPJam Inspector | Restored visibility | Native App launch/install entry and configuration guide |
| mcp-use | Restored visibility | Framework guide, separate from its Inspector |
| Smithery Playground | Restored visibility | Platform guide, separate from the CLI |

The seven restored entries were already retained in the discovery catalog but
had no bundled logo and were consequently hidden. New branded assets and aliases
make all eleven searchable and pinnable. Web/framework pages expose guides instead
of native launch settings, Trace, or process capture. Postman and MCPJam use their
own App names; Arca does not execute an ambiguous `inspector` command or install
packages automatically.

Cursor, VS Code + GitHub Copilot, Goose CLI/Desktop, Claude Desktop, Codex
CLI/Desktop, QoderWork, Qoder Desktop/IDE/CLI, OpenCode CLI/Desktop, Claude Code,
Cline, Zed, Gemini CLI, and Windsurf were already present. Their audited native
asset capabilities remain unchanged. In particular, Desktop and CLI identities
stay separate; no MCP Apps support flag is inferred for either surface.

## Why these entries remain guided

- [LibreChat's implementation notes](https://github.com/danny-avila/LibreChat/blob/e1dfc10449ff713faffacd60273fddcfe2c0a698/docs/mcp-apps.md)
  describe host rendering and `mcpSettings.apps`. Configuration belongs to the
  deployment's `librechat.yaml` and server settings, not a verified per-user
  global file. Support depends on deployed version and opt-in configuration.
- [mcp-use Inspector](https://github.com/mcp-use/mcp-use/blob/ad8fbee97af6c39fc95b46486a04142d68644822/libraries/typescript/packages/inspector/README.md)
  provides browser connection management and App testing. Browser storage is not
  a native global Agent configuration file.
- [MCPJam's Skills loader](https://github.com/MCPJam/inspector/blob/5f652c40b574921ba800727dd7ad0b4159c1633b/mcpjam-inspector/server/utils/skill-tools.ts)
  skips directory entries that are not `isDirectory()`. That excludes directory
  symlinks, while Arca assigns Skills by symlink. Its known scan roots alone do
  not justify enabling Arca assignment. The [official package](https://github.com/MCPJam/inspector/blob/5f652c40b574921ba800727dd7ad0b4159c1633b/mcpjam-inspector/package.json)
  names the native application `MCPJam Inspector`.
- Postman requests, Open WebUI accounts, and cloud connectors are configured in
  their own products. Arca does not edit their private databases or invent paths.
- [CopilotKit](https://github.com/CopilotKit/CopilotKit/tree/6e4cb6ad9ce32881636edaa212b7dbb440f0a0c3),
  [AG-UI](https://github.com/ag-ui-protocol/ag-ui/tree/a1131c90709d51d562202b7b6cc0a2a1b48b05fc)
  and mcp-use are integration infrastructure. Their consuming applications own
  configuration and rendering. Open WebUI community bridges do not establish
  native support in its stable core.

[MCP-UI's host list](https://github.com/MCP-UI-Org/mcp-ui/blob/2b10490910943640deeb2d9012ad417d4c0e9025/README.md)
was used as a discovery cross-check, not as proof of a writable file contract.

## Maintenance

Edit `data/agent-catalog-supplements.json` for reviewed identities, notes, and
source evidence, then run the catalog generator's `--reviewed-only` mode. Guides,
docs links, original brand assets, and native launcher candidates have separate
presentation catalogs. Any future promotion to writable support must satisfy the
[full native contract audit](agent-catalog.md#promotion-criteria), including
round-trip preservation and compatible Skills loading.
