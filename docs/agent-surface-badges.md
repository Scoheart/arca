# Agent 形态角标

右下角的小角标表示产品形态，用来区分共用品牌图标的不同 Agent。它不表示安装状态、运行状态、固定状态或资源同步状态。

## 五种形态

| 值 | 中文 | 符号 | 颜色 | 示例 |
| --- | --- | --- | --- | --- |
| `cli` | 命令行 | 终端提示符 | 石板灰 | Cursor CLI、Codex CLI |
| `desktop` | 桌面应用 | 显示器 | 蓝色 | ChatGPT、Claude Desktop |
| `ide` | IDE 编辑器 | 代码括号 | 紫色 | Cursor、Qoder |
| `plugin` | IDE 插件 | 插头 | 琥珀色 | Codex IDE、Cline |
| `web` | Web 应用 | 地球 | 青绿色 | ChatGPT、Claude 网页版 |

## 数据与显示规则

- `desktop/src/assets/agents/aliases.json` 决定使用哪个品牌图标。
- `desktop/src/assets/agents/surfaces.json` 显式声明已核验的产品形态。不能从显示名或宽泛的 catalog category 猜测形态。
- `brandIcons.tsx` 按最终图标 key 分组；同一图标存在至少两种明确形态时，为组内已标注的 Agent 显示角标。
- 缺少图标、缺少明确形态、只有一种形态的图标组、自定义未知 Agent 都不添加推断出来的角标。
- Cursor 和 Cursor CLI 原来都缺少形态声明。现在分别标为 `ide`、`cli`，保留共用的 Cursor 品牌图标。
- WorkBuddy 的两个发行版都为 `desktop`，因此不使用形态角标消歧；Qoder 独立桌面版使用不同品牌图标，也不强行添加角标。

## 组件职责

`AgentGlyph` 统一品牌图标、碰撞判断、名称和本地化形态说明。所有既有使用位置自动获得相同表现，包括固定栏、选择卡片、详情页、编辑对话框和资源消费者。

`AgentSurfaceBadge.tsx` 维护五种合法类型、各自的简化 SVG 和尺寸规则；`AgentSurfaceBadge.css` 维护位置、颜色、边框、隔离环和强制对比模式。页面不添加 Cursor 专用的条件判断，也不维护另一套角标样式。

| 品牌图标尺寸 | 角标尺寸 |
| --- | --- |
| 小于 20px | 10px |
| 20–24px | 12px |
| 25–36px | 15px |
| 37–52px | 19px |
| 大于 52px | 24px |

角标位于右下角，不改变品牌图标的布局尺寸。品牌图标不大于 36px 时向外偏移 2px，较大图标偏移 3px，避免紧凑固定栏与消费者列表的角标挤到边缘。圆角按角标尺寸的 28% 取整，保持小尺寸下的圆角方形。白色描边和随主题变化的隔离环区分角标、品牌图标与背景；符号本身也有区别，不仅依赖颜色。角标不拦截点击。

悬停说明和品牌图标的无障碍名称均包含产品名和形态，例如「Cursor · IDE 编辑器」。角标 SVG 本身为装饰元素，不额外获得焦点或重复朗读。
