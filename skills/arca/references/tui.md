# Arca MCP TUI

在目标用户的交互终端运行无参数的 `arca`。TUI 提供 Registry、Sources、Agents 三个 MCP 页面。Model、凭据和 Skills 用 [CLI](cli.md) 或 [桌面 GUI](gui.md)。无参数命令打印帮助时，检查 `ARCA_NO_TUI` 是否存在以及当前环境是否有交互终端。

```bash
arca --version
env -u ARCA_NO_TUI arca
```

## 键位和焦点

| 场景 | 按键 |
| --- | --- |
| 主导航 | `1` Registry、`2` Sources、`3` Agents。Tab 顺序切换，Shift+Tab 反向切换 |
| 普通列表 | ↑/↓ 或 `k`/`j` |
| 帮助、重读、退出 | `?` 显示帮助，Ctrl+R 重读全部，导航中用 `q` 或 Ctrl+C 退出 |
| Registry 浏览 | `/` 搜索，`[`/`]` 或 ←/→ 切换来源筛选，Enter 查看详情 |
| Registry 处理条目 | `n` 新建、`e` 编辑、`p` 粘贴、`i` 分配、`d` 审阅移除 |
| Sources 管理 | `s` 订阅，`l` 添加本地来源，`o` 带入精选订阅，`r` 刷新，`d` 审阅移除 |
| Sources 开关 | Space 或 Enter 立即提交所选来源的启停 |
| Agents 左栏 | Enter/→ 进入已有 MCP 列表，`a` 加入中央 MCP，`e` 编辑配置，`n` 新建 Agent |
| Agents 左栏开关 | Space 改当前 Agent 的启停设置 |
| Agents 右栏 | Space 启停选中 MCP，`d` 审阅解除关系，Esc/← 返回左栏 |

搜索框、表单和弹窗优先接收按键。Registry 搜索框中 Esc、Enter 或 ↓ 退出输入，可能保留当前查询。需要清空时重新按 `/`，删掉搜索词，再退出输入。帮助和详情可以用 Esc、Enter 或 `q` 关闭，然后再用主导航键。确认框的 `y` 或 Enter 会执行变更，Esc 取消。

## 找到准确 MCP

1. 按 `1` 进入 Registry。回到全部来源后按 `/` 输入 MCP 名称。
2. 退出搜索输入，再用方向键定位。确认来源、传输和端点或命令。同名 STDIO 和 HTTP 是两个条目，SSE 归在 HTTP 身份下。
3. Enter 看详情，核对即将使用的连接。详情里 `e` 会进入编辑。复制含密钥的完整配置时转到任务授权的安全目的地，相关边界见 [MCP 配置与导出](workflows/mcp.md)。

搜索、来源筛选和选中条目分别影响结果。预期条目不可见时先清空搜索、切回全部来源，再看 Sources 状态和 Registry 中的准确传输。

## 新建或编辑中央 MCP

1. Registry 按 `n` 新建，已有条目按 `e`。用 ↑/↓ 移动字段，Enter 进入文本编辑，再用 Enter 或 Esc 结束字段输入。字段导航状态按 Esc 放弃当前编辑页。
2. 新建时在 **传输** 字段按 Enter 切换 STDIO / HTTP。已有条目的名称或传输是否可改，以表单锁定提示为准。
3. STDIO 填可执行命令，参数可写为 JSON 字符串数组，环境可写为 JSON 键值对象。HTTP 填类型、完整 URL，以及按需提供的 Header 键值对象。通用字段包括名称、描述、逗号分隔标签与仓库 URL。
4. Ctrl+S 提交。回 Registry 核对新条目和来源。提示同名同传输冲突时，应定位原条目或来源，再选择编辑或建立另一准确身份。

Registry 的 `p` 可粘贴已有配置。粘贴窗口的 Enter 添加换行，Ctrl+S 识别并提交，Esc 放弃。多条配置、批量影响或来源覆盖规则较复杂时，可先用 [MCP 手册](workflows/mcp.md) 核对输入和写入范围。

