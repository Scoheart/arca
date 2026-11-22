# Models 与 Providers

Provider 定义一套 API 连接，Model 引用它并指定协议与 Model ID。一份 Provider 可供多个模型使用，中央 Model 又可分配给多个兼容 Agent。

## 添加连接与模型

1. 在 **Models → 添加 Provider** 中选择模板或自定义连接。
2. 填写名称和 Base URL。需要 API Key 时选好 **凭据存储**，再输入 Key。
3. **服务商入口 URL** 可修改；右侧按钮打开密钥页、控制台或配置指南。不同地区与 Coding Plan 使用对应入口。
4. 根据需要填写 **Models 列表 URL**，启用协议并编辑 **Endpoint Path**。这些字段直接展示；完整请求 URL 随输入更新。
5. 保存 Provider，再点击 **添加 Models**，选 Provider、协议与准确的 Model ID。上下文和最大输出等设置按实际模型填写。
6. 进入 Agent 的 Models 页，添加中央模型并选择当前模型；需要时重启客户端或新建会话。


Provider 使用一个 Base URL。不同协议可以有不同的相对 Endpoint Path；不同域名或账号应创建独立 Provider。例如 `https://gateway.example.com/v1` 与 `/responses` 对应 `https://gateway.example.com/v1/responses`。

仍有 Model 使用某协议时，Arca 会阻止停用该协议。若客户端无法无损表达自定义请求路径，Arca 会拒绝该关系，不会忽略你的 Path。

## 模型试验台

在桌面版 **Models → 模型试验台** 点击打开浏览器，或运行 `arca model playground`。在右侧 **运行设置** 为每条请求选择已保存的模型与参数，在底部输入共同提示词，一次最多并行运行六条请求。不同 Provider、不同协议可以混用。用 **复制此项** 保留同一个模型，再调整参数进行对照。

留空的参数使用该服务的默认值。高级 JSON 支持协议原生的参数，例如 Anthropic 的 `thinking` 或 Gemini Interactions 的 `generation_config`。结果区分别展示回答、耗时、服务返回的 Token 用量和错误。试验台使用当前电脑上的 Arca 连接，参数只作用于本次请求。终端启动时按 Ctrl+C 关闭面板服务。

- **返回方式**：逐步显示会在收到文本时更新回答，并记录首段延迟。网关或模型不支持流式时，可选完整返回。每条请求都可以单独停止等待和重试。
- **请求预览**：发送前可查看当前请求的目标地址、认证请求头名称和协议 JSON。预览无需读取凭据。
- **运行记录**：修改参数后，上一轮的参数和回答仍可查看。记录保留在当前浏览器标签页，可以载入本轮设置或导出 JSON。

## 支持的协议

| 协议 | 常见默认路径 |
|---|---|
| Anthropic Messages | `/v1/messages`，具体以 Provider 模板为准 |
| OpenAI Responses | `/responses` |
| OpenAI Chat Completions | `/chat/completions` |
| Gemini GenerateContent | `/models/{model}:generateContent` |

协议兼容并不代表任意模型都能运行。准确的 Model ID、模型能力、账号权限和客户端要求仍由服务商与 Agent 决定。

## Agent 支持与当前模型

下表由发布版 Core 能力导出。名称与身份取自 Arca 核验目录，CLI 与 Desktop 形态分别列出。

<AgentReference models-only />

**已添加、已启用、当前模型**是不同状态。原生多模型 Agent 可以保留多个模型；单模型 Agent 最多一个。停用或移除当前模型时，计划会展示影响和可用的后续选择。

- **Codex CLI 与桌面入口**共用用户配置；Model writer 在 `codex` 上管理同一配置。
- **OpenCode CLI 与 Desktop**共用原生配置，可从对应入口管理 Models。
- **Qoder IDE、Qoder Desktop、Qoder CLI**分别保留身份。IDE 的 Models 是官方引导；Desktop 0.1.8+ 与 CLI 1.1.50+ 可以自动配置自定义端点。
- **Qoder Desktop**在会话里选用模型；**Qoder CLI**支持全局当前模型。它们共用 settings 文件，但有独立的 Arca Provider 身份；共享文件仍有其他消费关系时，不能清空整个库。

## 凭据存储

