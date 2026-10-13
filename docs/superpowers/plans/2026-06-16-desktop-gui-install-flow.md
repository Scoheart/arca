# Arca 桌面 GUI · Plan 2：仓库主视图 + 安装闭环 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Plan 1 的 Rust core 之上，做出可用的核心闭环：浏览 MCP 仓库 → 打开安装弹窗（选 scope × 多 agent + 逐 agent 覆写）→ 预览将写入的内容 → 应用到各 agent 真实配置文件（自动备份），并能看到「某服务器已装在哪些地方」。

**Architecture:** Rust 后端新增 agents/overrides/effective 模块与一组 Tauri command（list_agents / scan_installed / preview_install / apply_install / uninstall）；React 前端做仓库网格 + 安装弹窗 + 预览/应用。安装状态以「扫描真实配置文件」为准（不引入独立 state.json），覆写持久化到 `~/.arca/overrides.json`。

**Tech Stack:** Rust（serde / serde_json / toml / dirs）、Tauri v2 command + `tauri-plugin-dialog`（选项目目录）、React + TypeScript + Vite。

依赖：Plan 1（`docs/superpowers/plans/2026-06-16-desktop-gui-foundation.md`）已完成 —— Rust core 在 `desktop/src-tauri/src/core/`，命令 `list_registry` 已通。本计划在分支 `feat/desktop-gui` 上继续。运行 cargo 前先 `export PATH="$HOME/.cargo/bin:$PATH"`。

---

## 文件结构

**Rust（`desktop/src-tauri/src/`）**
- Create `core/paths.rs`：解析 `~/.arca` 目录、backups、overrides.json 路径。
- Create `core/agents.rs`：从 root `agents.json`（include_str!）加载 18 个 agent；若 `~/.arca/agents.json` 存在则优先。
- Create `core/overrides.rs`：`OverrideRecord` + load/save `overrides.json`。
- Create `core/effective.rs`：`effective_config(entry, transport_pref, patch)` 把 registry 条目 + 覆写算成最终 `McpConfig`。
- Modify `core/mod.rs`：声明新模块。
- Modify `commands.rs`：新增 `list_agents` / `scan_installed` / `preview_install` / `apply_install` / `uninstall`。
- Modify `lib.rs`：注册新命令 + `tauri-plugin-dialog`。
- Modify `src-tauri/Cargo.toml`：加 `dirs`、`tauri-plugin-dialog`。

**前端（`desktop/src/`）**
- Create `lib/types.ts`：与 Rust 对齐的 TS 类型。
- Create `lib/api.ts`：`invoke` 封装。
- Create `components/Layout.tsx`：侧栏导航 + 主区。
- Create `components/RegistryGrid.tsx`：搜索 + 卡片网格（显示已装处数）。
- Create `components/InstallDialog.tsx`：scope/agent/项目/覆写/预览/应用。
- Modify `src/App.tsx`：组装 Layout + RegistryGrid + 弹窗状态。
- Modify `src/App.css`：基础样式（替换脚手架死样式）。

---

## Task 1：Rust `paths.rs` — 解析 ~/.arca 目录

**Files:** Create `desktop/src-tauri/src/core/paths.rs`; Modify `core/mod.rs`; Modify `Cargo.toml`

- [ ] **Step 1: 加 `dirs` 依赖**

在 `desktop/src-tauri/Cargo.toml` `[dependencies]` 加：`dirs = "5"`

- [ ] **Step 2: 写实现 + 测试**

`desktop/src-tauri/src/core/paths.rs`：
```rust
use std::path::PathBuf;

/// `~/.arca` —— 与 CLI 共用的数据目录
pub fn arca_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".arca")
}

pub fn backups_dir() -> PathBuf {
    arca_dir().join("backups")
}

pub fn overrides_file() -> PathBuf {
    arca_dir().join("overrides.json")
}

pub fn user_agents_file() -> PathBuf {
    arca_dir().join("agents.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arca_dir_ends_with_dot_arca() {
        assert!(arca_dir().ends_with(".arca"));
    }
    #[test]
    fn backups_under_arca_dir() {
        assert!(backups_dir().starts_with(arca_dir()));
    }
}
```

在 `core/mod.rs` 加 `pub mod paths;`

- [ ] **Step 3: 测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test --manifest-path desktop/src-tauri/Cargo.toml paths::`
Expected: 2 tests ok.

- [ ] **Step 4: 提交**

```bash
git add desktop/src-tauri/src/core/paths.rs desktop/src-tauri/src/core/mod.rs desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock
git commit -m "feat(desktop): add paths module for ~/.arca resolution"
```

---

## Task 2：Rust `agents.rs` — 加载 agent 定义

**Files:** Create `desktop/src-tauri/src/core/agents.rs`; Modify `core/mod.rs`

行为：内置 18 个 agent 来自 root `agents.json`（编译期内嵌）；若 `~/.arca/agents.json` 运行期存在则用它（与 CLI 共用），否则用内置。

- [ ] **Step 1: 写实现 + 测试**

`desktop/src-tauri/src/core/agents.rs`：
```rust
use crate::core::paths::user_agents_file;
use crate::core::types::AgentDefinition;
use std::collections::BTreeMap;
use std::fs;

