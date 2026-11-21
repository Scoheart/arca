# Provider 协议、凭据存储和 Agent 交付

在选择或排查协议、维护密钥存储，或核对目标 Agent 如何得到 Key 时读此页。基础创建与分配见 [Provider 与模型手册](../workflows/models.md)。

## Provider 与 Profile 字段

| 对象 | 主要字段 | 可选或版本相关字段 |
| --- | --- | --- |
| Provider 草稿 | `name`、`provider`、`base_url`、`protocols`、`auth_requirement` | `model_catalog_url`、`portal_url`，保存时由 Arca 生成 ID |
| Model Profile 草稿 | `name`、`provider_id`、`protocol`、`model` | `model_vendor`、`context_window`、`max_output_tokens`、`reasoning`。适配器还可使用 `env_key` |
| `protocols` 的一个项目 | 键是协议 ID，值为 `{"endpoint_path":"/path"}` | 依该上游连接的实际契约。Model 必须使用 Provider 已启用的协议 |

Provider 的 `provider` 表示访问渠道，`model_vendor` 可表达模型开发商。不同账号、区域与 Base URL 分别建立连接。认证要求是 `required`、`optional` 或 `none`。Key 通过保存命令的专用通路进入所选存储。编辑时用 `--id` 命中现有记录，表单中的协议与连接字段按任务保留。

4.0.2 的 Core 声明以下中央协议和默认请求路径。上游与 Agent 支持分别核对，特别是原生客户端对自定义路径及认证形式的限制。

| 协议 ID | 默认 Endpoint Path |
| --- | --- |
| `anthropic-messages` | `/v1/messages` |
| `openai-responses` | `/responses` |
| `openai-completions` | `/chat/completions` |
| `gemini-generate-content` | `/models/{model}:generateContent` |
| `gemini-interactions` | `/interactions` |
| `aws-bedrock-converse` | `/model/{model}/converse` |

