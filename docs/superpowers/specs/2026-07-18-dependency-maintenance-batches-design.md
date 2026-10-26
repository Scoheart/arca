# Arca 依赖维护批次设计

日期：2026-07-18

## 背景与目标

Arca 当前有 11 个未关闭的 Dependabot PR。每个合入 `main` 的普通提交都会触发一次完整 macOS 构建并发布 Pre-release；逐个合并会产生不必要的构建与发布噪音。另一方面，前端工具链和 TOML 1.1 都存在已验证的兼容性失败，不能与纯依赖刷新混成一个不可诊断的大 PR。

目标是在不降低测试门禁、不绕过 lockfile 规则、不直接发布 Stable 的前提下，将这些 PR 收敛为三个可独立审查、验证和回滚的维护批次。原 Dependabot PR 只在对应替代批次合并后关闭。

## 方案比较

### 方案 A：逐个更新并合并

优点是每个依赖的差异最小。缺点是最多触发 11 次 Pre-release，后续 PR 会因共享 lockfile 反复落后或冲突，整体交付成本最高。

### 方案 B：合成一个总维护 PR

优点是只触发一次主线构建。缺点是纯版本刷新、TypeScript/Vite 兼容修复和 TOML 写入语义迁移相互耦合；一旦失败，定位、回滚和审查都困难。

### 方案 C：三个风险批次（采用）

按“已全绿的机械更新”“前端构建工具链”“配置解析与保真写入”分成三个 PR。它在构建次数、隔离性和可审查性之间取得平衡。

## 批次一：绿色依赖整合

整合 Dependabot `#22/#23/#24/#27/#29/#32/#33`：

- GitHub Actions：`Swatinem/rust-cache`、`actions/setup-node`、`actions/checkout`。
- Root Cargo workspace：`uuid`、`regex`、`clap` 的 minor/patch 更新，以及 `similar 3.1.1`。
- Desktop Tauri：`ureq 3.3.0`、`dirs 6.0.0`。

只接受 manifest、lockfile 和 workflow action pin 的必要变化，不加入产品行为改动。验证包括 root workspace tests、Desktop Tauri tests、Desktop frontend tests/build、Website build、release metadata check 和完整 CI。

## 批次二：前端工具链联动升级

联合替代 Dependabot `#25/#26/#28`：

- 同时升级 `vite 8.1.5` 与 `@vitejs/plugin-react 6.0.3`，满足双方 peer dependency。
- 升级 `typescript 7.0.2`，显式保留 Node 测试类型并将语言库目标提高到项目实际已使用的 `ES2022`。
- 只修复 TypeScript 7 暴露的真实类型不兼容；不得用 `skip`、`any` 扩散、`--force` 或 `--legacy-peer-deps` 绕过。

验证必须覆盖 `npm ci`、全部 Vitest、`tsc`、Vite production build、Agent icon check 和 Tauri build入口。若 TypeScript 7 需要超出配置与局部类型收窄的广泛源码改造，则从该批次移除，保留在 `5.8.x` 并关闭 `#25` 为暂不计划。

## 批次三：TOML 1.1 迁移

联合替代 Dependabot `#30/#31`，同时升级 root Core 与 Desktop Tauri 的 `toml`，避免两个工作区长期采用不同解析口径。

已知失败是 TOML 1.1 的序列化/解析行为改变导致六个 Core 测试失败。修复必须保持 Arca 的安全不变量：未知字段、注释、格式和非目标策略继续由 `toml_edit` 保真；损坏或歧义输入继续 fail closed；不得通过修改 fixture 预期掩盖真实配置损坏。

验证包括全部 root workspace tests、TOML adapter 定向测试、Agent codec round-trip、Desktop Tauri tests，以及针对未知字段与非目标配置保留的回归用例。

## GitHub 与发布流程

每批从最新 `main` 创建独立分支，使用 Conventional Commit，推送后创建普通 PR。只有前一批合并且主线 CI/Pre-release 成功后，下一批才重放到新 `main` 并提交，避免共享 lockfile 冲突。

替代 PR 合并后再关闭对应 Dependabot PR，并在关闭说明中链接替代 PR。三批均不修改 `version.txt` 或 release-owned 版本字段，不合并 Release Please PR，也不发布 Stable。Release Please 产生的版本提案保留给后续单独批准。

## 完成标准

- 11 个原始 Dependabot PR 均已被合并内容覆盖或以明确原因关闭。
- 三个维护 PR 的必要 CI 全绿并已合入 `main`。
- `main` 本地工作树与 `origin/main` 对齐，完整验证通过。
- 只产生三个预期的普通主线 Pre-release，不产生额外 Stable。
- 依赖升级、失败原因、替代 PR 和验证证据写入父仓 daily brief。
