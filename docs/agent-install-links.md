# Agent 安装入口维护

核验日期：2026-10-22。权威数据为 `data/agent-install-links.json`，Core 向 CLI/Desktop 暴露同一地址；MCP、Skills 等配置文档继续使用各自的能力文档。

## 选择标准

- 「安装文档」必须直接落到官方安装步骤；长页使用已核验的安装章节锚点。可参照 [Pi 的安装章节](https://pi.dev/docs/latest/quickstart#1-install-pi)，其中给出平台、命令、依赖要求和版本验证。
- 先按 `data/agent-launchers.json` 核对实际形态：CLI 指向 CLI 安装步骤，扩展指向其宿主编辑器的安装步骤，桌面应用指向官方下载或安装页。不要让同品牌的多个形态共用一个泛化首页。
- 优先官方专用安装页，其次有安装步骤的 quickstart/README 精确章节；不要根据 URL 猜锚点，不将版本固定的二进制下载 URL 作为长期入口。
- 核验正文与跳转后的实际产品，HTTP 200 本身不代表入口正确。没有核验到替代步骤的入口保持原值，不宣称已经优化。

## 本次核验并修改的入口

以下地址已读取官方正文；Pi、Cline、Continue、Claude Code、Copilot、Factory、Gemini、OpenCode、Auggie、Hermes、Junie 的章节与实际安装/入门步骤对应。其余保留专用安装或官方下载页。

| Agent ID | 安装入口 |
| --- | --- |
| `agentkube` | https://agentkube.com/downloads |
| `amp` | https://ampcode.com/docs/cli#install |
| `augment` | https://docs.augmentcode.com/cli/overview#getting-started |
| `claude-code` | https://code.claude.com/docs/en/quickstart#step-1-install-claude-code |
| `cline` | https://docs.cline.bot/getting-started/installing-cline#ide-extension |
| `continue` | https://docs.continue.dev/ide-extensions/install |
| `continue-cli` | https://docs.continue.dev/cli/quickstart#install |
| `copilot-cli` | https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli#installing-or-updating-copilot-cli |
| `factory-droid` | https://docs.factory.com/droid-cli/quickstart#step-1-install-and-start-droid |
| `gemini` | https://geminicli.com/docs/#install |
| `goose` | https://goose-docs.ai/docs/getting-started/installation/ |
| `hermes` | https://hermes-agent.nousresearch.com/docs#without-hermes-desktop |
| `junie` | https://www.jetbrains.com/help/ai-assistant/junie-agent.html#junie-get-started |
| `opencode` | https://opencode.ai/docs/#install |
| `openclaw` | https://docs.openclaw.ai/install |
| `pi` | https://pi.dev/docs/latest/quickstart#1-install-pi |
| `cline-cli` | https://docs.cline.bot/getting-started/installing-cline#cli |
| `kilo-vscode` | https://kilo.ai/docs/getting-started/installing |

本次不是全部 Agent 外链的完整有效性审计。已核验的关键区别包括：Amp 旧 manual 跳转介绍页；Factory 使用新域名和 CLI quickstart；Continue/Cline 区分扩展与 CLI；Auggie 是 CLI；Agentkube 使用实际下载页。
