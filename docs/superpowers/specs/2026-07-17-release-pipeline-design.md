# Arca GitHub 交付与自动发布设计

日期：2026-07-17

状态：用户已批准核心流程，等待书面规范复核

## 1. 目标

将 Arca 从“允许直接推送 `main`、每次推送直接创建 Pre-release、正式版本靠人工改版本和打 tag”改造为可审查、可恢复的持续交付流程：

1. `main` 只接受 Pull Request，合并前必须通过 CI。
2. 每个普通 PR 合并到 `main` 后自动生成一个可安装的 Pre-release 包。
3. Release Please 为 `main` 上尚未正式发布的改动维护唯一一个 Release PR。
4. 只有用户合并 Release PR 才启动正式发布；无需再手工同步版本、整理 Changelog 或创建 tag。
5. 正式 Release 在 DMG、CLI、Updater 资产全部构建、签名和验证完成前保持 Draft，不能进入客户端自动更新通道。
6. CI 和构建在不牺牲发布完整性的前提下缩短等待时间，并保留失败后的安全恢复路径。

## 2. 非目标

- 不把每个普通 `main` 合并都发布为正式版本。
- 不把 Desktop、CLI 和 Core 拆成独立版本或多个 Release PR。
- 不引入长期维护分支、Merge Queue、强制他人审批或 CODEOWNERS 审批。
- 不改变 Tauri updater 的稳定通道 URL，也不让 Pre-release 进入自动更新。
- 不在本设计阶段修改工作流、GitHub Ruleset、Secret、Release 或正式安装包。

## 3. 当前状态与问题

当前仓库没有 Branch Protection 或 Repository Ruleset，管理员可以直接推送 `main`。`quality-monitor.yml` 和 `build-desktop.yml` 都在 `main` push 时运行；后者会为每次 push 创建唯一 Pre-release，并在 `vX.Y.Z` tag push 时创建正式 Release。

实测结果：

- Ubuntu 质量检查通常约 1 分 40 秒至 2 分钟。
- macOS 热缓存构建通常约 3 至 5 分钟。
- Rust 缓存失效时 macOS 构建可接近 9 分钟。
- 主要瓶颈是 Tauri/Rust 的 `Build .dmg`；发布和 CLI 增量编译只占很小部分。

现有流程的主要风险：

- `main` 没有 PR 和必需检查门禁，误推即可进入交付链路。
- Desktop、CLI、Core、Tauri、npm 与 lockfile 的版本靠人工同步，已经出现 `desktop/package-lock.json` 顶层版本落后于应用版本的漂移。
- 正式 Release 由 tag 直接触发；版本准备、tag 创建和资产发布没有一个可审查的统一入口。
- GitHub Actions 使用版本标签引用第三方 Action，不是完整 commit SHA。
- npm 安装使用 `npm install`，缓存键也没有以 lockfile 作为唯一依赖输入。

## 4. 交付模型

### 4.1 三种状态

Arca 只保留三种清晰状态：

| 状态 | 触发 | 产物 | 是否进入自动更新 |
|---|---|---|---|
| PR 验证 | 功能 PR 创建或更新 | 测试和构建结果 | 否 |
| Pre-release | 普通 PR 合并到 `main` | 唯一 build tag、DMG、CLI | 否 |
| Stable Release | Release PR 合并 | `vX.Y.Z`、DMG、CLI、Updater、`latest.json` | 是 |

进入 `main` 不等于立即正式发布，但意味着该代码会进入下一次正式构建。暂时不能交付给用户的功能必须留在分支、回滚，或使用默认关闭的 Feature Flag；Release PR 不能从最终安装包中排除已经存在于 `main` 的代码。

### 4.2 主流程

```text
功能分支
  -> Pull Request
  -> required CI: verify
  -> Squash merge 到 main
  -> 自动构建并发布唯一 Pre-release
  -> Release Please 创建或更新同一个 Release PR
  -> 用户决定何时合并 Release PR
  -> 创建稳定 tag 和 Draft Release
  -> 构建、签名、验证并上传完整资产
  -> 发布 Stable Release
  -> 客户端通过 latest.json 收到更新
```

普通 PR 和 Release PR 都必须经过同一个 `verify` 门禁。Release PR 合并时不再额外创建 Pre-release，只走 Stable Release 路径，避免同一个版本重复构建两次。

## 5. 单一 Release PR 契约

Release Please 使用一个目标分支和一个发布组件：

