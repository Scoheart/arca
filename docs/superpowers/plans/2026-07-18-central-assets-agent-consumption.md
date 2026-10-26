# Arca 中央资产与 Agent 消费模型实施计划

> 状态（2026-07-18）：核心、Desktop、文档与自动门禁均已完成；正式安装版 UI 验收仍需用户当次明确授权，commit/push/PR/release 均未执行。下方任务清单保留为实施追踪记录，最终交付证据以测试结果和实际 diff 为准。

**目标：** 将 MCP、Model、Skill 统一为“中央资产入库与维护 → Agent 建立消费关系 → Arca 对账实际状态”的产品模型，消除 Agent 页面重新安装 Skill、MCP 只靠扫描反推关系以及 Model 编辑清空 assignment 的错误逻辑。

**设计权威：**
`docs/superpowers/specs/2026-07-18-central-assets-agent-consumption-design.md`

**基线：** 分支 `codex/central-asset-consumption-design`，设计提交 `0734329`，基于 Arca `main` 的 `c81a6da`。

**架构：** Rust `core::consumption` 统一 desired relationship、observed inventory、compatibility、plan、commit、rollback 和 migration。MCP、Model、Skill adapter 保留各自类型和安全写入。Tauri 只做 tagged wire 转换，React 只展示 core 状态并提交用户意图。

**技术栈：** Rust 2021、serde、现有 safe-write/settings/Skills transaction 基础、Tauri 2、React 19、TypeScript 7、Vitest 4、Testing Library、CSS。

## 全局约束

- 顶层 MCPs、Models、Skills 是唯一中央资产创建、导入、编辑、更新和删除入口。
- Agent 页面只能管理中央资产消费关系，不得解析来源、选择本地目录或创建同一资产。
- MCP、Skills 每个 Agent 可消费多个；Model 每个 Agent 最多一个当前 Profile。
- Agent 文件只提供 observed state，不得自动反向创建资产或 desired relationship。
- 外部资产只读发现；显式导入只创建中央资产，不顺手建立消费关系。
- 中央更新或删除必须覆盖全部消费者；任一未解决漂移、冲突或并发变化阻止整个 commit。
- 保留未知字段、注释、格式、非目标策略、权限、备份和 fail-closed 行为。
- Model secrets 只存在系统 Keychain；不得进入 settings、plan、journal、日志、fixture、DOM 或截图。
- Skill 继续使用中央副本、物理 target 归一化、plan/commit hash、风险确认和 recovery journal。
- 所有测试隔离 `HOME` 和 `ARCA_HOME`，不得读取真实 Agent 配置、`~/.arca` 或 Keychain。
- 每个阶段必须可构建、可测试、可审查；切换领域写入口后立即移除对应 Desktop 旧调用。
- 不修改 release-owned version、tag、Release、签名、公证或 `/Applications/Arca.app`。
- 实施提交使用 `<type>(<scope>): <summary>`，body 解释为什么；只暂存当前任务文件。

## 共同验证命令

聚焦 Rust：

```bash
cargo fmt --check
cargo test -p arca-core consumption
cargo test -p arca-core --test consumption_inventory
cargo test -p arca-core --test consumption_lifecycle
cargo test -p arca-core --test consumption_recovery
```

Rust phase gate：

```bash
cargo fmt --check
cargo test --workspace
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```

Desktop 聚焦与 phase gate：

```bash
cd desktop
npx vitest run <focused-test-files>
npm test
npm run check:agent-icons
npm run build
```

任何变更 wire contract 的任务都同时运行 Rust、Tauri、TypeScript 聚焦测试。

---

## Phase 1 — 建立只读消费契约

### Task 1.1：定义 settings 与共享 wire types

**文件：**

- Create: `core/src/consumption.rs`
- Create: `core/src/consumption/types.rs`
- Modify: `core/src/lib.rs`
- Modify: `core/src/settings.rs`
- Modify: `core/src/types.rs`（只在稳定 identity 类型必须共享时）
- Test: `core/src/settings.rs`
- Test: `core/src/consumption/types.rs`

**产出接口：**

- `AssetRef::{Mcp, Model, Skill}`
- `ConsumptionStatus::{Synced, Pending, Drifted, Conflicted, Unsupported, External}`
- `ConsumptionView`
- `McpConsumptionRecord`
- `Settings.mcp_consumptions`
- tagged `AgentConsumptionSelection`

**步骤：**

