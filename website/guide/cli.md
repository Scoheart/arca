# 命令行 / TUI

`arca` 是和桌面 App 共用 `arca-core` 与 `~/.arca/` 的原生 Rust CLI。它把 MCP、Model、Skill 都视为“中央资产 + Agent 消费关系 + 外部观测”，不会为三个域维护互相矛盾的状态模型。

> 还没安装？看 [安装 · 命令行](/guide/install#命令行-tui-arca)。

## 给 Agent 安装 CLI Skill

仓库提供可独立安装的 [arca-cli Skill](https://github.com/Scoheart/arca/tree/main/skills/arca-cli)，指导 Agent 使用本机 CLI 配置启动路径、参数和环境变量，管理 MCP、模型与 Skills，并读取 JSON 结果、预览改动和处理部分失败。

在本地仓库根目录执行：

```bash
arca skill inspect-source --path ./skills/arca-cli --json
arca skill install --path ./skills/arca-cli --name arca-cli --dry-run --json
arca skill install --path ./skills/arca-cli --name arca-cli --yes --json
arca agent skill assign arca-cli --agent codex --dry-run --json
arca agent skill assign arca-cli --agent codex --yes --json
arca agent skill status --agent codex --json
```

`codex` 是 Codex CLI；ChatGPT 使用 `codex-desktop`，其他 Agent 的准确 ID 从 `arca agent list --json` 获取。如果安装计划报告风险，先阅读 findings，明确接受后再加 `--accept-risk`。桌面端也可从 **Skills → 添加** 导入 `skills/arca-cli` 文件夹，再分配给 Agent。使用其他支持 Skills 的工具时，需保留 `SKILL.md` 和 `references/` 的相对结构。分配后新建会话，或使用目标 Agent 支持的重新加载方式。

## 两个入口

- `arca`：进入 MCP 终端工作区；适合交互维护 MCP 目录、来源和 Agent 关系。
- `arca <命令组> <操作>`：使用可脚本化的资产和 Agent 命令。

脚本中可设置 `ARCA_NO_TUI=1`，让无参数调用打印帮助而不是进入 TUI。

## 统一命令模型

`arca mcp`、`arca model`、`arca skill` 分别管理自己的中央资产库。Provider 在 `arca model provider` 下，统一提供连接配置和模型目录查询。

`arca agent list` 列出 Agent ID，`arca agent status codex` 查看 Codex 的 MCP、模型和 Skill 状态。管理单个 Agent 用到的资产时，走 `arca agent <mcp|model|skill>`，用 `--agent` 指定目标。

```text
arca mcp {list,show,add,save,delete,export,source,icon}
arca mcp source {list,subscribe,add-local,add-builtin,refresh,enable,disable,remove}
arca model {list,show,save,delete,import,provider,store,curl}
arca model provider {list,show,templates,catalog,save,delete,docs}
arca model store {list,save,check,delete,revisions,retire}
arca skill {list,show,inspect-source,install,import,update,remove,repair,check-updates}

arca agent {list,status,save,configure,enable,disable,run,launch,mcp,model,skill}
arca agent status <agent-id>
arca agent mcp {status,show,assign,unassign,enable,disable,enable-all,disable-all,converge}
arca agent model {status,assign,unassign,enable,disable,use,delivery,converge}
arca agent skill {status,assign,unassign,enable,disable,converge}
arca agent launch {show,configure,reset}
arca settings {show,terminal,locale,pins}
arca mcp icon {list,set,import,reset}
arca network proxy {show,set,clear}
arca discover [mcp|model|skill]
arca status --agent <id> [--agent <id>] [--capability mcp|model|skill]
arca operation {apply,review} --file request.json
arca trace {list,show}
arca capture {list,show}
arca workspace
arca upgrade
```

既有脚本仍可使用旧命令，例如 `arca model use PROFILE_ID --agent codex`；新旧入口的 JSON 输出一致。

三个资产域使用相同的命令术语：

| 命令 | 语义 |
|---|---|
| `list` / `show` | 查询中央资产 |
| `status` | 对照 desired / observed，显示 ownership、启停/current、状态和可用动作 |
| `assign` | 只新增指定 Agent 的 desired relationship，不删除其他关系 |
| `unassign` | 只移除指定关系，不删除中央资产 |
| `enable` / `disable` | 保留关系，只改变 desired 启停状态 |
| `converge` | 对一个准确 observation 执行采用外部、恢复 Arca 或解除管理 |

域专属命令表达真实差异：MCP 支持中央条目、来源和完整配置导出；Model 支持 Profile、共享 Provider、凭据交付方式与 current；Skill 支持来源解析、安装、导入、更新、删除和修复。这些命令调用 Desktop 使用的 Core 用例。

## Agent 自动化与诊断

用 `arca agent status` 查询一个 Agent 的全部资产，或用 `arca agent <mcp|model|skill> status --agent <id>` 只查一种。顶层 `arca status` 支持同时查询多个 Agent：

```bash
arca agent status codex --json
arca status --agent codex --agent opencode --capability mcp --json
arca trace list --agent codex --limit 20 --json
arca trace show SESSION_ID --json
arca trace show SESSION_ID --event EVENT_ID --revision PAGE_REVISION --json
arca trace show --file /path/to/session.jsonl --json
arca capture list --agent codex --json
arca capture show SESSION_ID --limit 100 --json
arca capture show SESSION_ID --flow FLOW_ID --json
```

Trace 按 Core 的分页和脱敏规则读取，后续页使用 `--cursor`；事件详情必须绑定页面返回的 revision。CLI 每次调用重新建立可信会话定位，不接受把任意路径编码成 session ID。Capture 的 `list/show` 只读已保存记录；`run` 可以记录已有进程，或通过 `--launch` 启动追加 CA 的新实例。`state` 核验会话所属进程的 PID 与启动时间，Desktop 和 CLI 使用同一套 Core 流程。详见 [网络记录](/guide/traces#网络记录与代理)。

Trace JSONL 每次读取会重建必要的凭据脱敏上下文。上下文与本次读取合计最多 32 MiB / 100000 条记录；损坏、未写完整或超限时会返回错误，避免在上下文缺失时输出正文。大文件可在 Agent 中导出较小的独立会话再读取。

普通成功 JSON 附带 `backend.status` 和经过脱敏的启动警告阶段。可恢复错误保留合法的 `retry_at` 和 Skill 风险确认 hash，原始 parser 内容、路径及未知 details 仍不输出。Agent 安装状态区分 `runtime_detected`（运行时检测，未知为 null）与 `config_detected`（配置已存在）；插件宿主可启动并不代表插件已安装。

### 审阅并执行同一份计划

`--dry-run` 继续表示一次性预览。需要把计划交给人或另一个 Agent 审阅，再执行原计划时，使用保持进程存活的会话：

```bash
arca operation review --file request.json --json
```

`request.json` 使用 Core 的 `PlanOperationRequest`，例如：

```json
{
  "operation": "set_mcp_enabled",
  "request": {
    "agent_id": "opencode",
    "asset_key": "github::stdio",
    "enabled": true
  }
}
```

有变更时，stdout 先输出一行 `phase: "review"` 的 JSON，其中 `data.plan` 是脱敏计划，`data.commit_decision` 是绑定当前 operation ID、candidate hash 和必要 findings hash 的完整确认模板。调用者完成审阅后，将该模板作为一行 JSON 写回同一进程的 stdin；或者写入 `{"action":"cancel"}`。输入 EOF 也会取消。确认内容不匹配、目标发生变化时，操作拒绝执行。

stdout 的中间计划与最终结果均为单行 NDJSON；最终结果带 `phase: "complete"`。发生错误时 stderr 返回一个 JSON 错误文档，并以非零状态退出。`--file -`、`--yes`、`--dry-run`、`--accept-risk` 不适用于此会话：stdin 留给决定，风险确认由返回模板中的精确 findings hash 表达。没有变更时直接返回 complete，不等待决定。

计划和秘密草稿留在原进程中，不为交接而写入明文凭据。退出进程会结束会话；已有 Skills 计划仍受其 24 小时过期规则约束，长时间等待后应重新审阅。若已有执行授权，可使用 `arca operation apply --file request.json --yes --json`，它复用普通命令的确认与风险检查。

## 外部变更与收敛

Agent 文件和用户级 Skill 目录是合法输入源。用户在外部新增、删除、启停或修改配置不会让 Arca 进入迁移冲突；下一次状态查询会重新扫描，Desktop 也会实时刷新。

```bash
arca agent mcp converge github::stdio --agent claude-code adopt
arca agent mcp converge github::stdio --agent claude-code restore
arca agent mcp converge github::stdio --agent claude-code detach

arca agent model converge external-<candidate-id> --agent codex adopt
arca agent model converge work --agent codex restore
arca agent model converge work --agent codex detach

arca agent skill converge review-changes --agent codex adopt
arca agent skill converge review-changes --agent codex restore
arca agent skill converge review-changes --agent codex detach
```

| 动作 | 含义 |
|---|---|
| `adopt` | 采用当前 Agent 现场，更新中央资产或 desired state；原现场字节保持不变 |
| `restore` | 只把选中的准确关系恢复成 Arca desired state |
| `detach` | 解除 Arca ownership；漂移或外部内容保留，并重新显示为外部观测 |

每个 convergence 计划都由 Core 绑定规划时的完整 inventory revision。计划生成后 Core 会再扫描一次，提交时还会校验 candidate hash 与目标快照；现场变化会返回 `observation_stale`。`status` 的范围快照 revision 只标识该范围的读取结果，不能用来替代完整写入计划的 revision。

旧 `reapply`、顶层 `adopt` 和 `migration review/resolve` 命令已删除。明确的 convergence action 就是收敛意图，不再使用第二套漂移确认令牌。

## 状态含义

| 状态 | 含义 |
|---|---|
| `synced` | desired 与 observed 一致 |
| `external-added` | Agent 中新增、Arca 尚未管理 |
| `external-changed` | 受管字段、启停或 current 被外部修改 |
| `external-removed` | desired 仍在，但 Agent 目标已删除 |
| `unparseable` | 单个现场目标无法解析 |
| `ambiguous` | 单个现场目标存在身份或值歧义 |
| `unsupported` | 现场状态无法无损接管或表达 |

后三类状态只隔离对应资产。普通外部修改也只影响对应关系；MCP、Model 或 Skill 的单点错误不会锁住其他域。只有共享 settings 或未完成事务无法安全恢复时，整个 workspace 才进入只读恢复。

JSON 状态中的 `capability_errors` 表示能力域局部不可用；`recovery_error` 只表示共享事务恢复边界。前者不会阻止其他域的查询或 mutation。

## 稳定 ID 与 Agent 选择

所有 mutation 使用准确稳定 ID，不按显示名模糊匹配：

| 域 | ID | 示例 |
|---|---|---|
| MCP | `name::transport` | `github::stdio` |
| Model | Profile ID 或 `status` 返回的 external ID | `work` |
| Skill | 中央 Skill 名称 | `review-changes` |
| Agent | Agent ID | `claude-code`、`codex` |

关系命令必须显式带一个 `--agent <id>`。`assign` / `unassign` 可在一次命令中处理多个准确资产 ID；`enable` / `disable` / `use` / `converge` 一次处理一个。
同一 Agent 的同名 Skill 若出现在多个受管目录，`skill enable/disable` 还需用 `--target <target_id>` 指明 `status` 中的物理目标，防止开关命中另一份副本；单一目标可省略。
MCP 的 `unassign --all` 审阅后清空该 Agent 的受管项、停用快照和外部项。Model 也支持 `unassign --all`，按 Core 声明的 storage authority 清空；原生注册表 Agent 包括外部模型。中央资产、Provider 和凭据保留；共享物理配置冲突由 Core 拒绝。

```bash
arca agent mcp assign github::stdio filesystem::stdio --agent claude-code
arca agent mcp unassign --all --agent qoder
arca agent skill unassign source-explainer --agent codex
arca agent model assign work backup --agent pi
arca agent model use work --agent pi
```

同名 `github::stdio` 与 `github::http` 是两个资产，必须分别指定。

## MCP

```bash
arca mcp list
arca mcp show github::stdio
arca agent mcp status --agent claude-code
arca agent mcp assign github::stdio --agent claude-code
arca agent mcp disable github::stdio --agent claude-code
```

中央 MCP 专属操作：

```bash
arca mcp add github::stdio --command npx --arg -y --arg @example/server
arca mcp add docs::http --url https://mcp.example.com --http-type streamable-http
arca mcp save --file mcp.json --key github::stdio --dry-run
arca mcp delete github::stdio
arca mcp export
arca mcp export --out mcp.json --yes
```

`export --out` 只创建权限为 `0600` 的新文件，拒绝覆盖已有目标。stdout 导出按命令定义包含完整 MCP 配置，其他 JSON 投影保持脱敏。

MCP 来源管理：

```bash
arca mcp source list
arca mcp source subscribe --url https://example.com/mcp.json --name work --yes
arca mcp source add-local --path /path/to/mcp.json --yes
arca mcp source add-builtin --yes
arca mcp source refresh <source-id> --yes
arca mcp source disable <source-id> --yes
arca mcp source remove <source-id> --yes
```

`manual` / `discovered` 是自动管理来源，Core 拒绝通过来源删除或刷新接口改写它们。重新发现外部 MCP 使用 `arca discover mcp`。有消费关系的来源变更仍受 Core 的影响校验约束。

## Model

```bash
arca model list
arca model show work
arca agent model status --agent pi
arca agent model assign work backup --agent pi
arca agent model assign work --agent claude-code --replace
arca agent model disable backup --agent pi
arca agent model use work --agent pi
```

多模型 Agent 可保留多个已分配 Profile，但最多一个 current；单模型 Agent 由 capability 在计划阶段约束。重复 `assign`、`enable` 或 `use` 是 desired-state 幂等操作，不会借机覆盖外部漂移；需要收敛时使用准确的 `converge`。

外部 Model 按 candidate identity 分别显示。凭据无法安全接管或身份存在歧义的候选会显示 `unsupported` / `ambiguous`。

创建 Provider 后再创建引用它的 Model Profile：

```json
{
  "name": "Work",
  "provider": "custom",
  "base_url": "https://api.example.com/v1",
  "protocols": {"openai-completions": {"endpoint_path": "/chat/completions"}},
  "auth_requirement": "required"
}
```

将上面的文档保存为 `provider.json`。Arca 会生成 Provider 的 ID。先保存 Provider，再运行 `arca model provider list --json`，把返回的准确 ID 填入 `model.json` 的 `provider_id`。

```json
{
  "name": "Work model",
  "provider_id": "PASTE_PROVIDER_ID",
  "protocol": "openai-completions",
  "model": "your-model-id"
}
```

```bash
arca model provider templates
# Bash 或 Zsh，WORK_API_KEY 已由当前 shell 的运行环境提供。
builtin printf '%s' "$WORK_API_KEY" |
  arca model provider save --file provider.json --store keychain --credential-stdin --yes
arca model provider list
arca model provider catalog work-provider
arca model save --file model.json --yes
arca model save --file model.json --id work --dry-run
arca agent model delivery env --agent qoder-cli --profile work --dry-run
arca agent model unassign --all --agent qoder-cli --dry-run
arca model delete work --dry-run
arca model provider delete work-provider --dry-run
arca model import <candidate-id> --dry-run
```

`save` 默认创建记录并由 Arca 生成 ID。创建和编辑时都可以省略 JSON 里的 `id`。编辑时用 `--id` 指定已有记录，原 ID 会保留。示例中的 `work`、`backup`、`work-provider` 代表已有资产 ID，可通过 `list` 或 `status` 获取。Profile 引用已存在的 Provider，连接地址和凭据归 Provider 管理。`provider show` 输出脱敏概览，不能直接充当完整编辑文档。`import` 的 candidate ID 来自 `arca discover model`，Core 会绑定当前 fingerprint。

`arca model provider catalog <provider-id>` 查询该连接可用的模型。`arca model list` 查看已经保存到 Arca 的 Model Profile。旧写法 `arca model provider models <provider-id>` 仍可使用，两种写法的 JSON 输出保持一致。

`--store` 指定新 Key 的存储 ID，与 `--credential-stdin` 一起使用。JSON 从文件读取，Key 从管道读取。编辑时省略这两个参数即可保留原凭据。要更换存储，请向新存储提供新 Key。允许无鉴权访问的 Provider 可通过 `--clear-credential` 显式移除 Key。`delivery plaintext` 仅用于 Core 支持的 Agent，还需 `--confirm-plaintext`。

`arca agent list --json` 现在包含 `storage_authority` 和 `supports_global_selection`。例如 Qoder Desktop 在会话里选模型，Qoder CLI 可全局选模型；多模型能力不等于全局 current 能力。

### 凭据存储

`keychain` 是内置的 macOS Keychain 存储 ID。要注册 1Password，请给服务账号授予目标 Vault 的读取和写入权限，把 token 注入运行 CLI 的环境，并将以下连接配置保存为 `store.json`。填写实际的 `op` 路径和 26 字符 Vault ID。[1Password 服务账号设置](/guide/models#凭据存储)

```json
{
  "id": "work-vault",
  "name": "Work Vault",
  "backend": {
    "kind": "one-password",
    "command": "/opt/homebrew/bin/op",
    "vault_id": "REPLACE_WITH_VAULT_ID",
    "token_env": "OP_SERVICE_ACCOUNT_TOKEN"
  }
}
```

```bash
arca model store list
arca model store save --file store.json --yes
arca model store check work-vault
arca model store delete work-vault --dry-run
```

`check` 验证当前进程能访问目标 Vault。注册后，保存 Provider 时可用 `--store work-vault`。给已有连接改名时用原 ID 和原 backend 配置再次保存。仍被 Provider 引用的连接需先解除引用，之后才能移除。

Bitwarden 和 Vaultwarden 共用 `backend.kind = "bitwarden"`。将 `bw` 路径填入 `command`，并提供 `server_url`、`appdata_dir`、`user_id`、`folder_id` 和 `session_env`。准备好会话后，同样使用 `model store save` 和 `model store check`。[Bitwarden 和 Vaultwarden 接入](/guide/models#bitwarden-和-vaultwarden)

其他连接也用相同的保存和检查命令。JSON 文件仅填写连接配置和认证环境变量名。[各密钥库接入](/guide/models#凭据存储)

| `backend.kind` | 必填字段 | 可选字段 |
| --- | --- | --- |
| `keepassxc` | `command`、`database_path`、`password_env` | `key_file` |
| `passbolt` | `command`、`server_url`、`user_id`、`private_key_path`、`server_fingerprint`、`password_env` | `folder_id` |
| `openbao` | `server_url`、`mount`、`prefix`、`token_env` | |

## Skill

```bash
arca skill list
arca skill show review-changes
arca agent skill status --agent codex
arca agent skill assign review-changes source-explainer --agent codex
arca agent skill disable review-changes --agent codex
```

Skill relationship 把 `~/.arca/assets/skills/items/` 中央副本链接到已核验用户级 target。多个 Agent 可能共用同一物理目录，因此计划会列出完整 `affected_agent_ids`。`restore` 只重建可证明安全的受管链接；外部目录、普通文件或异向链接不会被覆盖。`detach` 会保留这些外部内容。

Skill 中央生命周期：

```bash
arca skill inspect-source --github owner/repo
arca skill inspect-source --path /path/to/skills
arca skill inspect-source --archive /path/to/skills.zip
arca skill install --github owner/repo --name review-changes --dry-run
arca skill install --github owner/repo --name review-changes --yes
arca skill import <external-identity> --dry-run
arca skill check-updates --yes
arca skill update review-changes --dry-run
arca skill repair review-changes --dry-run
arca skill repair review-changes --target <target-id> --dry-run
arca skill remove review-changes --dry-run
```

先用 `inspect-source` 获得准确候选名称，再用一个或多个 `--name` 明确选择安装项。来源只能指定 GitHub、本地目录、压缩包中的一种。解析暂存会在查询结束、取消或失败后清理；安装和外部导入先进入中央库，分配 Agent 使用单独的 `assign`。

存在本地修改或冲突时，只有明确的 `--replace-local-changes` / `--replace-conflicts` 才允许生成对应替换计划。高风险计划先用 `--dry-run` 阅读 findings；决定接受后，在提交命令中额外传 `--accept-risk`，Core 仍以本次实际计划的 findings hash 校验。普通 `--yes` 不再自动同意风险。

## Agent 与网络设置

```bash
arca agent save my-agent --file agent.json --dry-run
arca agent configure codex --file agent-patch.json --dry-run
arca network proxy show
arca network proxy set socks5://127.0.0.1:7890 --yes
arca network proxy clear --yes
```

`agent save` 接收 Core 的 `AgentDefinition`，修改已有 ID 还需 `--replace`。`agent configure` 接收 `AgentConfigurationPatch`，仅修改文档中出现的能力，例如 `{"model":{"paths":["~/.codex/config.toml"]}}`。Core 校验路径、共享目录影响和已消费资源；CLI 不自行拼 Agent 文件。代理配置由 CLI 与 Desktop 共用，不接受 URL 中的凭据。

## 只读发现

```bash
arca discover
arca discover mcp
arca discover model
arca discover skill
```

`discover` 只列出外部观测和候选详情，不创建 ownership、不写 Agent 配置。需要改变归属时，从 `status` 取得准确 asset ID 后执行 `converge`。

## 通用选项

| 选项 | 作用 |
|---|---|
| `--json` | 输出稳定 JSON envelope；mutation 仍必须同时选择 `--yes` 或 `--dry-run` |
| `--yes` | 跳过交互提示，但不绕过安全校验或自动同意 Skill 风险 |
| `--accept-risk` | 明确接受当前 Skill 计划的风险 findings；仍需正常提交确认 |
| `--dry-run` | 生成并展示计划，随后取消，不提交当前 mutation |
| `--no-color` | 关闭 ANSI 颜色 |

`--yes` 与 `--dry-run` 互斥。传给纯查询命令会报错，避免脚本把无效参数当成已确认行为。

## Workspace 与 JSON

```bash
arca workspace
arca --json workspace
```

`workspace` 返回统一 revision、Agent capability、中央资产、desired relationships、observed inventory 与每项 `available_actions`。JSON 成功 envelope 示例：

```json
{
  "schema_version": 1,
  "ok": true,
  "command": "model.status",
  "changed": false,
  "data": {}
}
```

失败写 stderr，并包含稳定的 `error.code` 与脱敏 details。若中央配置已保存而部分 Agent 尚未收敛，退出码仍为 1，`error.code` 是 `pending_convergence`，`error.details` 包含 `changed: true`、`operation_id` 和本次操作的 `target_incidents`。先查看对应域的 `status`，再使用列出的恢复动作，不能把它当作“什么都没保存”直接重复创建资产。API key、token、原始配置值和绝对私有路径不会进入普通状态 JSON。

## TUI 键位

无参数 TUI 目前聚焦 MCP：

| 键 | 作用 |
|---|---|
| `1` / `2` / `3` | Registry / Sources / Agents |
| `↑` `↓` 或 `j` `k` | 移动 |
| `/` | 搜索 |
| `i` / `a` | 安装或给 Agent 添加 MCP |
| `Space` | 启停来源、Agent 或 MCP |
| `d` | 删除中央 MCP 或解除 Agent 关系 |
| `Ctrl-R` | 重新扫描 |
| `?` | 帮助 |
| `q` | 退出 |

资源变更调用同一个 Core planner；来源注册、Agent 定义和代理偏好使用共享 Core 用例。后者的 `--dry-run` 展示操作意图，不会进行远程刷新或声称已完成写入时才执行的完整校验。CLI 无参数 TUI 仍聚焦 MCP；图标、固定 Agent、语言、桌面启动、文件选择和凭据显示／校验界面留在 Desktop。

## 更新

```bash
arca upgrade
```

独立下载或 `cargo install` 的 CLI 可用 `arca upgrade` 跟随 Stable；Desktop 内置 CLI 随 App 更新。设置 `ARCA_NO_UPDATE_CHECK=1` 可关闭普通命令后的每日版本检查。

下一步 → [支持的 Agent](/guide/agents)


## 启动 Agent 与共享设置

CLI 和 Desktop 读取同一套启动设置，不需要分别配置：

```bash
arca agent launch show opencode
arca agent launch configure opencode --file launch.json --default-directory ~/Code --yes
arca agent run opencode --directory ~/Code --yes
arca agent run opencode --dry-run --json
arca agent launch reset opencode --yes
arca settings terminal
arca settings terminal ghostty --yes
arca settings locale zh-CN --yes
arca settings pins codex opencode --yes
```

`launch.json` 使用 `app`、`cli` 或 `web` 类型，例如：

```json
{"kind":"app","path":"/Applications/ZCode.app","args":[],"env":{"NODE_USE_SYSTEM_CA":"1"},"new_instance":true}
```

仅设置目录时可以省略 `--file`；`--default-directory ''` 清除固定目录，省略此参数则保留原设置。`launch reset` 恢复自动检测。参数和环境变量通过文件或 `--file -` 输入；查询仅显示参数数量与变量名称。

## 复制请求与官方文档

```bash
arca model provider docs openai
arca model curl PROFILE_ID
arca model curl PROFILE_ID --include-api-key
```

cURL 默认使用 `$ARCA_API_KEY` 占位；`--include-api-key` 明确导出配置的凭据。无需认证的模型不添加认证请求头。Desktop 的复制操作和 CLI 共用同一协议、URL、JSON 与 shell 转义实现；命令只生成文本，不会发送请求。

## 批量开关与图标

```bash
arca agent mcp disable-all --agent opencode --yes
arca agent mcp enable-all --agent opencode --yes
arca mcp icon list
arca mcp icon set 'filesystem::stdio' files --yes
arca mcp icon import 'filesystem::stdio' --file ./icon.png --yes
arca mcp icon reset 'filesystem::stdio' --yes
```

批量开关保留分配关系，复用 Desktop 同一份 core 计划与提交。图标导入支持 PNG、JPEG、WebP，最大 1 MiB。