/// 内置 agent 定义：编译期内嵌 root agents.json（与 TS CLI 共用的单一数据源）
const BUILTIN_AGENTS_JSON: &str = include_str!("../../../../data/agents.json");

pub fn builtin_agents() -> BTreeMap<String, AgentDefinition> {
    serde_json::from_str(BUILTIN_AGENTS_JSON).expect("agents.json must be valid")
}

/// 优先读 ~/.arca/agents.json（与 CLI 共用），否则用内置
pub fn load_agents() -> BTreeMap<String, AgentDefinition> {
    if let Ok(content) = fs::read_to_string(user_agents_file()) {
        if let Ok(map) = serde_json::from_str::<BTreeMap<String, AgentDefinition>>(&content) {
            return map;
        }
    }
    builtin_agents()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_loads_18_plus() {
        let a = builtin_agents();
        assert!(a.len() >= 18);
        assert_eq!(a["claude-code"].key, "mcpServers");
        assert_eq!(a["codex"].format, "toml");
    }
}
```

在 `core/mod.rs` 加 `pub mod agents;`

- [ ] **Step 2: 测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test --manifest-path desktop/src-tauri/Cargo.toml agents::`
Expected: `builtin_loads_18_plus ... ok`。

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/agents.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add agents loader (builtin + user override)"
```

---

## Task 3：Rust `overrides.rs` — 覆写持久化

**Files:** Create `desktop/src-tauri/src/core/overrides.rs`; Modify `core/mod.rs`

- [ ] **Step 1: 写实现 + 测试**

`desktop/src-tauri/src/core/overrides.rs`：
```rust
use crate::core::r#override::OverridePatch;
use crate::core::paths::overrides_file;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// 一条覆写记录：对某 (server, agent, scope, project) 用差异 patch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverrideRecord {
    pub server: String,
    pub agent: String,
    pub scope: String,                 // "global" | "project"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

impl OverrideRecord {
    pub fn to_patch(&self) -> OverridePatch {
        OverridePatch {
            args: self.args.clone(),
            env: self.env.clone(),
            url: self.url.clone(),
            headers: self.headers.clone(),
        }
    }
    pub fn matches(&self, server: &str, agent: &str, scope: &str, project_dir: Option<&str>) -> bool {
        self.server == server && self.agent == agent && self.scope == scope
            && self.project_dir.as_deref() == project_dir
    }
}

pub fn load_overrides_from(path: &Path) -> Vec<OverrideRecord> {
    fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

pub fn load_overrides() -> Vec<OverrideRecord> {
    load_overrides_from(&overrides_file())
}

pub fn save_overrides_to(path: &Path, records: &[OverrideRecord]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(records).map_err(|e| e.to_string())?;
    fs::write(path, json + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("arca-ovr-{}-{}.json", name, std::process::id()))
    }
    #[test]
    fn save_then_load_roundtrips() {
        let p = tmp("rt");
        let mut env = HashMap::new();
        env.insert("T".into(), "b".into());
        let rec = OverrideRecord {
            server: "github".into(), agent: "cursor".into(), scope: "global".into(),
            project_dir: None, args: None, env: Some(env), url: None, headers: None,
        };
        save_overrides_to(&p, &[rec.clone()]).unwrap();
        let back = load_overrides_from(&p);
        assert_eq!(back.len(), 1);
        assert!(back[0].matches("github", "cursor", "global", None));
        assert_eq!(back[0].to_patch().env.unwrap().get("T").unwrap(), "b");
        let _ = std::fs::remove_file(&p);
    }
    #[test]
    fn load_missing_is_empty() {
        assert!(load_overrides_from(Path::new("/nonexistent/ovr.json")).is_empty());
    }
}
```

在 `core/mod.rs` 加 `pub mod overrides;`

- [ ] **Step 2: 测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test --manifest-path desktop/src-tauri/Cargo.toml overrides::`
Expected: 2 tests ok.

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/overrides.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add override persistence (overrides.json)"
```

---

## Task 4：Rust `effective.rs` — 算最终配置

**Files:** Create `desktop/src-tauri/src/core/effective.rs`; Modify `core/mod.rs`

把 registry 条目（含 stdio 和/或 http）+ 可选 patch → 最终 `McpConfig`。选 transport：有 stdio 用 stdio，否则用 http（与 TS `resolveConfigForMcp` 一致）。

- [ ] **Step 1: 写实现 + 测试**

`desktop/src-tauri/src/core/effective.rs`：
```rust
use crate::core::r#override::{apply_override, OverridePatch};
use crate::core::types::{McpConfig, RegistryEntry};

