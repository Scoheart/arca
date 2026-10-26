# Arca 桌面 GUI · Plan 1：基础层（共享数据 + Rust Core）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把内置注册表与 agent 定义抽离为共享 JSON（TS core 改读它、CLI 行为不变），并搭建 Tauri 骨架与等价于现有 TS core 的 Rust core（adapters/scanner/differ/applier/registry/override），产出可独立测试的后端基础层。

**Architecture:** 单一数据源 `data/registry.json` + `data/agents.json`，TS 与 Rust 各自读取。新建 `desktop/` Tauri 项目，Rust 后端在 `desktop/src-tauri/src/core/` 等价重写现有 TS core；Rust 单元测试以现有 vitest 用例为行为基线。

**Tech Stack:** TypeScript（现有 CLI）、Rust（serde / serde_json / toml）、Tauri v2、Vitest（TS 测试）、cargo test（Rust 测试）。

这是 3 个计划中的第 1 个（基础层）。Plan 2（GUI 主流程：仓库视图 + 安装弹窗）与 Plan 3（矩阵/项目/扫描/Agents）依赖本计划产出的 Rust core 与 IPC 接口，在本计划完成后再编写。

---

## 文件结构

**Part A — 共享数据抽离（TS 侧，根目录现有项目）**
- 新建 `data/registry.json`：内置 40+ 服务器（源自 `src/builtin-registry.ts`）。
- 新建 `data/agents.json`：18 个 agent 定义（源自 `src/constants.ts` 的 `DEFAULT_AGENTS`）。
- 新建 `scripts/extract-data.ts`：一次性从现有 TS 常量导出上述 JSON 的脚本。
- 修改 `src/builtin-registry.ts`：改为从 `data/registry.json` 加载。
- 修改 `src/constants.ts`：`DEFAULT_AGENTS` 改为从 `data/agents.json` 加载。
- 修改 `package.json`：`files` 增加 `data`。
- 新建 `tests/core/shared-data.test.ts`：校验 JSON 与原行为一致。

**Part B — Tauri 骨架 + Rust core（`desktop/` 新项目）**
- `desktop/src-tauri/src/core/types.rs`：Rust 数据结构。
- `desktop/src-tauri/src/core/adapter.rs`：`Adapter` trait。
- `desktop/src-tauri/src/core/json_adapter.rs`：JSON 适配器。
- `desktop/src-tauri/src/core/toml_adapter.rs`：TOML 适配器。
- `desktop/src-tauri/src/core/registry.rs`：读共享 registry + 用户自定义。
- `desktop/src-tauri/src/core/scanner.rs`：扫描各 agent 配置。
- `desktop/src-tauri/src/core/differ.rs`：计算 add/remove/change。
- `desktop/src-tauri/src/core/applier.rs`：写入 + 备份。
- `desktop/src-tauri/src/core/override.rs`：覆写 patch 合并。
- `desktop/src-tauri/src/core/mod.rs`：模块汇总。
- `desktop/src-tauri/src/commands.rs`：Tauri command（`list_registry`）。
- `desktop/src-tauri/src/main.rs`：应用入口。
- `desktop/src-tauri/tauri.conf.json`、`Cargo.toml`：Tauri 配置。
- `desktop/data/`：构建期从根 `data/` 同步的共享 JSON（见 Task B1 说明）。

---

## Task 0：前置 — 安装 Rust 与 Tauri 工具链

**Files:** 无（环境准备）

- [ ] **Step 1: 安装 Rust toolchain**

Run:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version && cargo --version
```
Expected: 打印 `rustc 1.x` 与 `cargo 1.x` 版本号（不再是「未安装」）。

- [ ] **Step 2: 安装 Tauri CLI**

Run:
```bash
cargo install tauri-cli --version "^2.0" --locked
cargo tauri --version
```
Expected: 打印 `tauri-cli 2.x`。

- [ ] **Step 3: 确认 macOS 构建依赖（Xcode CLT）**

Run: `xcode-select -p`
Expected: 打印一个路径（如 `/Library/Developer/CommandLineTools`）。若报错，运行 `xcode-select --install` 后重试。

---

## Task 1：从 TS 常量导出共享 JSON

**Files:**
- Create: `scripts/extract-data.ts`
- Create（脚本产物）: `data/registry.json`, `data/agents.json`

- [ ] **Step 1: 编写导出脚本**

`scripts/extract-data.ts`：
```ts
import { writeFileSync, mkdirSync } from "node:fs";
import { BUILTIN_REGISTRY } from "../src/builtin-registry.js";
import { DEFAULT_AGENTS } from "../src/constants.js";

