# 安装

Arca 有桌面 App 和 CLI / TUI 两个入口，共享 `~/.arca/`。桌面 App 已内置 CLI，通常无需重复安装。

## 桌面 App（macOS）

1. 打开[最新正式版 Release](https://github.com/Scoheart/arca/releases/latest)，选择标注为 **Desktop installer · Apple Silicon** 的资源（文件名形如 `Arca-Desktop-Installer-*-macOS-Apple-Silicon.dmg`）。
2. 打开 dmg，把 **Arca.app** 拖进 `/Applications`。
3. 首次打开。

不要直接双击 dmg 里的 Arca 长期使用。dmg 是只读磁盘映像，应用无法在其中替换自身；
直接运行时，Arca 会在更新前提示先将 App 安装到“应用程序”。

### 更新提示只读文件系统？

这表示 Arca 仍从 dmg 或 macOS 的 App Translocation 隔离目录运行。退出当前 Arca，
将 dmg 里的 **Arca.app** 拖到 `/Applications`（已有旧版时选择替换），再从“应用程序”
重新打开。目录与 Agent 状态保存在 `~/.arca/`，替换 App 不会删除现有配置。

### 提示「Arca 已损坏，无法打开」？

当前发布包未经过 Apple Developer ID 公证，macOS 可能因隔离属性（quarantine）阻止首次启动，并不代表文件损坏。确认下载来源是本项目 Release 后，可以清掉隔离属性：

```bash
xattr -dr com.apple.quarantine /Applications/Arca.app
```

然后正常打开。（或者：右键 App → 打开 → 在弹窗里再点"打开"。）

## 命令行 / TUI（`arca`）

`arca` 是一个原生 Rust 二进制，和桌面 App 共用同一个核心。

### 方式一：随桌面 App 安装（推荐）

正式版 App 启动后会把包内 CLI 软链到 `~/.local/bin/arca`。如果终端找不到 `arca`，把目录加入 `PATH`：

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
arca --version
```

这个软链随 App 自动修复；App 更新后，CLI 也同步更新。

### 方式二：单独下载预编译二进制

在正式版 Release 中选择标注为 **Command-line tool · Apple Silicon** 的资源，文件名为 `arca_v<版本>_aarch64-apple-darwin.tar.gz`：

```bash
# 到 Releases 下载后：
tar xzf arca_v*_aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/bin
mv arca ~/.local/bin/arca
arca --version
```

独立安装的 CLI 可以运行 `arca upgrade` 跟随最新正式版；桌面 App 自带的 CLI 会提示改由 App 更新。

### 方式三：从源码安装（需要 Rust）

```bash
git clone https://github.com/Scoheart/arca
cd arca
cargo install --path cli       # 装到 ~/.cargo/bin/arca
```

### 用法

无参数运行进入**交互式 TUI**：

```bash
arca
```

也可以用子命令脚本化（设 `ARCA_NO_TUI=1` 让无参数时打印帮助而不进 TUI）：

```bash
arca mcp list
arca mcp assign github::stdio --agent claude-code --yes
arca model use work --agent pi --yes
arca skill assign review-changes --agent codex --yes
arca mcp export --out mcp.json --yes
arca agent list
arca upgrade --yes
```

Agent 关系写操作必须使用准确资产 ID 和显式 `--agent <id>`。脚本可使用全局 `--json`、`--yes`、`--dry-run` 和 `--no-color`；`--yes` 与 `--dry-run` 互斥。

详见 [命令行 / TUI](/guide/cli)。

## 数据放在哪

所有用户数据共享 `~/.arca/`。中央 MCP 来源、Model 元数据和 Skill 内容位于 `assets/`；设置与消费关系保存在 `settings.json`，审阅、恢复与备份分别保留在专用目录。密钥正文保存在系统 Keychain。详见 [数据布局](/guide/concepts#数据布局)。

下一步 → [核心概念](/guide/concepts)