- 目标分支：`main`
- 发布组件：仓库根组件 `arca`
- 版本关系：Desktop、CLI、Core 共用一个 SemVer
- `separate-pull-requests`: `false`
- Release PR 标题固定为 `chore(main): release X.Y.Z`

因此，不论多少功能分支合入 `main`，同一时间最多只有一个带 `autorelease: pending` 状态的 Release PR。新提交会更新现有 PR 的版本、Changelog 和版本文件；Release PR 合并并完成当前版本后，后续可发布提交才创建下一张 Release PR。

Release PR 必须与最新 `main` 同步。若功能 PR 在 Release PR 打开期间合入 `main`，Release PR 变为过期状态，Release Please 更新它并重新触发 CI；在新检查通过前不能合并。

默认版本规则：

| Conventional Commit | 版本影响 | Changelog |
|---|---|---|
| `feat!`、`fix!` 或 `BREAKING CHANGE` | major | 显示 |
| `feat` | minor | 显示 |
| `fix`、`deps` | patch | 显示 |
| `docs`、`test`、`chore`、`ci`、普通 `refactor` | 不单独触发版本 | 默认隐藏或按配置归类 |

不触发版本的提交仍然存在于 `main`，下一次正式构建会包含它们。需要指定特殊版本时使用 Release Please 的显式版本覆盖，而不是人工修改多个 manifest。

## 6. 版本 Source of Truth

新增根级 `version.txt` 作为人类和 Release Please 的单一版本入口，`.release-please-manifest.json` 记录 Release Please 已发布状态。Release Please 使用 `simple` 根组件和 `extra-files` 更新源 manifest，随后由同一 Release PR 上的受控同步步骤重新生成 lockfile。最终 Release PR 必须同步：

- `core/Cargo.toml`
- `cli/Cargo.toml`
- `desktop/package.json`
- `desktop/package-lock.json` 的顶层版本和根 package 版本
- `desktop/src-tauri/Cargo.toml`
- `desktop/src-tauri/tauri.conf.json`
- 根 `Cargo.lock` 中的 Arca workspace packages
- `desktop/src-tauri/Cargo.lock` 中的 Desktop 与 Arca packages

新增版本一致性检查，比较所有权威 manifest 与 lockfile，并验证 npm regular/optional dependency 在 lockfile 的 Node 解析层级中完整闭包。任何不一致都使 `verify` 失败。npm lock 同步必须在不含开发机 `node_modules` 的临时目录运行：完整 lock 只通过 `npm version --allow-same-version --no-git-tag-version` 更新根版本元数据，发现平台依赖被裁剪时才从 `package.json` 全量重建，避免普通 Release PR 意外升级依赖。同步结果提交到同一 Release PR 分支，而不是运行时临时修改或忽略漂移；`npm ci` 和 `cargo test --locked` 不承担修复版本漂移的职责。

普通功能 PR 不应直接修改版本文件。版本提升只进入 Release PR，避免功能提交、版本提交和 tag 三者失去对应关系。

## 7. GitHub 工作流架构

### 7.1 CI 与质量监控

保留 `quality-monitor.yml` 作为 required check 的来源，但把串行验证拆成可并行的三个任务：

1. Rust：`cargo test --locked -p arca-core -p arca-cli`
2. Desktop：`npm ci`、单元测试、前端生产构建和版本一致性检查
3. Website：`npm ci`、文档生产构建

三个任务完成后由稳定命名的 `verify` 汇总任务给 Ruleset 报告唯一门禁结果。失败 Issue、自动修复派发和恢复后关闭 Issue 的现有行为挂在汇总结果之后，不复制到每个子任务。Desktop 与 Website 的 `package-lock.json` 必须入库，CI 使用 `npm ci` 和 lockfile-keyed cache；不能依赖开发机 `node_modules` 或安装时重新解析版本范围。

Node 版本统一到 Node 24 LTS，`actions/setup-node` 使用 lockfile 驱动的 npm cache；Rust 继续使用 `Swatinem/rust-cache`。依赖安装统一改为 `npm ci`，确保 CI 不修改 lockfile。

### 7.2 Release Please

新增 `release-please.yml`，在 `main` push 和手动恢复时运行。它负责：

- 解析 Conventional Commits。
- 创建或更新唯一 Release PR。
- 更新 `version.txt`、Changelog、manifest 与 lockfile。
- Release PR 合并后创建 `vX.Y.Z` tag 和 Draft GitHub Release。

Release Please 配置 `draft: true` 和 `force-tag-creation: true`。Draft Release 不会成为 `releases/latest`，因此客户端在安装包尚未上传时仍看到上一个完整稳定版。