mkdirSync("data", { recursive: true });
writeFileSync("data/registry.json", JSON.stringify(BUILTIN_REGISTRY, null, 2) + "\n");
writeFileSync("data/agents.json", JSON.stringify(DEFAULT_AGENTS, null, 2) + "\n");
console.log(`registry: ${BUILTIN_REGISTRY.length} servers, agents: ${Object.keys(DEFAULT_AGENTS).length}`);
```

- [ ] **Step 2: 运行脚本生成 JSON**

Run: `npx tsx scripts/extract-data.ts`
Expected: 打印类似 `registry: 40 servers, agents: 18`，且生成 `data/registry.json`、`data/agents.json`。
（若无 tsx：`npm i -D tsx` 后重试。）

- [ ] **Step 3: 校验 JSON 内容**

Run: `node -e "const r=require('./data/registry.json'),a=require('./data/agents.json'); console.log(r.length, Object.keys(a).length, r[0].name)"`
Expected: 打印 `40 18 filesystem`（数量以实际为准，名称应为首个内置服务器）。

- [ ] **Step 4: 提交**

```bash
git add scripts/extract-data.ts data/registry.json data/agents.json
git commit -m "chore: extract builtin registry and agents to shared JSON"
```

---

## Task 2：TS core 改读共享 JSON（保持 CLI 行为不变）

**Files:**
- Modify: `src/builtin-registry.ts`
- Modify: `src/constants.ts:10`（`DEFAULT_AGENTS`）
- Modify: `package.json`（`files` 数组）
- Test: `tests/core/shared-data.test.ts`

- [ ] **Step 1: 写失败测试（JSON 与类型契合、数量正确）**

`tests/core/shared-data.test.ts`：
```ts
import { describe, it, expect } from "vitest";
import { BUILTIN_REGISTRY } from "../../src/builtin-registry.js";
import { DEFAULT_AGENTS } from "../../src/constants.js";

describe("shared data", () => {
  it("registry loads from JSON with expected shape", () => {
    expect(Array.isArray(BUILTIN_REGISTRY)).toBe(true);
    expect(BUILTIN_REGISTRY.length).toBeGreaterThanOrEqual(40);
    const first = BUILTIN_REGISTRY.find((e) => e.name === "filesystem");
    expect(first).toBeDefined();
    expect(first!.config.stdio?.command).toBe("npx");
  });

  it("agents load from JSON with expected shape", () => {
    expect(DEFAULT_AGENTS["claude-code"].format).toBe("json");
    expect(DEFAULT_AGENTS["claude-code"].key).toBe("mcpServers");
    expect(DEFAULT_AGENTS["codex"].format).toBe("toml");
    expect(Object.keys(DEFAULT_AGENTS).length).toBeGreaterThanOrEqual(18);
  });
});
```

- [ ] **Step 2: 运行测试确认通过（当前 TS 常量已满足）**

Run: `npx vitest run tests/core/shared-data.test.ts`
Expected: PASS（此刻仍用 TS 常量，作为重构前的行为基线）。

- [ ] **Step 3: 将 `src/builtin-registry.ts` 改为从 JSON 加载**

替换整个文件内容为：
```ts
import type { RegistryEntry } from "./types.js";
import registryData from "../data/registry.json" with { type: "json" };

/**
 * Built-in MCP Server registry — loaded from the shared data/registry.json,
 * which is also consumed by the Tauri Rust core.
 */
export const BUILTIN_REGISTRY: RegistryEntry[] = registryData as RegistryEntry[];
```

- [ ] **Step 4: 将 `src/constants.ts` 的 `DEFAULT_AGENTS` 改为从 JSON 加载**

把 `src/constants.ts:10` 起的 `DEFAULT_AGENTS` 大字面量替换为：
```ts
import agentsData from "../data/agents.json" with { type: "json" };

export const DEFAULT_AGENTS: Record<string, AgentDefinition> =
  agentsData as Record<string, AgentDefinition>;