- [ ] 写失败测试：`mcp_consumptions`、未知顶层字段、CLI-owned sections 和现有 Model/Skill assignments 在 settings round-trip 后保持不变。
- [ ] 写失败测试：三种 `AssetRef` JSON 形态稳定，MCP identity 必须包含合法 `name::transport`，Model/Skill identity 不接受空值。
- [ ] 写失败测试：`AgentConsumptionSelection::Model` 拒绝多个 Profile；MCP/Skill 集合去重并稳定排序。
- [ ] 实现类型，不增加任意 `serde_json::Value` domain payload。
- [ ] 为 `mcp_consumptions` 使用 `AgentId -> stable asset key -> record` 结构，并将 Agent override 限制为现有 `OverridePatch` 所有字段。
- [ ] 保持字段 optional，使旧 settings 无需先写迁移即可读取。
- [ ] 运行 settings 与 consumption type 测试、`cargo fmt --check`。

**聚焦命令：**

```bash
cargo test -p arca-core settings::tests
cargo test -p arca-core consumption::types::tests
```

**提交：** `feat(consumption): define desired state contract`

### Task 1.2：建立统一 Agent capability 与兼容性投影

**文件：**

- Create: `core/src/consumption/compatibility.rs`
- Modify: `core/src/agents.rs`
- Modify: `core/src/models.rs`
- Modify: `core/src/skills/inventory.rs`
- Test: `core/src/consumption/compatibility.rs`
- Test: `core/tests/agent_formats.rs`

**步骤：**

- [ ] 写失败测试：MCP 根据 `supported_transports` 分类兼容；Model 根据 protocol 和 managed/guided mode 分类；Skill 根据已核验 capability、安装探针和 physical target 分类。
- [ ] 证明 Agent identity 使用 catalog canonical id（如 `claude-code`、`codex`、`pi`），不新增前端 alias。
- [ ] 定义 `CompatibilityView { compatible, reason, affected_agent_ids }`；不兼容必须有稳定 reason code 与用户文案。
- [ ] 从 Models 提取只读 capability helper，禁止 compatibility 模块复制 Agent 清单。
- [ ] 从 Skills target graph 暴露只读查询接口，保留共享 alias 和真实 affected Agents。
- [ ] guided/read-only Model Agent 返回 `unsupported`/引导原因，不产生 managed relationship。
- [ ] 运行 compatibility、Agent catalog、Model、Skill inventory 聚焦测试。

**提交：** `feat(consumption): unify agent compatibility`

### Task 1.3：实现 desired / observed 只读 inventory

**文件：**

- Create: `core/src/consumption/inventory.rs`
- Create: `core/tests/consumption_inventory.rs`
- Modify: `core/src/ops.rs`
- Modify: `core/src/models.rs`
- Modify: `core/src/skills/inventory.rs`
- Modify: `core/src/consumption.rs`

**测试矩阵：**

- MCP：active exact match、disabled snapshot、customized drift、missing、external、unsupported transport。
- Model：assigned exact match、owned-field drift、missing credential、guided external config、unknown Profile。
- Skill：assigned exact link、broken link、conflicting link、shared target、external copy。

**步骤：**

- [ ] 创建隔离的 `TestHome` fixture，向三类 Agent target 写最小真实格式配置。
- [ ] 写失败黑盒测试：每个矩阵项返回正确 `desired`、`observed`、`status`、`reason` 和 `affected_agent_ids`。
- [ ] 实现 domain reader adapter；reader 只读取自己拥有的字段，不把未知字段差异误判为中央资产变化。
- [ ] desired relationship 即使 target missing/conflicted 也必须留在 inventory，禁止异常项消失。
- [ ] external item 使用独立 projection，不混入 desired 主列表。
- [ ] inventory 保持确定性排序，重复 refresh 不写文件、不改 settings。
- [ ] 运行黑盒测试两次，并比较测试目录哈希证明读取无副作用。

**提交：** `feat(consumption): reconcile desired and observed state`

### Task 1.4：停止 MCP 自动探索入库并生成接管候选

**文件：**