Release Please 不能使用默认 `GITHUB_TOKEN` 创建 PR 和 tag，因为该 token 生成的事件不会再次触发 CI 和 tag 构建。使用仓库专用、可过期、最小权限的 fine-grained token `RELEASE_PLEASE_TOKEN`：

- 仅授权 `Scoheart/arca`
- Contents: read/write
- Pull requests: read/write
- Issues: read/write，仅用于 Release Please labels
- 不授予 Actions、Administration 或其他仓库权限

该 token 不复用 `COPILOT_PAT`，避免发布与自动修复权限耦合。未来若需要更强的凭证治理，可替换为 GitHub App 短期 token，不改变发布状态机。

### 7.3 Desktop 构建与发布

重构 `build-desktop.yml`，保留三种入口：

- 普通 `main` push：构建并发布 Pre-release。
- 稳定 `vX.Y.Z` tag：构建并完成 Draft Stable Release。
- `workflow_dispatch`：重跑指定 tag 或恢复失败的 Draft Release。

普通 `main` push 先判断是否为 Release PR 合并。判断以 Release Please manifest 版本变化和固定 Release PR 提交格式为双重信号；命中时跳过 Pre-release，等待稳定 tag 路径。仓库 squash 标题必须固定使用 PR title，不能依赖 GitHub 的 `COMMIT_OR_PR_TITLE` 默认值，否则单提交 Release PR 可能因首个 commit 标题漂移而误走 Pre-release。

Stable tag 路径必须：

1. 验证 tag 严格匹配 `vX.Y.Z`，且等于所有 manifest 的版本。
2. 构建并签名 Tauri App、DMG、Updater payload 和 CLI。
3. 验证 App version、CLI version、codesign、DMG 完整性和挂载后的 App。
4. 确认同一 commit 的 required `verify` 已成功；缺失、失败或超时均 fail closed。
5. 找到 Release Please 创建的同 tag Draft Release。
6. 上传并标注全部资产，包括 `latest.json`。
7. 最后一步才把 Draft 改为正式 Release。

任何步骤失败时 Draft 保持未发布，`releases/latest` 和客户端更新通道不改变。基础设施瞬时故障可对同一 tag 重跑；如果是产品代码缺陷，不移动或复用既有 tag，而是修复后发布新的 patch 版本。

### 7.4 Pre-release 约定

普通 `main` 合并继续使用唯一 build tag：

```text
v<当前稳定基线>-build.<零填充 run_number>
```

每个 Pre-release 记录 commit SHA，并包含 DMG 与 CLI。它不生成 `latest.json`，不影响 `releases/latest`。为了满足“每个 `main` 更新都有可安装包”的产品约束，不启用会取消旧 main 构建的 concurrency 策略；失败的 `main` 检查不会发布包。

## 8. 仓库治理

创建作用于默认分支、最终切换为 Active 的 Ruleset：

- Require a pull request before merging
- Required approvals: `0`
- Require status checks: `verify`，来源固定为 GitHub Actions
- Require branch to be up to date before merging
- Require conversation resolution
- Require linear history
- Block force pushes
- Restrict deletions
- 不配置日常 bypass

仓库合并设置：

- 只允许 Squash merge
- 关闭 Merge commit 和 Rebase merge
- 合并后自动删除分支

创建 `v*` tag 规则，禁止更新、强推和删除已创建的发布 tag。正式 Release 流程稳定后启用 GitHub Immutable Releases；所有资产名称和 label 必须在 Draft 阶段完成，发布后不再修改。

Ruleset 优先以 Evaluate 模式观察，确认功能 PR、Release PR 和 Actions tag 流程都能正常工作，再切换 Active。GitHub 当前只向 Enterprise 提供 Evaluate；若仓库套餐拒绝该状态，则保持 Disabled，完成本地与一次性 PR 演练并准备回滚后，才可经单独授权切换 Active。任何兼容性降级都不能直接创建 Active Ruleset。

## 9. Action 与供应链安全

- 所有第三方 Action 固定到完整 commit SHA，并在行尾保留对应版本注释。
- Dependabot 负责提出 GitHub Actions 与 npm/Rust 依赖更新 PR。
- 各 job 使用最小 `permissions`；只有 Release Please 和发布 job 拥有写权限。
- `pull_request_target` 不 checkout 或执行 PR 代码；现有自动修复通知继续保持这一边界。
- Signing key、PAT 和 updater key 不进入日志、fixture、文档或构建产物。