在 **Models → 凭据存储** 管理可供多个 Provider 使用的连接。macOS Keychain 是内置选项。添加 1Password 时，先安装官方 `op` CLI，并为服务账号授予所选 Vault 的读取和写入权限。[1Password 服务账号设置](https://www.1password.dev/service-accounts/get-started)

在 **添加连接 → 1Password** 中输入连接名称、`op` 绝对路径、Vault ID 和 token 环境变量名。默认变量为 `OP_SERVICE_ACCOUNT_TOKEN`。将 token 注入实际读取 Vault 的进程环境，包括 Arca Desktop、CLI 或 Agent 的凭据读取进程。保存后点击 **检查连接**，确认该环境可以访问所选 Vault。可重命名已注册的连接，移除连接前需先处理仍引用它的 Provider。[1Password CLI 服务账号用法](https://www.1password.dev/service-accounts/use-with-1password-cli)

### Bitwarden 和 Vaultwarden

在 **添加连接 → Bitwarden / Vaultwarden** 中选择服务器上的个人文件夹。先在客户端创建用于 Arca 的文件夹，再为官方 `bw` CLI 配置独立的数据目录。在该目录对应的 CLI profile 中登录目标服务器，并用 `bw unlock --raw` 获取会话。以下命令初始化配置并查找账号和文件夹的 ID。

```bash
export BITWARDENCLI_APPDATA_DIR="$HOME/.config/arca-bitwarden-cli"
mkdir -p -m 700 "$BITWARDENCLI_APPDATA_DIR"
bw config server https://vault.example.com
bw login
export BW_SESSION="$(bw unlock --raw)"
bw status | jq -r '.userId'
bw get folder Arca | jq -r '.id'
```

表单需要 `bw` 绝对路径、服务器 URL、CLI 数据目录、账号 ID 和文件夹 ID。会话变量默认用 `BW_SESSION`。把会话传入实际运行 Arca 或其凭据命令的进程环境，保存后检查连接。Arca 在指定个人文件夹内用安全备注的隐藏字段保存 Key，轮换产生新条目。退役的条目移入回收站。[Bitwarden CLI 文档](https://bitwarden.com/help/cli/)

### NordPass

通过官方 `np` CLI 登录并解锁账号，创建一个专供 Arca 使用的个人文件夹。用 `np whoami --format json` 和 `np folder list --type personal --format json` 核对当前账号与文件夹 ID。保存 `nordpass` 连接时填写 `np` 的绝对路径、登录邮箱和个人文件夹 ID。CLI 支持个人免费计划。[NordPass CLI 使用说明](https://support.nordpass.com/hc/en-us/articles/51095299365137-How-to-use-NordPass-CLI) · [适用计划](https://nordpass.com/blog/command-line-interface-feature/)

Arca 用提供商名称创建密码条目，并把 Key 写入密码字段。

### Proton Pass

安装官方 `pass-cli` 2.6.0 或更新版本，选择权限为 `0700` 的专用会话目录。用该目录和系统钥匙串登录，在相同环境下查询账号 ID 和保险库的 Share ID。CLI 需要含相应功能的付费计划。[Proton Pass CLI 配置](https://protonpass.github.io/pass-cli/get-started/configuration/) · [计划说明](https://proton.me/pass/developer-features)

```bash
proton_session_dir="$HOME/.config/arca-proton-pass"
mkdir -p -m 700 "$proton_session_dir"
export PROTON_PASS_SESSION_DIR="$proton_session_dir"
export PROTON_PASS_KEY_PROVIDER=keyring
pass-cli login
pass-cli info --output json
pass-cli share list --only-vaults true --output json
```

保存 `proton-pass` 连接时填写 `pass-cli` 的绝对路径、该会话目录、账号 ID 和目标 Share ID。目标保险库需允许写入。Arca 把 Key 写入安全笔记的隐藏字段。笔记名称包含提供商名和版本标记，用于在保存中断后找回同一条笔记。

### Keeper Secrets Manager

安装官方 `ksm` CLI。使用启用了 Secrets Manager 的 Keeper Business 账号，给应用授权一个专用共享文件夹的编辑权限，并注册对应客户端。保存 `keeper` 连接时填写 `ksm` 的绝对路径、共享文件夹 UID、客户端 ID 和配置环境变量名。通过启动环境提供已注册客户端的 Base64 配置，默认变量是 `KSM_CONFIG`。Arca 用提供商名称创建记录，并把 Key 写入密码字段。[Keeper 接入条件](https://docs.keeper.io/keeperpam/secrets-manager/quick-start-guide) · [Keeper Secrets Manager CLI](https://docs.keeper.io/keeperpam/secrets-manager/secrets-manager-command-line-interface)

### KeePassXC

在 **添加连接 → KeePassXC** 中填写官方 `keepassxc-cli` 的绝对路径和现有 KDBX 数据库路径，数据库上限为 64 MiB。数据库使用密钥文件时一并填写。让运行 Arca 或凭据命令的环境提供数据库密码，默认变量名为 `ARCA_KEEPASSXC_PASSWORD`。

Arca 为每个凭据版本写入独立条目，在保存时检查并发改动，并保存一份加密 KDBX 回滚副本。退役移除该条目的活动记录，启用回收站的数据库仍可从中找回条目。[KeePassXC CLI 手册](https://github.com/keepassxreboot/keepassxc/blob/develop/docs/man/keepassxc-cli.1.adoc)

### Passbolt

在 **添加连接 → Passbolt** 中填写 HTTPS 服务器、账号 ID、账号 OpenPGP 私钥文件、服务端 OpenPGP 指纹和可选的个人文件夹 ID。默认口令变量名为 `ARCA_PASSBOLT_PASSPHRASE`。私钥使用仅文件所有者可访问的普通文件，例如 `0600` 或 `0400`。

先从 Arca 源码构建基于 Passbolt 官方 SDK 的助手，再将其绝对路径填入表单。Go 的版本要求见助手目录的 `go.mod`。

```bash
mkdir -p "$HOME/.local/bin"
cd integrations/passbolt-helper
go build -mod=readonly -trimpath -o "$HOME/.local/bin/arca-passbolt" .
```

Arca 为选定账号创建私有凭据资源，API Key 上限为 4096 个字符。轮换生成新资源，退役只处理经核对的 Arca 条目。[Passbolt 官方 SDK](https://github.com/passbolt/go-passbolt)

### OpenBao

在 **添加连接 → OpenBao** 中填写服务器 HTTPS 地址（域名和可选端口）、现有 KV v2 挂载路径和专供 Arca 使用的前缀。默认 token 环境变量名为 `BAO_TOKEN`。让运行 Arca 或凭据命令的环境提供该 token，并在保存后检查连接。

以下权限以 `secret` 挂载和 `arca/providers` 前缀为例。用所填的挂载路径和前缀替换示例值。

```hcl
path "secret/config" {
  capabilities = ["read"]
}
path "secret/data/arca/providers/*" {
  capabilities = ["create", "read"]
}
path "secret/delete/arca/providers/*" {
  capabilities = ["update"]
}
path "sys/capabilities-self" {
  capabilities = ["update"]
}
```

Arca 用每个凭据的版本 ID 作为独立条目名，只创建新条目。退役软删除对应的版本。[OpenBao KV v2](https://openbao.org/docs/secrets/kv/kv-v2/)

### 给 Provider 选择存储

在 Provider 表单输入 API Key 并选择存储。留空编辑会保留现有凭据，切换存储需提供新的 Key。允许无鉴权访问的 Provider 可显式移除已保存的 Key。

本次更新使用独立的 `~/.arca/assets/models/registry.json`。以前的中央 Model 目录数据和 Keychain 条目保留供手工处理。按需重新建立 Provider 和 Model，再检查 Agent 关联。已有 Agent 配置仍会作为原生观测呈现，可显式采用支持的候选。

## Agent 凭据交付

实际交付方式取决于 Agent 能力。支持读取命令的 Agent 可通过 Arca CLI 解析已启用的 Model 凭据。环境变量方式在 Agent 配置中保存变量名，通过 Arca 启动受支持的 CLI Agent 时，启动进程从 Provider 所选存储读取 Key 并注入环境。自行启动 Agent 时，由启动环境提供相应的值。

对支持私有配置交付的 Agent，Core 使用已核验的文件路径和受保护的回滚。显式选择 plaintext 交付还需审阅，实际写入受 `0600` 权限与原文件版本检查约束。密钥读取失败会阻止依赖该密钥的操作。编辑 Provider 时，Core 分别收敛受影响的 Agent 目标。

## 外部配置与删除

扫描到的外部模型不会自动变成中央资产。采用或导入准确的候选后，再按需要分配；普通分配不会覆盖外部漂移。

删除中央 Model 会先展示消费者，保留其他 Model 和 Provider。删除 Provider 是另一项操作，需处理仍在引用它的 Model。Agent 页的“移除全部”只清理选定 Agent 的模型范围；中央资产和凭据保留，共享存储冲突由 Core 拒绝。

[Provider 模板目录](/guide/providers) · [CLI 创建与审阅](/guide/cli#model)
