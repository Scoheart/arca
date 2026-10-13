# QwenWork desktop integration

Verified on 2026-09-21 against official documentation and the installed, vendor-distributed application bundles. No user configuration, credentials, or account databases were read or changed.

| Arca Agent | Application | Bundle ID | User MCP file | User Skills directory |
|---|---|---|---|---|
| `qwenwork` | QwenWork.app (international, inspected 1.0.4) | `com.qwenwork.desktop.mac` | `~/.qwenwork/mcp.json` | `~/.qwenwork/skills` |
| `qwenwork-cn` | QwenWorkCN.app (China, inspected 1.1.0) | `cn.qwenwork.desktop.mac` | `~/.qwenworkcn/mcp.json` | `~/.qwenworkcn/skills` |

Each edition discovers its own app in `/Applications` or `~/Applications`. Neither edition falls back to launching the other or projects resources into the other's directory. Personal and enterprise plans are subscription/workspace options, not additional independently configurable desktop applications. Qwen Code remains a separate existing Agent.

## Verified contracts

- Official [international connector documentation](https://docs.qwenwork.ai/desktop/connectors) describes `mcpServers`, stdio, SSE, and `streamable-http` configurations. The [Skills documentation](https://docs.qwenwork.ai/desktop/skills) specifies the user directory and `SKILL.md` structure.
- Both vendor app bundles contain `out/main/main.js` with `CUSTOM_MCP_CONFIG_PATH` resolving the product directory plus `mcp.json`, and `getUserSkillsDir` resolving the same directory plus `skills`.
- International `constants-BtQtHdt9.js` resolves the product directory to `.qwenwork`. CN `constants-D8Z2dPrh.js` and `constants-DVUv7Rt5.js` resolve it to `.qwenworkcn`. These are bundle source observations, not assumptions based on branding.
- Both clients separately store built-in connectors and OAuth state under Electron userData. Arca only targets the user MCP file, never those internal files.
- The QwenWork codec shares the explicit transport wire format with QoderWork, but independently rejects inactive (`enabled: false`) entries and enterprise/market-owned entries. Root metadata, unrelated servers, and Agent-owned fields are preserved by the existing lossless adapter.
- Model account/BYOK storage has no verified Arca writer in this integration; Models remains unsupported rather than writing speculative paths.

## Sources

- [International product and downloads](https://qwenwork.ai/)
- [International desktop connectors](https://docs.qwenwork.ai/desktop/connectors)
- [International desktop Skills](https://docs.qwenwork.ai/desktop/skills)
- [China product and downloads](https://qwenwork.cn/)
- [China desktop connectors](https://qwenwork.cn/docs/features/connectors)
- [China desktop Skills](https://qwenwork.cn/docs/features/skills)
- [Enterprise subscription scope](https://docs.qwenwork.ai/benefits/enterprise)
- Official local bundle code and icon containers listed above; extracted vendor code is research evidence outside the repository and is not redistributed.

Regression fixtures cover both editions' distinct paths and launch candidates, stdio and HTTP round trips, inactive/managed server protection, metadata preservation, and resource removal. Tests are retained for manual execution under the repository's fast-delivery policy; no live application login or resource mutation was performed.
