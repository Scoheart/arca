# Arca Model Provider v3 设计

## 目标

将连接与认证配置从每个 Model 中拆出，形成 `Provider 1 -> N Models`：

- Provider 保存稳定 ID、显示名、Provider 类型、按协议划分的 Endpoint、环境变量引用和一份 Keychain 凭据。
- Model 通过 `provider_id` 引用 Provider，只保存 Model ID、显示名、协议、能力与模型级参数。
- Agent 仍选择具体 Model；写入 Agent 配置时由 Model 解析其 Provider。

## 数据契约

`settings.json` 升级为 v3，并增加 `model_providers`。Provider ID 和类型在编辑时不可变；同一 Provider 下 Model ID 必须唯一。协议 Endpoint 属于 Provider，Model 只可选择 Provider 已配置的协议。

API Key 不进入 `settings.json`。永久 Keychain service 使用
`com.scoheart.arca.model-provider.<providerId>`，同一 Provider 的所有 Model
解析到同一 service。事务 rollback 凭据是短期恢复证据，不属于永久配置。

## 迁移

启动顺序固定为 v2 Profile 迁移、v3 Provider 迁移、Agent reconciliation。
v2 Profile 按以下保守身份归并：

1. Provider 类型相同；
2. Endpoint origin 相同；
3. 环境变量引用相同；
4. Keychain 凭据指纹相同；
5. 同一协议的完整 Endpoint 相同。

任一条件不同就创建独立 Provider，避免把个人账号、团队账号或不同网关误合并。旧 Profile Keychain 项在 Provider 记录落盘后归并为一份 Provider Keychain 项；写入失败时保留旧项并阻止 Model reconciliation，后续启动可重试。冲突的旧凭据不会自动择一。

## 生命周期

- 新建首个 Model 时同时创建 Provider。
- 在已有 Provider 下新增 Model 时继承 Provider Endpoint、环境变量和凭据，不重复显示连接字段。
- Provider 更新通过中央资产计划传播到全部子 Model，并重新应用全部关联 Agent。
- Provider API Key 只能在 Provider 编辑中统一保留、替换或清除。
- 删除普通子 Model 不影响 Provider；删除最后一个 Model 时，审阅界面明确提示并同时清理空 Provider 和共享凭据。
- 不提供会静默级联删除全部 Models 的 Provider 删除操作。

## 事务与验证

Provider 更新将 settings、所有 Agent 目标文件和 Provider Keychain 凭据纳入同一恢复域。commit marker 前失败会恢复全部子 Model、Agent 文件和旧凭据；恢复证据不完整时 fail closed，保留现场等待启动恢复。

测试覆盖 v3 迁移幂等性、保守归并、单一永久 Keychain 项、Model ID 唯一性、Provider 更新传播、共享凭据替换、最后一个 Model 删除及多子 Model 回滚。
