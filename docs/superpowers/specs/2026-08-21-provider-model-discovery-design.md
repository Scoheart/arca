# Provider 模型发现设计

## 背景与目标

Arca 当前要求用户在新增 Model 时手动填写 Model ID。多数内置 Provider 已提供模型列表接口。本功能让 Arca 在新增 Model 时按 Provider 能力动态获取可用模型，同时保留手动输入作为可靠兜底。

完整 Provider 调研与依据见 [`2026-08-21-provider-model-discovery-research.md`](2026-08-21-provider-model-discovery-research.md)。结论是：当前 51 个内置 Provider 类型中，首版支持 48 个；`github-models`（服务已下线）、`wandb`（缺少必需 Project 参数）、`custom`（不盲探测）除外。

目标：

- Provider 明确声明支持模型发现时，新建 Model 自动获取一次模型列表。
- Model ID 输入支持搜索、选择、刷新和自由输入。
- 请求由 Rust Core 发起；API Key 只从 Provider Keychain 临时读取。
- 拉取失败不阻塞用户手动填写或保存 Model。
- 新 Provider 只需在 Core 能力表增加声明，不需要修改 Desktop 交互。

非目标：

- 不把远端模型列表持久化到 `settings.json`，也不建立定时同步任务。
- 不以列表结果替代真实推理请求验证。
- 不自动覆盖用户填写的 Model 名称、上下文窗口或输出 Token 上限。
- 本期不增加图片/视频模型专用目录。
- 本期不增加任意 Custom Provider discovery path，也不为 W&B 扩展 Provider schema。
- 本期不顺带删除已经下线的 `github-models` Provider 模板。

## 方案选择

采用 Core 能力声明方案。React 不直接请求 Provider，也不会对 Custom Provider 盲试 `/models`。

Core 内部使用五种发现规范：

| 规范 | Provider 范围 | URL / 鉴权 | 响应适配 |
|---|---|---|---|
| OpenAI-compatible | 调研确认的 44 个类型 | 从已配置 OpenAI 请求 URL 推导同级 `/models`；Bearer Key 按 Provider 为必需或可选 | 顶层数组或 `data[]` |
| Anthropic | `anthropic` | 同源 `/v1/models?limit=1000`；`X-Api-Key` + `anthropic-version` | `data[]` |
| Gemini | `google` | 同源 `/v1beta/models?pageSize=1000`；`x-goog-api-key` | `models[]` |
| Cohere | `cohere` | 同源 `/v1/models`；Bearer | `models[]` |
| Fireworks | `fireworks` | 同源公共 serverless 目录；Bearer | `models[]` |

OpenAI-compatible URL 推导以 Provider instance 的完整协议请求 URL 为输入：精确移除末尾 `/responses` 或 `/chat/completions`，再追加 `/models`。这样同时兼容 `base_url=https://openrouter.ai/api/v1 + endpoint_path=/responses` 和迁移后的 `base_url=https://openrouter.ai + endpoint_path=/api/v1/responses`。若实例没有可推导的 OpenAI 协议端点则 fail closed，不猜测其他路径；用户仍可手填 Model ID。

原生适配器同样只可改写已配置 URL 的 path/query，scheme、host、port 必须保持不变。

## Core 契约

新增只读用例：

```text
discover_provider_models(provider_id) -> ProviderModelSummary[]
```

返回对象只包含界面需要的非敏感字段：

```text
ProviderModelSummary {
  id: String,
  name: Option<String>,
  context_length: Option<u64>,
}
```

Core 按以下顺序执行：

1. 从设置读取指定 Provider instance，并根据其 `provider` 类型查能力表。
2. 构造经过验证的同源模型列表 URL；不接受前端传入任意 URL、Header 或 codec。
3. 按能力的 `required` / `optional` / `none` 凭据策略从永久 Provider Keychain 项读取 API Key。`required` 缺失时在发网前返回可识别错误；`optional` 有 Key 就附带，以获得该 Key 的真实可见目录。
4. 使用 Arca 现有代理感知的 `ureq` agent 发起 GET，15 秒全局超时并关闭自动重定向。
5. 每页响应限制为 4 MiB；有官方 continuation token 的原生适配器最多跟随 10 页，所有页面合计最多接收 2,000 个有效条目。
6. 跳过空 ID，按首次出现去重，并保留 Provider 返回顺序。
7. 只返回摘要；不返回响应头、原始响应或凭据。

