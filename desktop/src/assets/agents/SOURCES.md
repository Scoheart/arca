# Agent icon sources

## Capy — 2026-10-02

- `capy.png`: the original 180px Apple touch icon from Capy's official site at <https://capy.ai/_marketing/favicon/apple-touch-icon.png>; its black tile and white mascot are retained unchanged. The official desktop download is <https://capy.ai/download/>.

## Step Code — 2026-09-23

- `step-code.png`: the mascot region of the official Step Code README banner at <https://static-openapi.stepfun.com/resource/test.jpg> (pixels x=1070–1249 of the 1260×180 RGBA source). Pixels are unchanged; only the square mascot was cut out. It is not the StepFun provider mark.

## QwenWork editions — 2026-09-21

- `qwenwork.png`: original 128px PNG (`ic07`) extracted losslessly from the official QwenWork 1.0.4 macOS bundle's `Contents/Resources/icon.icns`; bundle ID `com.qwenwork.desktop.mac`, download <https://qwenwork.ai/>.
- `qwenwork-cn.png`: corresponding original PNG from the official QwenWorkCN 1.1.0 bundle; bundle ID `cn.qwenwork.desktop.mac`, download <https://qwenwork.cn/>. The two bundled images differ and are intentionally retained separately.
- Both IDs have their own assets and desktop surface metadata; they do not fall back to Qwen Code's logo. With these two additions the merged catalog contains 225 entries, 70 with bundled icons and 155 hidden pending verified artwork.

## Built-in catalog audit — 2026-09-20

The full built-in directory merges 204 entries from `data/agent-catalog.json` with 68 audited entries from `data/agents.json`, with audited entries taking precedence, matching `core::agents::builtin_agents`. This produces 223 unique Agents. Of these, 68 have bundled images and 155 have no verified bundled image and are hidden from navigation, search and pagination. The initial 68-entry audit incorrectly omitted the broader catalog; it was not a complete directory audit. Hidden does not mean the product has no logo, only that Arca has no verified asset yet.

Within the 68 audited definitions, 60 previously had an image and 8 used monograms. All 8 missing marks below are now bundled. No substitute artwork or generated initials were used for these additions.

| Added asset | Retrieved source | Identity |
|---|---|---|
| `cortex-code.svg` | <https://docs.snowflake.com/images/favicon/favicon.svg> | Snowflake mark linked by the Cortex Code documentation |
| `dirac.svg` | <https://github.com/dirac-run/dirac/blob/master/agent-registry/dirac/icon.svg> | Product's own ACP entry mark |
| `docker-agent.png` | <https://www.docker.com/app/uploads/2024/02/cropped-docker-logo-favicon-192x192.png> | Docker vendor mark |
| `minion-code.svg` | <https://github.com/agentclientprotocol/registry/blob/main/minion-code/icon.svg> | ACP Registry entry points to `femto/minion-code`; the product repository itself has no image logo |
| `poolside.png` | <https://docs.poolside.ai/mintlify-assets/_mintlify/favicons/poolside/9TIRP1d6OHt2GpzJ/_generated/favicon/android-chrome-192x192.png> | Poolside mark linked by its pool CLI documentation |
| `raycast.png` | <https://www.raycast.com/favicon-production.png> | Official site favicon |
| `theiaai-theiaide.svg` | <https://theia-ide.org/> | Original SVG decoded from the site's embedded `rel="icon"` data URL |
| `zencoder.png` | <https://zencoder.ai/hubfs/export.png> | Official site favicon; direct page requests initially returned 403, then the rendered page confirmed this URL |

Navigation and the hand picker use the runtime `builtin` flag and bundled icon availability, including persisted built-ins absent from the current catalog. ID-only display cards use both catalogs. Missing built-ins are hidden until an asset is added. Custom Agents keep their existing fallback. Core catalog entries, configuration, consumption relationships and hidden pinned IDs are retained; operational impact/review records must still disclose affected Agents.

New assets are copied from official product sites, official repositories, or the vendor's own brand package. Sources were retrieved on or after 2026-07-14.

