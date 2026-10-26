# Agent Model 真实配置权威设计

## 背景

Agent 页面当前把 Arca desired consumption 与 Agent 真实配置混合展示。“清空 Models”只把 desired Model 集合设为空，再删除能通过当前 Profile ID 证明由 Arca 拥有的配置项。外部配置、旧版 Arca 配置和无法重新关联到当前 Profile 的条目仍会被重新扫描并显示，于是出现“0 个已添加”但仍有大量 Model 卡片的矛盾状态。

以 Pi 为例，当前真实状态是：Arca managed 集合为空，但 `~/.pi/agent/models.json` 仍有 8 个 Provider、16 个 Model。CLI 将它们投影为 6 个可导入 external、8 个 ambiguous、2 个 unsupported。这个结果符合旧规格“external observations remain unchanged”，但不符合用户要求：Agent 页面必须与 Agent 的真实 Model 配置同步；有原生 Model 存储的 Agent，添加和清空必须修改该存储；没有原生 Model 列表的 Agent，Arca 才维护映射。

## 目标

- Agent 页面中的 Model 列表、数量和当前 Model 与该 Agent 的实际能力和权威存储一致。
- 原生 Model registry Agent 以真实配置为权威；managed/external 只作为附加状态。
- mapping-only Agent 以 Arca 映射为列表权威，并把可写的当前 Model 槽位作为 observed 状态。
- `清空全部 Models` 对原生 registry 删除真实配置中全部 Model，包括手工添加和 external Model。
- 清空不删除中央 Models、Provider、Keychain 凭据或其他 Agent 的关系。
- 写入失败后页面继续显示真实配置，绝不先显示假空状态。

## 非目标

- 不删除中央 Models 库中的 Profile。
- 不删除中央 Provider 或系统 Keychain 凭据。
- 不删除 Agent 配置中的非 Model 字段、注释、未知字段或独立策略。
- 不把 external Model 自动导入中央 Models 库。
- 不为 `guided` Agent 绕过其安全限制或写入明文凭据。
- 不重新开放项目级 Model 配置。

## Core 能力模型

在 `ModelAgentCapabilityView` 和 Desktop 类型中增加明确的存储权威：

```text
ModelStorageAuthority = native-registry | arca-mapping | guided
```

React 只能消费该能力，不得根据 `config_paths`、`supports_multiple` 或 Agent ID 猜测。

### native-registry

Agent 配置可以持久化多个 Model 或 Provider/Model 条目。真实配置是列表权威。

当前归类：

- Pi
- Grok Build
- OpenCode
- Kilo Code CLI
- Qwen Code
- Crush
- Mistral Vibe
- Hermes Agent
- Factory Droid
- Goose

### arca-mapping

Agent 没有可安全管理的多 Model registry，只能表达一个当前 Model/endpoint，或需要 Arca 维护“已添加”集合。Arca mapping 是列表权威；实际当前槽位是 observed 状态。

当前归类：

- Claude Code
- Codex

### guided

Arca 没有安全、受支持的非交互 writer。页面保持只读引导，不提供 Arca 添加或清空。

当前归类：

- MiniMax Code
- Qoder

新增 Agent 时必须在 Core 同时声明 authority、observer、writer/clearer 和安全边界；缺少任一能力时不得进入 `native-registry`。

## 真实配置投影

### 原生 registry Agent

Core adapter 提供一个对称契约：

```text
observe_configured_models(paths) -> AgentConfiguredModel[]
prepare_clear_all_configured_models(paths, observation_revision) -> PreparedModelFiles
```

`AgentConfiguredModel` 使用 Agent 原生稳定身份，包含显示所需的名称、Model ID、Provider、协议、是否当前以及可选中央 Profile 匹配。中央匹配失败不会隐藏真实 Model，只把 ownership 标为 external/ambiguous/unsupported。

Agent 页面直接使用 `configured_models` 作为卡片与计数来源：

- 卡片数量等于真实配置中 observer 暴露的 Model 数量；
- 当前状态来自真实 active/default 指针；
- Arca desired/managed 状态叠加为 Badge 或收敛动作；
- 外部修改配置后刷新即可反映；
- Arca mapping 已清空但文件仍有条目时，条目继续显示。

observer 与 clear-all 必须覆盖完全相同的 Model surface：页面展示的每个原生 Model 都必须进入 clear-all 计划；clear-all 不得删除页面没有识别为 Model 的其他配置。

### mapping-only Agent

卡片和计数来自 Arca `model_consumptions`。若 Agent 有可观察的当前 Model 字段，Core 将其叠加为 synced、missing、drifted 或 external-current 状态。清空删除全部映射，并只移除 Arca 拥有的当前 Model/endpoint 字段。

### guided Agent

只展示 Core 能可靠观察到的状态和 Agent 自身设置入口。不得显示会写入 Arca mapping 但无法同步 Agent 的“添加 Model”或“清空 Models”按钮。

## 添加 Model

### native-registry

现有添加入口继续选择中央 Profile，但成功条件改为真实配置收敛：

1. Core 基于配置 revision 和目标哈希生成 reviewed plan。
2. commit 备份并原子写入 Agent 配置。
3. Core 重新观察真实配置。
4. 只有 `configured_models` 中出现对应原生身份才返回 converged success。
5. Desktop 使用返回的 observed inventory 渲染，不从刚提交的 desired selection 乐观插入卡片。

### arca-mapping

添加写入 Arca mapping；若 Agent 有可安全写入的当前槽位，按现有 writer 同步。页面从提交后的 mapping/inventory 重新渲染。

## 清空全部 Models

新增独立的 reviewed Core 操作：

```text
clear_agent_models(agent_id)
```

它不再复用 `set_agent_consumption` 的空 selection，因为后者只描述 desired 关系，无法表达删除 external 原生配置。

