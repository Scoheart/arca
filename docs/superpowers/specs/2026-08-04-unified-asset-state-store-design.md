# Arca 统一资产状态仓储与语义前置条件设计

## 状态

- 日期：2026-08-04
- 状态：已实现
- 范围：MCP、Model、Skill 的中央状态读取、desired relationship 与 Asset operation precondition

## 决策

Arca 统一资产的**状态访问和生命周期契约**，不把三类异构 payload
复制进新的磁盘 manifest：

```text
AssetStateStore
  ├── CentralAsset(MCP | Model | ModelProvider | Skill)
  ├── AssetConsumers(exact central asset)
  ├── AgentConsumption(capability, agent)
  ├── AgentConfiguration(capability, agent)
  ├── SkillTargetGraph
  └── CredentialPresence(profile | provider)
```

现有唯一权威保持不变：

- MCP：有序 source 定义与 `sources/{remote,local}` 缓存；
- Model：`settings.json` 中的 Provider/Profile，凭据仅在 Keychain；
- Skill：`settings.json` 中的 metadata/relationship 与
  `~/.arca/skills/<name>` 中的一份正文。

因此不创建 `assets.json`，不移动 Skill 目录，也不改变 Keychain service。

## 语义 revision

旧 Asset operation 把整个 `settings.json` 的字节 hash 作为前置条件。
任何无关 UI、network、其他 Agent 或其他资产变化都会让已审阅计划失效。

operation schema v3 改为持久化 secret-free `StatePrecondition[]`：

```text
subject + SHA-256(canonical semantic projection)
```

计划只绑定真实读取和写入集合：

- MCP source copies 保留 precedence、shadow 与 in-effect 状态；
- Model Profile 同时绑定它引用的 Provider；
- Skill 同时绑定 metadata 与中央正文 tree hash；
- desired relationship 按 capability 和 Agent 投影；
- 中央更新、重命名和删除额外绑定 exact asset 的全局反向消费者集合，
  避免审阅后新增消费者被遗漏；
- target graph 与 credential 只保存结构或 presence，不保存内容；
- operational incident、UI 和 network 默认不进入资产 revision。

同一次 capture/verify 会复用 MCP catalog 与 Skill inventory 投影，避免多消费者
计划在 settings lock 内重复解析相同 source 和 target graph。

Agent 原生配置仍使用逐 target 的 anchored content fingerprint；settings
路径只绑定安全父目录与 regular-file slot，不再绑定无关内容或原子替换产生的 inode。

## 兼容和安全

- operation schema v2 继续读取、验证和恢复；不能因升级丢弃已有 rollback evidence。
- v3 candidate hash 覆盖全部 typed precondition 与 settings namespace guard。
- settings schema 高于当前支持版本时拒绝读写，旧 binary 不得降级覆盖未知嵌套字段。
- 所有 revision material 只落 SHA-256；MCP env/header、Model API key 和 Skill
  文件内容都不进入计划、日志或前端 wire。
- `settings.json` mutation 继续走跨进程锁、CAS、私有临时文件和原子替换。
- rollback 以捕获的 exact anchored settings state 作为首次写入 CAS；快照后出现的
  非协作写入会拒绝当前 operation，且没有 write evidence 时不会被 rollback 覆盖。

## 验收

1. UI/network 和无关中央资产变化不会使 MCP 计划 stale。
2. 相关 source copy、Provider/Profile、Skill metadata/tree 或 Agent relationship
   变化会精确使计划 stale。
3. Model legacy active pointer 与 canonical installed selection 得到相同 revision。
4. operation v2 可继续 load、commit 和 recovery。
5. future settings schema 写入失败且原始字节不变。
6. 空 HOME 查询与规划仍遵守现有隔离和私有权限契约。
7. 中央计划审阅后新增同一资产消费者会 stale；新增无关资产消费者不会 stale。
8. Agent writer 路径审阅后新增 MCP/Model relationship 会 stale，不能绕过
   `agent_configuration_in_use` 门禁。