| Asset | Official source |
|---|---|
| `agentkube.svg` | <https://agentkube.mintlify.app/favicon.svg> |
| `amp.svg` | <https://ampcode.com/amp-mark-color.svg> |
| `antigravity.png` | <https://antigravity.google/assets/image/antigravity-logo.png> |
| `augment.svg` | <https://www.augmentcode.com/favicon.svg> |
| `boltai.png` | <https://boltai.com/assets/app-icon-v4.png> |
| `chatmcp.svg` | <https://github.com/daodao97/chatmcp/blob/main/macos/Runner/Assets.xcassets/AppIcon.appiconset/app_icon_128.png> |
| `codebuddy-code.svg` | <https://codebuddy-1328495429.cos.accelerate.myqcloud.com/web/ide/logo.svg> |
| `codewhale.svg` | <https://github.com/Hmbown/CodeWhale/blob/main/web/app/icon.svg> |
| `crush.png` | <https://stuff.charm.sh/crush/charm-crush.png> |
| `factory-droid.svg` | <https://www.factory.ai/favicon.svg> |
| `firebender.svg` | <https://firebender.com/icon.svg> |
| `freebuff.svg` | <https://freebuff.com/favicon.svg> |
| `goose.png` | <https://github.com/block/goose/blob/main/documentation/docs/assets/logo.png> |
| `grok-build.png` | <https://github.com/xai-org/grok-build> (official README asset, retrieved 2026-07-16) |
| `hermes.png` | <https://github.com/NousResearch/hermes-agent/blob/main/apps/desktop/assets/icon.png> |
| `kilo-code.svg` | <https://github.com/Kilo-Org/kilocode/blob/main/packages/kilo-docs/public/favicon/favicon.svg> |
| `kimi-code.png` | <https://github.com/MoonshotAI/kimi-cli/blob/main/web/public/logo.png> |
| `lmstudio.png` | <https://lmstudio.ai/assets/marketing/logo-192x192.png> |
| `minimax-code.png` | MiniMax Code `3.0.51` official signed macOS app icon from <https://agent.minimax.io/download> (retrieved 2026-07-16) |
| `mistral-vibe.png` | <https://mistral.ai/brand/> (`Mistral-Icon-Gradient-RGB.png`) |
| `openhands.svg` | <https://openhands.dev/favicon.svg> |
| `qwen-code.png` | <https://qwenlm.github.io/qwen-code-docs/favicon.png> |
| `rovo-dev.svg` | Atlassian `@atlaskit/logo@21.4.0`, `RovoDevIcon` |
| `stakpak.svg` | <https://github.com/agentclientprotocol/registry/tree/main/stakpak> (provider-submitted registry icon) |
| `tabnine.png` | <https://www.tabnine.com/wp-content/uploads/2024/09/cropped-tabnine-favicon-192x192.png> |
| `vt-code.svg` | <https://github.com/agentclientprotocol/registry/tree/main/vtcode> (provider-submitted registry icon) |

- `qoder-desktop.svg`: https://qoder.com/favIcon.svg, linked by the official https://qoder.com/download page (retrieved 2026-09-06). New Desktop uses the black outline on a light tile, independently of the legacy IDE/CLI `qoder.svg`. `surfaces.json` retains its Desktop identity; the distinct logo needs no collision badge.

Official assets retrieved 2026-09-07 (original bytes; ICO retained without conversion):

- `trae.png`: <https://lf-cdn.trae.com.cn/obj/trae-com-cn/trae_website_prod_cn/favicon.png>
- `openclaw.svg`: <https://openclaw.ai/favicon.svg>
# ZCode Desktop

- Official product: https://zcode.z.ai/cn
- Icon: https://zcode.z.ai/favicon-192x192.png?v=20260707-transparent
- MCP: https://zcode.z.ai/cn/docs/mcp-services
- Skills: https://zcode.z.ai/cn/docs/skill
- Verified 2026-09-07. Desktop surface only; community CLI packages are not official ZCode releases.

- WorkBuddy: official favicon from https://download.codebuddy.ai/web/workbuddy/00aa368996ce0f8793afd87db1bcdf458d8ba952/assets/logo.svg (2026-09-08). CodeBuddy IDE shares the official CodeBuddy logo with CLI and uses the IDE surface marker.