API Key 不进入参数、配置、日志或错误文本。Core 使用 `zeroize::Zeroizing` 包装读取出的凭据及临时 Authorization 值，请求结束后清零。HTTP 状态错误只暴露状态与经过限制的通用说明，不拼接可能回显凭据的请求内容或响应正文。

`ModelProviderView` 和 `ModelProviderInstanceView` 下发只读的 `model_discovery_supported` 标志。具体 endpoint、鉴权、分页与 codec 仍是 Core 内部权威，Desktop 不复制 Provider 能力表。

## Desktop 交互

新增 Model 时：

1. 选择的 Provider 支持发现，则打开表单后自动请求一次。
2. Model ID 保持可自由输入的可搜索 combobox；结果可按 `name` 或 `id` 搜索。
3. 选择建议只写入 `draft.model = result.id`，不自动覆盖名称或 Token 配置。
4. 提供显式刷新按钮；同一弹窗内按 Provider instance 缓存成功结果，避免重复请求。
5. 切换 Provider 时加载对应缓存或请求一次；较早请求晚返回时不得覆盖当前 Provider 的结果。

编辑已有 Model 时不自动联网；用户切换 Provider 或点击刷新后再请求。这样避免仅查看或修改其他字段时产生无关网络请求。

状态展示：

- 加载中：Model ID 字段保持可输入，显示紧凑进度状态。
- 成功：显示匹配建议和结果数量。
- API Key 缺失、网络失败、非成功状态或格式错误：字段下方显示行内错误及重试入口。
- 不支持发现：保持当前纯手输界面，不显示错误。

任何发现错误都不参与 `valid` 判定；只要现有 Provider、Protocol、请求 URL和手填 Model ID 有效，仍可进入现有中央资产审阅流程。

## 边界与安全

- React 只传 `provider_id`，不能指定任意 URL、Header 或响应解析器。
- Core 只请求设置中已存在的 Provider instance，并使用该 Provider 类型的已审核能力。
- 模型发现 URL 必须与 Provider 配置 URL 同 scheme/host/port；关闭重定向，避免凭据离开已配置 origin。
- 请求复用 Arca 全局网络代理设置，不创建第二套代理或直连兜底逻辑。
- 本功能不修改 Provider/Model 持久化 schema，不触发 settings 版本迁移。
- 本地 Provider 保留 HTTP 能力；远端 Provider 继续服从现有 Provider URL 校验。
- 模型列表只存在于当前 Desktop 弹窗内，关闭后释放。

## 测试策略

按测试先行实现：

- Core：用隔离 HOME/ARCA_HOME 和本地 TCP fixture 验证五种 adapter、URL 推导、必需/可选鉴权、特殊 Header、分页上限、响应 codec、去重顺序、缺少凭据、重定向、非成功状态、格式错误、超大响应以及错误不泄露密钥。
- Application/Tauri：验证只读用例导出、异步阻塞边界、命令注册与 wire-safe 序列化。
- Desktop：验证新建时自动请求、搜索和选择真实 ID、刷新、Provider 切换的竞态保护、编辑时不自动请求、错误后仍可手填并保存、不支持 Provider 不发请求。
- 回归：运行相关 Rust 单测、Desktop `ModelsView` 单测、Desktop 全量测试与 build；完成前在从远端分支导出的临时目录独立复验。

测试先作为单独远端 commit 提交并观察预期失败，再提交实现使同一组测试转绿。

## 交付

设计、测试和实现均从 GitHub 最新 `main` 在远端 `codex/provider-model-discovery` 分支创建提交，并持续更新 Draft PR #128。不会修改、提交或 push 用户当前本地工作树，也不改 release 版本与 Changelog。
