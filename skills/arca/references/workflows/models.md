# Provider、模型和试验台

先确定目标层级。Provider 管共享 API 连接及凭据引用，Model Profile 管协议和上游 Model ID，Agent 关系管已添加、已启用与当前模型。查能力后再选择具体交付及选模方式。

## 建立 Provider

1. GUI 进入 **Models → 添加 Provider**，从模板或自定义连接起步。CLI 可先看 `arca model provider templates --json` 及 `arca model provider docs ID --json`。
2. 定好服务商、账号或套餐。填写完整 Base URL、鉴权要求、必要的模型列表 URL 和服务商入口。逐个启用实际协议并填相对 Endpoint Path。例如 `https://gateway.example.test/v1` 和 `/responses` 组成 `https://gateway.example.test/v1/responses`。检查完整 URL 避免重复路径段。
3. 需要 Key 时选凭据存储并安全输入。已有 Provider 留空可保留当前 Key，换存储需提供新 Key。GUI 可以在 **Models → 凭据存储** 找连接和 **检查连接**。
4. 保存后，在对应 Provider 详情核对名称、协议与 URL。本地保存和查询远程目录分别反映配置和联网能力。

CLI 的 `provider.json` 可从下列形状开始。示例域名与模型值用于配置说明。把协议和接口地址换成实际服务保证支持的值。

```json
{
  "name": "Work connection",
  "provider": "custom",
  "base_url": "https://gateway.example.test/v1",
  "protocols": {
    "openai-completions": {"endpoint_path": "/chat/completions"}
  },
  "auth_requirement": "required"
}
```

有认证的 macOS Keychain 示例。`WORK_API_KEY` 由用户授权的运行环境提供，执行环境应关闭 shell 跟踪。每次只选预览或已授权提交之一，Key 通过内建 `printf` 的 stdin 管道传递。

```bash
arca model store list --json
builtin printf '%s' "$WORK_API_KEY" |
  arca model provider save --file provider.json --store keychain \
    --credential-stdin --dry-run --json
builtin printf '%s' "$WORK_API_KEY" |
  arca model provider save --file provider.json --store keychain \
    --credential-stdin --yes --json
arca model provider list --json
arca model provider show "$PROVIDER_ID" --json
```

无需认证的服务按其契约用 `"auth_requirement": "none"`，并省略凭据选项。已有 Provider 用 `save --file provider.json --id "$PROVIDER_ID"`，完整草稿从现有表单或受保护的原始输入取得。`provider show` 是脱敏视图。Provider 名称、Base URL、协议和凭据变化可能影响多个 Model 与多个 Agent，先看消费者及计划。可选字段、后端类型与轮换见 [连接与凭据技术参考](../technical/model-connections.md)。

## 保存 Model Profile

`arca model provider catalog "$PROVIDER_ID" --json` 会用该连接实际请求服务商目录。可从返回值选对应协议可用的 Model ID，也可依据服务商说明手动填。GUI 从 **Models → 添加 Models** 选择 Provider 和协议。

单个 `model.json` 示例。`PASTE_PROVIDER_ID` 使用 `provider list` 返回的准确 ID，模型 ID 使用上游原值。

```json
{
  "name": "Work chat",
  "provider_id": "PASTE_PROVIDER_ID",
  "protocol": "openai-completions",
  "model": "exact-upstream-model-id",
  "context_window": 32768,
  "max_output_tokens": 4096
}
```

```bash
arca model save --file model.json --dry-run --json
arca model save --file model.json --yes --json
arca model list --json
arca model show "$PROFILE_ID" --json
```

量化限制按该模型实际能力填写，可省略。保存时生成中央 Profile ID，编辑加 `--id "$PROFILE_ID"` 保留原身份。`model list` 是已保存 Profile。`provider catalog` 返回服务商能列出的上游模型。已有 Agent 原生模型从 `arca discover model --json` 取候选 ID，用 `arca model import "$CANDIDATE_ID" --dry-run --json` 审阅显式导入；原生身份接管见 [同步与恢复](recovery.md)。

## 给 Agent 使用

```bash
arca agent list --json
arca agent model status --agent "$AGENT" --json
arca agent model assign "$PROFILE_ID" --agent "$AGENT" --dry-run --json
arca agent model enable "$PROFILE_ID" --agent "$AGENT" --dry-run --json
arca agent model use "$PROFILE_ID" --agent "$AGENT" --dry-run --json
```