## 把一个 MCP 分配给 Agent

**从 MCP 找 Agent**

1. Registry 选中准确 MCP，按 `i` 打开 Agent 选择器。选择器按可用全局路径和传输过滤，初始无勾选。
2. 用方向键移动，Space 勾选用户要求的 Agent。Ctrl+A 会切换全部选中或全部取消，只有确实要处理全组时使用。
3. Enter 提交已选目标，Esc 放弃。若没有兼容 Agent，到 Agents 确认能力，或从 CLI 查 `agent list` 与域内 `status`。

**从 Agent 找 MCP**

1. 按 `3` 到 Agents，在左栏定位准确 Agent，按 `a`。
2. 输入名称搜索，用方向键选准确传输，Enter 提交给当前 Agent。可选列表会过滤已启用的条目和不支持的传输。原有已停用关系优先到右栏启用。
3. 左栏按 Enter 或 → 进入已有 MCP 右栏。无条目时焦点会留在左栏。右栏核对名称、传输和 ✓ / ✗。页签切换会保留原有栏位焦点，再次进入 Agents 时看选中行和高亮所在栏。

验收持久化状态时，可退出 TUI 后用同一用户和数据根执行 `arca agent mcp status --agent AGENT --json`。查 Agent 打开后能否真正加载该服务，要看其 MCP 面板、原生配置状态或相关错误。[MCP 分配验收](workflows/mcp.md)

## 管理来源

| 场景 | 操作及核对 |
| --- | --- |
| 加远程订阅 | 在 Sources 按 `s`，填 URL 和可选名称，Tab 或方向键切换字段，Enter 提交。回 Registry 选此来源看条目 |
| 导入本地 MCP 来源 | 按 `l`，填当前设备上可读的配置文件路径和可选名称，Enter 提交。核对识别结果与来源名称 |
| 添加官方精选 | 按 `o` 打开预填好的订阅表单，核对地址和名称后提交。它属于远程来源 |
| 刷新来源 | 选准确来源按 `r`。刷新后查生效条目以及已有 Agent 消费关系 |
| 停用来源 | 确认光标在 Sources 中的目标行，再用 Space。回 Registry 查同身份是否由其他来源提供 |
| 移除来源 | `d` 打开确认。检查名称、来源范围以及实际消费者，再决定提交或取消 |

手动和自动管理来源的可用动作由 Core 限定。如果 TUI 只显示一句失败或意外结果，用 [来源命令](workflows/mcp.md) 查询准确 ID 和计划细节。更新订阅与启动 MCP 服务是独立操作。

## 取消、重读和恢复

- 想只停用某个 MCP 时，先从右上详情确认 Agent ID，核对右栏所选名称和传输。对当前已启用的受管项在右栏按一次 Space，已经停用则保持。Agents 左栏的同一个键作用于 Agent，Sources 页作用于来源。
- 验证影响时用 `arca agent list --json` 确认 Agent 的设置，用 `arca agent mcp status --agent AGENT --json` 比较关系的 `enabled` 与 `observed_enabled`。右栏 ✓ / ✗ 来自实际配置与停用快照。只拿到事后状态时，可确定目前状态和是否存在漂移。
- 希望移除一个 Agent 的关系时在右栏按 `d`。Registry 页的 `d` 作用于中央条目，Sources 页的 `d` 作用于来源。
- Ctrl+R 重读前返回主导航。状态提示被截断时，可扩大终端并查 CLI 对应域状态或 Arca 运行日志。明确计划、共享目标和局部失败详见 [同步与恢复](workflows/recovery.md)。
- 按键不生效时先根据标题、搜索框或弹窗判断当前焦点。按该层的退出键回列表，再切换页面。

[Arca 4.0.2 TUI 说明](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md#tui-键位)
