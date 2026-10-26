# Pinned Agent Shortcuts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent top-bar launcher for up to six configurable Agents, with pin, unpin, drag, and keyboard ordering inside the existing Agent selector.

**Architecture:** Store ordered Agent IDs in a strongly typed `settings.ui.pinned_agents` section and expose validated read/write operations from a focused Rust core module. Keep Tauri and TypeScript API layers thin, isolate deterministic picker transformations in a unit-tested TypeScript module, and move Agent navigation out of `Layout.tsx` into a dedicated component backed by a rollback-capable persistence hook.

**Tech Stack:** Rust 2021, serde/serde_json, Tauri 2 commands, React 19, TypeScript 5.8, native HTML drag events, Node test runner, Tailwind utility classes plus `desktop/src/index.css`.

## Global Constraints

- Display at most 6 pinned Agents; only catalog entries with `has_global = true` are pinnable.
- Preserve pinned order exactly and persist it in `~/.arca/settings.json` under `ui.pinned_agents`.
- Preserve every other settings section and unknown top-level field through the existing strict, locked, atomic, optimistic-concurrency write path.
- Never use `localStorage` as the pinned Agent source of truth.
- Keep the closed Agent selector exactly `220px` wide normally and `170px` at widths up to `1080px`; keep its popup exactly `390px` wide.
- Use `30px` pinned icons normally and `28px` at widths up to `1080px`; selected styling must not change layout dimensions.
- Preserve all top-bar commands without horizontal scrolling or clipping at `1200x820` and `900x600`.
- Provide tooltip/accessibility names, keyboard pin actions, and `Option+ArrowUp` / `Option+ArrowDown` ordering equivalents.
- Do not add a drag-and-drop dependency; native drag events plus tested pure reorder functions are sufficient for a maximum of six rows.
- Do not bump versions, tag, push, publish, or replace `/Applications/Arca.app` during implementation. Real installed-app screenshots require a separate, explicitly approved stable-release step.

---

## File Structure

| File | Responsibility |
|---|---|
| `core/src/settings.rs` | Strongly typed optional `UiSettings` section and settings round-trip tests |
| `core/src/pinned_agents.rs` | Maximum-count constant, semantic normalization, validation, strict read, and atomic replace operation |
| `core/src/lib.rs` | Export the focused pinned-Agent core module |
| `desktop/src-tauri/src/commands.rs` | Thin Tauri commands for reading and replacing ordered IDs |
| `desktop/src-tauri/src/lib.rs` | Register the two commands |
| `desktop/src/lib/api.ts` | Typed TypeScript invoke wrappers |
| `desktop/src/lib/pinnedAgents.ts` | Pure filtering, grouping, pin/unpin, and reorder transformations |
| `desktop/src/lib/pinnedAgents.test.ts` | Node unit tests for every deterministic picker transition |
| `desktop/src/hooks/usePinnedAgents.ts` | Load, optimistic draft display, save serialization, rollback, and toast handling |
| `desktop/src/components/AgentNavigation.tsx` | Pinned top-bar launcher and the complete searchable management popup |
| `desktop/src/components/Layout.tsx` | Render the extracted Agent navigation inside the existing top bar |
| `desktop/src/components/icons.tsx` | Pin and drag-handle icons following the existing icon API |
| `desktop/src/index.css` | Stable dimensions, popup sections, row actions, drag states, and responsive rules |

---

### Task 1: Persist and expose ordered pinned Agent IDs

**Files:**
- Modify: `core/src/settings.rs`
- Create: `core/src/pinned_agents.rs`
- Modify: `core/src/lib.rs`
- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `desktop/src/lib/api.ts`

**Interfaces:**
- Consumes: `crate::agents::load_agents()`, `crate::settings::{load_settings_strict, mutate_settings}`.
- Produces: `MAX_PINNED_AGENTS: usize`, `get_pinned_agents() -> Result<Vec<String>, String>`, `set_pinned_agents(Vec<String>) -> Result<Vec<String>, String>`, Tauri commands with the same names, and TypeScript wrappers returning `Promise<string[]>`.

- [ ] **Step 1: Add failing settings and core behavior tests**

Add this settings round-trip test inside `core/src/settings.rs`'s existing test module:

```rust
#[test]
fn ui_section_and_unknown_fields_survive_settings_roundtrip() {
    let json = r#"{
      "ui": {"pinned_agents": ["claude-code", "codex"]},
      "future_section": {"keep": true}
    }"#;
    let settings: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(
        settings.ui.as_ref().unwrap().pinned_agents,
        vec!["claude-code", "codex"]
    );
    let encoded = serde_json::to_value(settings).unwrap();
    assert_eq!(encoded["ui"]["pinned_agents"][0], "claude-code");
    assert_eq!(encoded["future_section"]["keep"], true);
}

#[test]
fn mutation_refuses_a_concurrent_settings_change() {
    let home = crate::testenv::TestHome::new("settings-concurrent");
    let path = home.home.join(".arca/settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"imported":"original"}"#).unwrap();

    let result = mutate_settings(|settings| {
        settings.imported = Some("candidate".into());
        std::fs::write(&path, r#"{"imported":"concurrent"}"#).unwrap();
    });

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        r#"{"imported":"concurrent"}"#,
    );
}
```

