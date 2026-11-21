# Skills 来源、中央副本与分配

一个 Skill 目录以 `SKILL.md` 为入口，可带参考资料、脚本和资源。Arca 将候选保存为中央副本，再用链接给兼容 Agent 使用。先明确来源、候选名字、中央是否同名，以及这次是中央导入、分配、更新还是修一个实际目录。

## 从来源取得候选

| 来源 | CLI 选择 | GUI 选择 |
| --- | --- | --- |
| 可读的本地目录 | `--path "$SKILL_DIR"` | 为单个 Skill 选择它的目录，为一组选择共同上级 |
| GitHub 公开仓库或子目录 | `--github "$GITHUB_SOURCE"` | 输入仓库或精确子目录 URL |
| 本地压缩包 | `--archive "$SKILL_ARCHIVE"` | 选择 `.zip`、`.tar.gz`、`.tgz` 或 `.tar` |
| 现有 Agent 的外部副本 | `arca discover skill` 取得精确外部观测 | 查看对应 Agent 或 Skills 中的实际副本与可用导入动作 |

入口包含其相对路径依赖。传播包含多级参考的 Skill 时，保留整个目录结构。当前 Arca 操作范围是已核验的用户级目录，GitHub 来源使用公开访问。创建或编辑 `SKILL.md` 在选好的源码目录完成，再交给 Arca 导入或更新。

### 用 CLI 只选指定候选

```bash
arca skill inspect-source --path "$SKILL_DIR" --json
arca skill install --path "$SKILL_DIR" --name "$SKILL_NAME" --dry-run --json
arca skill install --path "$SKILL_DIR" --name "$SKILL_NAME" --yes --json
arca skill show "$SKILL_NAME" --json
```

1. 每次从三种来源选其一。`inspect-source` 会解析来源并返回候选名称与检查信息，GitHub 来源会联网。检查来源、候选、风险说明和同名中央项。
2. 把结果中的准确名称传给 `install --name`。有多个明确目标时重复 `--name`。比较计划影响与用户选择，然后执行已授权安装。中央安装保持现有分配及停用状态。
3. 计划要求风险确认时，阅读当前候选的 findings。当前内容已获明确同意才使用 `--accept-risk`，或用 [同一份计划审阅](../technical/cli-protocol.md) 提交程序给出的绑定确认。普通 `--yes` 是执行确认。

### 用桌面导入

进入 **Skills → 添加**，提交来源即开始导入。当前 GUI 导入该来源下找到的全部候选。只处理某个 Skill 时选择精确子目录，或用 CLI 明确筛选候选。同名中央副本先备份再替换，已有的 Agent 关系和停用状态保留。打开卡片看来源、中央副本、实际副本、风险和更新状态。

### 导入 Agent 内现有副本

```bash
arca discover skill --json
arca agent skill status --agent "$AGENT" --json
arca skill import "$EXTERNAL_ID" --dry-run --json
```

`EXTERNAL_ID` 从本次发现结果取得，区分不同实际目录中的同名副本。只想保存一份中央资产时审阅导入计划。已经有受管关系且希望接管外部更改时，根据状态选择 `converge`。[外部观测和接管](recovery.md)

## 分配给 Agent 与选择物理目录

```bash
arca agent list --json
arca agent skill status --agent "$AGENT" --json
arca agent skill assign "$SKILL_NAME" --agent "$AGENT" --dry-run --json
arca agent skill assign "$SKILL_NAME" --agent "$AGENT" --yes --json
arca agent skill status --agent "$AGENT" --json
```

1. 从 `agent list` 识别身份、Skills 能力和已核验路径。GUI 进入 **选择 Agent → Skills → 添加**，选择准确中央 Skill。
2. 预览实际目标和 `affected_agent_ids`。同一物理目录可能被多个 Agent 读取，例如 Codex CLI 与其桌面入口可能共享用户 Skills。按用户授权核对全部影响。
3. 执行后在关系状态中确认中央名、实际目录、链接和启停状态。默认数据根下的受管链接指向 `~/.arca/assets/skills/items/<skill-name>/`。本地真实路径来自本次状态。
4. 用户需要验证加载时，在目标 Agent 开新会话或用该 Agent 支持的重新加载方式验证 Skill 可用。中央卡片出现仅能证明中央资产已导入。

## 停用、解除或删除

| 意图 | 命令与影响 |
| --- | --- |
| 暂停在某个目标加载 | `arca agent skill disable NAME --agent AGENT`。保留关系及中央内容 |
| 恢复启用 | `arca agent skill enable NAME --agent AGENT`。复查链接与目标状态 |
| 多个物理目标里只切一份 | 上述 enable / disable 加 `--target TARGET_ID`。ID 从本次关系状态选 |
| 移除该 Agent 的关系 | `arca agent skill unassign NAME --agent AGENT`。中央保留，只清理可证明属于 Arca 的目标 |
| 删除中央 Skill | `arca skill remove NAME`。审阅全部消费者与中央备份 |

先用 `--dry-run --json` 核对目标。外部目录、普通文件和指向别处的链接属于各自的现场内容，与受管链接分开处理。`assign` 参数是 Agent 和中央 Skill，目标按 Core 的分配计划确定。

## 更新与同名修改

```bash
arca skill check-updates --dry-run --json
arca skill check-updates --yes --json
arca skill update "$SKILL_NAME" --dry-run --json
```

- 检查更新可联网并刷新检查缓存，中央正文在执行更新时才改变。GitHub 跟踪源先检查可用 revision，再计划更新。固定版本和外部导入来源没有普通上游更新路径。文件夹或压缩包按记录的来源读取。
- 点开详情区分上游更新、中央本地修改和另一份外部副本。经由受管链接编辑的文件会改中央内容。
- 只有当前任务明确要覆盖本地修改或冲突时，依据安装或更新的 `--help` 使用该入口提供的替换标志并重新审阅。覆盖范围、来源 revision、备份与风险一起核对。上游 revision 变了但内容相同时，可以只更新版本元数据。
- 更新后重查中央内容状态和已有关系。复查一个目标的实际文件或新会话，验证它读取了预期中央版本。

## 修复一个失败目标

先读取 `pending_convergence` 详情与当前 `agent skill status`。已经成功的实际目录保留。只有具体目标确认为已分配且启用、中央内容有效、链接缺失或符合受管断链条件时，审阅以下修复：

```bash
arca skill repair "$SKILL_NAME" --target "$TARGET_ID" --dry-run --json
```

计划显示可以安全执行后提交并重查同一目标。现有外部目录、异向链接、中央内容损坏或目标冲突各有不同处理。中央修复用产品实际提供的计划和当前来源，受管现场漂移按 `available_actions` 处理。详见 [同步与恢复](recovery.md)。

[Arca 4.0.2 Skills 指南](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/skills.md) · [发布版 Skill 命令](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md#skill)