```
保留文件顶部 `import type { AgentDefinition } from "./types.js";` 与 `MCP_HUB_DIR` 等其余常量不变。

- [ ] **Step 5: package.json 的 files 增加 data**

把 `"files": ["dist", "src", "README.md", "LICENSE"]` 改为：
```json
"files": ["dist", "src", "data", "README.md", "LICENSE"],
```

- [ ] **Step 6: 运行全部测试 + 构建确认行为不变**

Run: `npx vitest run && npm run build`
Expected: 所有既有测试 PASS（registry.test.ts、agents 相关等），`tsc` 构建无错误。

- [ ] **Step 7: 提交**

```bash
git add src/builtin-registry.ts src/constants.ts package.json tests/core/shared-data.test.ts
git commit -m "refactor: load builtin registry and agents from shared JSON"
```

---

## Task B1：Tauri 项目骨架

**Files:**
- Create: `desktop/`（Tauri scaffold）
- Create: `desktop/scripts/sync-data.sh`

- [ ] **Step 1: 用 create-tauri-app 生成骨架（React + TS + Vite）**

Run:
```bash
cd desktop 2>/dev/null || true
cd /Users/scoheart/scoheart/mcp-hub
npm create tauri-app@latest desktop -- --template react-ts --manager npm --yes
```
Expected: 生成 `desktop/`，含 `src/`（React 前端）、`src-tauri/`（Rust）、`package.json`。

- [ ] **Step 2: 安装依赖并确认能编译**

Run:
```bash
cd /Users/scoheart/scoheart/mcp-hub/desktop && npm install && cargo build --manifest-path src-tauri/Cargo.toml
```
Expected: `npm install` 成功；`cargo build` 完成（首次较慢），结尾 `Finished`。

- [ ] **Step 3: 添加共享数据同步脚本**

`desktop/scripts/sync-data.sh`：
```bash
#!/usr/bin/env bash
set -euo pipefail
# 把根目录共享 JSON 复制进 desktop，供 Rust include_str! / 运行期读取
SRC="$(cd "$(dirname "$0")/../.." && pwd)/data"
DST="$(cd "$(dirname "$0")/.." && pwd)/data"
mkdir -p "$DST"
cp "$SRC/registry.json" "$DST/registry.json"
cp "$SRC/agents.json" "$DST/agents.json"
echo "synced registry.json + agents.json -> $DST"
```

- [ ] **Step 4: 运行同步脚本**

Run: `bash /Users/scoheart/scoheart/mcp-hub/desktop/scripts/sync-data.sh`
Expected: 打印 `synced ...`，生成 `desktop/data/registry.json`、`desktop/data/agents.json`。

- [ ] **Step 5: 在 Cargo.toml 添加依赖**

在 `desktop/src-tauri/Cargo.toml` 的 `[dependencies]` 增加：
```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
chrono = "0.4"
```

- [ ] **Step 6: 提交**

```bash
git add desktop
git commit -m "feat(desktop): scaffold Tauri react-ts app with data sync"
```

---

## Task B2：Rust 数据结构（types.rs）

**Files:**
- Create: `desktop/src-tauri/src/core/types.rs`
- Create: `desktop/src-tauri/src/core/mod.rs`

- [ ] **Step 1: 编写类型 + 一个序列化测试**

`desktop/src-tauri/src/core/types.rs`：
```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StdioConfig {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpConfig {
    #[serde(rename = "type")]
    pub kind: String, // "http" | "sse"
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum McpConfig {
    Stdio(StdioConfig),
    Http(HttpConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RegistryConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdio: Option<StdioConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryEntry {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub config: RegistryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDefinition {
    pub global: Option<String>,
    pub project: Option<String>,
    pub format: String, // "json" | "toml"
    pub key: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_entry_roundtrips_stdio() {
        let json = r#"{"name":"git","description":"d","tags":["builtin"],
            "config":{"stdio":{"command":"npx","args":["-y","x"]}}}"#;
        let e: RegistryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(e.name, "git");
        assert_eq!(e.config.stdio.as_ref().unwrap().command, "npx");
    }
}
```

`desktop/src-tauri/src/core/mod.rs`：
```rust
pub mod types;
```

- [ ] **Step 2: 运行测试确认通过**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml types::`
Expected: `test core::types::tests::registry_entry_roundtrips_stdio ... ok`。
（注意：需在 `main.rs` 顶部加入 `mod core;` 才能编译；见 Task B9 Step 1，若此时报 `unresolved module`，先在 `main.rs` 临时加 `mod core;`。）

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/types.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add Rust core data types"
```

---

## Task B3：Adapter trait + JSON 适配器

**Files:**
- Create: `desktop/src-tauri/src/core/adapter.rs`
- Create: `desktop/src-tauri/src/core/json_adapter.rs`
- Modify: `desktop/src-tauri/src/core/mod.rs`

行为基线（对照 `src/adapters/json-adapter.ts`）：`read` 返回文件中 `key` 下的 `{name: config}`，文件不存在或无该键返回空；`write` 读出整个文件对象、设置 `obj[key]=mcps`、保留其它字段后写回；`remove` 删除 `obj[key]` 下指定名字。

- [ ] **Step 1: 写失败测试**

`desktop/src-tauri/src/core/json_adapter.rs`：
```rust
use crate::core::types::McpConfig;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub struct JsonAdapter {
    pub key: String,
}

impl JsonAdapter {
    pub fn new(key: &str) -> Self {
        Self { key: key.to_string() }
    }

    pub fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return BTreeMap::new(),
        };
        let root: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return BTreeMap::new(),
        };
        match root.get(&self.key) {
            Some(section) => serde_json::from_value(section.clone()).unwrap_or_default(),
            None => BTreeMap::new(),
        }
    }

    pub fn write(&self, path: &Path, mcps: &BTreeMap<String, McpConfig>) {
        let mut root: serde_json::Value = fs::read_to_string(path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        root[&self.key] = serde_json::to_value(mcps).unwrap();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path, serde_json::to_string_pretty(&root).unwrap() + "\n").unwrap();
    }

    pub fn remove(&self, path: &Path, names: &[String]) {
        let mut current = self.read(path);
        for n in names {
            current.remove(n);
        }
        self.write(path, &current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{McpConfig, StdioConfig};
    use std::collections::BTreeMap;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("arca-json-{}-{}.json", name, std::process::id()));
        d
    }

    #[test]
    fn write_then_read_roundtrips() {
        let p = tmp("rt");
        let adapter = JsonAdapter::new("mcpServers");
        let mut m = BTreeMap::new();
        m.insert("git".to_string(), McpConfig::Stdio(StdioConfig {
            command: "npx".into(), args: Some(vec!["-y".into()]), env: None,
        }));
        adapter.write(&p, &m);
        let back = adapter.read(&p);
        assert!(back.contains_key("git"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn write_preserves_other_keys() {
        let p = tmp("preserve");
        std::fs::write(&p, r#"{"otherKey":42,"mcpServers":{}}"#).unwrap();
        let adapter = JsonAdapter::new("mcpServers");
        adapter.write(&p, &BTreeMap::new());
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["otherKey"], 42);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn read_missing_file_is_empty() {
        let adapter = JsonAdapter::new("mcpServers");
        assert!(adapter.read(Path::new("/nonexistent/xyz.json")).is_empty());
    }
}
```

`desktop/src-tauri/src/core/adapter.rs`（保留 trait 供后续统一调用）：
```rust
use crate::core::types::McpConfig;
use std::collections::BTreeMap;
use std::path::Path;

pub trait Adapter {
    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig>;
    fn write(&self, path: &Path, mcps: &BTreeMap<String, McpConfig>);
    fn remove(&self, path: &Path, names: &[String]);
}
```

在 `mod.rs` 增加：
```rust
pub mod adapter;
pub mod json_adapter;
```

- [ ] **Step 2: 运行测试确认通过**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml json_adapter::`
Expected: 三个测试均 `ok`。

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/adapter.rs desktop/src-tauri/src/core/json_adapter.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add Adapter trait and JSON adapter (Rust)"
```

---

## Task B4：TOML 适配器

**Files:**
- Create: `desktop/src-tauri/src/core/toml_adapter.rs`
- Modify: `desktop/src-tauri/src/core/mod.rs`

行为基线（对照 `src/adapters/toml-adapter.ts`）：read/write/remove 同 JSON，但读写 TOML，section 名为 `key`（如 `mcp_servers`）。

- [ ] **Step 1: 写失败测试 + 实现**

`desktop/src-tauri/src/core/toml_adapter.rs`：
```rust
use crate::core::types::McpConfig;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub struct TomlAdapter {
    pub key: String,
}

impl TomlAdapter {
    pub fn new(key: &str) -> Self {
        Self { key: key.to_string() }
    }

    pub fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return BTreeMap::new(),
        };
        let root: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(_) => return BTreeMap::new(),
        };
        match root.get(&self.key) {
            Some(section) => {
                // toml::Value -> JSON -> McpConfig，统一走 serde
                let json = serde_json::to_value(section).unwrap_or_default();
                serde_json::from_value(json).unwrap_or_default()
            }
            None => BTreeMap::new(),
        }
    }

    pub fn write(&self, path: &Path, mcps: &BTreeMap<String, McpConfig>) {
        let mut root: toml::Value = fs::read_to_string(path)
            .ok()
            .and_then(|c| c.parse().ok())
            .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
        // mcps -> JSON -> toml::Value
        let json = serde_json::to_value(mcps).unwrap();
        let section: toml::Value = serde_json::from_value(json).unwrap();
        if let toml::Value::Table(ref mut t) = root {
            t.insert(self.key.clone(), section);
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path, toml::to_string_pretty(&root).unwrap()).unwrap();
    }

    pub fn remove(&self, path: &Path, names: &[String]) {
        let mut current = self.read(path);
        for n in names {
            current.remove(n);
        }
        self.write(path, &current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{McpConfig, StdioConfig};

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("arca-toml-{}-{}.toml", name, std::process::id()));
        d
    }

    #[test]
    fn write_then_read_roundtrips() {
        let p = tmp("rt");
        let adapter = TomlAdapter::new("mcp_servers");
        let mut m = BTreeMap::new();
        m.insert("github".to_string(), McpConfig::Stdio(StdioConfig {
            command: "npx".into(), args: Some(vec!["-y".into()]), env: None,
        }));
        adapter.write(&p, &m);
        let back = adapter.read(&p);
        assert!(back.contains_key("github"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn remove_deletes_entry() {
        let p = tmp("rm");
        let adapter = TomlAdapter::new("mcp_servers");
        let mut m = BTreeMap::new();
        m.insert("a".to_string(), McpConfig::Stdio(StdioConfig {
            command: "x".into(), args: None, env: None }));
        m.insert("b".to_string(), McpConfig::Stdio(StdioConfig {
            command: "y".into(), args: None, env: None }));
        adapter.write(&p, &m);
        adapter.remove(&p, &["a".to_string()]);
        let back = adapter.read(&p);
        assert!(!back.contains_key("a"));
        assert!(back.contains_key("b"));
        let _ = std::fs::remove_file(&p);
    }
}
```

在 `mod.rs` 增加：`pub mod toml_adapter;`

- [ ] **Step 2: 运行测试确认通过**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml toml_adapter::`
Expected: 两个测试均 `ok`。

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/toml_adapter.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add TOML adapter (Rust)"
```

---

## Task B5：Registry 加载（共享 JSON + 用户自定义）

**Files:**
- Create: `desktop/src-tauri/src/core/registry.rs`
- Modify: `desktop/src-tauri/src/core/mod.rs`

行为基线（对照 `src/core/registry.ts`）：内置来自 `data/registry.json`；用户条目来自 `registry_dir/*.json`；同名时用户覆盖内置。

- [ ] **Step 1: 写失败测试 + 实现**

`desktop/src-tauri/src/core/registry.rs`：
```rust
use crate::core::types::RegistryEntry;
use std::fs;
use std::path::Path;

/// 内置注册表：编译期内嵌 desktop/data/registry.json
const BUILTIN_JSON: &str = include_str!("../../../data/registry.json");

pub fn builtin_registry() -> Vec<RegistryEntry> {
    serde_json::from_str(BUILTIN_JSON).expect("registry.json must be valid")
}

pub fn read_registry(registry_dir: &Path) -> Vec<RegistryEntry> {
    let mut user: Vec<RegistryEntry> = Vec::new();
    if registry_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(registry_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(c) = fs::read_to_string(&p) {
                        if let Ok(entry) = serde_json::from_str::<RegistryEntry>(&c) {
                            user.push(entry);
                        }
                    }
                }
            }
        }
    }
    let user_names: std::collections::HashSet<_> =
        user.iter().map(|e| e.name.clone()).collect();
    let mut result: Vec<RegistryEntry> = builtin_registry()
        .into_iter()
        .filter(|b| !user_names.contains(&b.name))
        .collect();
    result.extend(user);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_loads_40_plus() {
        assert!(builtin_registry().len() >= 40);
    }

    #[test]
    fn user_entry_overrides_builtin() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("arca-reg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("filesystem.json"),
            r#"{"name":"filesystem","description":"custom","tags":[],
                "config":{"stdio":{"command":"custom-cmd"}}}"#,
        ).unwrap();
        let all = read_registry(&dir);
        let fs_entry = all.iter().find(|e| e.name == "filesystem").unwrap();
        assert_eq!(fs_entry.description, "custom");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

在 `mod.rs` 增加：`pub mod registry;`

- [ ] **Step 2: 运行测试确认通过**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml registry::`
Expected: 两个测试 `ok`。若 `include_str!` 路径报错，确认已运行 Task B1 Step 4 的 sync-data 脚本生成 `desktop/data/registry.json`。

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/registry.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add registry loader merging builtin + user entries"
```

---

## Task B6：Override 合并（canonical ⊕ patch）

**Files:**
- Create: `desktop/src-tauri/src/core/r#override.rs`（文件名 `override.rs`）
- Modify: `desktop/src-tauri/src/core/mod.rs`

`override` 是 Rust 关键字，模块名用 `r#override`，文件名 `override.rs`。

- [ ] **Step 1: 写失败测试 + 实现**

`desktop/src-tauri/src/core/override.rs`：
```rust
use crate::core::types::{HttpConfig, McpConfig, StdioConfig};
use std::collections::HashMap;

/// 部分覆写：仅含与 canonical 不同的字段
#[derive(Debug, Clone, Default)]
pub struct OverridePatch {
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

/// effective = canonical ⊕ patch
pub fn apply_override(base: &McpConfig, patch: &OverridePatch) -> McpConfig {
    match base {
        McpConfig::Stdio(s) => McpConfig::Stdio(StdioConfig {
            command: s.command.clone(),
            args: patch.args.clone().or_else(|| s.args.clone()),
            env: patch.env.clone().or_else(|| s.env.clone()),
        }),
        McpConfig::Http(h) => McpConfig::Http(HttpConfig {
            kind: h.kind.clone(),
            url: patch.url.clone().unwrap_or_else(|| h.url.clone()),
            headers: patch.headers.clone().or_else(|| h.headers.clone()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patch_overrides_env_keeps_command() {
        let base = McpConfig::Stdio(StdioConfig {
            command: "npx".into(),
            args: Some(vec!["-y".into()]),
            env: Some(HashMap::from([("T".into(), "a".into())])),
        });
        let mut env = HashMap::new();
        env.insert("T".to_string(), "b".to_string());
        let patch = OverridePatch { env: Some(env), ..Default::default() };
        if let McpConfig::Stdio(eff) = apply_override(&base, &patch) {
            assert_eq!(eff.command, "npx");
            assert_eq!(eff.env.unwrap().get("T").unwrap(), "b");
        } else {
            panic!("expected stdio");
        }
    }
}
```

在 `mod.rs` 增加：`pub mod r#override;`

- [ ] **Step 2: 运行测试确认通过**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml override::`
Expected: `patch_overrides_env_keeps_command ... ok`。

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/override.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add override patch merge (canonical + patch)"
```

---

## Task B7：Scanner（扫描各 agent 配置 + drift 数据源）

**Files:**
- Create: `desktop/src-tauri/src/core/scanner.rs`
- Modify: `desktop/src-tauri/src/core/mod.rs`

行为基线（对照 `src/core/scanner.ts`）：遍历 agents，按 format 选适配器，读 global（展开 `~`）与 project（相对 project_dir）文件，返回 `ScannedMcp { name, config, agent, scope, file_path }`。

- [ ] **Step 1: 写失败测试 + 实现**

`desktop/src-tauri/src/core/scanner.rs`：
```rust
use crate::core::json_adapter::JsonAdapter;
use crate::core::toml_adapter::TomlAdapter;
use crate::core::types::{AgentDefinition, McpConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScannedMcp {
    pub name: String,
    pub config: McpConfig,
    pub agent: String,
    pub scope: String, // "global" | "project"
    pub file_path: String,
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(p)
}

fn read_section(format: &str, key: &str, path: &Path) -> BTreeMap<String, McpConfig> {
    if format == "toml" {
        TomlAdapter::new(key).read(path)
    } else {
        JsonAdapter::new(key).read(path)
    }
}

pub fn scan_agents(
    agents: &BTreeMap<String, AgentDefinition>,
    project_dir: Option<&Path>,
    scan_all: bool,
) -> Vec<ScannedMcp> {
    let mut out = Vec::new();
    for (name, def) in agents {
        if !scan_all && !def.enabled {
            continue;
        }
        if let Some(g) = &def.global {
            let path = expand_tilde(g);
            for (mcp_name, cfg) in read_section(&def.format, &def.key, &path) {
                out.push(ScannedMcp {
                    name: mcp_name, config: cfg, agent: name.clone(),
                    scope: "global".into(), file_path: path.display().to_string(),
                });
            }
        }
        if let (Some(proj), Some(base)) = (&def.project, project_dir) {
            let path = base.join(proj);
            for (mcp_name, cfg) in read_section(&def.format, &def.key, &path) {
                out.push(ScannedMcp {
                    name: mcp_name, config: cfg, agent: name.clone(),
                    scope: "project".into(), file_path: path.display().to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::AgentDefinition;

    #[test]
    fn scans_project_json_config() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-scan-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("mcp.json"),
            r#"{"mcpServers":{"git":{"command":"npx"}}}"#).unwrap();
        let mut agents = BTreeMap::new();
        agents.insert("test".to_string(), AgentDefinition {
            global: None, project: Some("mcp.json".into()),
            format: "json".into(), key: "mcpServers".into(),
            enabled: true, builtin: Some(true),
        });
        let found = scan_agents(&agents, Some(&base), false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "git");
        assert_eq!(found[0].scope, "project");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn skips_disabled_unless_scan_all() {
        let mut agents = BTreeMap::new();
        agents.insert("off".to_string(), AgentDefinition {
            global: Some("~/nope.json".into()), project: None,
            format: "json".into(), key: "mcpServers".into(),
            enabled: false, builtin: None,
        });
        assert_eq!(scan_agents(&agents, None, false).len(), 0);
    }
}
```

在 `mod.rs` 增加：`pub mod scanner;`

- [ ] **Step 2: 运行测试确认通过**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml scanner::`
Expected: 两个测试 `ok`。

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/scanner.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add config scanner (Rust)"
```

---

## Task B8：Differ + Applier（含备份）

**Files:**
- Create: `desktop/src-tauri/src/core/differ.rs`
- Create: `desktop/src-tauri/src/core/applier.rs`
- Modify: `desktop/src-tauri/src/core/mod.rs`

行为基线：`differ` 对照 `src/core/differ.ts`（期望 vs 实际 → add/remove）；`applier` 对照 `src/core/applier.ts`（写前备份到 backups 目录，文件名带时间戳）。

- [ ] **Step 1: 写 differ 测试 + 实现**

`desktop/src-tauri/src/core/differ.rs`：
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum DiffAction { Add, Remove }

#[derive(Debug, Clone, PartialEq)]
pub struct DiffEntry {
    pub action: DiffAction,
    pub mcp_name: String,
    pub agent: String,
    pub scope: String,
}

#[derive(Debug, Clone)]
pub struct DesiredMcp {
    pub name: String,
    pub agents: Vec<String>,
    pub scopes: Vec<String>, // 已展开的 global/project
}

#[derive(Debug, Clone)]
pub struct CurrentMcp {
    pub name: String,
    pub agent: String,
    pub scope: String,
}

pub fn compute_diff(desired: &[DesiredMcp], current: &[CurrentMcp]) -> Vec<DiffEntry> {
    let mut diffs = Vec::new();
    let mut desired_set = std::collections::HashSet::new();
    for d in desired {
        for scope in &d.scopes {
            for agent in &d.agents {
                let key = format!("{}|{}|{}", d.name, agent, scope);
                desired_set.insert(key.clone());
                let exists = current.iter().any(|c|
                    c.name == d.name && &c.agent == agent && &c.scope == scope);
                if !exists {
                    diffs.push(DiffEntry { action: DiffAction::Add,
                        mcp_name: d.name.clone(), agent: agent.clone(), scope: scope.clone() });
                }
            }
        }
    }
    for c in current {
        let key = format!("{}|{}|{}", c.name, c.agent, c.scope);
        if !desired_set.contains(&key) {
            diffs.push(DiffEntry { action: DiffAction::Remove,
                mcp_name: c.name.clone(), agent: c.agent.clone(), scope: c.scope.clone() });
        }
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adds_missing_and_removes_extra() {
        let desired = vec![DesiredMcp {
            name: "git".into(), agents: vec!["claude-code".into()],
            scopes: vec!["global".into()] }];
        let current = vec![CurrentMcp {
            name: "old".into(), agent: "claude-code".into(), scope: "global".into() }];
        let diffs = compute_diff(&desired, &current);
        assert!(diffs.iter().any(|d| d.action == DiffAction::Add && d.mcp_name == "git"));
        assert!(diffs.iter().any(|d| d.action == DiffAction::Remove && d.mcp_name == "old"));
    }
}
```

- [ ] **Step 2: 运行 differ 测试**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml differ::`
Expected: `adds_missing_and_removes_extra ... ok`。

- [ ] **Step 3: 写 applier 测试 + 实现**

`desktop/src-tauri/src/core/applier.rs`：
```rust
use crate::core::differ::{DiffAction, DiffEntry};
use crate::core::json_adapter::JsonAdapter;
use crate::core::toml_adapter::TomlAdapter;
use crate::core::types::{AgentDefinition, McpConfig};
use crate::core::scanner::expand_tilde;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn target_path(def: &AgentDefinition, scope: &str, project_dir: Option<&Path>) -> Option<PathBuf> {
    if scope == "global" {
        def.global.as_ref().map(|g| expand_tilde(g))
    } else {
        match (&def.project, project_dir) {
            (Some(p), Some(base)) => Some(base.join(p)),
            _ => None,
        }
    }
}

fn backup(path: &Path, backups_dir: &Path, stamp: &str) {
    if !path.exists() { return; }
    let _ = fs::create_dir_all(backups_dir);
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let _ = fs::copy(path, backups_dir.join(format!("{}-{}", fname, stamp)));
}

/// configs: 待 add 的 (mcp_name -> 已计算 effective config)
pub fn apply_diffs(
    diffs: &[DiffEntry],
    agents: &BTreeMap<String, AgentDefinition>,
    configs: &BTreeMap<String, McpConfig>,
    backups_dir: &Path,
    project_dir: Option<&Path>,
    timestamp: &str,
) {
    let mut backed_up = std::collections::HashSet::new();
    for diff in diffs {
        let Some(def) = agents.get(&diff.agent) else { continue };
        let Some(path) = target_path(def, &diff.scope, project_dir) else { continue };
        if !backed_up.contains(&path) && path.exists() {
            backup(&path, backups_dir, timestamp);
            backed_up.insert(path.clone());
        }
        let is_toml = def.format == "toml";
        match diff.action {
            DiffAction::Add => {
                let Some(cfg) = configs.get(&diff.mcp_name) else { continue };
                let mut current = if is_toml {
                    TomlAdapter::new(&def.key).read(&path)
                } else {
                    JsonAdapter::new(&def.key).read(&path)
                };
                current.insert(diff.mcp_name.clone(), cfg.clone());
                if is_toml { TomlAdapter::new(&def.key).write(&path, &current); }
                else { JsonAdapter::new(&def.key).write(&path, &current); }
            }
            DiffAction::Remove => {
                let names = vec![diff.mcp_name.clone()];
                if is_toml { TomlAdapter::new(&def.key).remove(&path, &names); }
                else { JsonAdapter::new(&def.key).remove(&path, &names); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{AgentDefinition, StdioConfig};

    #[test]
    fn applies_add_and_creates_backup() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-apply-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let cfg_path = base.join("mcp.json");
        std::fs::write(&cfg_path, r#"{"mcpServers":{"old":{"command":"x"}}}"#).unwrap();

        let mut agents = BTreeMap::new();
        agents.insert("test".to_string(), AgentDefinition {
            global: None, project: Some("mcp.json".into()),
            format: "json".into(), key: "mcpServers".into(),
            enabled: true, builtin: None });

        let mut configs = BTreeMap::new();
        configs.insert("git".to_string(), McpConfig::Stdio(StdioConfig {
            command: "npx".into(), args: None, env: None }));

        let diffs = vec![DiffEntry { action: DiffAction::Add,
            mcp_name: "git".into(), agent: "test".into(), scope: "project".into() }];
        let backups = base.join("backups");
        apply_diffs(&diffs, &agents, &configs, &backups, Some(&base), "STAMP");

        let written = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(written.contains("git"));
        assert!(backups.join("mcp.json-STAMP").exists());
        std::fs::remove_dir_all(&base).ok();
    }
}
```

在 `mod.rs` 增加：`pub mod differ;` 与 `pub mod applier;`

- [ ] **Step 4: 运行 applier 测试**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml applier::`
Expected: `applies_add_and_creates_backup ... ok`。

- [ ] **Step 5: 提交**

```bash
git add desktop/src-tauri/src/core/differ.rs desktop/src-tauri/src/core/applier.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add differ and applier with backup (Rust)"
```

---

## Task B9：暴露第一个 Tauri command 并跑通构建

**Files:**
- Create: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/main.rs`
- Modify: `desktop/src/App.tsx`

- [ ] **Step 1: 在 main.rs 注册 core 模块与命令**

确保 `desktop/src-tauri/src/main.rs` 顶部含：
```rust
mod core;
mod commands;
```
并在 `tauri::Builder` 上注册 `invoke_handler`（见 Step 3）。

- [ ] **Step 2: 编写 list_registry 命令**

`desktop/src-tauri/src/commands.rs`：
```rust
use crate::core::registry::builtin_registry;
use crate::core::types::RegistryEntry;

#[tauri::command]
pub fn list_registry() -> Vec<RegistryEntry> {
    builtin_registry()
}
```

- [ ] **Step 3: 注册 invoke_handler**

在 `main.rs` 的 builder 链中加入：
```rust
.invoke_handler(tauri::generate_handler![commands::list_registry])
```

- [ ] **Step 4: 前端调用并渲染数量**

把 `desktop/src/App.tsx` 的组件体改为调用命令（保留原 import 风格）：
```tsx
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

function App() {
  const [count, setCount] = useState<number | null>(null);
  useEffect(() => {
    invoke<unknown[]>("list_registry").then((r) => setCount(r.length));
  }, []);
  return <main><h1>Arca Desktop</h1><p>内置服务器：{count ?? "加载中…"}</p></main>;
}
export default App;
```

- [ ] **Step 5: 构建确认整体编译通过**

Run: `cd /Users/scoheart/scoheart/mcp-hub/desktop && cargo build --manifest-path src-tauri/Cargo.toml && npm run build`
Expected: Rust `Finished`；前端 `vite build` 成功，无类型错误。

- [ ] **Step 6: 跑全部 Rust 测试做回归**

Run: `cargo test --manifest-path /Users/scoheart/scoheart/mcp-hub/desktop/src-tauri/Cargo.toml`
Expected: 所有 core 测试（types/json_adapter/toml_adapter/registry/override/scanner/differ/applier）全部 `ok`。

- [ ] **Step 7: 提交**

```bash
git add desktop/src-tauri/src/commands.rs desktop/src-tauri/src/main.rs desktop/src/App.tsx
git commit -m "feat(desktop): expose list_registry command and render count"
```

---

## 自检结果（writing-plans self-review）

- **Spec 覆盖**：本计划覆盖 spec §2 架构（Tauri+Rust+共享 JSON）、§3 数据模型的 Rust 落地（types/override/Installation 概念的 differ 输入）、§5 错误处理中「写前备份」「解析失败返回空不破坏文件」。spec §4 界面、§6 前端测试、§7 的 GUI 项（仓库视图/矩阵/项目/扫描/Agents）属于 Plan 2、3，不在本计划范围——已在抬头说明。
- **占位符扫描**：无 TBD/TODO；每个代码步骤含完整代码与可运行命令。
- **类型一致性**：Rust 侧 `McpConfig`/`StdioConfig`/`HttpConfig`/`RegistryEntry`/`AgentDefinition` 贯穿各 Task 命名一致；适配器 `read/write/remove` 签名一致；`apply_diffs` 接收的 `configs: BTreeMap<String, McpConfig>` 即「已 ⊕ override 的 effective 配置」，与 B6 的 `apply_override` 产物对接。
- **已知衔接点（留给 Plan 2）**：「期望状态/安装记录的持久化（state.json、overrides.json、projects.json）」与「effective 配置的组装（registry + override → configs）」在 Plan 2 的 IPC 命令层完成；本计划仅提供纯函数 `apply_override` 与 `apply_diffs`。