Create `core/src/pinned_agents.rs`, initially containing only the imports and this test module, and add `pub mod pinned_agents;` to `core/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::load_agents;
    use crate::testenv::TestHome;
    use serde_json::Value;
    use std::fs;

    fn settings_path(home: &TestHome) -> std::path::PathBuf {
        home.home.join(".arca/settings.json")
    }

    #[test]
    fn valid_ids_roundtrip_in_input_order_and_preserve_unknown_fields() {
        let home = TestHome::new("pinned-roundtrip");
        let path = settings_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"future_section":{"keep":true}}"#).unwrap();

        let saved = set_pinned_agents(vec!["codex".into(), "claude-code".into()]).unwrap();
        assert_eq!(saved, vec!["codex", "claude-code"]);
        assert_eq!(get_pinned_agents().unwrap(), saved);

        let value: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["future_section"]["keep"], true);
        assert_eq!(value["ui"]["pinned_agents"][0], "codex");
    }

    #[test]
    fn write_rejects_limit_duplicates_unknown_and_read_only_agents() {
        let _home = TestHome::new("pinned-validation");
        let configurable: Vec<String> = load_agents()
            .into_iter()
            .filter_map(|(id, definition)| definition.global.is_some().then_some(id))
            .collect();
        assert!(configurable.len() >= MAX_PINNED_AGENTS + 1);
        assert!(set_pinned_agents(configurable[..MAX_PINNED_AGENTS + 1].to_vec()).is_err());
        assert!(set_pinned_agents(vec!["codex".into(), "codex".into()]).is_err());
        assert!(set_pinned_agents(vec!["missing-agent".into()]).is_err());

        let read_only = load_agents()
            .into_iter()
            .find_map(|(id, definition)| definition.global.is_none().then_some(id))
            .expect("catalog must retain at least one read-only Agent");
        assert!(set_pinned_agents(vec![read_only]).is_err());
    }

    #[test]
    fn read_normalizes_stale_duplicates_and_excess_without_rewriting_file() {
        let home = TestHome::new("pinned-normalize");
        let path = settings_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{
          "ui": {
            "pinned_agents": [
              "codex", "missing-agent", "codex", "claude-code",
              "qoder", "pi", "cursor", "gemini", "opencode"
            ]
          }
        }"#;
        fs::write(&path, original).unwrap();

        let loaded = get_pinned_agents().unwrap();
        assert_eq!(loaded.first().map(String::as_str), Some("codex"));
        assert_eq!(loaded.iter().filter(|id| id.as_str() == "codex").count(), 1);
        assert!(loaded.len() <= MAX_PINNED_AGENTS);
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn corrupt_settings_are_not_replaced() {
        let home = TestHome::new("pinned-corrupt");
        let path = settings_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"ui":{"pinned_agents":["#;
        fs::write(&path, original).unwrap();

        assert!(get_pinned_agents().is_err());
        assert!(set_pinned_agents(vec!["codex".into()]).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }
}
```

- [ ] **Step 2: Run the focused tests and confirm the missing schema/API failure**

Run:

```bash
cargo test -p arca-core pinned_agents -- --nocapture
cargo test -p arca-core ui_section_and_unknown_fields_survive_settings_roundtrip -- --nocapture
cargo test -p arca-core mutation_refuses_a_concurrent_settings_change -- --nocapture
```

Expected: compilation fails because `Settings::ui`, `MAX_PINNED_AGENTS`, `get_pinned_agents`, and `set_pinned_agents` do not exist; the concurrency test itself compiles against the existing mutation API.

- [ ] **Step 3: Add the typed UI settings section**

Add this type above `Settings` in `core/src/settings.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UiSettings {
    #[serde(default)]
    pub pinned_agents: Vec<String>,
}
```

Add this field to `Settings` immediately before the CLI-owned fields:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub ui: Option<UiSettings>,
```

- [ ] **Step 4: Implement strict normalization, validation, and atomic replacement**

Replace the test-only `core/src/pinned_agents.rs` shell with these production definitions above its existing tests:

```rust
use crate::agents::load_agents;
use crate::settings::{load_settings_strict, mutate_settings, UiSettings};
use std::collections::BTreeSet;

pub const MAX_PINNED_AGENTS: usize = 6;

fn configurable_agent_ids() -> BTreeSet<String> {
    load_agents()
        .into_iter()
        .filter_map(|(id, definition)| definition.global.is_some().then_some(id))
        .collect()
}

fn normalize_loaded(ids: Vec<String>, configurable: &BTreeSet<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| configurable.contains(id))
        .filter(|id| seen.insert(id.clone()))
        .take(MAX_PINNED_AGENTS)
        .collect()
}

fn validate_requested(
    ids: &[String],
    configurable: &BTreeSet<String>,
) -> Result<(), String> {
    if ids.len() > MAX_PINNED_AGENTS {
        return Err(format!("最多只能置顶 {MAX_PINNED_AGENTS} 个 Agent"));
    }
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err(format!("置顶 Agent 不能重复: {id}"));
        }
        if !configurable.contains(id) {
            return Err(format!("Agent 不存在或没有全局配置能力: {id}"));
        }
    }
    Ok(())
}

pub fn get_pinned_agents() -> Result<Vec<String>, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let ids = settings.ui.unwrap_or_default().pinned_agents;
    Ok(normalize_loaded(ids, &configurable_agent_ids()))
}

pub fn set_pinned_agents(ids: Vec<String>) -> Result<Vec<String>, String> {
    let configurable = configurable_agent_ids();
    validate_requested(&ids, &configurable)?;
    let saved = ids.clone();
    mutate_settings(move |settings| {
        settings.ui.get_or_insert_with(UiSettings::default).pinned_agents = ids;
    })
    .map_err(|error| error.to_string())?;
    Ok(saved)
}
```

- [ ] **Step 5: Add and register thin Tauri commands**

Add to `desktop/src-tauri/src/commands.rs` after `list_agents`:

```rust
#[tauri::command]
pub fn get_pinned_agents() -> Result<Vec<String>, String> {
    arca_core::pinned_agents::get_pinned_agents()
}

#[tauri::command]
pub fn set_pinned_agents(agent_ids: Vec<String>) -> Result<Vec<String>, String> {
    arca_core::pinned_agents::set_pinned_agents(agent_ids)
}
```

Register both immediately after `commands::list_agents` in `desktop/src-tauri/src/lib.rs`:

```rust
commands::get_pinned_agents,
commands::set_pinned_agents,
```

Add the TypeScript wrappers immediately after `listAgents` in `desktop/src/lib/api.ts`:

```typescript
export const getPinnedAgents = () =>
  invoke<string[]>("get_pinned_agents");