- Create: `core/src/consumption/migration.rs`
- Create: `core/tests/consumption_migration.rs`
- Modify: `core/src/registry.rs`
- Modify: `core/src/ops.rs`
- Modify: `core/src/sources.rs`
- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src/hooks/useInstallState.ts`
- Modify: `desktop/src/hooks/useInstallState.test.tsx`（若不存在则创建）

**步骤：**

- [ ] 写失败测试：refresh/scan 发现新 MCP 后，Registry、sources 和 settings 字节不变。
- [ ] 写失败测试：旧版本已持久化的 managed discovered entry 仍保留在中央 Registry。
- [ ] 写失败测试：exact central match 生成 `adoptable`；Agent-specific customization 生成 drift/external；二者都不创建关系。
- [ ] 将 `refreshAll → importDiscovered` 替换为 `refresh registry + scan observed + refresh consumption inventory`。
- [ ] 保留显式“导入外部 MCP”core 入口，但不从 refresh 调用。
- [ ] 实现只读 migration candidates；candidate 带 settings/target hash，稍后 plan 时必须重验。
- [ ] 更新 useInstallState 测试，禁止刷新调用任何写入 command。
- [ ] 运行 Registry/source/MCP scan、Tauri command 和 hook 测试。

**提交：** `fix(mcp): keep discovery read only`

**Phase 1 gate：**

```bash
cargo fmt --check
cargo test --workspace
cd desktop && npm test && npm run build
```

---

## Phase 2 — Typed plan、事务与领域 adapter

### Task 2.1：建立统一 operation envelope 与持久化 plan store

**文件：**

- Create: `core/src/consumption/planner.rs`
- Create: `core/src/consumption/transaction.rs`
- Modify: `core/src/consumption/types.rs`
- Modify: `core/src/skills/types.rs`
- Modify: `core/src/skills/ops.rs`
- Test: `core/src/consumption/planner.rs`
- Test: `core/tests/consumption_recovery.rs`

**产出接口：**

- `AssetOperationPlan`
- `AssetOperationKind::{SetConsumption, UpdateAsset, DeleteAsset, Adopt}`
- `DomainPlan::{Mcp, Model, Skill}`
- `AssetCommitRequest { operation_id, candidate_hash }`; external-state intent is carried by the explicit convergence action, not a second confirmation token.
- `cancel_asset_operation`、`recover_asset_operation`

**步骤：**

- [ ] 写失败测试：plan serialization 不包含 secrets、绝对私有目录和未脱敏配置预览。
- [ ] 写失败测试：operation id、settings hash、target hashes 和 candidate hash 与 plan 绑定；旧 plan 在并发变化后失效。
- [ ] 定义 typed `DomainPlan`，Skill 通过适配现有 `OperationPlan` 接入，不复制其风险和 hash 规则。
- [ ] 将 plan 持久化到 Arca 私有 staging/journal 路径，权限沿用 Skills `0600/0700` 门禁。
- [ ] transaction coordinator 只负责 envelope、lock、dispatch 和 lifecycle；领域 adapter 负责自己的 mutation spec 与 rollback。
- [ ] cancel 等待进行中的 commit，且对同 operation id 幂等。
- [ ] recovery 不持久化 Keychain secret；需要秘密时返回 `credential_required` 并要求重新计划。
- [ ] 运行 plan/cancel/concurrency/recovery 测试。

**提交：** `feat(consumption): add typed operation lifecycle`

### Task 2.2：实现 MCP consumption planner 与原子多目标事务

**文件：**

- Create: `core/src/consumption/mcp.rs`
- Create: `core/tests/consumption_mcp_flow.rs`
- Modify: `core/src/ops.rs`
- Modify: `core/src/adapter.rs`
- Modify: `core/src/disabled.rs`
- Modify: `core/src/safe_write.rs`（仅添加共享的 prepare/verify helper）

**步骤：**

- [ ] 写失败测试：从空集合到多个 MCP、部分解除、enabled 切换和 override 变化产生正确 diff。
- [ ] 写失败测试：关系和 Agent config 同时成功；任一 target 写入失败时全部回滚。
- [ ] 写失败测试：手工 customized target 阻止 commit；未知字段、注释和非目标 section 保持不变。
- [ ] adapter 先读取全部 target snapshot、准备新内容和备份，再开始写入。
- [ ] 将 settings relationship 写入纳入同一 journal；settings 保存失败时回滚 Agent targets。
- [ ] commit 后 rescan 验证 exact observed state；验证失败进入 recovery，不返回假成功。
- [ ] 让现有 `install/disable/enable/delete` core 函数在过渡期委托单 Agent typed plan，Desktop 尚未切换前保持行为。
- [ ] 覆盖 stdio/http 同名变体和 disabled snapshot 恢复。

**提交：** `feat(mcp): transact agent consumption`

### Task 2.3：实现 Model consumption 与 Profile 传播事务

**文件：**

- Create: `core/src/consumption/model.rs`
- Create: `core/tests/consumption_model_flow.rs`
- Modify: `core/src/models.rs`
- Modify: `core/src/settings.rs`
- Modify: `desktop/src-tauri/tests/model_commands.rs`（若不存在则创建）

**步骤：**

- [ ] 写失败测试：一个 Agent 不能选择两个 Profile；切换 Profile 替换 relationship。
- [ ] 写失败测试：更新一个被多个 Agent 使用的 Profile 会为全部 consumers 生成 writes，并保留 assignments。
- [ ] 写回归测试：`save_profile` 不再因 metadata change 清除 `model_assignments`。
- [ ] 将 Profile validation、credential presence、Agent owned-field prepare 从直接写逻辑中拆出纯 planner helper。
- [ ] 计划中只保存 `credential_present/required`，不保存凭据值；Keychain 更新在进程内持有 rollback snapshot。
- [ ] 事务覆盖 Claude Code、Codex、Pi 的多文件 writer；任一 target 或 settings 失败时回滚已写文件。
- [ ] guided/read-only Agent 返回稳定 `unsupported`，不得写 assignment。
- [ ] commit 后读取 owned fields 和 assignment，只有一致才返回 synced。

**提交：** `fix(models): propagate profile updates to consumers`

### Task 2.4：拆分 Skill 中央入库与 assignment

**文件：**

- Modify: `core/src/skills/types.rs`
- Modify: `core/src/skills/ops.rs`
- Modify: `core/tests/skills_install_flow.rs`
- Modify: `core/tests/skills_import_flow.rs`
- Modify: `core/tests/skills_remove_repair.rs`
- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/tests/skills_commands.rs`

