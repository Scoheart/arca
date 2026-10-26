# 发布流程复核与提速 — 2026-10-14

基于 Arca 2.0.10 主线，复核显式 Direct Stable → main 范围内的 macOS build → Draft 发布 → 独立验包。普通 push 不发布，版本与不可变 tag 仍由 Direct Stable 生成。抓包新增能力与本轮发布优化适合 minor 范围。

## 已核验的瓶颈

GitHub job steps 提供的两次实际耗时：

| 构建 | macOS job | Build .dmg | 上传并发布 |
|---|---:|---:|---:|
| [2.0.10](https://github.com/Scoheart/arca/actions/runs/38537895331) | 217 秒 | 150 秒 | 22 秒 |
| [2.0.8](https://github.com/Scoheart/arca/actions/runs/38480733998) | 418 秒 | 336 秒 | 18 秒 |

不能将这些波动都归因于缓存；源码改动、runner 与依赖特征也影响耗时。旧流程的 CLI 与 Tauri 各有一个 target 目录，即使依赖重叠，也不能直接复用另一目录的编译结果。Core 在两个入口启用的特征可能不同，共用目录不保证只编译一次 Core，但允许复用相同 fingerprint 的依赖。

## 改动

- CI 显式指定同一个绝对 `CARGO_TARGET_DIR`，保留 main 范围的 Rust cache；全部打包/验包路径跟随该目录。
- sidecar 使用 Cargo metadata 的真实 target_directory，避免把新编译目录的产物误当成旧目录产物。
- frontend 与 CLI 准备并行，必须等两者都成功才开始 Tauri 打包。
- 上传时附带 asset label，正常发布省去五次串行 PATCH；重试仅修正不匹配 label，仍拒绝覆盖不同字节的既有资产。
- 独立验证收敛到仓库的 `verify-stable-release.sh`，匹配 Arca 的五资产协议。下载并行，Release published 即开始检查，不等 post-run cache 保存。
- updater 检查使用 Node 内置 Ed25519/BLAKE2b，验证 payload 和 trusted comment；不把“base64 可解析”当成签名有效。
- 公钥与版本读取不可变 tag 对应源码；逐资产 digest、App/CLI/DMG 版本、架构、严格 codesign、DMG 挂载和 latest 最大语义版本检查均保留。

已有 CLI 升级桥是上游品牌迁移的运输契约，本轮只验证与新 CLI 字节相同，没有新建兼容层。没有删除签名或版本检查来换取速度。

## 验证和限制

本地生产构建使用统一 target 目录并通过；发布器的现有定向 shell 回归覆盖首次上传、重试、内容冲突、label 失败和乱序完成。新增验包脚本以真实 2.0.10 包进行验证，再用于本次正式发布。完整 cargo/npm 测试、fmt、clippy、图标检查与 push preflight 按项目极速模式跳过。

首次切换缓存目录可能需要建立新的桌面缓存；稳定复用后的收益应通过后续发布观测，不能用一次冷构建承诺固定加速百分比。必要安装验收是独立授权范围，不与发布绑定。