### native-registry 清空

计划必须列出：

- authority 为 `native-registry`；
- observer 发现的全部 Model 数量；
- 其中 managed、external、ambiguous、unsupported 的数量；
- 将修改的精确物理路径；
- 每个目标的候选哈希与风险提示；
- 明确文案“将删除手工添加和外部 Model”。

commit 顺序遵守现有跨目标不变量：

1. 持久化该 Agent 的空 Arca Model mapping。
2. 对每个物理 target 使用已审阅 revision/CAS 执行 adapter 的 clear-all candidate。
3. 每个 target 先备份，再同目录临时文件和原子替换；单 target 失败 fail closed。
4. target 失败形成最小 write set 的 model incident，不回滚已成功的无关 target。
5. 重新观察真实配置；只有 `configured_models` 为空才标记该 Agent Model target converged。

因为 UI 以真实配置为权威，即使中央 mapping 已先清空，target 写入失败时原有卡片仍存在，并显示 incident；不会出现“0 个已添加”但把真实 Models 当成已清空的成功状态。

### Pi 清空结果

Pi adapter 在同一 target transaction 中：

- 将 `~/.pi/agent/models.json` 的 `providers` 清为空对象；若文件不存在则不创建；
- 从 `~/.pi/agent/settings.json` 删除 `defaultProvider` 与 `defaultModel`；
- 保留两个文件中的其他字段、JSONC 格式、未知字段、权限和所有非 Model 设置；
- 不读取、删除或回显 Keychain 凭据；
- 清空后 observer 必须返回 0 个 configured Model。

这会删除 Pi 中所有 16 个真实 Model，包括 external、冲突、unsupported 以及旧式 `arca-*` 遗留条目。

### 其他 native adapter

每个 adapter 清空其 observer 所暴露的完整原生 Model registry 和耦合的 active/default 指针，同时保留该 Agent 的非 Model 配置。任何无法无歧义解析的文件必须在计划阶段 fail closed，不得退化为删除整个文件或重写未知结构。

### arca-mapping 清空

清除该 Agent 的所有 Arca Model consumption/assignment，并移除 Arca writer 拥有的当前 Model/endpoint 字段。外部当前槽位若无法证明由 Arca 拥有则保留并显示 drift，不删除整个配置文件。

### guided Agent

不提供 clear operation；页面引导用户在 Agent 内操作，刷新后重新观察。

## UI 契约

- 原生 Agent 标题显示“配置中 X 个”，不再显示仅 desired 的“X 个已添加”。
- mapping-only Agent 显示“Arca 管理 X 个”。
- `清空全部 Models` 对 native Agent 以 configured count 启用，对 mapping-only 以 mapped count 启用。
- review 必须突出 total/external 数量与目标文件；确认是唯一删除授权。
- commit 返回 `converged: false` 或 incident 时显示失败/待收敛，不显示成功 Toast。
- 清空成功后重新读取；native Agent 只有真实配置为 0 时页面才为空。
- central library、Provider、Keychain 和其他 Agent 不受影响。

## 安全与兼容

- 本功能是对“外部扫描结果只读”规则的一个显式、受审阅的 Agent-scope 例外：只允许专用 `clear_agent_models` 删除当前 Agent observer 已列出的全部原生 Models；普通卡片操作仍不能修改 external observation。
- 计划必须绑定 Agent capability revision、配置路径、候选哈希与文件 revision，避免审阅后配置变化导致删错。
- 配置损坏、重复键、路径歧义、跨 HOME、符号链接越界或并发变化均 fail closed。
- 测试必须隔离 `HOME`/`ARCA_HOME`，不得读取真实配置或 Keychain。
- release feature commit 不修改版本、Changelog 或 release-owned lockfile 字段。

## 测试策略

测试代码按破坏性边界覆盖，但遵守仓库极速模式：除非当前用户明确要求，不在本地执行测试/build；正式 Release 仍必须通过生产编译、签名和四资产验证。

Core：

- capability matrix 精确覆盖当前 14 个 Model Agent；
- Pi mixed fixture 含 managed、external、ambiguous、unsupported 和未知字段，clear-all 后 `providers` 为空、默认指针删除、其他字段字节语义保留；
- 每个 native adapter 的 observer surface 与 clear-all surface 对称；
- mapping-only 只清 mapping/Arca-owned current fields；
- guided 拒绝 clear；
- stale hash、重复键、格式损坏、路径冲突和写入失败 fail closed；
- central Models、Providers、Keychain 与其他 Agent 不变；
- target 失败后 mapping 可为空，但 observed inventory 仍返回真实 Models 和 incident；
- post-write observation 非零时 commit 不得返回 converged success。

Desktop：

- native count 来自 configured inventory，mapping count 来自 consumption；
- native external rows 能启用 clear-all；
- review 显示删除总数和 external 风险；
- 失败后真实卡片保留且无成功 Toast；
- guided 不显示添加/清空；
- 添加 native Model 后只有 observed inventory 确认写入才显示。

## 验收标准

1. Pi Agent 页面显示的 Model 数量与 `models.json` observer 结果一致。
2. Pi 点击并确认 `清空全部 Models` 后，真实 `providers` 为空、默认指针删除、页面为 0。
3. 清空包含全部 external/手工 Model，不依赖 Arca ownership 或当前 Profile ID。
4. Claude Code/Codex 继续使用 Arca mapping，清空映射与 Arca-owned 当前覆盖字段。
5. MiniMax Code/Qoder 不伪装成可写 Agent。
6. 添加、外部修改、失败和重试后，页面始终由对应 authority 的最新 inventory 驱动。
7. 中央 Models、Provider、Keychain、非 Model 配置和其他 Agent 保持不变。