export const setPinnedAgents = (agentIds: string[]) =>
  invoke<string[]>("set_pinned_agents", { agentIds });
```

- [ ] **Step 6: Run focused and adapter verification**

Run:

```bash
cargo fmt --check
cargo test -p arca-core pinned_agents -- --nocapture
cargo test -p arca-core ui_section_and_unknown_fields_survive_settings_roundtrip -- --nocapture
cargo test -p arca-core mutation_refuses_a_concurrent_settings_change -- --nocapture
bash desktop/scripts/prepare-sidecar.sh
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```

Expected: all commands exit 0; the four pinned-Agent core tests and the UI settings round-trip test pass.

- [ ] **Step 7: Commit the persistence slice**

```bash
git add core/src/settings.rs core/src/pinned_agents.rs core/src/lib.rs desktop/src-tauri/src/commands.rs desktop/src-tauri/src/lib.rs desktop/src/lib/api.ts
git commit -m "feat(core): persist pinned Agents" -m "Keep ordered UI preferences behind strict settings validation and the existing atomic write boundary."
```

---

### Task 2: Define and test deterministic picker transitions

**Files:**
- Create: `desktop/src/lib/pinnedAgents.ts`
- Create: `desktop/src/lib/pinnedAgents.test.ts`

**Interfaces:**
- Consumes: `AgentInfo` from `desktop/src/lib/types.ts`.
- Produces: `MAX_PINNED_AGENTS`, `buildAgentPickerSections`, `togglePinnedAgent`, `movePinnedAgentBy`, and `movePinnedAgentBefore` for the React component.

- [ ] **Step 1: Write the failing TypeScript unit tests**

Create `desktop/src/lib/pinnedAgents.test.ts`:

```typescript
import assert from "node:assert/strict";
import test from "node:test";
import type { AgentInfo } from "./types.ts";
import {
  MAX_PINNED_AGENTS,
  buildAgentPickerSections,
  movePinnedAgentBefore,
  movePinnedAgentBy,
  togglePinnedAgent,
} from "./pinnedAgents.ts";

function agent(id: string, name: string, hasGlobal = true): AgentInfo {
  return {
    id,
    name,
    format: "json",
    key: "mcpServers",
    has_global: hasGlobal,
    has_project: false,
    enabled: true,
    supported_transports: ["stdio", "http"],
    global: hasGlobal ? `~/.${id}/settings.json` : null,
    project: null,
    docs: null,
    note: null,
    category: "coding",
    evidence: "official",
    verified_at: null,
    builtin: true,
  };
}

const agents = [
  agent("codex", "Codex"),
  agent("claude-code", "Claude Code"),
  agent("qoder", "Qoder CLI"),
  agent("catalog-only", "Catalog Only", false),
];

test("sections preserve pinned order and exclude read-only or duplicate rows", () => {
  const sections = buildAgentPickerSections(
    agents,
    ["qoder", "missing", "codex", "qoder"],
    "",
  );
  assert.deepEqual(sections.pinned.map(({ id }) => id), ["qoder", "codex"]);
  assert.deepEqual(sections.available.map(({ id }) => id), ["claude-code"]);
  assert.equal(sections.searchResults, null);
});

test("search merges pinned and available matches without duplicates", () => {
  const sections = buildAgentPickerSections(agents, ["qoder"], "code");
  assert.deepEqual(
    sections.searchResults?.map(({ id }) => id),
    ["claude-code", "codex"],
  );
});

test("toggle removes existing pins, appends new pins, and enforces the limit", () => {
  assert.deepEqual(togglePinnedAgent(["codex", "qoder"], "codex"), ["qoder"]);
  assert.deepEqual(togglePinnedAgent(["codex"], "qoder"), ["codex", "qoder"]);
  const full = Array.from({ length: MAX_PINNED_AGENTS }, (_, index) => `agent-${index}`);
  assert.deepEqual(togglePinnedAgent(full, "overflow"), full);
});

test("keyboard and drag ordering are stable at boundaries", () => {
  const ids = ["claude-code", "codex", "qoder"];
  assert.deepEqual(movePinnedAgentBy(ids, "codex", -1), ["codex", "claude-code", "qoder"]);
  assert.deepEqual(movePinnedAgentBy(ids, "claude-code", -1), ids);
  assert.deepEqual(movePinnedAgentBefore(ids, "qoder", "claude-code"), ["qoder", "claude-code", "codex"]);
  assert.deepEqual(movePinnedAgentBefore(ids, "codex", "codex"), ids);
});
```

- [ ] **Step 2: Run the unit test and verify the missing-module failure**

Run:

```bash
cd desktop
npm run test:unit
```

Expected: FAIL because `./pinnedAgents.ts` does not exist.

- [ ] **Step 3: Implement the pure picker model**

Create `desktop/src/lib/pinnedAgents.ts`:

```typescript
import type { AgentInfo } from "./types";

export const MAX_PINNED_AGENTS = 6;

export interface AgentPickerSections {
  pinned: AgentInfo[];
  available: AgentInfo[];
  searchResults: AgentInfo[] | null;
}

function compareAgents(left: AgentInfo, right: AgentInfo): number {
  return left.name.localeCompare(right.name, undefined, { sensitivity: "base" });
}

export function buildAgentPickerSections(
  agents: AgentInfo[],
  pinnedIds: string[],
  query: string,
): AgentPickerSections {
  const configurable = agents.filter((agent) => agent.has_global);
  const byId = new Map(configurable.map((agent) => [agent.id, agent]));
  const seen = new Set<string>();
  const pinned = pinnedIds.flatMap((id) => {
    const match = byId.get(id);
    if (!match || seen.has(id) || seen.size >= MAX_PINNED_AGENTS) return [];
    seen.add(id);
    return [match];
  });
  const available = configurable
    .filter((agent) => !seen.has(agent.id))
    .sort(compareAgents);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return { pinned, available, searchResults: null };
  const searchResults = configurable
    .filter((agent) =>
      [agent.name, agent.id, agent.category]
        .join(" ")
        .toLocaleLowerCase()
        .includes(normalizedQuery),
    )
    .sort(compareAgents);
  return { pinned, available, searchResults };
}