只有一个 Base URL。Endpoint Path 从其后追加，应检查是否重复 `/v1` 等路径段。`{model}` 保留给支持按路径展开模型名的协议。服务商实际模板可用 `arca model provider templates --json` 查看。仍有 Model 使用某协议时，先处理引用再移除协议。[4.0.2 协议定义](https://github.com/Scoheart/arca/blob/v4.0.2/data/model-protocols.json) · [Provider 与 Profile 类型](https://github.com/Scoheart/arca/blob/v4.0.2/core/src/domain/types.rs)

## 选择和检查凭据存储

macOS 内置存储 ID 为 `keychain`。其他连接可在 GUI **Models → 凭据存储 → 添加连接** 注册，或用 `arca model store save --file store.json`。注册 JSON 包含 `id`、`name` 和 `backend`，在 backend 中用 `kind` 区分连接。

| `backend.kind` | 所需字段与环境 |
| --- | --- |
| `keychain` | macOS 内置。Provider 直接选 `keychain` |
| `one-password` | 官方 `op` 的绝对路径 `command`、Vault 的 `vault_id`、`token_env`。服务账号需对目标 Vault 有读写权限 |
| `bitwarden` | Bitwarden/Vaultwarden 共用。`command`、`server_url`、`appdata_dir`、`user_id`、`folder_id`、`session_env`。核对 CLI profile 和已解锁会话 |
| `keepassxc` | `command`、`database_path`、`password_env`，可选 `key_file`。目标为现有 KDBX |
| `passbolt` | `command`、`server_url`、`user_id`、`private_key_path`、`server_fingerprint`、`password_env`，可选 `folder_id`。`command` 使用 Arca 的 Passbolt helper |
| `openbao` | HTTPS `server_url`、KV v2 `mount`、专用 `prefix`、`token_env`。按既有服务策略确认该范围权限 |

`store.json` 示例。填写实际绝对路径和 26 字符 Vault ID，环境变量名与运行环境相符。

```json
{
  "id": "work-vault",
  "name": "Work Vault",
  "backend": {
    "kind": "one-password",
    "command": "/absolute/path/to/op",
    "vault_id": "REPLACE_WITH_VAULT_ID",
    "token_env": "OP_SERVICE_ACCOUNT_TOKEN"
  }
}
```

```bash
arca model store list --json
arca model store save --file store.json --dry-run --json
arca model store save --file store.json --yes --json
arca model store check "$STORE_ID" --json
```

存储表单里填连接参数和认证变量**名**。token、会话、密码与私钥由对应受保护的入口提供。可复用同一存储连接给多个 Provider。`check` 会在当前运行进程访问后端，GUI 启动环境与 CLI shell 可能不同。变更名称使用同一存储 ID，删除前先处理仍引用该存储的 Provider。

### 保存或轮换 Key

- 新建/换存储或换 Key 时，使用受授权的管道给 `model provider save --file provider.json --store STORE_ID --credential-stdin`。草稿走具名文件，stdin 给 Key。空 stdin 会失败，支持的无鉴权连接可用 `--clear-credential` 显式移除。
- 编辑其余字段时省略这两个凭据选项保留现有凭据。GUI 的对应做法是在编辑表单里保留现有存储、留空 Key。
- 轮换后先验使用该 Provider 的目标 Agent 和实际 delivery。用 `arca model store revisions --json` 查看留存元数据。仅在旧 revision 已停用且不再交付、用户明确要退役时，用 `arca model store retire REVISION --dry-run --json` 审阅，再执行获授权的退役。
- 信息只报告存储 ID、可访问性、关联和轮换状态。避免回显值或把敏感输入带进捕获、命令跟踪、配置样例与 Git。

[4.0.2 各凭据后端接入](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/models.md#凭据存储) · [凭据后端类型](https://github.com/Scoheart/arca/blob/v4.0.2/core/src/domain/credentials.rs)

## Agent 凭据交付

存储可访问、Profile 已启用、Agent 读到了有效 Key 是三项独立验证。当前客户端支持的方式以 Core 计划和 Agent 的配置能力为准。

| 方式 | 重点核对 |
| --- | --- |
| `auto` | 检查 Core 为当前 Agent 和 Profile 解析出的方式 |
| `env` | 配置引用变量名。Arca 可为支持的 CLI 启动注入，手动启动时由实际启动环境提供 |
| `command` | Agent 调用受支持的本地凭据解析命令。确认同一安装、工作区及目标 Profile 可读取 |
| `agent-store` | 使用支持的客户端原生储存接口，按能力和本次计划确认 |
| `plaintext` | 只有 Core 支持的特定私有配置路径适用，需用户明确审阅该方式，随后使用 `--confirm-plaintext`。检查受保护写入与实际目标 |

```bash
arca agent model status --agent "$AGENT" --json
arca agent model delivery env --agent "$AGENT" --profile "$PROFILE_ID" --dry-run --json
```

`--profile` 将交付方式限定到这一现有分配。完成后重查 Agent 状态和实际启动环境。应用或 helper 的本地环境无法证明另一个客户端能读取相同后端。凭据 resolver 的 stdout 可含秘密，交给它被设计服务的客户端消费，通过状态与实际授权请求诊断。

## 请求预览和故障定位

- `arca model curl "$PROFILE_ID"` 生成基于实际 Provider、协议和 Model 的 cURL 文本。需鉴权时默认用 `$ARCA_API_KEY` 占位。生成文本与实际联网执行是两步。普通排查使用占位版本，包含真实 Key 的输出需专门授权和安全目的地。
- GUI **试验台 → 请求预览** 可核对目标 URL、认证头名称与请求内容。用当前有效请求验证连接时，只发送用户授权的提示词，限制目标和成本。
- 可保存连接而远程调用失败时，依次查 endpoint、模型 ID、鉴权方式、存储读取以及网络路径。同一个 Key 能正常查模型列表，只能证明该目录路径的权限。
- GUI 正常、Agent 401 时优先验证实际启动方式和读取者。被 CLI 写入计划拒绝时先查询 Agent 的自定义路由、原生格式与交付约束。[诊断手册](../workflows/diagnostics.md)

[Arca 4.0.2 交付方式](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/models.md#agent-凭据交付)
