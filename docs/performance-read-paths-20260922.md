# Arca 读取与交互性能优化

本次处理启动、切页、搜索和长列表滚动。保留当前布局、配色和配置写入规则。

## 发现与处理

| 瓶颈 | 改动 |
| --- | --- |
| Tauri 的 Provider、MCP、Agent、偏好读取及部分写操作同步等待磁盘、Keychain、网络或 workspace gate | 27 个 command 改为后台 worker 执行；原错误类型、锁与校验保留 |
| 默认 MCP 页面排在多个扫描任务后，还固定展示 420 ms 骨架屏 | 优先读取首屏；数据到达即展示 |
| MCP 生效列表、所有副本、手动键和来源计数分别读取相同文件 | 新增 core `RegistrySnapshot`，一次读取生成四个视图；保留来源顺序、覆盖优先级和停用来源的计数 |
| Skills 列表与消费关系各扫描一次 Skills | 新增 core `ResourceObservation`，启动和同时失效时共用扫描，两个结果保留独立错误 |
| 只读请求并发时，较旧响应可能晚到 | Hook 请求代次保护；新请求或 hydrate 后旧响应不覆盖当前状态 |
| 内置 Agent JSON 在每次发现/能力查询时重复解析、验证 | 仅缓存编译期不变定义；用户覆盖、文件与安装探测每次重读 |
| 每张 MCP 卡片遍历全部消费关系 | 每批 inventory 建一次按资产索引，保留跨域隔离、启用过滤和去重 |
| 搜索每个字符触发排序、文本拼接和整列表渲染 | 建立搜索索引；排序与输入分离；列表 memo 与 React deferred rendering 配合，输入无固定防抖等待 |
| 切回 Models 页面重复调用三路读取 | 最多 30 秒的显示数据缓存与并发请求合并；文件观察/重新扫描立即失效，保存后强制刷新；失败和迟到响应不污染缓存 |
| models.dev 并发下载同一目录，缓存子集相互覆盖 | 共享下载、5 分钟内存复用、同一目录版本的子集合并；失败不缓存，保留旧元数据回退 |
| 滚动时屏幕外卡片也参与布局和绘制 | 支持的 WebView 使用 `content-visibility: auto`；保留查找/键盘焦点，hover/focus 时取消包含以保留 Agent 扇形展开 |

显示缓存不存密钥值、不用于写操作校验。复制 cURL 和编辑凭据仍即时调用 core。CAS、文件权限、备份、事务及安全扫描没有省略。

## 测量

2026-09-22，Apple M4 Pro。Rust 使用同一机器、相同 dev 构建配置；预热一次后记录 15 次，中位数如下。Node 计算记录 21 次。数据均为合成 fixture，Rust 使用 `TestHome` 隔离 HOME、ARCA_HOME、安装探测与 PATH，无真实用户配置或 Keychain 访问。

| 场景 | 优化前 | 优化后 | 变化 |
| --- | ---: | ---: | ---: |
| 内置 Agent 定义读取 100 次 | 237.83 ms | 12.93 ms | -94.6% |
| 12 个来源、960 条副本的 MCP 完整刷新（含来源计数） | 11.81 ms | 4.23 ms | -64.2% |
| 120 个 Skill 的列表＋消费关系读取 | 64.72 ms | 38.85 ms | -40.0% |
| 300 张卡片、6,400 条关系的消费 Agent 计算 | 6.77 ms | 0.43 ms | -93.7% |
| 30 秒内进入 Models 10 次、无失效事件 | 30 次 IPC 读取 | 3 次 IPC 读取 | -90% |
| 20 个不同模型并发请求 models.dev 元数据 | 20 次下载、仅保留最后 1 个缓存 | 1 次下载、保留 20 个缓存 | 下载 -95% |

完整 MCP 刷新使用同一轮运行内旧组合接口与新快照对照；旧接口实现未修改。其余 Rust 优化前记录取自修改前，同一基线 commit `824f94c18fa4ce735c763d60c7e86015cf0f3168`。Node 旧实现从该 commit 加载。

以上是读路径/计算的微基准，不是正式安装版启动耗时或 FPS。Keychain 实际延迟、真实磁盘规模以及滚动绘制改善未给出百分比。

## 验证与复现

```sh
cargo run --offline -p arca-core --example perf_reads
node --experimental-vm-modules scripts/benchmark-desktop-reads.mjs \
  --baseline 824f94c18fa4ce735c763d60c7e86015cf0f3168
cd desktop && npm run build
```

已完成的定向验证：

- 新旧 MCP 投影逐项相等，包括 manual 优先级、manual 停用后的覆盖键与来源计数。
- 关系索引与原算法等价，包括重复外部项、停用项和跨域同名资产。
- 模型读取缓存的重入、强制刷新、观察失效、迟到响应和错误恢复。
- 并发元数据请求返回各自结果，并合并所有缓存子集。
- 前端 TypeScript / Vite 生产构建和 Tauri 原生开发构建。

按仓库极速模式未运行完整 cargo/npm 测试套件。本页记录功能提交前的性能验证；发布状态以 GitHub Release 为准。

实现参考：[React deferred rendering](https://react.dev/reference/react/useDeferredValue)、[content-visibility 的行为与可访问性](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/content-visibility)。