export function togglePinnedAgent(ids: string[], id: string): string[] {
  if (ids.includes(id)) return ids.filter((item) => item !== id);
  if (ids.length >= MAX_PINNED_AGENTS) return [...ids];
  return [...ids, id];
}

export function movePinnedAgentBy(ids: string[], id: string, offset: -1 | 1): string[] {
  const from = ids.indexOf(id);
  const to = from + offset;
  if (from < 0 || to < 0 || to >= ids.length) return [...ids];
  const next = [...ids];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

export function movePinnedAgentBefore(
  ids: string[],
  draggedId: string,
  targetId: string,
): string[] {
  if (draggedId === targetId || !ids.includes(draggedId) || !ids.includes(targetId)) {
    return [...ids];
  }
  const next = ids.filter((id) => id !== draggedId);
  next.splice(next.indexOf(targetId), 0, draggedId);
  return next;
}
```

- [ ] **Step 4: Run unit tests and TypeScript compilation**

Run:

```bash
cd desktop
npm run test:unit
npx tsc --noEmit
```

Expected: all Node tests pass and TypeScript exits 0.

- [ ] **Step 5: Commit the deterministic interaction model**

```bash
git add desktop/src/lib/pinnedAgents.ts desktop/src/lib/pinnedAgents.test.ts
git commit -m "feat(desktop): model pinned Agent interactions" -m "Keep filtering and ordering behavior deterministic, dependency-free, and covered by the existing Node test runner."
```

---

### Task 3: Build the pinned launcher and management popup

**Files:**
- Create: `desktop/src/hooks/usePinnedAgents.ts`
- Create: `desktop/src/components/AgentNavigation.tsx`
- Modify: `desktop/src/components/Layout.tsx`
- Modify: `desktop/src/components/icons.tsx`
- Modify: `desktop/src/index.css`

**Interfaces:**
- Consumes: Task 1's `getPinnedAgents` / `setPinnedAgents`, Task 2's pure transformations, `AgentGlyph`, `AgentInfo[]`, selected Agent ID, and existing navigation callbacks.
- Produces: `usePinnedAgents(): { agentIds; saving; commit }` and `AgentNavigation` props `{ agents, selectedAgentId, onSelectAgent, onAddAgent }`.

- [ ] **Step 1: Add the rollback-capable persistence hook**

Create `desktop/src/hooks/usePinnedAgents.ts`:

```typescript
import { useCallback, useEffect, useRef, useState } from "react";
import { useToast } from "../components/Toast";
import { formatError } from "../lib/format";
import { getPinnedAgents, setPinnedAgents } from "../lib/api";

export interface PinnedAgentsState {
  agentIds: string[];
  saving: boolean;
  commit(agentIds: string[]): Promise<boolean>;
}

export function usePinnedAgents(): PinnedAgentsState {
  const [agentIds, setAgentIds] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const savedRef = useRef<string[]>([]);
  const savingRef = useRef(false);
  const { show } = useToast();

  useEffect(() => {
    let active = true;
    getPinnedAgents()
      .then((loaded) => {
        if (!active) return;
        savedRef.current = loaded;
        setAgentIds(loaded);
      })
      .catch((error) => {
        if (active) show({ kind: "error", msg: `读取置顶 Agent 失败: ${formatError(error)}` });
      });
    return () => {
      active = false;
    };
  }, [show]);

  const commit = useCallback(async (nextIds: string[]) => {
    if (savingRef.current) return false;
    const previous = savedRef.current;
    savingRef.current = true;
    setSaving(true);
    setAgentIds(nextIds);
    try {
      const persisted = await setPinnedAgents(nextIds);
      savedRef.current = persisted;
      setAgentIds(persisted);
      return true;
    } catch (error) {
      setAgentIds(previous);
      show({ kind: "error", msg: `保存置顶 Agent 失败: ${formatError(error)}` });
      return false;
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [show]);

  return { agentIds, saving, commit };
}
```

- [ ] **Step 2: Add the missing pin and drag icons**

Append these components to `desktop/src/components/icons.tsx`:

```tsx
export function PinIcon({ className, style }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" className={className} style={style} stroke="currentColor" fill="none"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 17v5" />
      <path d="M5 17h14" />
      <path d="M7 3h10l-1 6 3 3v2H5v-2l3-3Z" />
    </svg>
  );
}

export function GripVerticalIcon({ className, style }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" className={className} style={style} stroke="currentColor" fill="none"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="9" cy="6" r="1" fill="currentColor" stroke="none" />
      <circle cx="15" cy="6" r="1" fill="currentColor" stroke="none" />
      <circle cx="9" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="15" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="9" cy="18" r="1" fill="currentColor" stroke="none" />
      <circle cx="15" cy="18" r="1" fill="currentColor" stroke="none" />
    </svg>
  );
}
```

- [ ] **Step 3: Create the dedicated Agent navigation component**

Create `desktop/src/components/AgentNavigation.tsx`. Its top-level structure and event contract must be exactly:

```tsx
import { useEffect, useMemo, useRef, useState } from "react";
import type { DragEvent, KeyboardEvent } from "react";
import type { AgentInfo } from "../lib/types";
import {
  buildAgentPickerSections,
  MAX_PINNED_AGENTS,
  movePinnedAgentBefore,
  movePinnedAgentBy,
  togglePinnedAgent,
} from "../lib/pinnedAgents";
import { usePinnedAgents } from "../hooks/usePinnedAgents";
import { AgentGlyph } from "./brandIcons";
import {
  CheckIcon,
  ChevronDownIcon,
  GripVerticalIcon,
  PackageIcon,
  PinIcon,
  PlusIcon,
  SearchIcon,
  XIcon,
} from "./icons";

interface AgentNavigationProps {
  agents: AgentInfo[];
  selectedAgentId: string | null;
  onSelectAgent(id: string): void;
  onAddAgent?: () => void;
}

export function AgentNavigation({
  agents,
  selectedAgentId,
  onSelectAgent,
  onAddAgent,
}: AgentNavigationProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const anchorRef = useRef<HTMLDivElement>(null);
  const { agentIds, saving, commit } = usePinnedAgents();
  const sections = useMemo(
    () => buildAgentPickerSections(agents, agentIds, query),
    [agentIds, agents, query],
  );
  const pinnedIds = sections.pinned.map(({ id }) => id);
  const selectedAgent = agents.find(({ id }) => id === selectedAgentId) ?? null;
  const pinLimitReached = pinnedIds.length >= MAX_PINNED_AGENTS;

  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    const closeOnPointerDown = (event: PointerEvent) => {
      if (!anchorRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("keydown", closeOnEscape);
    document.addEventListener("pointerdown", closeOnPointerDown);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      document.removeEventListener("pointerdown", closeOnPointerDown);
    };
  }, [open]);

  const selectAgent = (id: string) => {
    onSelectAgent(id);
    setOpen(false);
  };

  const sameOrder = (left: string[], right: string[]) =>
    left.join("\u0000") === right.join("\u0000");

  const saveOrder = (next: string[], movedId: string) => {
    if (sameOrder(next, pinnedIds)) return;
    void commit(next).then((saved) => {
      if (!saved) return;
      const moved = agents.find(({ id }) => id === movedId);
      setAnnouncement(`${moved?.name ?? movedId} 已移动到第 ${next.indexOf(movedId) + 1} 位`);
    });
  };

  const togglePin = (id: string) => {
    const next = togglePinnedAgent(pinnedIds, id);
    if (sameOrder(next, pinnedIds)) return;
    const wasPinned = pinnedIds.includes(id);
    void commit(next).then((saved) => {
      if (!saved) return;
      const changed = agents.find((agent) => agent.id === id);
      setAnnouncement(`${changed?.name ?? id} 已${wasPinned ? "取消置顶" : "置顶"}`);
    });
  };

  const moveByKeyboard = (event: KeyboardEvent<HTMLButtonElement>, id: string) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    const next = movePinnedAgentBy(pinnedIds, id, event.key === "ArrowUp" ? -1 : 1);
    saveOrder(next, id);
  };

  const dropBefore = (event: DragEvent<HTMLDivElement>, targetId: string) => {
    event.preventDefault();
    if (!draggedId) return;
    const next = movePinnedAgentBefore(pinnedIds, draggedId, targetId);
    const movedId = draggedId;
    setDraggedId(null);
    saveOrder(next, movedId);
  };

  const agentRow = (agent: AgentInfo, isPinned: boolean, sortable: boolean) => {
    const active = selectedAgentId === agent.id;
    const pinDisabled = saving || (!isPinned && pinLimitReached);
    return (
      <div
        key={agent.id}
        className="arca-agent-picker-row"
        data-active={active ? "true" : undefined}
        data-dragging={draggedId === agent.id ? "true" : undefined}
        onDragOver={sortable ? (event) => event.preventDefault() : undefined}
        onDrop={sortable ? (event) => dropBefore(event, agent.id) : undefined}
      >
        {sortable && (
          <button
            type="button"
            className="arca-agent-order-handle"
            draggable={!saving}
            disabled={saving}
            title="拖拽排序；Option + 上下方向键调整"
            aria-label={`调整 ${agent.name} 的置顶顺序`}
            onDragStart={(event) => {
              event.dataTransfer.effectAllowed = "move";
              setDraggedId(agent.id);
            }}
            onDragEnd={() => setDraggedId(null)}
            onKeyDown={(event) => moveByKeyboard(event, agent.id)}
          >
            <GripVerticalIcon className="w-4 h-4" />
          </button>
        )}
        <button
          type="button"
          className="arca-agent-picker-select"
          aria-current={active ? "page" : undefined}
          onClick={() => selectAgent(agent.id)}
        >
          <AgentGlyph id={agent.id} name={agent.name} size={32} />
          <span className="min-w-0 flex-1">
            <span className="arca-agent-picker-name">{agent.name}</span>
            <span className="arca-agent-picker-meta">{agent.format.toUpperCase()} · {agent.id}</span>
          </span>
          {active && <CheckIcon className="arca-agent-picker-check" />}
        </button>
        <button
          type="button"
          className="arca-agent-pin-action"
          data-pinned={isPinned ? "true" : undefined}
          disabled={pinDisabled}
          title={isPinned ? "取消置顶" : pinLimitReached ? "最多置顶 6 个 Agent" : "置顶"}
          aria-label={`${isPinned ? "取消置顶" : "置顶"} ${agent.name}`}
          aria-pressed={isPinned}
          onClick={() => togglePin(agent.id)}
        >
          {isPinned ? <XIcon className="w-3.5 h-3.5" /> : <PinIcon className="w-3.5 h-3.5" />}
        </button>
      </div>
    );
  };

  return (
    <div className="arca-agent-navigation">
      <span className="sr-only" aria-live="polite">{announcement}</span>
      {sections.pinned.length > 0 && (
        <nav className="arca-pinned-agent-bar" aria-label="置顶 Agent">
          {sections.pinned.map((agent) => (
            <button
              type="button"
              key={agent.id}
              className="arca-pinned-agent"
              data-active={selectedAgentId === agent.id ? "true" : undefined}
              aria-current={selectedAgentId === agent.id ? "page" : undefined}
              aria-label={agent.name}
              title={agent.name}
              onClick={() => onSelectAgent(agent.id)}
            >
              <AgentGlyph id={agent.id} name={agent.name} size={30} />
            </button>
          ))}
        </nav>
      )}

      <div className="arca-agent-picker-anchor" ref={anchorRef}>
        <button
          type="button"
          className="arca-agent-picker-trigger"
          data-active={selectedAgent ? "true" : undefined}
          data-open={open ? "true" : undefined}
          aria-haspopup="dialog"
          aria-expanded={open}
          title={selectedAgent?.name}
          onClick={() => {
            setOpen((wasOpen) => {
              if (!wasOpen) setQuery("");
              return !wasOpen;
            });
          }}
        >
          {selectedAgent ? (
            <AgentGlyph id={selectedAgent.id} name={selectedAgent.name} size={24} />
          ) : (
            <PackageIcon className="w-5 h-5 flex-shrink-0" />
          )}
          <span className="arca-agent-picker-trigger-name">
            {selectedAgent?.name ?? "选择 Agent"}
          </span>
          <ChevronDownIcon className="arca-agent-picker-chevron" />
        </button>

        {open && (
          <section className="arca-agent-picker" role="dialog" aria-label="选择和置顶 Agent">
            <div className="arca-agent-picker-search">
              <SearchIcon className="w-4 h-4 flex-shrink-0" />
              <input
                type="search"
                autoFocus
                spellCheck={false}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="按名称或 ID 搜索"
                aria-label="搜索 Agent"
              />
              <button
                type="button"
                className="arca-agent-picker-search-clear"
                data-visible={query ? "true" : undefined}
                disabled={!query}
                tabIndex={query ? 0 : -1}
                aria-label="清除搜索"
                title="清除搜索"
                onPointerDown={(event) => event.preventDefault()}
                onClick={() => setQuery("")}
              >
                <XIcon className="w-3.5 h-3.5" />
              </button>
            </div>

            <div className="arca-agent-picker-list">
              {sections.searchResults ? (
                sections.searchResults.length > 0 ? (
                  sections.searchResults.map((agent) =>
                    agentRow(agent, pinnedIds.includes(agent.id), false),
                  )
                ) : (
                  <div className="arca-agent-picker-empty">未找到匹配项</div>
                )
              ) : (
                <>
                  <div className="arca-agent-picker-section-heading">
                    <span>已置顶</span><span>{sections.pinned.length}/{MAX_PINNED_AGENTS}</span>
                  </div>
                  {sections.pinned.length > 0 ? (
                    sections.pinned.map((agent) => agentRow(agent, true, true))
                  ) : (
                    <div className="arca-agent-picker-hint">在常用 Agent 右侧点击 Pin</div>
                  )}
                  <div className="arca-agent-picker-section-heading"><span>全部 Agent</span></div>
                  {sections.available.map((agent) => agentRow(agent, false, false))}
                </>
              )}
            </div>

            {onAddAgent && (
              <div className="arca-agent-picker-footer">
                <button
                  type="button"
                  onClick={() => {
                    setOpen(false);
                    onAddAgent();
                  }}
                >
                  <PlusIcon className="w-4 h-4" />
                  添加自定义 Agent
                </button>
              </div>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Replace the inline picker in `Layout` with `AgentNavigation`**

In `desktop/src/components/Layout.tsx`:

1. Reduce the React import to `ReactNode, useEffect, useState`.
2. Remove `CheckIcon`, `ChevronDownIcon`, `PlusIcon`, `SearchIcon`, `XIcon`, and `AgentGlyph` imports.
3. Add `import { AgentNavigation } from "./AgentNavigation";`.
4. Remove `agentPickerOpen`, `agentQuery`, `agentPickerRef`, `selectedAgent`, `writableCount`, `visibleAgents`, and the picker Escape/outside-pointer effect.
5. Replace the entire existing `.arca-agent-picker-anchor` JSX block with:

```tsx
<AgentNavigation
  agents={agents}
  selectedAgentId={view.kind === "agent" ? view.id : null}
  onSelectAgent={onSelectAgent}
  onAddAgent={onAddAgent}
/>
```

Keep the existing flex spacer immediately before it and the existing divider immediately after it.

- [ ] **Step 5: Replace the Agent picker CSS block with fixed launcher dimensions**

In `desktop/src/index.css`, replace the block from `.arca-agent-picker-anchor` through its reduced-motion media rule with styles that enforce these selectors and dimensions:

```css
.arca-agent-navigation { display: flex; min-width: 0; flex: 0 0 auto; align-items: center; gap: 8px; }
.arca-pinned-agent-bar { display: flex; height: 40px; align-items: center; gap: 3px; padding: 3px 5px; border: 1px solid var(--border-hairline); border-radius: 8px; background: color-mix(in srgb, var(--surface-raised) 78%, transparent); box-shadow: var(--glass-highlight); }
.arca-pinned-agent { position: relative; display: inline-flex; width: 34px; height: 34px; flex: 0 0 34px; align-items: center; justify-content: center; padding: 0; border: 1px solid transparent; border-radius: 7px; background: transparent; cursor: pointer; }
.arca-pinned-agent:hover { background: color-mix(in srgb, var(--text-primary) 7%, transparent); }
.arca-pinned-agent[data-active="true"] { border-color: color-mix(in srgb, var(--color-blue) 42%, var(--border-hairline)); background: color-mix(in srgb, var(--color-blue) 10%, transparent); }
.arca-pinned-agent[data-active="true"]::after { position: absolute; right: 8px; bottom: 1px; left: 8px; height: 2px; border-radius: 2px; background: var(--color-blue); content: ""; }
.arca-pinned-agent:focus-visible { outline: 2px solid color-mix(in srgb, var(--color-blue) 56%, transparent); outline-offset: 1px; }
.arca-agent-picker-anchor { position: relative; flex: 0 0 auto; }
.arca-agent-picker-trigger { display: inline-flex; width: 220px; height: 40px; align-items: center; gap: 7px; padding: 0 10px; border: 1px solid var(--border-hairline); border-radius: 8px; background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--shadow-card), var(--glass-highlight); cursor: pointer; }
.arca-agent-picker-trigger:hover, .arca-agent-picker-trigger[data-active="true"] { border-color: color-mix(in srgb, var(--color-blue) 38%, var(--border-hairline)); }
.arca-agent-picker-trigger[data-open="true"] { border-color: color-mix(in srgb, var(--color-blue) 62%, var(--border-hairline)); box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-blue) 10%, transparent), var(--shadow-card); }
.arca-agent-picker-trigger-name { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; font: 650 12px/1 var(--font-sans); }
.arca-agent-picker-chevron { width: 14px; height: 14px; flex: 0 0 auto; color: var(--text-secondary); transition: transform .14s ease, color .14s ease; }
.arca-agent-picker-trigger[data-open="true"] .arca-agent-picker-chevron { color: var(--color-blue); transform: rotate(180deg); }
.arca-agent-picker { position: absolute; z-index: 520; top: calc(100% + 5px); right: 0; display: flex; width: 390px; max-height: min(590px, calc(100vh - 70px)); flex-direction: column; overflow: hidden; border: 1px solid color-mix(in srgb, var(--color-blue) 18%, var(--border-hairline)); border-radius: 9px; background: var(--surface-popover); box-shadow: 0 18px 44px rgba(24, 29, 43, .20), 0 4px 12px rgba(24, 29, 43, .10); animation: arca-agent-picker-in .14s ease-out; }
@keyframes arca-agent-picker-in { from { opacity: 0; transform: translateY(-5px) scale(.992); } to { opacity: 1; transform: translateY(0) scale(1); } }
.arca-agent-picker-search { display: flex; height: 38px; margin: 10px 10px 8px; align-items: center; gap: 9px; padding: 0 6px 0 10px; border: 1px solid transparent; border-radius: 8px; background: var(--surface-app); color: var(--text-secondary); box-shadow: inset 0 0 0 1px var(--border-hairline); }
.arca-agent-picker-search:focus-within { background: var(--surface-raised); color: var(--color-blue); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-blue) 42%, var(--border-hairline)), 0 0 0 2px color-mix(in srgb, var(--color-blue) 8%, transparent); }
.arca-agent-picker-search input { height: 100%; min-width: 0; flex: 1; padding: 0; border: 0; outline: 0; appearance: none; background: transparent; color: var(--text-primary); font: 13px/1 var(--font-sans); }
.arca-agent-picker-search input::placeholder { color: var(--text-secondary); }
.arca-agent-picker-search input::-webkit-search-cancel-button { display: none; }
.arca-agent-picker-search-clear, .arca-agent-order-handle, .arca-agent-pin-action { display: inline-flex; width: 26px; height: 26px; flex: 0 0 26px; align-items: center; justify-content: center; padding: 0; border: 0; border-radius: 6px; background: transparent; color: var(--text-secondary); cursor: pointer; }
.arca-agent-picker-search-clear { opacity: 0; visibility: hidden; pointer-events: none; }
.arca-agent-picker-search-clear[data-visible="true"] { opacity: 1; visibility: visible; pointer-events: auto; }
.arca-agent-picker-list { min-height: 220px; overflow-y: auto; padding: 4px 6px 7px; border-top: 1px solid var(--border-hairline); }
.arca-agent-picker-section-heading { display: flex; height: 28px; align-items: center; justify-content: space-between; padding: 0 8px; color: var(--text-secondary); font-size: 10px; font-weight: 650; }
.arca-agent-picker-section-heading:not(:first-child) { margin-top: 4px; border-top: 1px solid var(--border-hairline); }
.arca-agent-picker-hint { padding: 12px 8px; color: var(--text-secondary); font-size: 11px; }
.arca-agent-picker-row { display: flex; width: 100%; min-height: 48px; align-items: center; gap: 3px; padding: 4px; border-radius: 6px; color: var(--text-primary); }
.arca-agent-picker-row:hover { background: color-mix(in srgb, var(--text-primary) 6%, transparent); }
.arca-agent-picker-row[data-active="true"] { background: color-mix(in srgb, var(--color-blue) 11%, transparent); }
.arca-agent-picker-row[data-dragging="true"] { opacity: .48; }
.arca-agent-picker-select { display: flex; min-width: 0; flex: 1; align-items: center; gap: 9px; padding: 2px 4px; border: 0; background: transparent; color: inherit; text-align: left; cursor: pointer; }
.arca-agent-picker-name, .arca-agent-picker-meta { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.arca-agent-picker-name { font-size: 12px; font-weight: 600; }
.arca-agent-picker-meta { margin-top: 3px; color: var(--text-secondary); font: 10px/1 var(--font-mono); }
.arca-agent-picker-check { width: 15px; height: 15px; flex: 0 0 auto; color: var(--color-blue); }
.arca-agent-order-handle { cursor: grab; }
.arca-agent-order-handle:active { cursor: grabbing; }
.arca-agent-pin-action { opacity: 0; }
.arca-agent-picker-row:hover .arca-agent-pin-action, .arca-agent-pin-action:focus-visible, .arca-agent-pin-action[data-pinned="true"] { opacity: 1; }
.arca-agent-pin-action[data-pinned="true"] { color: var(--color-blue); }
.arca-agent-order-handle:hover, .arca-agent-pin-action:hover:not(:disabled), .arca-agent-picker-search-clear:hover { background: var(--color-gray-150); color: var(--text-primary); }
.arca-agent-order-handle:disabled, .arca-agent-pin-action:disabled { opacity: .32; cursor: default; }
.arca-agent-picker-select:focus-visible, .arca-agent-order-handle:focus-visible, .arca-agent-pin-action:focus-visible, .arca-agent-picker-footer button:focus-visible { outline: 2px solid color-mix(in srgb, var(--color-blue) 56%, transparent); outline-offset: -2px; }
.arca-agent-picker-empty { padding: 28px 12px; color: var(--text-secondary); font-size: 12px; text-align: center; }
.arca-agent-picker-footer { display: flex; min-height: 48px; align-items: center; padding: 7px 10px; border-top: 1px solid var(--border-hairline); background: var(--surface-app); }
.arca-agent-picker-footer button { display: inline-flex; height: 32px; align-items: center; gap: 7px; padding: 0 10px; border: 0; border-radius: 7px; background: transparent; color: var(--color-blue); font: 600 12px/1 var(--font-sans); cursor: pointer; }
.arca-agent-picker-footer button:hover { background: color-mix(in srgb, var(--color-blue) 9%, transparent); }
@media (max-height: 680px) { .arca-agent-picker { max-height: calc(100vh - 66px); } .arca-agent-picker-list { min-height: 160px; } }
@media (max-width: 1080px) { .arca-agent-picker-trigger { width: 170px; } .arca-pinned-agent-bar { gap: 2px; padding-right: 4px; padding-left: 4px; } .arca-pinned-agent { width: 31px; height: 31px; flex-basis: 31px; } .arca-pinned-agent > * { transform: scale(.933333); transform-origin: center; } }
@media (prefers-reduced-motion: reduce) { .arca-agent-picker { animation: none; } }
```

Remove the older `@media (max-width: 1080px) { .arca-agent-picker-trigger { width: 270px; } }` declaration near the Agent page styles so it cannot override the new `170px` contract.

- [ ] **Step 6: Run focused UI verification**

Run:

```bash
cd desktop
npm run test:unit
npm run check:agent-icons
npm run build
```

Expected: all commands exit 0; no unused imports or JSX nesting errors; existing and pinned-Agent unit tests pass.

- [ ] **Step 7: Commit the complete UI slice**

```bash
git add desktop/src/hooks/usePinnedAgents.ts desktop/src/components/AgentNavigation.tsx desktop/src/components/Layout.tsx desktop/src/components/icons.tsx desktop/src/index.css
git commit -m "feat(ui): add pinned Agent launcher" -m "Expose six persistent shortcuts while keeping the full selector compact, searchable, sortable, and keyboard accessible."
```

---

### Task 4: Verify safety, responsiveness, and delivery readiness

**Files:**
- Modify only if verification finds a defect: files named in Tasks 1–3.

**Interfaces:**
- Consumes: completed core persistence, bridge, pure transformations, hook, launcher, picker, and styles.
- Produces: a clean, fully tested feature commit series that is ready for review but not published.

- [ ] **Step 1: Run the complete shared verification matrix**

Run from the repository root:

```bash
cargo fmt --check
cargo test --workspace
cd desktop && npm run test:unit && npm run check:agent-icons && npm run build
cd .. && bash desktop/scripts/prepare-sidecar.sh
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```

Expected: every command exits 0.

- [ ] **Step 2: Inspect the production bundle without launching a preview**

Run:

```bash
rg -n "arca-pinned-agent|arca-agent-picker-trigger|width: 390px|width: 170px" desktop/dist/assets
```

Expected: the production bundle contains the pinned launcher and both fixed responsive dimensions. Do not launch Vite, a target bundle, a renamed app, or a synthetic Tauri page.

- [ ] **Step 3: Review settings safety with an isolated test home**

Run:

```bash
cargo test -p arca-core pinned_agents -- --nocapture
cargo test -p arca-core mutation_refuses_to_replace_corrupt_settings -- --nocapture
cargo test -p arca-core mutation_refuses_a_concurrent_settings_change -- --nocapture
cargo test -p arca-core settings_are_written_with_private_permissions -- --nocapture
```

Expected: all focused tests pass, malformed settings remain byte-identical, and settings permissions remain `0600`.

- [ ] **Step 4: Review the final diff against the approved design**

Check all of these directly in the diff:

- Core write accepts only unique, configurable IDs and rejects more than six.
- Read-time normalization never rewrites settings.
- React commits only valid IDs resolved from the current `has_global` Agent list.
- Search mode has one result list and no drag controls.
- Non-search mode separates pinned and available Agents without duplicates.
- All icon buttons have `title`, `aria-label`, and focus-visible styling.
- `Layout.tsx` no longer owns picker search, popup, or row logic.
- No key, MCP configuration, model profile, or Agent configuration content crosses the new commands.

- [ ] **Step 5: Check whitespace and worktree scope**

Run:

```bash
git diff --check
git status --short
git log --oneline -4
```

Expected: no whitespace errors, no generated output is staged, and the latest three feature commits correspond to core persistence, deterministic transitions, and UI integration.

- [ ] **Step 6: Commit only evidence-backed corrections**

If Steps 1–5 expose a product defect, fix only the owning file, rerun the failing command plus its enclosing verification group, then commit:

```bash
git add core/src/settings.rs core/src/pinned_agents.rs core/src/lib.rs desktop/src-tauri/src/commands.rs desktop/src-tauri/src/lib.rs desktop/src/lib/api.ts desktop/src/lib/pinnedAgents.ts desktop/src/lib/pinnedAgents.test.ts desktop/src/hooks/usePinnedAgents.ts desktop/src/components/AgentNavigation.tsx desktop/src/components/Layout.tsx desktop/src/components/icons.tsx desktop/src/index.css
git commit -m "fix(ui): harden pinned Agent shortcuts" -m "Correct issues found by the final persistence, accessibility, or responsive verification pass."
```

If verification passes without corrections, do not create an empty commit.

---

## Stable Release And Screenshot Gate

The implementation plan ends with review-ready source. It intentionally does not publish a release because repository rules permit UI review only against the official stable `/Applications/Arca.app`, while publishing and replacing that app are separate state-changing delivery operations.

After the user explicitly approves formal publication:

1. Rebase or merge the reviewed feature onto the intended release branch without coupling it to unrelated `feat/user-skills-management` work.
2. Prepare a separate release plan that updates all seven version sources, creates the annotated stable tag, pushes it, waits for the GitHub workflow, and verifies every published asset.
3. Follow `../../skills/tool/arca-ui-review/SKILL.md` to install only the verified stable GitHub Release into `/Applications/Arca.app`.
4. Capture the real installed app at `1200x820` and `900x600`, showing six pins, the `220px`/`170px` selector, pin/unpin, drag order, search mode, dark/light themes, and restart persistence.
5. Report release URL, asset hashes, codesign result, installed version, screenshot paths, and any remaining UI defect.
