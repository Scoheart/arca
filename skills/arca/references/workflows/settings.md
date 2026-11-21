# 安装、工作区、偏好和更新

按当前用户指定的设备与安装实例操作。先确认任务是安装、检查可用版本、应用更新，还是修改某项配置。查询与执行分别选择相应入口。

## 识别当前安装

| 对象 | 查看方式 |
| --- | --- |
| 正式桌面 App | **设置 → 检查更新** 一行旁查看版本，同时确认当前运行的应用包位置 |
| PATH 上的 CLI | `type -a arca`、`command -v arca`、`arca --version` |
| macOS 正式 App 内置 CLI | `/Applications/Arca.app/Contents/MacOS/arca --version` |
| 版本与发布渠道 | 查看项目正式 GitHub Release 的版本和资产类型 |

4.0.2 的预编译桌面版和 CLI 发布面向 macOS Apple Silicon。桌面安装使用官方 DMG，把 `Arca.app` 拖到 `/Applications`，再从应用程序打开。正式 App 可以创建 `~/.local/bin/arca` 到包内 CLI 的链接，终端找不到时查当前 PATH，先评估本次 shell 的路径调整。有其他独立 CLI 时可继续用绝对路径判断是否指向目标版本。[安装说明](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/install.md)

## 确认数据根与配置归属

| 数据 | 默认位置或处理方式 |
| --- | --- |
| Arca 数据根 | `~/.arca`。`ARCA_HOME` 可按进程设为目标数据目录本身 |
| Arca 设置与消费关系 | `settings.json` |
| 中央资源 | `assets/mcps/`、`assets/models/`、`assets/skills/items/` |
| 计划、事务与备份 | `staging/`、`journals/`、`backups/` |
| 运行诊断 | `logs/`，路径用 `arca logs path --json` 查看 |
| 新建 Provider 的 Key 正文 | Provider 所选的凭据存储。中央元数据保存引用 |
| Agent 现场 | Agent 的原生全局文件与用户级目录。用 Agent 配置或域内 status 定位 |

同一用户、同一数据根下 CLI、TUI 和 GUI 可以互相验证中央数据。CLI 在其他用户、不同 `ARCA_HOME`、SSH 或隔离环境中运行时先核对实际范围。需要验证写入示例时使用独立临时目录和隔离凭据环境，并确保路径实际指向该目录。改变数据根不会自动切换 Agent 的原生用户目录。

对于数据判断，优先用 `workspace --json`、作用域较小的 `status` 和 GUI 详情。需要恢复时遵循 [同步与恢复](recovery.md)，避免直接编辑事务目录或用重建工作区替代定位问题。

## 桌面和启动偏好

```bash
arca settings show --json
arca settings terminal --json
arca settings locale --json
arca settings pins --json
```

| 操作 | GUI | CLI 预览写法 |
| --- | --- | --- |
| 设置默认终端 | **设置 → 默认终端** | `arca settings terminal TERMINAL_ID --dry-run --json`，ID 从终端查询输出选择 |
| 切换语言 | **设置 → 语言**，支持跟随系统 | `arca settings locale zh-CN --dry-run --json`，另有 `en-US` 和 `system` |
| 调整置顶 Agent | 选择器内置顶，顶栏可拖动排序 | `arca settings pins codex opencode --dry-run --json`，提供整份期望的有序列表 |
| 清空置顶 | 取消当前置顶 | `arca settings pins --clear --dry-run --json` |
| 调整外观和默认编辑器 | **设置 → 外观 / 文件编辑器** | 走桌面入口 |
| 核对最新现场 | **设置 → 重新扫描** | 按所需能力执行 `arca discover` 或 `status` |

在用户授权范围内用 `--yes` 写入，再读取对应偏好或窗口状态。默认终端作用于 Arca 的 CLI Agent 启动，已有终端会话按原状态运行。

## Arca 自身的代理

桌面进入 **设置 → 网络代理**。CLI 与 Desktop 共用保存的设置。

```bash
arca network proxy show --json
arca network proxy set socks5://127.0.0.1:7890 --dry-run --json
arca network proxy clear --dry-run --json
```

示例地址需换成用户实际代理。4.0.2 保存的代理 URL 支持 `http`、`socks4`、`socks4a`、`socks5`，填写协议、主机及端口，保持地址不含凭据、路径、查询或片段。设置后先重读代理设置，再用用户要完成的具体 Arca 联网动作验证。

Arca 的应用代理用于自身联网功能，例如远程来源、公开 GitHub Skills 和更新检查。目标 Agent、MCP 服务或 Computer Use 各自的进程、权限和联网路径独立核对。超时归因见 [诊断手册](diagnostics.md)。

## 更新或重启

1. GUI 先核对当前版本。用户只要求检查时查看 **设置 → 检查更新** 的结果。用户要求更新时沿实际正式版更新界面处理下载和重启，留意未提交表单与正在进行的操作。
2. 如果 App 在 DMG、只读卷或 App Translocation 中运行，退出此实例并按安装流程把官方 App 放到 `/Applications`，从实际安装位置启动。
3. 桌面包内 CLI 跟随 App 更新，独立 CLI 用 `arca upgrade`。`arca upgrade --dry-run --json` 用于识别管理类型和更新意图。独立 CLI 的真实版本检查和更新走已授权的 `arca upgrade --yes --json`。
4. 重启或更新后，检查应用包或 `arca --version`、目标窗口和资源列表。多个同名窗口同时存在时确认实际打开的是目标实例。

`ARCA_NO_UPDATE_CHECK=1` 只作用于当前环境的普通 CLI 更新检查。开发构建、正式安装与版本分支需按用户指定的目标核对。[正式 Release](https://github.com/Scoheart/arca/releases) · [发布版 CLI 更新说明](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md#更新)
