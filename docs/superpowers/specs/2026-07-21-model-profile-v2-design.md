# Arca Model Profile v2 设计

## 状态

- 日期：2026-07-21
- 状态：已实现
- 范围：Rust core、Tauri commands、macOS Desktop、历史 Model 配置导入
- 不包含：项目级配置、MiniMax Code/Qoder 自动写入、正式版发布

## 身份与分类

Model Profile 是一份可被多个 Agent 消费的连接配置实例，不等同于上游模型型号。

- `profile_id` 是 Arca 自动生成且永不改变的内部主键。新格式为可读的
  `provider-model-random`，总长不超过 64 个安全字符。普通表单不展示或编辑，技术详情可复制。
- `provider` 表示实际 API/计费渠道，例如 `openrouter`、`anthropic` 或 `custom`。
- `model_vendor` 表示模型开发商；能可靠推导时写入，未知时为空。
- `catalog_key = provider/model` 只用于分类、搜索和重复提示，不是唯一键。
- `name` 是可编辑显示名，在同一 Provider 内唯一。新建时由 Arca 生成默认名并解决重名。
- 同一个 `provider + model` 可以存在多个 Profile，以支持个人/团队凭据、不同 Endpoint 和协议。

Provider 由内置注册表与 `Custom Provider` 共同组成。已知官方 Host 可保守识别；未知 Host
归入 `custom`，不得根据模型名字猜测访问渠道。通过 OpenRouter 调用 Anthropic 模型时，
Provider 是 OpenRouter，model vendor 才是 Anthropic。

## Breaking schema migration

Model schema 升级到 v2，不保留旧 Model Profile 数据结构的写入兼容。

1. 为全部旧 Profile 补齐 Provider/vendor 并生成新内部 ID。
2. 原子迁移 `model_profiles`、`model_assignments`、`model_consumptions`、全部受影响 Agent
   原生 provider identity 和 Keychain credential。
3. 成功后移除旧 ID 和旧 Keychain item；失败时恢复 settings、Agent 文件与所有 credential。
4. 未包含 Model 数据的旧 settings 可直接升为 v2。

未知顶层 settings 字段、Agent 非目标字段、注释、格式与权限必须保持不变。

## 历史配置导入

扫描支持安全 writer 的 Agent 中全部可解析 Model，而不仅是当前模型。候选只向 UI 暴露：

- 非敏感 Profile metadata；
- Provider、协议和模型；
- credential kind/reference，不含 secret value；
- 来源 Agent、当前状态、目标文件 hash 和候选 fingerprint。

导入前必须预览并显式确认。Core 在提交前重新核对 settings 与目标文件 hash，同一 Profile
的中央创建、Agent 接管、当前指针和 credential 迁移属于一个原子事务。

去重仅在 `provider + protocol + normalized base_url + model + credential identity` 完全一致时
发生。环境变量名属于 credential identity；密钥正文不得用于 UI 去重。相同型号但 Endpoint、
变量名或凭据形态不同的候选保持独立。

凭据规则：

- 环境变量只保存变量名。
- 已有 Arca Keychain command 只迁移关联。
- Claude Code、Codex、Pi 的可信 literal secret 可由 Core 在提交阶段直接转存 Keychain；
  secret 不经过 React、IPC、日志、计划或普通备份。
- env-only Agent 的 literal secret 不自动接管，要求先改为环境变量。
- 任意外部 command 不执行。
- MiniMax Code 与 Qoder 继续 Agent 自管。

无法安全导入的候选保留为外部只读，不阻塞其他候选。

## Agent 消费状态

每个 `Agent × Profile` 使用以下状态机：

```text
NotAdded
AddedDisabled
AddedEnabledInactive
AddedEnabledActive
```

正交健康状态为 `Synced | Missing | Drifted | Conflicted | Unsupported | External`。

不变量：

- `Active => Enabled => Added`；
- 每个 Agent 最多一个 Active；
- 单模型 Agent 最多一个 Added Profile；
- External 是未管理 observed state，不伪装成消费关系。

停用、移除或删除当前 Profile 时，按最近使用时间选择仍启用的 fallback；没有候选则回到
Agent 原生默认。计划和确认 UI 必须展示具体 fallback。Arca desired current 与 Agent observed
current 不一致时不得静默覆盖，提供“恢复 Arca 配置”和“采用 Agent 当前配置”。

## Desktop 交互

- 顶层 Models 管理中央 Profile 生命周期；Agent 页面只管理添加、启用、当前和移除。
- Models 按 Provider 分类，协议作为次级筛选；普通创建表单不出现内部 ID。
- 设为当前、启用和添加非当前模型直接提交并 Toast 提示。
- 停用或移除当前模型先展示 fallback；中央删除、外部导入和 credential 迁移展示完整审阅。
- 选择器提前禁用协议或 credential 不兼容的 Profile并说明原因。
- 存在外部模型时显示“查看并导入”，不允许普通添加流程覆盖。
- 审阅契约必须表达 added/enabled/active 的 before/after 与 fallback，不能把所有 Model Add
  都写成“切换模型”。

## 验证门禁

- settings v1→v2 migration、Keychain rollback、ID/名称冲突与未知字段 round-trip；
- 每个可导入 Agent 至少一个安全 candidate fixture，literal/command/歧义配置 fail closed；
- Model adoption 的 stale hash、原子回滚、active preservation 与 dedup；
- Model state change review contract 和 Desktop 文案测试；
- `cargo fmt --check`、`cargo test --workspace`、Tauri tests、Desktop tests/build/icon check；
- 最终只以正式安装的 `/Applications/Arca.app` 做 UI 验收。