**步骤：**

- [ ] 添加 `PlanSkillAssetInstallRequest`，只接收 resolution、skill names 和 conflict policy，不接收 Agent ids。
- [ ] 写失败测试：中央 install/import commit 后，`managed_skills` 与 central content 存在，但 `skill_assignments` 和 Agent targets 不变。
- [ ] 保留 assignment plan 为独立 operation，并通过 consumption adapter 接受 Agent-level selection。
- [ ] 复用现有 physical target normalization、risk confirmation、candidate hash、journal 和 recovery；不重写成熟事务。
- [ ] 将旧 `PlanInstallRequest.agent_ids` 标记为内部过渡或移除 wire exposure，Desktop 新 API 不再发送。
- [ ] 更新 Tauri tests，证明安装 command 无法通过隐藏字段顺手分配 Agent。
- [ ] 运行所有 `skills_*` 集成测试，而非只运行安装测试。

**提交：** `refactor(skills): separate assets from assignments`

### Task 2.5：统一双向关系 planner

**文件：**

- Modify: `core/src/consumption/planner.rs`
- Modify: `core/src/consumption/mcp.rs`
- Modify: `core/src/consumption/model.rs`
- Create: `core/src/consumption/skill.rs`
- Create: `core/tests/consumption_selection.rs`

**步骤：**

- [ ] 实现 `plan_set_agent_consumption(agent, domain selection)`。
- [ ] 实现 `plan_set_asset_consumers(asset, agent selection)`。
- [ ] 两个入口先转换为完整 desired relation set，再调用同一个 diff planner。
- [ ] 写测试：从两个方向表达同一目标时，relationship mutations、target writes 和 candidate hash 一致。
- [ ] 对 MCP/Skills 集合执行 add/remove diff；Model 单值执行 replace/remove。
- [ ] Skill 先将 Agent intent 归一化为 physical targets，并在计划中展示所有额外受影响 Agent。
- [ ] 拒绝同一 Skill physical target 的矛盾选择；拒绝未安装/未核验 Agent 新增 assignment，但允许清理历史 target。
- [ ] 确保 planner 不接受 external asset identity；必须先显式导入中央库。

**提交：** `feat(consumption): normalize relationship planning`

### Task 2.6：中央更新与级联删除 planner

**文件：**

- Create: `core/tests/central_asset_lifecycle.rs`
- Modify: `core/src/consumption/planner.rs`
- Modify: `core/src/consumption/mcp.rs`
- Modify: `core/src/consumption/model.rs`
- Modify: `core/src/consumption/skill.rs`
- Modify: `core/src/registry.rs`
- Modify: `core/src/models.rs`
- Modify: `core/src/skills/ops.rs`

**步骤：**

- [ ] 写跨领域参数化测试：中央更新计划枚举全部消费者并保留关系。
- [ ] 写失败注入测试：任一 consumer drift/conflict/concurrent change 阻止整个 commit，中央资产字节不变。
- [ ] MCP source-owned asset 继续只读；手动 override 更新走统一传播计划。
- [ ] Model Profile metadata/credential 更新走统一计划，不直接 save 后再补写 Agent。
- [ ] Skill update 复用 existing candidate/risk diff，并把所有 affected targets 纳入统一 review projection。
- [ ] 中央删除计划包含全部 relationship removals、Agent writes、Skill shared impacts、backups 和 central deletion。
- [ ] unresolved target 阻止 delete；禁止只删中央 metadata 留下 Arca-managed orphan。
- [ ] post-verify 证明资产不存在、关系不存在、Agent owned fields/links 已清理。

**提交：** `feat(consumption): transact central asset lifecycle`

**Phase 2 gate：**

```bash
cargo fmt --check
cargo test --workspace
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```

---

## Phase 3 — Tauri wire 与前端共享状态

### Task 3.1：暴露薄 Tauri consumption commands

**文件：**

- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/lib.rs`
- Create: `desktop/src-tauri/tests/consumption_commands.rs`
- Modify: `desktop/src/lib/types.ts`
- Modify: `desktop/src/lib/api.ts`

**commands：**

- `list_consumption_inventory`
- `plan_set_agent_consumption`
- `plan_set_asset_consumers`
- `plan_update_central_asset`
- `plan_delete_central_asset`
- `plan_adopt_mcp_consumptions`
- `commit_asset_operation`
- `cancel_asset_operation`
- `recover_asset_operation`

**步骤：**

- [ ] 为每个 command 写 wire round-trip、structured error 和 unknown-field rejection 测试。
- [ ] 长时间 scan/plan/commit 使用现有 async/blocking worker 模式，不阻塞 Tauri UI thread。
- [ ] Tauri 不读取文件、不计算 compatibility、不拼接 plan，只调用 core facade。
- [ ] TypeScript tagged union 与 Rust serde tag/rename 完全对应。
- [ ] `api.ts` 只做 `invoke` 映射；不在前端修正状态或默认选择。
- [ ] command 注册测试确保所有入口存在，旧 direct commands 暂保留到对应 UI Task 切换。

**提交：** `feat(tauri): expose consumption operations`

### Task 3.2：新增 `useConsumptionState` 与纯 selectors

**文件：**

- Create: `desktop/src/hooks/useConsumptionState.ts`
- Create: `desktop/src/hooks/useConsumptionState.test.tsx`
- Create: `desktop/src/lib/consumption.ts`
- Create: `desktop/src/lib/consumption.test.ts`
- Create: `desktop/src/test/consumptionFixtures.ts`
- Modify: `desktop/src/App.tsx`

**步骤：**

- [ ] 写 hook 测试：initial load、refresh generation、single active commit、cancel、recovery、stale response discard。
- [ ] 复用 `useSkillsState` 已验证的 operation ownership 模式，不允许同时提交两个 asset operations。
- [ ] selectors 只做按 Agent/domain/asset 分组和稳定排序，不重新判断 compatibility/status。
- [ ] fixtures 覆盖 synced、pending、drifted、conflicted、unsupported、external 和 Skill shared target。
- [ ] App 只创建一个 consumption state 并传给 top-level/Agent views，避免 Models/Agent 各自重复加载关系。
- [ ] 任一 commit 成功后刷新 consumption inventory 和受影响 domain state；失败保留 core structured error。

**提交：** `feat(ui): add consumption state hook`

### Task 3.3：新增共享消费组件与 Review Dialog

**文件：**

- Create: `desktop/src/components/AgentConsumptionPanel.tsx`
- Create: `desktop/src/components/AgentConsumptionPanel.test.tsx`
- Create: `desktop/src/components/ConsumptionPickerDialog.tsx`
- Create: `desktop/src/components/ConsumptionPickerDialog.test.tsx`
- Create: `desktop/src/components/AssetConsumerDialog.tsx`
- Create: `desktop/src/components/AssetConsumerDialog.test.tsx`
- Create: `desktop/src/components/AssetOperationReviewDialog.tsx`
- Create: `desktop/src/components/AssetOperationReviewDialog.test.tsx`
- Create: `desktop/src/components/ConsumptionStatus.tsx`
- Modify: `desktop/src/index.css`

**步骤：**

- [ ] Panel 测试：只渲染 desired relationships；missing/conflicted 项仍显示；external 使用独立 notice。
- [ ] Picker 测试：当前 Agent、搜索、compatible assets、selected state、Model 单选、MCP/Skill 多选、不兼容摘要和“前往资产库”。
- [ ] AssetConsumerDialog 测试：Agent 方向和资产方向产生正确 tagged request，不直接 mutate local list。
- [ ] Review 测试：central changes、relationships、target files、backups、warnings、shared Agents、drift/conflict 和 specific verb。
- [ ] pending commit 时阻止 dismiss；Escape 只关闭 topmost dialog；完成后恢复焦点。
- [ ] 状态组件只显示 core reason，不根据 CSS class 猜测错误。
- [ ] 在 `900x600` 下 Dialog 保持 `16px` inset、body 独立滚动、无横向 overflow。

**提交：** `feat(ui): add asset consumption controls`

---

## Phase 4 — Agent 页面切换到中央选择器

### Task 4.1：先修正 Agent Skills 的重新安装逻辑

**文件：**

- Modify: `desktop/src/components/SkillInstallDialog.tsx`
- Modify: `desktop/src/components/SkillInstallDialog.test.tsx`
- Modify: `desktop/src/lib/skills.ts`
- Modify: `desktop/src/lib/skills.test.ts`
- Modify: `desktop/src/components/SkillsView.tsx`
- Modify: `desktop/src/components/SkillsView.test.tsx`
- Modify: `desktop/src/components/AgentSkillsSection.tsx`
- Modify: `desktop/src/components/AgentSkillsSection.test.tsx`
- Modify: `desktop/src/lib/types.ts`
- Modify: `desktop/src/lib/resourceNavigation.ts`
- Modify: `desktop/src/lib/resourceNavigation.test.ts`

**步骤：**

- [ ] 删除 install wizard 的 `selectedAgentIds`、Agent grid、`initialAgentId` 和 Agent install navigation。
- [ ] 更新 reducer 测试：来源解析后只选择 candidates 与 conflict policy。
- [ ] Agent Skills 的“管理 Skills”打开 ConsumptionPickerDialog，候选只来自中央 managed Skills。
- [ ] 已消费 rows 来自 consumption inventory，而不是在组件内重新推导 target graph。
- [ ] external notice 只读展示并跳转顶层“导入资产库”，不得打开 assignment review。
- [ ] 保留 Skill shared target、风险、recovery、详情跳转和解除使用 review。
- [ ] 添加明确回归断言：AgentView、AgentSkillsSection 和 navigation types 中不存在 `kind: "install"`。
- [ ] 运行全部 Skills component/hook tests 和 build。

**提交：** `fix(skills): consume central assets from agents`

### Task 4.2：切换 Agent MCP 到 desired consumption

**文件：**

- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentResourcePanel.tsx`
- Modify: `desktop/src/components/AgentResourcePanel.test.tsx`
- Modify: `desktop/src/components/ResourcePickerDialog.tsx`（迁移完成后删除或限缩其它用途）
- Modify: `desktop/src/hooks/useInstallState.ts`
- Modify: `desktop/src/lib/mcp.ts`
- Create/Modify: `desktop/src/components/AgentView.test.tsx`

