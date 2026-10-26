# ZCode 自定义模型接入设计

## 目标与当前状态

让 Arca 管理 ZCode Desktop 的自定义模型，复用中央 Models 资产与 Agent 消费关系。当前 ZCode 在 `core/src/resources/model/mod.rs` 中为 guided，已有 MCP、Skills 接入与图标。本设计不改变这两项能力。

本机安装包和用户配置的只读审计确认：用户级模型注册表为 `~/.zcode/v2/config.json`，根字段 `provider` 下以 Provider ID 为键。Provider 的 `name`、`kind`、`enabled`、`source`、`options.baseURL`、`options.apiKey` 与 `models` 对应设置页面。模型以 ID 为键，包含 `limit`、`modalities`、`reasoning` 和 `zcode` 元数据。

审计未读取或输出凭据到本文。ZCode 的会话模型选择不属于本次写入范围。私有 schema 未来变化时必须重新审计，不能以猜测字段替代已验证契约。

## 产品行为

- Models 页继续负责中央模型创建、导入与编辑。
- ZCode Agent 页展示已有外部模型及 Arca 分配的模型，允许选择中央模型并添加、解除分配。
- 支持多个已添加模型，不显示由 Arca 管理的全局“当前模型”；用户在 ZCode 会话中选择。
- 外部模型只读；导入是独立显式操作，扫描不能自动接管。
- 不将“配置已写入”表述为“接口可用”。模型不存在、权限不足与 TLS 故障不由配置投影修复。
- 配置应用后提示重新加载或重启 ZCode；未验证热加载前不承诺立即生效。

## 凭据决策：用户已批准路径 1（2026-09-09）

安装包的 Provider 转换路径直接把 Provider 的 apiKey 写入 `options.apiKey`。CLI 底层存在 apiKeyEnv，但尚未证明桌面配置转换和连接测试会传递它，不能据此宣称桌面支持环境变量引用或 Keychain helper。

项目 AGENTS.md 明确要求 API key/token 只存 Keychain、不进入配置。现有 Qoder/OpenCode 实现虽有显式 plaintext delivery，也不能自动视为对 ZCode 的规则豁免。

可选路径：

1. **原生完整接入**：复用 Arca 已有显式 plaintext delivery 流程，中央密钥仍在 Keychain，用户分配时明确选择导出至 ZCode 的私有配置。能创建新 Provider 和分配任意中央模型；需要用户授权 ZCode 原生配置中的密钥落盘例外，并同步 AGENTS.md 的精确例外范围。
2. **仅复用现有 Provider**：只为已有 ZCode Provider 添加/移除 models 子项，不创建或修改凭据。Provider 需由用户在 ZCode 中预先配置，Arca 保存显式 Provider ID 绑定。不能声称具有完整中央 Provider 投影能力。
3. **本地凭据代理**：ZCode 指向 Arca 本地服务，由服务访问 Keychain 并转发请求。需要常驻生命周期、访问控制、流式协议、TLS 和网络代理支持；本次不推荐增加此子系统。

为满足原始完整配置需求，优先选择路径 1，但用户批准前不得实施任何真实密钥落盘或修改安全规则。若选择路径 2，需将下列 Provider 所有权改为既有容器内的模型级所有权。

## Core 适配设计（路径 1）

新增 `core/src/resources/model/adapters/zcode.rs`，集中负责读取、校验、投影、观察及清理。前端、CLI 和 Tauri 不自行写 JSON。

- 配置 authority 为 native-registry，路径唯一为 `~/.zcode/v2/config.json`。
- 每个中央 Profile 使用稳定的 Arca Provider ID，独占一个模型，避免不同模型共享 Provider 时更新 URL/凭据影响兄弟模型。
- 首批协议只开放经过完整写入和运行时读取核验的格式；当前已确认 `openai-compatible` 的 Chat Completions，其他协议不能仅依据 UI 名称开放。
- 写入 Provider 的连接与目标 models 字段，保留所有未知字段和非目标 Provider。拒绝碰触 builtin Provider。
- 外部多模型 Provider 不直接接管；导入只创建中央资产；随后在 ZCode 页显式添加才使用新 Arca Provider，保留原配置。
- 解除分配只删除对应受管模型，保留 Provider、凭据与外部兄弟模型；空 Provider 不擅自删除。
- 全量清空如接入既有 reviewed clear_agent_models，只删除已审阅模型，不删除 Provider、凭据或中央资产。未实现一致语义前不暴露入口。
- 外部配置中的 enabled 状态如实展示；不伪造会话 current，也不自动启用外部 Provider。

## 写入与安全

- 使用现有 lossless JSONC CST 工具保留格式；重复键、类型错误、Provider 身份冲突与 schema 歧义均 fail closed。
- 复用 core 的 plan/commit、CAS、目标独立收敛和 incident；已有文件原地改写并保留 inode。
- 此文件已含第三方凭据，所有相关操作采用现有私有加密回滚快照，禁止普通明文全文件备份。
- 错误与预览只显示字段名和脱敏变更，不能包含 Provider 原文或密钥。
- 若获准 plaintext delivery，只在 commit 时解析 Keychain 值；按现有私有目标权限策略处理，不扩大备份/日志泄漏面。

## 文件改动范围

| 文件 | 职责 |
| --- | --- |
| `core/src/resources/model/adapters/zcode.rs` | ZCode 专用注册表适配 |
| `core/src/resources/model/adapters.rs` | 接入 prepare/apply/clear/observe 分发 |
| `core/src/resources/model/mod.rs` | 将 guided 改为可管理；路径、协议、多模型、无全局选择和私有提交 |
| `core/src/resources/model/credential.rs` | 仅按用户选定方案声明已验证交付能力 |
| `core/src/assets/model_migration.rs` | 外部模型发现、脱敏与显式导入 |
| `data/agents.json` | 更新 ZCode 能力说明；目录生成物通过既有脚本生成 |
| `docs/zcode-models.md`、`README.md` | 存储契约、交付方式、重载与限制 |

现有图标优先复用。发现前端针对 Agent ID 的能力白名单时同步消费者，不复制业务编排。

## 验收与交付

保留隔离 fixture/round-trip 用例：空配置添加两个模型、重复应用幂等、解除一个保留另一个、未知字段与凭据保留、损坏/重复键拒写、禁用 Provider 观察、无全局 current、外部模型脱敏、私有回滚与 CAS。只使用虚构凭据；测试不访问真实 HOME/Keychain。

遵循项目极速模式：未获明确要求不运行测试、fmt、clippy 等门禁；检查 diff，交付必需的生产编译与发布按现有流程执行。不为了测试功能修改用户真实 ZCode 配置。

功能提交不改版本文件，按本地 main 链路交付，由 Direct Stable 生成 patch。发布和安装使用现有 Arca 技能与已授予的常驻授权。