/// 选定 registry 条目的基础配置：优先 stdio，其次 http
pub fn base_config(entry: &RegistryEntry) -> Option<McpConfig> {
    if let Some(s) = &entry.config.stdio {
        return Some(McpConfig::Stdio(s.clone()));
    }
    if let Some(h) = &entry.config.http {
        return Some(McpConfig::Http(h.clone()));
    }
    None
}

/// 最终配置 = base ⊕ patch
pub fn effective_config(entry: &RegistryEntry, patch: Option<&OverridePatch>) -> Option<McpConfig> {
    let base = base_config(entry)?;
    Some(match patch {
        Some(p) => apply_override(&base, p),
        None => base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{RegistryConfig, StdioConfig};
    fn entry() -> RegistryEntry {
        RegistryEntry {
            name: "git".into(), description: "d".into(), tags: vec![],
            config: RegistryConfig {
                stdio: Some(StdioConfig { command: "npx".into(), args: Some(vec!["-y".into()]), env: None }),
                http: None,
            },
        }
    }
    #[test]
    fn no_patch_returns_base() {
        let c = effective_config(&entry(), None).unwrap();
        match c { McpConfig::Stdio(s) => assert_eq!(s.command, "npx"), _ => panic!() }
    }
    #[test]
    fn patch_applies() {
        let patch = OverridePatch { args: Some(vec!["-x".into()]), ..Default::default() };
        let c = effective_config(&entry(), Some(&patch)).unwrap();
        match c { McpConfig::Stdio(s) => assert_eq!(s.args.unwrap(), vec!["-x"]), _ => panic!() }
    }
}
```

在 `core/mod.rs` 加 `pub mod effective;`

- [ ] **Step 2: 测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test --manifest-path desktop/src-tauri/Cargo.toml effective::`
Expected: 2 tests ok.

- [ ] **Step 3: 提交**

```bash
git add desktop/src-tauri/src/core/effective.rs desktop/src-tauri/src/core/mod.rs
git commit -m "feat(desktop): add effective config builder (base + patch)"
```

---

## Task 5：Rust 命令层 — list_agents / scan_installed

**Files:** Modify `desktop/src-tauri/src/commands.rs`; Modify `lib.rs`

- [ ] **Step 1: 写命令**

在 `desktop/src-tauri/src/commands.rs` 追加（保留已有 `list_registry`）：
```rust
use crate::core::agents::load_agents;
use crate::core::scanner::scan_agents;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub format: String,
    pub key: String,
    pub has_global: bool,
    pub has_project: bool,
    pub enabled: bool,
}

#[tauri::command]
pub fn list_agents() -> Vec<AgentInfo> {
    load_agents()
        .into_iter()
        .map(|(id, d)| AgentInfo {
            id,
            format: d.format,
            key: d.key,
            has_global: d.global.is_some(),
            has_project: d.project.is_some(),
            enabled: d.enabled,
        })
        .collect()
}

#[derive(Serialize)]
pub struct InstalledMcp {
    pub name: String,
    pub agent: String,
    pub scope: String,
    pub file_path: String,
}

/// 扫描真实配置文件，返回「谁装在哪」。project_dir 为空则只扫 global。
#[tauri::command]
pub fn scan_installed(project_dir: Option<String>) -> Vec<InstalledMcp> {
    let agents = load_agents();
    let pd = project_dir.as_deref().map(Path::new);
    scan_agents(&agents, pd, true)
        .into_iter()
        .map(|s| InstalledMcp {
            name: s.name, agent: s.agent, scope: s.scope, file_path: s.file_path,
        })
        .collect()
}
```

- [ ] **Step 2: 注册命令**

在 `lib.rs` 的 `generate_handler!` 列表里加 `commands::list_agents, commands::scan_installed`（与 `commands::list_registry` 并列）。

- [ ] **Step 3: 构建 + 回归测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo build --manifest-path desktop/src-tauri/Cargo.toml && cargo test --manifest-path desktop/src-tauri/Cargo.toml`
Expected: build Finished；所有测试通过。

- [ ] **Step 4: 提交**

```bash
git add desktop/src-tauri/src/commands.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): add list_agents and scan_installed commands"
```

---

## Task 6：Rust 命令层 — preview_install / apply_install / uninstall

**Files:** Modify `desktop/src-tauri/src/commands.rs`; Modify `lib.rs`

请求结构统一：服务器名 + scope + 目标 agents + 可选 project_dir + 逐 agent 覆写 patch（map: agentId → {args/env/url/headers}）。preview 返回每个 (agent) 将写入的文件路径与最终配置 JSON；apply 真正写入（备份）；uninstall 移除。

- [ ] **Step 1: 写命令**

在 `commands.rs` 追加：
```rust
use crate::core::applier::{apply_diffs, ApplyError};
use crate::core::differ::DiffEntry; // 复用 DiffAction
use crate::core::differ::DiffAction;
use crate::core::effective::effective_config;
use crate::core::r#override::OverridePatch;
use crate::core::paths::backups_dir;
use crate::core::registry::read_registry;
use crate::core::paths::arca_dir;
use std::collections::{BTreeMap, HashMap};

#[derive(serde::Deserialize)]
pub struct PatchInput {
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}
impl PatchInput {
    fn to_patch(&self) -> OverridePatch {
        OverridePatch { args: self.args.clone(), env: self.env.clone(),
            url: self.url.clone(), headers: self.headers.clone() }
    }
}

#[derive(serde::Deserialize)]
pub struct InstallRequest {
    pub server_name: String,
    pub scope: String,                       // "global" | "project"
    pub agents: Vec<String>,
    pub project_dir: Option<String>,
    #[serde(default)]
    pub overrides: HashMap<String, PatchInput>, // agentId -> patch
}

#[derive(serde::Serialize)]
pub struct PlannedWrite {
    pub agent: String,
    pub file_path: String,
    pub config_json: String,
}

fn resolve_entry(server_name: &str) -> Result<crate::core::types::RegistryEntry, String> {
    let reg = read_registry(&arca_dir().join("registry"));
    reg.into_iter().find(|e| e.name == server_name)
        .ok_or_else(|| format!("server not found: {}", server_name))
}

fn target_file(agent: &crate::core::types::AgentDefinition, scope: &str, project_dir: Option<&str>) -> Option<std::path::PathBuf> {
    use crate::core::scanner::expand_tilde;
    if scope == "global" {
        agent.global.as_ref().map(|g| expand_tilde(g))
    } else {
        match (&agent.project, project_dir) {
            (Some(p), Some(base)) => Some(std::path::Path::new(base).join(p)),
            _ => None,
        }
    }
}

#[tauri::command]
pub fn preview_install(req: InstallRequest) -> Result<Vec<PlannedWrite>, String> {
    let entry = resolve_entry(&req.server_name)?;
    let agents = load_agents();
    let mut out = Vec::new();
    for agent_id in &req.agents {
        let Some(def) = agents.get(agent_id) else { continue };
        let Some(path) = target_file(def, &req.scope, req.project_dir.as_deref()) else { continue };
        let patch = req.overrides.get(agent_id).map(|p| p.to_patch());
        let cfg = effective_config(&entry, patch.as_ref())
            .ok_or_else(|| format!("no config for {}", req.server_name))?;
        out.push(PlannedWrite {
            agent: agent_id.clone(),
            file_path: path.display().to_string(),
            config_json: serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn apply_install(req: InstallRequest) -> Result<(), Vec<String>> {
    let entry = resolve_entry(&req.server_name).map_err(|e| vec![e])?;
    let agents = load_agents();
    // 组 diffs（全部 Add 当前服务器）+ effective configs
    let mut diffs = Vec::new();
    let mut configs: BTreeMap<String, McpConfig> = BTreeMap::new();
    // 注意：每个 agent 的 effective 可能不同（覆写不同），applier 的 configs 是按 mcp 名取的，
    // 因此对「逐 agent 覆写不同」的场景，按 agent 分别调用 applier。
    let mut errors: Vec<String> = Vec::new();
    for agent_id in &req.agents {
        let Some(def) = agents.get(agent_id) else { continue };
        if target_file(def, &req.scope, req.project_dir.as_deref()).is_none() { continue; }
        let patch = req.overrides.get(agent_id).map(|p| p.to_patch());
        let Some(cfg) = effective_config(&entry, patch.as_ref()) else { continue };
        let mut one = BTreeMap::new();
        one.insert(req.server_name.clone(), cfg);
        let mut adef = BTreeMap::new();
        adef.insert(agent_id.clone(), def.clone());
        let diff = vec![DiffEntry { action: DiffAction::Add,
            mcp_name: req.server_name.clone(), agent: agent_id.clone(), scope: req.scope.clone() }];
        if let Err(errs) = apply_diffs(&diff, &adef, &one, &backups_dir(),
            req.project_dir.as_deref().map(std::path::Path::new), "STAMP") {
            for e in errs { errors.push(format!("{}: {}", e.target, e.error)); }
        }
        let _ = (&mut diffs, &mut configs); // silence unused if refactored
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

#[tauri::command]
pub fn uninstall(req: InstallRequest) -> Result<(), Vec<String>> {
    let agents = load_agents();
    let mut errors = Vec::new();
    for agent_id in &req.agents {
        let Some(def) = agents.get(agent_id) else { continue };
        if target_file(def, &req.scope, req.project_dir.as_deref()).is_none() { continue; }
        let mut adef = BTreeMap::new();
        adef.insert(agent_id.clone(), def.clone());
        let diff = vec![DiffEntry { action: DiffAction::Remove,
            mcp_name: req.server_name.clone(), agent: agent_id.clone(), scope: req.scope.clone() }];
        let empty: BTreeMap<String, McpConfig> = BTreeMap::new();
        if let Err(errs) = apply_diffs(&diff, &adef, &empty, &backups_dir(),
            req.project_dir.as_deref().map(std::path::Path::new), "STAMP") {
            for e in errs { errors.push(format!("{}: {}", e.target, e.error)); }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
```
（注：顶部需 `use crate::core::types::McpConfig;`。`"STAMP"` 时间戳：先用固定串占位；若要真实时间戳，用 `chrono::Local::now().format("%Y-%m-%dT%H-%M-%S")`，Cargo 已有 chrono。实现时改用 chrono 生成。）

- [ ] **Step 2: 用真实时间戳**

把两处 `"STAMP"` 替换为：
```rust
&chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string()
```

- [ ] **Step 3: 注册命令**

`lib.rs` 的 `generate_handler!` 加 `commands::preview_install, commands::apply_install, commands::uninstall`。

- [ ] **Step 4: 写集成测试（在 commands.rs 的 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_returns_planned_write_for_known_server() {
        // filesystem 是内置服务器
        let req = InstallRequest {
            server_name: "filesystem".into(), scope: "global".into(),
            agents: vec!["claude-code".into()], project_dir: None,
            overrides: HashMap::new(),
        };
        let plan = preview_install(req).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].agent, "claude-code");
        assert!(plan[0].config_json.contains("command"));
    }
}
```

- [ ] **Step 5: 构建 + 测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo build --manifest-path desktop/src-tauri/Cargo.toml && cargo test --manifest-path desktop/src-tauri/Cargo.toml`
Expected: build Finished；新测试 + 全部既有测试通过。修掉编译器指出的 unused import / 未用变量（如 `diffs`/`configs` 占位若多余请删除）。

- [ ] **Step 6: 提交**

```bash
git add desktop/src-tauri/src/commands.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): add preview_install/apply_install/uninstall commands"
```

---

## Task 7：前端类型 + API 封装

**Files:** Create `desktop/src/lib/types.ts`, `desktop/src/lib/api.ts`

- [ ] **Step 1: 类型**

`desktop/src/lib/types.ts`：
```ts
export interface StdioConfig { command: string; args?: string[]; env?: Record<string, string>; }
export interface HttpConfig { type: "http" | "sse"; url: string; headers?: Record<string, string>; }
export interface RegistryEntry {
  name: string; description: string; tags: string[];
  config: { stdio?: StdioConfig; http?: HttpConfig };
}
export interface AgentInfo {
  id: string; format: string; key: string;
  has_global: boolean; has_project: boolean; enabled: boolean;
}
export interface InstalledMcp { name: string; agent: string; scope: string; file_path: string; }
export interface PlannedWrite { agent: string; file_path: string; config_json: string; }
export interface PatchInput {
  args?: string[]; env?: Record<string, string>; url?: string; headers?: Record<string, string>;
}
export interface InstallRequest {
  server_name: string; scope: "global" | "project"; agents: string[];
  project_dir?: string; overrides: Record<string, PatchInput>;
}
```

- [ ] **Step 2: API 封装**

`desktop/src/lib/api.ts`：
```ts
import { invoke } from "@tauri-apps/api/core";
import type { RegistryEntry, AgentInfo, InstalledMcp, PlannedWrite, InstallRequest } from "./types";

export const listRegistry = () => invoke<RegistryEntry[]>("list_registry");
export const listAgents = () => invoke<AgentInfo[]>("list_agents");
export const scanInstalled = (projectDir?: string) =>
  invoke<InstalledMcp[]>("scan_installed", { projectDir: projectDir ?? null });
export const previewInstall = (req: InstallRequest) =>
  invoke<PlannedWrite[]>("preview_install", { req });
export const applyInstall = (req: InstallRequest) =>
  invoke<void>("apply_install", { req });
export const uninstall = (req: InstallRequest) =>
  invoke<void>("uninstall", { req });
```

- [ ] **Step 3: 类型检查**

Run: `cd /Users/scoheart/scoheart/mcp-hub/desktop && npx tsc --noEmit`
Expected: 无类型错误。

- [ ] **Step 4: 提交**

```bash
git add desktop/src/lib/types.ts desktop/src/lib/api.ts
git commit -m "feat(desktop): add frontend types and api wrappers"
```

---

## Task 8：前端 Layout + 仓库网格

**Files:** Create `desktop/src/components/Layout.tsx`, `desktop/src/components/RegistryGrid.tsx`; Modify `src/App.tsx`, `src/App.css`

- [ ] **Step 1: Layout**

`desktop/src/components/Layout.tsx`：
```tsx
import { ReactNode } from "react";

export function Layout({ children }: { children: ReactNode }) {
  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="brand">Arca</div>
        <nav>
          <div className="nav-item active">📦 仓库</div>
        </nav>
      </aside>
      <main className="content">{children}</main>
    </div>
  );
}
```

- [ ] **Step 2: 仓库网格（搜索 + 卡片 + 已装处数）**

`desktop/src/components/RegistryGrid.tsx`：
```tsx
import { useEffect, useMemo, useState } from "react";
import { listRegistry, scanInstalled } from "../lib/api";
import type { RegistryEntry, InstalledMcp } from "../lib/types";

export function RegistryGrid({ onPick }: { onPick: (e: RegistryEntry) => void }) {
  const [entries, setEntries] = useState<RegistryEntry[]>([]);
  const [installed, setInstalled] = useState<InstalledMcp[]>([]);
  const [q, setQ] = useState("");

  useEffect(() => {
    listRegistry().then(setEntries).catch(console.error);
    scanInstalled().then(setInstalled).catch(console.error);
  }, []);

  const countFor = (name: string) => installed.filter((i) => i.name === name).length;

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    if (!s) return entries;
    return entries.filter(
      (e) => e.name.toLowerCase().includes(s) || e.description.toLowerCase().includes(s)
    );
  }, [entries, q]);

  return (
    <div>
      <input className="search" placeholder="🔍 搜索服务器…" value={q}
        onChange={(e) => setQ(e.target.value)} />
      <div className="grid">
        {filtered.map((e) => {
          const c = countFor(e.name);
          return (
            <button key={e.name} className="card" onClick={() => onPick(e)}>
              <div className="card-name">{e.name}</div>
              <div className="card-desc">{e.description}</div>
              <div className={"card-badge" + (c ? " on" : "")}>
                {c ? `已装 ${c} 处` : "未安装"}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
```

- [ ] **Step 3: App.tsx 组装（弹窗状态留待 Task 9 接入）**

`desktop/src/App.tsx`：
```tsx
import { useState } from "react";
import "./App.css";
import { Layout } from "./components/Layout";
import { RegistryGrid } from "./components/RegistryGrid";
import { InstallDialog } from "./components/InstallDialog";
import type { RegistryEntry } from "./lib/types";

function App() {
  const [picked, setPicked] = useState<RegistryEntry | null>(null);
  return (
    <Layout>
      <RegistryGrid onPick={setPicked} />
      {picked && <InstallDialog entry={picked} onClose={() => setPicked(null)} />}
    </Layout>
  );
}
export default App;
```
（注：`InstallDialog` 在 Task 9 创建；本步若先构建会因缺组件报错，可先放一个最小占位 `InstallDialog`，Task 9 再补全。为避免红，本步同时创建占位文件 `components/InstallDialog.tsx`：`export function InstallDialog(_: any){ return null; }`，Task 9 覆盖它。）

- [ ] **Step 4: 基础样式（替换脚手架死样式）**

把 `desktop/src/App.css` 内容整体替换为：
```css
:root { color-scheme: light dark; font-family: system-ui, sans-serif; }
body { margin: 0; }
.layout { display: flex; height: 100vh; }
.sidebar { width: 180px; background: #1c1c22; color: #ddd; padding: 16px; }
.brand { font-weight: 700; font-size: 20px; margin-bottom: 16px; }
.nav-item { padding: 8px 10px; border-radius: 6px; cursor: pointer; }
.nav-item.active { background: #2a4d6e; }
.content { flex: 1; padding: 20px; overflow: auto; }
.search { width: 100%; padding: 10px 12px; font-size: 14px; border: 1px solid #ccc;
  border-radius: 8px; margin-bottom: 16px; box-sizing: border-box; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 12px; }
.card { text-align: left; border: 1px solid #ddd; border-radius: 10px; padding: 12px;
  background: transparent; cursor: pointer; }
.card:hover { border-color: #6ea8fe; }
.card-name { font-weight: 600; margin-bottom: 4px; }
.card-desc { font-size: 12px; opacity: .7; min-height: 32px; }
.card-badge { font-size: 11px; opacity: .5; margin-top: 6px; }
.card-badge.on { color: #69db7c; opacity: 1; }
.dialog-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,.5);
  display: flex; align-items: center; justify-content: center; }
.dialog { background: #23232b; color: #eee; padding: 20px; border-radius: 12px;
  width: 520px; max-height: 80vh; overflow: auto; }
.dialog h2 { margin-top: 0; }
.field { margin: 10px 0; }
.agent-row { display: flex; align-items: center; gap: 8px; padding: 2px 0; }
.preview { background: #15151a; border-radius: 8px; padding: 10px; font-size: 12px;
  white-space: pre-wrap; max-height: 200px; overflow: auto; }
.btn { padding: 8px 14px; border-radius: 8px; border: none; cursor: pointer; }
.btn-primary { background: #2a6ee6; color: white; }
.btn-ghost { background: transparent; color: #aaa; }
.row-end { display: flex; gap: 8px; justify-content: flex-end; margin-top: 12px; }
```

- [ ] **Step 5: 构建**

Run: `cd /Users/scoheart/scoheart/mcp-hub/desktop && npm run build`
Expected: tsc + vite build 成功（占位 InstallDialog 存在则无错）。

- [ ] **Step 6: 提交**

```bash
git add desktop/src/components/Layout.tsx desktop/src/components/RegistryGrid.tsx desktop/src/components/InstallDialog.tsx desktop/src/App.tsx desktop/src/App.css
git commit -m "feat(desktop): registry grid view with search and install count"
```

---

## Task 9：前端安装弹窗（scope × agent × 覆写 → 预览 → 应用）

**Files:** Modify `desktop/src/components/InstallDialog.tsx`; Modify `Cargo.toml` + `lib.rs`（加 dialog 插件用于选目录）

- [ ] **Step 1: 加 tauri-plugin-dialog（选项目目录用）**

`Cargo.toml` `[dependencies]` 加 `tauri-plugin-dialog = "2"`；`lib.rs` 的 builder 链加 `.plugin(tauri_plugin_dialog::init())`。
前端装 JS 侧：`cd desktop && npm install @tauri-apps/plugin-dialog`。

- [ ] **Step 2: 安装弹窗组件**

`desktop/src/components/InstallDialog.tsx`（覆盖占位）：
```tsx
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listAgents, previewInstall, applyInstall } from "../lib/api";
import type { RegistryEntry, AgentInfo, PlannedWrite, InstallRequest, PatchInput } from "../lib/types";

export function InstallDialog({ entry, onClose }: { entry: RegistryEntry; onClose: () => void }) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [scope, setScope] = useState<"global" | "project">("global");
  const [projectDir, setProjectDir] = useState<string>("");
  const [selected, setSelected] = useState<Record<string, boolean>>({});
  const [overrides, setOverrides] = useState<Record<string, PatchInput>>({});
  const [preview, setPreview] = useState<PlannedWrite[] | null>(null);
  const [msg, setMsg] = useState<string>("");

  useEffect(() => { listAgents().then(setAgents).catch(console.error); }, []);

  const eligible = agents.filter((a) => (scope === "global" ? a.has_global : a.has_project));
  const chosen = eligible.filter((a) => selected[a.id]).map((a) => a.id);

  const req = (): InstallRequest => ({
    server_name: entry.name, scope, agents: chosen,
    project_dir: scope === "project" ? projectDir : undefined,
    overrides,
  });

  const pickFolder = async () => {
    const dir = await open({ directory: true });
    if (typeof dir === "string") setProjectDir(dir);
  };

  const doPreview = async () => {
    setMsg("");
    try { setPreview(await previewInstall(req())); }
    catch (e) { setMsg("预览失败：" + String(e)); }
  };

  const doApply = async () => {
    setMsg("");
    try { await applyInstall(req()); setMsg("✅ 已应用"); }
    catch (e) { setMsg("应用失败：" + (Array.isArray(e) ? e.join("; ") : String(e))); }
  };

  const setEnv = (agentId: string, val: string) => {
    // 极简覆写：把 "K=V,K2=V2" 解析成 env
    const env: Record<string, string> = {};
    val.split(",").map((s) => s.trim()).filter(Boolean).forEach((kv) => {
      const i = kv.indexOf("="); if (i > 0) env[kv.slice(0, i)] = kv.slice(i + 1);
    });
    setOverrides((o) => ({ ...o, [agentId]: { ...o[agentId], env } }));
  };

  const canSubmit = chosen.length > 0 && (scope === "global" || projectDir);

  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h2>安装 {entry.name}</h2>

        <div className="field">
          <b>Scope：</b>
          <label><input type="radio" checked={scope === "global"} onChange={() => setScope("global")} /> 全局</label>
          {" "}
          <label><input type="radio" checked={scope === "project"} onChange={() => setScope("project")} /> 项目</label>
          {scope === "project" && (
            <div style={{ marginTop: 6 }}>
              <button className="btn btn-ghost" onClick={pickFolder}>选择项目目录…</button>
              <span style={{ fontSize: 12, opacity: .7 }}> {projectDir || "(未选)"}</span>
            </div>
          )}
        </div>

        <div className="field">
          <b>目标 Agents：</b>
          {eligible.length === 0 && <div style={{ fontSize: 12, opacity: .6 }}>该 scope 下无可用 agent</div>}
          {eligible.map((a) => (
            <div className="agent-row" key={a.id}>
              <label>
                <input type="checkbox" checked={!!selected[a.id]}
                  onChange={(e) => setSelected((s) => ({ ...s, [a.id]: e.target.checked }))} />
                {" "}{a.id} <span style={{ fontSize: 11, opacity: .5 }}>({a.format})</span>
              </label>
              {selected[a.id] && (
                <input className="search" style={{ margin: 0, flex: 1 }}
                  placeholder="覆写 env: KEY=VALUE,KEY2=VALUE2"
                  onChange={(e) => setEnv(a.id, e.target.value)} />
              )}
            </div>
          ))}
        </div>

        {preview && (
          <div className="field">
            <b>预览（将写入）：</b>
            {preview.map((p) => (
              <div key={p.agent} className="preview">
                {p.agent} → {p.file_path}{"\n"}{p.config_json}
              </div>
            ))}
          </div>
        )}

        {msg && <div className="field" style={{ color: msg.startsWith("✅") ? "#69db7c" : "#ff6b6b" }}>{msg}</div>}

        <div className="row-end">
          <button className="btn btn-ghost" onClick={onClose}>取消</button>
          <button className="btn btn-ghost" disabled={!canSubmit} onClick={doPreview}>预览改动</button>
          <button className="btn btn-primary" disabled={!canSubmit} onClick={doApply}>应用</button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 3: 构建**

Run: `cd /Users/scoheart/scoheart/mcp-hub/desktop && cargo build --manifest-path src-tauri/Cargo.toml && npm run build`
Expected: 两侧均成功；无 TS 错误。

- [ ] **Step 4: 提交**

```bash
git add desktop/src/components/InstallDialog.tsx desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock desktop/src-tauri/src/lib.rs desktop/package.json desktop/package-lock.json
git commit -m "feat(desktop): install dialog with scope/agents/override, preview and apply"
```

---

## Task 10：端到端冒烟（手动）+ 文档

**Files:** Create `desktop/README.md`

- [ ] **Step 1: 写运行说明**

`desktop/README.md`：
```md
# Arca Desktop

Tauri + React 桌面端 MCP 配置管理。

## 开发运行
```bash
cd desktop
npm install
npm run tauri dev   # 打开 Arca 窗口
```

## 功能（Plan 2）
- 浏览/搜索内置 MCP 仓库（40+）
- 安装到 全局 / 项目，多 agent 一次应用
- 逐 agent 覆写 env
- 预览将写入的内容；写前自动备份到 ~/.arca/backups/

## 数据
- 服务器/agent 定义：仓库根 `data/registry.json` / `data/agents.json`（与 CLI 共用）
- 覆写：`~/.arca/overrides.json`
```

- [ ] **Step 2: 手动冒烟（需在有显示器的机器执行，记录结果）**

在本机执行 `cd desktop && npm run tauri dev`，验证：
1. 窗口打开，显示仓库网格 + 40+ 卡片。
2. 搜索 "git" 能过滤。
3. 点一张卡 → 弹窗；选「全局」+ 勾 claude-code → 「预览改动」显示 `~/.claude.json` 与配置 JSON。
4. 「应用」后提示 ✅；用 `cat ~/.claude.json` 确认 mcpServers 多了该服务器；`ls ~/.arca/backups/` 有备份。
5. 关闭重开，卡片显示「已装 1 处」。

把结果记录在 commit message 或 PR 描述中。

- [ ] **Step 3: 提交**

```bash
git add desktop/README.md
git commit -m "docs(desktop): add run instructions for Plan 2 install flow"
```

---

## 自检结果（writing-plans self-review）

- **Spec 覆盖**：实现 spec §4 仓库主视图 + 安装弹窗（scope×agent×覆写）+ 预览/应用，§3 数据模型的 Override 持久化与 effective 计算，§5 写前备份 + 单目标失败聚合（applier 已返回 Vec<ApplyError>）。矩阵总览/项目管理页/扫描导入页/Agents 管理 = Plan 3，不在此。
- **占位符扫描**：无 TODO/TBD；各步含完整代码与命令。唯一刻意的临时占位是 Task 8 Step 3 的最小 `InstallDialog` 占位，Task 9 覆盖——已显式说明。
- **类型一致性**：前端 `InstallRequest`/`PatchInput`/`PlannedWrite`/`AgentInfo`/`InstalledMcp` 与 Rust 端 serde 结构字段名一致（注意 Rust snake_case：`server_name`/`project_dir`/`has_global`/`file_path`/`config_json`——TS 类型也用 snake_case 匹配 serde 默认）。`api.ts` 的 `invoke("preview_install", { req })` 参数名 `req` 与命令签名 `req: InstallRequest` 一致；`scan_installed` 参数 `projectDir`（Tauri 自动 camelCase↔snake_case：命令参数 `project_dir` 在 JS 侧用 `projectDir`）。实现时若 Tauri v2 的参数大小写约定导致 mismatch，以「JS 端 camelCase、Rust 端 snake_case」为准调整。
- **已知风险**：Tauri v2 命令参数命名约定（camelCase vs snake_case）需在 Task 5/7 联调时确认；`scan_installed(scan_all=true)` 会扫描所有 agent（含未启用），符合「显示真实安装分布」意图。