**步骤：**

- [ ] 用 AgentConsumptionPanel 替换 `installedEntries/notInstalledEntries` 与 direct toggle/remove 逻辑。
- [ ] “管理 MCPs”提交完整 desired MCP key set；enabled/override 使用 domain-specific editor/review。
- [ ] 已接管 exact match 显示 synced；candidate 显示“可接管”；未接管 observed item 显示 external notice。
- [ ] 接管 flow 只创建关系，不重写 exact target；hash 变化时计划失效。
- [ ] 删除 AgentView 对 `toggle/setEnabled/remove` 的直接调用，保留 hook 旧 API 直到 Registry cutover Task。
- [ ] 覆盖 stdio/http 同名、disabled、customized drift、empty state 和 pending operation。

**提交：** `refactor(agent): manage central MCP consumption`

### Task 4.3：切换 Agent Model 到单选中央 Profile

**文件：**

- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Modify/Create: `desktop/src/components/AgentView.test.tsx`

**步骤：**

- [ ] 删除 AgentView 自有的 modelProfiles/modelAgents load 与 `applyModelProfile` direct mutation。
- [ ] Model Tab 只显示 desired current Profile；missing/drifted/credential_required 仍显示并提供 review action。
- [ ] “切换 Model”Picker 单选兼容中央 Profile，清空选择表示解除 managed relationship。
- [ ] guided/read-only Agent 显示官方引导，不显示可提交 selector。
- [ ] 从 Profile 详情返回 Agent 时保持 selected Profile 和 focus。
- [ ] 测试一个 Agent 不可能提交多个 Profile ids。

**提交：** `refactor(agent): select central model profile`

### Task 4.4：统一 Agent 页面文案、路径与状态

**文件：**

- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentResourcePanel.tsx`
- Modify: `desktop/src/components/AgentResourcePanel.test.tsx`
- Modify: `desktop/src/index.css`
- Modify: `desktop/src/lib/agentNavigationCss.test.ts`

**步骤：**

- [ ] 三个 Tab 统一标题“正在使用”和 domain-specific 管理按钮。
- [ ] 配置位置卡只解释 physical path/capability，不承担资产安装入口。
- [ ] 状态统一为已同步、待同步、有漂移、有冲突；不再使用“已添加 = 已安装”的混合文案。
- [ ] external notice 与 desired rows 视觉分层，外部项不能使用开关伪装成 managed。
- [ ] `1200x820` 三列配置位置和 `900x600` 堆叠布局无横向滚动。
- [ ] 运行 Agent UI、CSS contract、全量 Desktop tests/build。

**Phase 4 gate：**

```bash
cd desktop
npx vitest run src/components/AgentView.test.tsx src/components/AgentResourcePanel.test.tsx src/components/AgentSkillsSection.test.tsx
npm test
npm run build
```

**提交：** `style(agent): clarify central resource usage`

---

## Phase 5 — 中央资产端双向管理与旧写入口清理

### Task 5.1：为三类资产 Inspector 增加消费者管理

**文件：**

- Modify: `desktop/src/components/RegistryView.tsx`
- Modify: `desktop/src/components/RegistryView.test.tsx`
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Modify: `desktop/src/components/SkillInspector.tsx`
- Modify: `desktop/src/components/SkillInspector.test.tsx`
- Modify: `desktop/src/components/SkillCard.tsx`
- Modify: `desktop/src/components/SkillCard.test.tsx`
- Modify: `desktop/src/components/UnifiedResourceViews.test.tsx`

**步骤：**

- [ ] 卡片 impact 统一显示 desired consumers 和异常数量。
- [ ] Inspector 增加“正在使用此资产”和“管理 Agent”，调用 AssetConsumerDialog。
- [ ] consumer list 使用相同 inventory projection；禁止 Registry/Models/Skills 各自扫描 Agent。
- [ ] Skill 展示 physical target 额外影响；Model 过滤 incompatible/guided Agents；MCP 显示 transport compatibility。
- [ ] 写交叉测试：从资产端修改后，Agent panel 立即显示同一关系结果。

**提交：** `feat(ui): manage consumers from asset details`

### Task 5.2：切换 MCP 中央编辑、删除与外部导入

**文件：**

- Modify: `desktop/src/components/RegistryEditPage.tsx`
- Modify: `desktop/src/components/RegistryEditPage.test.tsx`
- Modify: `desktop/src/components/RegistryView.tsx`
- Modify: `desktop/src/components/RegistryView.test.tsx`
- Modify: `desktop/src/hooks/useInstallState.ts`
- Modify: `desktop/src/lib/api.ts`
- Modify: `desktop/src-tauri/src/commands.rs`

**步骤：**

- [ ] 保存中央 MCP draft 时先 `plan_update_central_asset`，review 列出全部 consumers。
- [ ] 删除使用 `plan_delete_central_asset`，不再 `forgetEntry + rescan` 后分步卸载。
- [ ] 外部 MCP “导入资产库”只写 Registry，不创建 `mcp_consumptions`。
- [ ] 删除 Desktop 对 `apply_install/uninstall/enable_mcp/disable_mcp/delete_mcp/resync_entry` 的 direct 调用。
- [ ] 若 CLI/TUI 仍使用旧 core facade，保留兼容 wrapper；Desktop commands 不再注册未使用 mutation。
- [ ] 运行 MCP editor/view、Tauri command 和 lifecycle tests。

**提交：** `refactor(mcp): route asset writes through plans`

### Task 5.3：切换 Model 中央编辑与删除

**文件：**

- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Modify: `desktop/src/lib/api.ts`
- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/tests/model_commands.rs`

**步骤：**

- [ ] Profile create 无 consumers 时仍通过 central-only plan 保存。
- [ ] Profile edit 通过传播 plan；不再调用直接 `save_model_profile`。
- [ ] Profile delete 通过级联 plan；review 显示所有 assigned Agents 和 Keychain credential removal。
- [ ] credential input 只停留在 Dialog state 和 invoke request memory，关闭/失败后清空。
- [ ] 删除 Desktop 对 `apply_model_profile/delete_model_profile/save_model_profile` 的 direct mutation。
- [ ] 回归测试 assignments 在 edit 后保持，delete 后全部清理。

**提交：** `refactor(models): route asset writes through plans`

### Task 5.4：切换 Skill 中央生命周期与外部导入

**文件：**

- Modify: `desktop/src/components/SkillsView.tsx`
- Modify: `desktop/src/components/SkillsView.test.tsx`
- Modify: `desktop/src/components/SkillInstallDialog.tsx`
- Modify: `desktop/src/components/SkillInspector.tsx`
- Modify: `desktop/src/components/SkillReviewDialog.tsx`
- Modify: `desktop/src/hooks/useSkillsState.ts`
- Modify: `desktop/src/lib/api.ts`

**步骤：**

- [ ] 顶层主操作文案改为“添加到资产库”，commit 后 consumers 保持空。
- [ ] external Skill 导入中央后不保留隐式 Agent assignment；原 observed copy 继续只读显示直到用户管理关系。
- [ ] update/delete/repair review 适配统一 operation envelope，保留风险 finding 二次确认。
- [ ] 删除 SkillsView 中重复的 assignment planner；所有消费者变更转给 useConsumptionState。
- [ ] useSkillsState 只保留中央 Skill source/update/recovery state，避免和 consumption hook 双写 assignments。
- [ ] 运行全部 Skills tests 与跨领域 operation tests。

**提交：** `refactor(skills): centralize asset lifecycle`

### Task 5.5：统一 recovery UI 并删除旧 Desktop mutation surface

**文件：**