## 10. 失败与恢复

| 失败位置 | 对用户影响 | 恢复方式 |
|---|---|---|
| 功能 PR CI | 无法合并 | 修复 PR 后重跑 |
| Pre-release 构建 | 本次没有测试包 | 修复后新 PR，或手动重跑同一 commit |
| Release Please 更新 | Release PR 暂停更新 | 修复 token/config 后手动重跑 |
| Release PR CI | 无法合并 | 修复 Release PR 自动同步或版本漂移 |
| Stable 构建/签名 | Draft 保留，稳定通道不变 | 瞬时故障重跑同 tag；代码问题发布新 patch |
| 资产上传 | Draft 保留，不发布半成品 | 幂等上传缺失资产后再发布 |
| Draft 发布 | 旧稳定版继续有效 | 核对资产后重试发布 API |

工作流必须对重复执行幂等：已存在的 tag、Draft、资产和 label 要么验证一致后复用，要么明确失败，不能静默覆盖不同内容。

## 11. 验证与验收

### 11.1 静态和自动化验证

- Release Please 配置通过其 JSON schema。
- 版本同步检查覆盖全部 manifest 和 lockfile。
- Workflow YAML 可解析，Action 权限和条件分支有自动检查。
- PR CI 三个子任务及 `verify` 汇总结果可稳定重跑。
- 普通 main commit、Release PR merge commit、稳定 tag 和手动恢复入口分别有条件测试。

### 11.2 端到端验收

1. 用无发布影响的测试 PR 验证 Ruleset 和 required `verify`。
2. 合并普通 PR，确认只创建一个 Pre-release，且 Release PR 被创建或更新。
3. 再合并第二个 PR，确认仍是同一个 Release PR，版本和 Changelog 更新。
4. 在 Release PR 过期时确认 GitHub 阻止合并，机器人同步后重新检查。
5. 使用 Draft 或测试版本演练 Stable 构建失败，确认 `releases/latest` 不变化。
6. 演练同 tag 手动恢复，确认资产不重复、Draft 能完成发布。
7. 正式发布后验证 DMG、CLI、Updater 签名、`latest.json` 和 `/Applications/Arca.app` 更新。

正式 tag、正式 Release、客户端更新和 Ruleset Active 都是对外或高影响操作，必须在实施阶段分别获得授权，不能用测试演练隐式代替。

## 12. 分阶段改造顺序

1. 增加版本 source of truth、同步脚本和一致性测试。
2. 并行化 CI、统一 Node/npm cache，并稳定 `verify` check 名称。
3. 接入 Release Please 的单组件 Release PR，但先只验证 PR 创建和更新。
4. 重构 Desktop Pre-release/Stable 构建，加入 Draft、资产完整性和恢复入口。
5. 在 Evaluate 模式配置 `main` 与 tag Ruleset，完成端到端演练。
6. 激活 Ruleset，关闭直接 push 和非 Squash 合并。
7. 经明确授权完成首个自动正式发布，再启用 Immutable Releases。

每一阶段独立提交并验证；不得在同一个不可审查提交中同时修改 CI、发布状态机、版本模型和 GitHub 外部设置。

## 13. 成功标准

- `main` 无法直接推送，所有合并都有 PR 和成功的 `verify`。
- 普通 `main` 合并自动产生唯一、可验证的 Pre-release。
- 不论多少功能分支进入 `main`，同一时间最多一个 Arca Release PR。
- 合并 Release PR 后无需人工改版本、打 tag 或整理 Release Notes。
- 正式资产未齐全时不会改变 `releases/latest` 或客户端更新通道。
- 版本文件和 lockfile 不再漂移。
- 热缓存 Desktop 构建保持或优于当前 3 至 5 分钟，PR CI 的反馈时间低于当前串行基线。
- 构建失败可安全重试，不移动稳定 tag、不覆盖不同资产、不发布半成品。

## 14. 外部依据

- [Release Please：Release PR、Conventional Commits 与单组件/多组件行为](https://github.com/googleapis/release-please)
- [Release Please Action：工作流、token 与发布输出](https://github.com/googleapis/release-please-action)
- [GitHub Ruleset 可用规则](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)
- [GitHub Immutable Releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)
- [GitHub Actions 安全使用与完整 SHA 固定](https://docs.github.com/en/actions/reference/security/secure-use)
- [Node.js 发布状态；Node 24 为 LTS](https://nodejs.org/en/about/previous-releases)
