# Arca 全链路品牌与升级迁移审计

日期：2026-10-13。基线：d5c1db8 / MUX 1.14.5。产品定位：Agent Asset Manager。

## 影响范围和决定

| 层 | 改造 | 兼容边界 |
|---|---|---|
| Desktop | Arca 字标、标题、提示、中英文文案、Tauri productName、com.scoheart.arca | 图标本身没有 MUX 文字，保留现有吉祥物；旧应用内更新可能保留 MUX.app 文件夹名 |
| CLI / TUI | arca 命令、帮助、标题、arca-cli / arca-core、ArcaCore、sidecar、文档例子 | 旧 standalone upgrade 固定请求的 mux 归档/member 保留桥接，内部是同一 Arca 二进制；Desktop 只在 bundle identity 可验证时重新指向旧 CLI symlink，不覆盖其他同名工具 |
| Core / 前端 | 代码标识、CSS 类、DOM 事件、ARCA_*、arca-store / arca-mapping | 旧环境变量和序列化值仍可读，输出只使用新 API 名称 |
| 用户数据 | ~/.arca 为新默认物理根；旧根恢复成功后原地 rename | 不复制、不合并两份库；保留 ~/.mux → ~/.arca 只读兼容路径入口，实际内容只有一份 |
| 凭据 | 所有用户可见提示改为 Arca | Keychain service、rollback snapshot service 与 Agent 已写入 credential command 的服务参数保持原值；不读取所有密钥做改名 |
| 原生 Agent 身份 | 类型、函数与说明改为 Arca | UUID、模型 provider ID、__mux_* native metadata 是所有权协议，保留稳定值，不靠品牌变更接管外部条目 |
| 崩溃恢复 | 代码与诊断改名 | .mux-* temp claim 名称及 schema UUID seed 是已有 durable protocol；继续核验旧记录 |
| 发布 | Arca.app、Arca-Desktop-*、arca_vX.Y.Z_*、更新 manifest、仓库链接 | 更新签名公钥完全不变；旧 GitHub URL 依赖仓库改名后的 redirect，旧 CLI 桥接资产是唯一额外包 |
| 文档 | Arca 标题、组件、CLI reference、what-is-arca、安装/升级说明 | 旧指南 URL redirect；历史 changelog / 审计 / 视频不伪造重录，原媒体文件保留；现有域名需另行部署才能改 DNS |

不能做不区分语义的零残留替换。第三方 ZenMux 的名称/网址、第三方依赖校验值、公钥、历史记录和旧用户状态里的所有权 ID，都不属于新的品牌文案。

## 数据迁移顺序

1. 无写入地检查 ~/.arca 与 ~/.mux；新的根必须是真实目录。两份真实目录、外部 symlink、非目录发生冲突时将 bootstrap 标为 brand_migration read-only，不猜测该合并哪份资产。
2. ARCA_HOME 优先；兼容 MUX_HOME。显式重定向按用户选择保留，不自动移动。
3. 在原物理根获取现有跨进程锁，先恢复 global / Skill / asset durable transactions，再完成正常 schema/layout 维护。恢复未完成或能力维护受阻时延后迁移，保留旧根以继续诊断。
4. 创建私有、同步落盘的迁移 marker；用 NOREPLACE rename 把目录移到 ~/.arca。资产 inode、备份与现有锁 inode 不变。
5. 建立旧根到新根的 symlink，并 sync home。若 rename 后、alias 前进程退出，下一次安全启动补建 alias。
6. 只有验证 alias 确实指向当前根时，Core 才将旧私有根路径规范化。任意用户路径、Agent 配置文件、外部 Skills 不参与品牌迁移。

不要让旧 MUX 与 Arca 并行操作同一资产。新 API 使用 arca-store，但旧客户端不知道它的语义；这也是该版本选择 major 的原因。

## 验证与实际边界

- 安装了校验过的独立 Rust toolchain；本地 Core/CLI 编译、前端 TypeScript/Vite 编译、双语 VitePress 构建完成。Cargo / npm 锁文件的第三方完整性数据原样保留。
- 使用真实编译的 CLI 和隔离 HOME 做运行验证：旧目录整体迁移、Skill symlink 内容与 inode、备份、未知 settings 字段、重复启动、两份真实目录冲突、rename 后恢复 alias、ARCA_HOME 优先及 MUX_HOME 显式目录保留。
- 原生 macOS 编译、codesign、DMG/App 与 updater 在正式发布流水线验证。本机是 Linux，不能冒充 Mac 原生 UI/安装验收。
- 依项目极速模式，没有运行 cargo/npm 测试套件、fmt、clippy 或 push preflight。现有测试与夹具随代码命名更新，保留后续手动验证入口。
- 旧 Provider 的 mux-store 元数据已用实际 CLI 验证可读，并在 API 输出为 arca-store。
- 旧 Keychain 状态需在 Mac 做安装验收；本机只核对 service 标识没有变动，未导出、打印或复制真实密钥。
- GitHub 仓库改名和 Release 的最终结果以交付时实际 API/下载核验为准；文档网站的源代码构建不等于现有 Vercel 站点已部署，更不等于新域名已配置。

## GitHub 管理权限边界

实测 repo metadata 返回 admin=true，但当前 GitHub integration 对 PATCH 仓库改名返回 HTTP 403 Resource not accessible by integration。不能把 admin 展示字段当作 API administration 权限。产品/代码/发布包仍按 Arca 交付；为避免把 updater、CLI upgrade、精选来源、issue/edit/release 链接指向不存在的仓库，对外运输 URL 暂留 Scoheart/mux。仓库所有者可在 Settings 将其改为 arca，旧地址由 GitHub redirect 继续工作。不要创建一个同名的新空仓库替代原仓库，也不要复制/覆盖已有 Stable tags。