1. 查该 Agent 的配置能力、协议、存储权限口径和 `supports_global_selection`。GUI 通过 **选择 Agent → Models** 查看支持的动作。
2. 缺少关系时先分配，已有停用关系按需启用。`assign` 默认增量添加。单模型目标满额时，只有用户要求替换整个选集才审阅 `assign --replace`。原来在用的模型纳入影响核对。
3. 用户要求全局当前模型时，确认已启用且 Agent 支持全局选择，再执行 `use`。原生多模型 Agent 可保留多项且最多一个全局 current。仅支持会话选择的客户端在会话里选并验收。
4. 依据目标 Agent 的可用交付方式验证凭据。Arca 启动与手动启动的环境可能不同。保留命令、环境引用或后端的访问证据，避免输出 Key 正文。[Agent 凭据交付](../technical/model-connections.md#agent-凭据交付)
5. 重查域内 status，核对关系、模型、协议、启停与 current。任务要求验证客户端行为时，按客户端加载规则重启或新开会话，再确认模型选择及实际请求。

移除前先判断范围。`agent model disable` 或 `unassign` 改 Agent 关系。`model delete` 审阅中央 Profile 及消费者。`model provider delete` 还需处理正在引用它的 Profile。`unassign --all` 的范围依 `storage_authority`，原生注册表可能包含外部模型。

### Codex 的配置与 401

在 4.0.2，为 Codex 配自定义模型时先确认上游确实支持 OpenAI Responses。把 Provider 的协议换成 `"openai-responses": {"endpoint_path": "/responses"}` 或上游支持的相对路径，并把 Model Profile 的 `protocol` 也选为 `openai-responses`。GUI 通过具体客户端选择器打开 Codex 的 Models 工作区。CLI 用 `codex` 管理桌面和 CLI 共用的原生 Model 配置。

```bash
arca agent model status --agent codex --json
arca agent launch show codex --json
```

如果桌面能返回但手动启动的 CLI 收到 401，先明确桌面请求来自 Arca 试验台还是 Codex 桌面端，再确认手动 CLI 的当前 Profile、原生配置路径和 `wire_api`。4.0.2 的 Codex 写入使用 `wire_api = "responses"`；有 Provider 凭据时配置 `auth.command` 与其参数，由 Codex 调用 Arca helper。核对手动启动进程的 Arca 位置、后端访问环境和实际读取的设置。查看脱敏的 `model_provider`、`base_url`、`wire_api` 与 helper 位置即可定位大多数差异，Key 输出留给客户端消费。[交付检查](../technical/model-connections.md#agent-凭据交付) · [Codex adapter 源码](https://github.com/Scoheart/arca/blob/v4.0.2/core/src/resources/model/mod.rs)

## 发起模型请求

在桌面版选择 **Models → 打开 Arca Web**。在网页配置 Provider 和模型，再检查请求预览。[Arca Web](https://arca-web-delta.vercel.app/)

### 用命令行试验台比较模型

1. 运行 `arca model playground`。仅需本地 URL 时可使用 `--no-open`，浏览器与服务应在能访问同一回环地址的设备上。
2. 在 **运行设置** 添加已保存的模型请求。一次最多六条，可混用协议或复制一项后调整参数。留空参数使用服务默认值。需要协议专用字段时，在高级 JSON 填入该协议的合法参数。
3. 在底部填共同提示词。用 **请求预览** 检查 URL、认证头名称和 JSON。预览可以在读取 Key 或发送请求之前完成。
4. 按本次任务授权发送。选择流式逐步显示或完整返回。比较响应、错误、总耗时、流式首段延迟和服务返回的 Token 用量。区分推理输出、连接问题和服务端参数错误。
5. 需要重试某项时只操作该项。运行记录保存在当前浏览器标签，可按需求加载设置或导出。终端启动的本地服务用 Ctrl+C 结束。

试验台用 Arca 自身连接与当前设置，Agent 可能使用不同的系统环境、代理和协议。通过试验台只能证明该请求路径的结果。需要生成可审阅的请求文本时用 `arca model curl "$PROFILE_ID"`；默认鉴权请求中仅有占位符。[请求、凭据和诊断](../technical/model-connections.md)

## 常见卡点

| 表现 | 核对顺序 |
| --- | --- |
| 模型目录空或 404 | Provider 的 Models 列表 URL、Base URL、套餐和目录鉴权，必要时确认上游是否提供目录 |
| 目录有模型，实际请求 404 | Model ID 大小写与订阅、Endpoint Path、响应协议 |
| 401 或 403 | 存储可访问、Key 与帐号/套餐匹配、请求协议，再核对目标 Agent 正在采用的交付方式 |
| 能保存，Agent 计划阻止 | 检查目标 Agent 的协议及原生格式是否支持自定义 Path、Auth Requirement 和交付方式 |
| 模型显示已添加，请求仍走旧模型 | 查启停、全局 current、会话选择与重载规则 |
| 改连接后只有部分 Agent 生效 | 到各真实目标查状态，按 [同步与恢复](recovery.md) 处理失败关系 |

[Arca 4.0.2 Models 指南](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/models.md) · [Provider 模板指南](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/providers.md)