`kimi-code-desktop.png` is the unmodified `Contents/Resources/build/icon.png` from the official [Kimi Code Desktop arm64 installer](https://code.kimi.com/kimi-code/desktop/download/KimiCode-mac-arm64.dmg), version 1.0.1, retrieved 2026-09-17. DMG SHA-256: `a4fbecb10cda6518feb5a8d1e857e9f926629a1f66088e20f478d1319469ae0a`. It intentionally uses the new Desktop icon rather than the historical CLI asset.

- WorkBuddy 中国版 (`workbuddy-cn`) 与海外版 (`workbuddy`) 共享官方 WorkBuddy 品牌图标；安装包 identity、launcher 与配置目标独立（2026-09-27）。

Official assets verified 2026-10-04 (original bytes):

- `jcode.svg`: <https://jcode.sh/favicon.svg>, official site links to <https://github.com/1jehuang/jcode>; unrelated same-name repositories were excluded.
- `ibm-bob.svg`: <https://bob.ibm.com/icon.svg?v=1>.
- `anythingllm.png`: <https://anythingllm.com/icon.png?0a59f488e9ddf54c>.
- `jan-desktop.png`: <https://github.com/janhq/jan/blob/14a720628f9592c8e60f7e081ef733076db8a6a9/src-tauri/icons/icon.png>; Jan Agent shares the official Jan icon.
- `mimo-code.png`: official Xiaomi MiMo brand asset already recorded in `../providers/SOURCES.md`, copied unchanged. This is the vendor brand, not a claim of a separately verified Desktop app icon.
- `deepseek-harness.png`: official DeepSeek brand asset already recorded in `../providers/SOURCES.md`, copied unchanged.
- Cline CLI, Kiro CLI, Junie CLI, Goose Desktop, Kilo Code VS Code and Codex IDE reuse their verified brand icons; `surfaces.json` distinguishes the product surface.

- `kilo-desktop.png`: PNG decoded from `Kilo.app/Contents/Resources/electron.icns` in the official [Kilo Desktop macOS arm64 ZIP](https://autoupdate.desktop.anaconda.com/kilo-desktop/latest/latest/macos/arm64/Kilo.zip), version 0.1.11, bundle ID `ai.kilo.desktop`, verified 2026-10-07. It preserves the Desktop icon's bright yellow background instead of reusing the older IDE/CLI asset. The installed app name is `Kilo`.


Assets added 2026-10-16, original bytes:

| Asset | Source |
|---|---|
| `librechat.svg` | [LibreChat logo](https://github.com/danny-avila/LibreChat/blob/e1dfc10449ff713faffacd60273fddcfe2c0a698/client/public/assets/logo.svg) |
| `open-webui.svg` | [Open WebUI favicon](https://github.com/open-webui/open-webui/blob/6a18585ff4c2e5f6805f682222cfbc46dc669ef8/static/static/favicon.svg) |
| `mcp-use.svg` | [mcp-use documentation favicon](https://github.com/mcp-use/mcp-use/blob/ad8fbee97af6c39fc95b46486a04142d68644822/docs/favicon.svg) |
| `mcp-use-inspector.svg` | [Inspector favicon](https://github.com/mcp-use/mcp-use/blob/ad8fbee97af6c39fc95b46486a04142d68644822/libraries/typescript/packages/inspector/public/favicon.svg) |
| `mcpjam.ico` | [MCPJam Inspector app icon](https://github.com/MCPJam/inspector/blob/5f652c40b574921ba800727dd7ad0b4159c1633b/mcpjam-inspector/assets/icon.ico), retained as ICO |
| `postman.png` | [Postman Labs documentation favicon](https://github.com/postmanlabs/labs-docs/blob/a95dc277cc0cef48883344d0ca35e6555c3e1bbc/src/images/favicon.png) |
| `copilotkit.svg` | [CopilotKit logo mark](https://github.com/CopilotKit/CopilotKit/blob/6e4cb6ad9ce32881636edaa212b7dbb440f0a0c3/examples/integrations/langgraph-js/public/copilotkit-logo-mark.svg) |
| `ag-ui.svg` | [AG-UI documentation favicon](https://github.com/ag-ui-protocol/ag-ui/blob/a1131c90709d51d562202b7b6cc0a2a1b48b05fc/docs/favicon.svg) |
| `smithery-playground.svg` | [Smithery brand asset in Raycast's MCP registry extension](https://github.com/raycast/extensions/blob/c0eba1590385588f2b265cec8bdefb653f30816f/extensions/model-context-protocol-registry/assets/smithery.svg), reused unchanged from that MIT-licensed extension; this is a third-party copy of the brand asset, not a provider-hosted download |

Claude.ai and ChatGPT reuse the already sourced Claude/OpenAI marks. Their Web surface badge distinguishes them from CLI and Desktop products.