- Modify: `desktop/src/App.tsx`
- Modify: `desktop/src/components/AssetOperationReviewDialog.tsx`
- Modify: `desktop/src/components/ResourceState.tsx`
- Modify: `desktop/src/hooks/useConsumptionState.ts`
- Modify: `desktop/src/lib/api.ts`
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/tests/consumption_commands.rs`

**步骤：**

- [ ] 启动时检测 unfinished asset journal；相关领域进入 recovery read-only，其他领域可继续只读浏览。
- [ ] 提供“继续到目标状态”“回滚 operation”“重新输入凭据并重新计划”明确入口。
- [ ] recovery 完成后强制 rescan/post-verify，再解除只读。
- [ ] 删除不再被 Desktop 使用的 legacy mutation invokes 和 command registrations。
- [ ] 添加静态 contract test，禁止 Agent/central view import legacy mutation APIs。
- [ ] 搜索 `applyInstall|uninstall|enableMcp|disableMcp|deleteMcp|applyModelProfile|planSkillAssignment`，只允许兼容层或 core tests 中出现。
- [ ] 全量 Desktop、Tauri、Rust tests 通过。

**提交：** `refactor(consumption): remove legacy desktop writes`

---

## Phase 6 — 端到端验证、文档与正式安装版验收

### Task 6.1：建立三领域端到端 lifecycle suites

**文件：**

- Create: `core/tests/central_assets_e2e.rs`
- Create/Modify: `desktop/src/components/CentralAssetsFlow.test.tsx`
- Modify: `desktop/src/test/consumptionFixtures.ts`
- Modify: `desktop/src-tauri/tests/consumption_commands.rs`

**每个领域验证：**

- [ ] 创建/导入中央资产，证明 Agent target 不变。
- [ ] 从 Agent 建立消费并验证 target + relationship。
- [ ] 从资产 Inspector 增减 consumers，验证与 Agent 入口等价。
- [ ] 编辑中央资产并传播全部 consumers。
- [ ] 制造外部漂移，证明整个更新被阻止且没有部分写入。
- [ ] 审阅后修复并恢复 synced。
- [ ] 删除中央资产并级联清理关系、target 和 credential/link/snapshot。
- [ ] 重新启动 inventory，证明状态来自持久化 desired + observed，而非 React cache。

**提交：** `test(consumption): cover central asset lifecycles`

### Task 6.2：更新用户文档和架构约束

**文件：**

- Modify: `AGENTS.md`
- Modify: `README.md`
- Modify: `website/guide/agents.md`
- Modify: `website/guide/models.md`
- Modify: `website/guide/skills.md`
- Modify: `website/en/guide/agents.md`
- Modify: `website/en/guide/skills.md`
- Modify if present: `website/en/guide/models.md`

**步骤：**

- [ ] AGENTS 写入中央资产、desired relationship、observed evidence 和禁止 Agent-scoped install 的稳定边界。
- [ ] README 用用户语言解释“先加入资产库，再让 Agent 使用”。
- [ ] 中英文指南删除 Agent 页面安装 Skill 和扫描自动入库描述。
- [ ] 文档说明 MCP adoption、外部资产导入、漂移、级联删除和 Model 单选。
- [ ] 不复制可从 catalog 或代码发现的 Agent 清单。
- [ ] 运行 Website build 和 link/static checks。

**提交：** `docs(arca): explain central asset consumption`

### Task 6.3：全量门禁与正式安装版 UI 验收

**自动门禁：**

```bash
cargo fmt --check
cargo test --workspace
cargo test --manifest-path desktop/src-tauri/Cargo.toml
cd desktop
npm ci
npm test
npm run check:agent-icons
npm run build
npm run tauri -- build --debug --no-bundle
cd ../website
npm ci
npm run build
node ../scripts/release-version.mjs check
```

**正式安装版手工验收：**

- [ ] 在获得当次明确授权后构建、签名或替换 `/Applications/Arca.app`；没有授权则只报告自动门禁，不能用 dev/mock 冒充。
- [ ] `1200x820` 与 `900x600`，浅色和深色各验证一次。
- [ ] 顶层 Skills 安装不出现 Agent 选择。
- [ ] Agent Skills 不出现 GitHub URL、本地目录或“安装 Skill”。
- [ ] MCP/Skills 多选和 Model 单选行为正确。
- [ ] 外部资产与 desired relationships 视觉分层。
- [ ] 双向管理、中央更新、漂移阻止、修复和级联删除完整走通。
- [ ] Dialog 无 clipping、横向 overflow、重复 Escape、焦点丢失或 pending dismissal。
- [ ] DOM、console、截图和剪贴板预览不包含秘密。

**最终提交（只在验收修复产生改动时）：** `fix(consumption): complete installed app validation`

## 完成判定

- 所有计划任务与自动门禁通过。
- Agent 页面不存在任何资产来源安装或创建入口。
- 三类资源都有持久化 desired relationship、可解释 observed state 和统一 plan/review/commit。
- 中央更新、删除与双向关系管理不存在部分写入或两套状态。
- MCP 自动探索不再写中央 Registry；旧 discovered 数据和用户现有配置未被破坏。
- Model Profile 编辑传播 consumers，不清除 assignment。
- Skills 中央 install 与 Agent assignment 在 core、wire 和 UI 三层均已拆开。
- legacy Desktop mutation surface 已删除或仅作为非 Desktop 兼容 wrapper 保留。
- Arca 工作树干净；提交、push、PR、Pre-release、Stable Release 和安装版替换分别按用户当次授权交付。
