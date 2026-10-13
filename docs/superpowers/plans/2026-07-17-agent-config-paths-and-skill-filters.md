# Agent Config Paths and Skill Filters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the misleading Skills content-type navigation and present independent Model, MCP, and Skills configuration locations on writable Agent pages.

**Architecture:** Keep the released `content_kind` persistence and recovery schema intact, but remove its only Desktop behavior. Add one read-only `skills_global_dir` projection to `AgentInfo`, then render a three-column summary from the independent Model, MCP, and Skills capability sources without cross-fallbacks.

**Tech Stack:** Rust 2021, Serde, React 19, TypeScript 5.8, Vitest, Testing Library, Tauri 2, CSS.

## Global Constraints

- Do not delete or change the serialized `SkillContentKind` / `content_kind` fields in settings, source resolutions, plans, snapshots, or recovery journals.
- Model paths come only from `ModelAgentView.config_path`; MCP paths come only from `AgentInfo.global`; Skills paths come only from trusted `AgentDefinition.skills.global_dir` projected as `AgentInfo.skills_global_dir`.
- Missing Model or Skills capabilities must render an explicit unavailable state and must never fall back to the MCP path.
- The configuration summary is three equal columns at normal desktop widths and `900×600`, then one column only at `≤820px`.
- Keep the lower management sections ordered Model, Skills, MCP and preserve their existing behavior.
- `has_global = false` reference pages remain unchanged and do not render writable configuration targets.
- Do not launch a source-built, Preview, synthetic IPC, or browser-mocked Arca app for visual acceptance.
- Do not publish or replace `/Applications/Arca.app` without separate release authorization.

---

### Task 1: Project the trusted Skills path into AgentInfo

**Files:**
- Modify: `core/src/agents.rs:7-28`
- Modify: `core/src/agents.rs:352-375`
- Test: `core/src/agents.rs:393-465`

**Interfaces:**
- Consumes: `AgentDefinition.skills: Option<AgentSkillsCapability>` and `AgentSkillsCapability.global_dir: String`.
- Produces: `AgentInfo.skills_global_dir: Option<String>` serialized as `skills_global_dir` for Desktop.

- [x] **Step 1: Write the failing projection test**

Add this test to `core/src/agents.rs`:

```rust
#[test]
fn agent_info_projects_only_trusted_primary_skills_directories() {
    let _home = crate::testenv::TestHome::new("agent-info-skills-path");
    let infos = list_infos();
    let codex = infos.iter().find(|agent| agent.id == "codex").unwrap();
    let claude_desktop = infos
        .iter()
        .find(|agent| agent.id == "claude-desktop")
        .unwrap();

    assert_eq!(
        codex.skills_global_dir.as_deref(),
        Some("~/.agents/skills")
    );
    assert_eq!(claude_desktop.skills_global_dir, None);
}
```

- [x] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test -p arca-core agents::tests::agent_info_projects_only_trusted_primary_skills_directories -- --exact
```

Expected: compilation fails because `AgentInfo` has no `skills_global_dir` field.

- [x] **Step 3: Add the read-only projection**

Add the field to `AgentInfo`:

```rust
pub skills_global_dir: Option<String>,
```

In `list_infos`, capture the trusted primary path before moving the remaining definition fields and include it in the view:

```rust
.map(|(id, d)| {
    let skills_global_dir = d
        .skills
        .as_ref()
        .map(|capability| capability.global_dir.clone());
    AgentInfo {
        supported_transports: supported_transports(&d),
        name: d.name.clone().unwrap_or_else(|| id.clone()),
        id,
        format: d.format,
        key: d.key,
        has_global: d.global.is_some(),
        has_project: d.project.is_some(),
        enabled: d.enabled,
        global: d.global,
        project: d.project,
        docs: d.docs,
        note: d.note,
        category: d.category.unwrap_or_else(|| "custom".into()),
        evidence: d.evidence.unwrap_or_else(|| "custom".into()),
        verified_at: d.verified_at,
        builtin: d.builtin == Some(true),
        skills_global_dir,
    }
})
```

Do not project aliases and do not read the filesystem.

- [x] **Step 4: Run the focused and module tests**

Run:

```bash
cargo test -p arca-core agents::tests::agent_info_projects_only_trusted_primary_skills_directories -- --exact
cargo test -p arca-core agents::tests
```

Expected: the focused test and all `agents::tests` pass.

- [x] **Step 5: Commit Task 1**

```bash
git add core/src/agents.rs
git commit -m "feat(agents): expose verified skills path"
```

---

### Task 2: Remove content-type navigation from the Skills workspace

**Files:**
- Modify: `desktop/src/components/SkillsView.tsx:1-84`
- Modify: `desktop/src/components/SkillsView.tsx:130-186`
- Modify: `desktop/src/components/SkillsView.tsx:382-442`
- Modify: `desktop/src/components/SkillsView.tsx:538-567`
- Test: `desktop/src/components/SkillsView.test.tsx:205-232`
- Modify: `desktop/src/lib/skills.ts:1-56`
- Test: `desktop/src/lib/skills.test.ts:168-243`

**Interfaces:**
- Consumes: existing `SkillInventoryItem`, `SkillStatusFilter`, and `SkillSourceFilter`.
- Produces: `SkillFilters = { status, source, query }` and a Skills sidebar containing only the source section.

- [x] **Step 1: Write the failing workspace test**

Add this test to the `SkillsView` describe block:

```tsx
it("omits inferred content-type navigation", () => {
  render(<SkillsView state={skillsStateFixture()} />);

  expect(screen.queryByText("内容类型")).not.toBeInTheDocument();
  expect(screen.getByText("来源")).toBeVisible();
  expect(screen.queryByRole("button", { name: /说明型/ })).not.toBeInTheDocument();
});
```

- [x] **Step 2: Run the component test and verify RED**

Run:

```bash
cd desktop
npm test -- src/components/SkillsView.test.tsx -t "omits inferred content-type navigation"
```

Expected: FAIL because the current sidebar renders `内容类型` and `说明型`.

- [x] **Step 3: Simplify the filter contract**

Change `desktop/src/lib/skills.ts` to remove `SkillContentKind`, `SkillContentFilter`, `SkillFilters.contentKind`, and `contentMatches`:

```ts
export interface SkillFilters {
  status: SkillStatusFilter;
  source: SkillSourceFilter;
  query: string;
}

export function filterSkills(
  items: SkillInventoryItem[],
  filters: SkillFilters,
): SkillInventoryItem[] {
  const query = filters.query.trim().toLowerCase();
  return items.filter((item) => {
    const statusMatches =
      filters.status === "all" ||
      (filters.status === "updates" && item.update.available) ||
      (filters.status === "external" && item.states.includes("external")) ||
      (filters.status === "needs_attention" &&
        (item.update.available ||
          item.risk?.level === "high" ||
          item.states.some((state) => attentionStates.has(state))));
    const sourceMatches =
      filters.source === "all" ||
      item.source?.kind === filters.source ||
      (filters.source === "local" && item.source?.kind === "imported");
    const queryMatches =
      query.length === 0 ||
      `${item.name} ${item.description}`.toLowerCase().includes(query);
    return statusMatches && sourceMatches && queryMatches;
  });
}
```

Update every `filterSkills` test input to omit `contentKind`. Rename the first filter test to `combines status, source, and search`; keep its expected `review-changes` result.

- [x] **Step 4: Remove the SkillsView content axis**

In `SkillsView.tsx`:

- remove `SkillContentFilter`, `SkillContentKind`, and `TerminalIcon` imports;
- delete `contentOptions`;
- delete the `contentKind` state and `changeContentKind` callback;
- build `filters` as `{ status, source, query }` and remove `contentKind` from memo dependencies and `countWith` overrides;
- remove `setContentKind("all")` from navigation-intent reset;
- delete the complete `<SidebarSection title="内容类型">…</SidebarSection>` block.

The remaining sidebar must be:

```tsx
<WorkspaceSidebar title="Skills" count={items.length}>
  <SidebarSection title="来源">
    {sourceOptions.map((option) => (
      <SidebarItem
        key={option.value}
        active={source === option.value}
        icon={option.icon}
        label={option.label}
        count={countWith({ source: option.value })}
        onClick={() => changeSource(option.value)}
      />
    ))}
  </SidebarSection>
</WorkspaceSidebar>
```

- [x] **Step 5: Run focused Desktop tests and build**

Run:

```bash
cd desktop
npm test -- src/lib/skills.test.ts src/components/SkillsView.test.tsx
npm run build
```

Expected: both test files pass and TypeScript/Vite build exits 0.

- [x] **Step 6: Commit Task 2**

```bash
git add desktop/src/lib/skills.ts desktop/src/lib/skills.test.ts desktop/src/components/SkillsView.tsx desktop/src/components/SkillsView.test.tsx
git commit -m "fix(skills): remove misleading type filter"
```

---

### Task 3: Render independent Model, MCP, and Skills configuration locations

**Files:**
- Modify: `desktop/src/lib/types.ts:28-40`
- Modify: `desktop/src/lib/pinnedAgents.test.ts:14-35`
- Modify: `desktop/src/components/SkillReviewDialog.test.tsx:15-40`
- Modify: `desktop/src/components/SkillsView.test.tsx:128-150`
- Modify: `desktop/src/components/AgentSkillsSection.test.tsx:183-227`
- Modify: `desktop/src/components/AgentSkillsSection.test.tsx:679-748`
- Modify: `desktop/src/components/AgentView.tsx:20-33`
- Modify: `desktop/src/components/AgentView.tsx:220-258`
- Modify: `desktop/src/components/AgentView.tsx:454-480`
- Modify: `desktop/src/index.css:1097-1110`
- Modify: `desktop/src/index.css:1277-1282`

**Interfaces:**
- Consumes: Task 1 `AgentInfo.skills_global_dir: Option<String>` over Tauri and the existing `SkillsInventory.agents` runtime view.
- Produces: a nullable-path `ConfigPath` presentation and the three-column configuration summary.

- [x] **Step 1: Extend the TypeScript wire type and fixtures**

Add the required nullable field to `AgentInfo`:

```ts
skills_global_dir: string | null;
```

Add `skills_global_dir: null` to generic Agent fixtures. In `AgentSkillsSection.test.tsx` set the Codex fixture to:

```ts
skills_global_dir: hasGlobal ? "~/.agents/skills" : null,
```

- [x] **Step 2: Write failing configuration-summary tests**

Add tests under `AgentView Skills placement`:

```tsx
it("shows independent Model, MCP, and Skills configuration locations", async () => {
  vi.mocked(api.listModelAgents).mockResolvedValueOnce([{
    id: "codex",
    name: "Codex",
    mode: "managed",
    installed: true,
    config_path: "~/.codex/config.toml",
    docs: "https://example.invalid/models",
    assigned_profile: null,
    supported_protocols: ["openai-responses"],
    note: "",
  }]);
  const inventory = graphInventory();
  inventory.agents = [];

  render(
    <ToastProvider>
      <AgentView
        state={installState()}
        skillsState={stateWith(inventory)}
        agentId="codex"
        onOpenModels={vi.fn()}
        onOpenSkills={vi.fn()}
      />
    </ToastProvider>,
  );

  expect(await screen.findByText("Model / MCP 共用")).toBeVisible();
  const region = screen.getByRole("region", { name: "配置位置" });
  expect(within(region).getByText("Model")).toBeVisible();
  expect(within(region).getByText("MCP")).toBeVisible();
  expect(within(region).getByText("Skills")).toBeVisible();
  expect(within(region).getAllByText("~/.codex/config.toml")).toHaveLength(2);
  expect(within(region).getByText("~/.agents/skills")).toBeVisible();
  expect(within(region).getByText("已核验目录 · 未检测到 Agent")).toBeVisible();
});

it("does not copy the MCP path into unavailable capabilities", async () => {
  const state = installState();
  state.agents[0] = { ...state.agents[0], skills_global_dir: null };
  render(
    <ToastProvider>
      <AgentView
        state={state}
        skillsState={stateWith(graphInventory())}
        agentId="codex"
        onOpenModels={vi.fn()}
        onOpenSkills={vi.fn()}
      />
    </ToastProvider>,
  );

  const region = screen.getByRole("region", { name: "配置位置" });
  expect(await within(region).findByText("尚未接入 Models")).toBeVisible();
  expect(within(region).getByText("尚未核验 Skills 目录")).toBeVisible();
  expect(within(region).getAllByText("~/.codex/config.toml")).toHaveLength(1);
});
```

Extend the CSS test:

```ts
expect(groupedDeclarations(agentCss, ".arca-agent-file-map")).toMatch(
  /grid-template-columns:\s*repeat\(3,\s*minmax\(0,\s*1fr\)\)/,
);
expect(groupedDeclarations(agentCss, ".arca-agent-file-copy code")).toMatch(
  /overflow-wrap:\s*anywhere/,
);
```

- [x] **Step 3: Run the AgentView tests and verify RED**

Run:

```bash
cd desktop
npm test -- src/components/AgentSkillsSection.test.tsx -t "configuration locations|unavailable capabilities|900×600"
```

Expected: FAIL because the current UI renders Agent/MCP only, has a two-column grid, and falls back to MCP.

- [x] **Step 4: Derive the three independent presentation states**

In `AgentView.tsx`, import `SparklesIcon` and replace the old aliases with:

```ts
const mcpConfigPath = agent.global ?? "";
const skillsConfigPath = agent.skills_global_dir;
const runtimeSkillAgent = skillsState.inventory?.agents.find(
  (item) => item.id === agentId,
) ?? null;
const modelRelationship = modelAgent
  ? samePath(modelAgent.config_path, mcpConfigPath)
    ? "Model / MCP 共用"
    : "Model / MCP 分离"
  : null;
const modelDescription = modelsLoading
  ? "正在读取模型配置…"
  : modelAgent?.mode === "guided"
    ? "官方引导"
    : modelAgent
      ? "模型配置文件"
      : "尚未接入 Models";
const skillsDescription = !skillsConfigPath
  ? "尚未核验 Skills 目录"
  : !skillsState.inventory
    ? "已核验用户级目录"
    : !runtimeSkillAgent
      ? "已核验目录 · 未检测到 Agent"
      : runtimeSkillAgent.affected_agent_ids.length > 1
        ? `用户级目录 · 共享影响 ${runtimeSkillAgent.affected_agent_ids.length} 个 Agent`
        : "用户级目录 · 已检测";
```

Do not inspect aliases or filesystem paths in React.

- [x] **Step 5: Replace the summary markup**

Render the section as an accessible region:

```tsx
<section
  className="arca-agent-section"
  aria-labelledby="agent-files-title"
  aria-label="配置位置"
>
  <div className="arca-agent-section-head">
    <div>
      <h3 id="agent-files-title">配置位置</h3>
      <p>Model、MCP 与 Skills 使用的用户级配置入口。</p>
    </div>
    {modelRelationship && <Badge tone="info">{modelRelationship}</Badge>}
  </div>
  <div className="arca-agent-file-map">
    <ConfigPath
      icon={<LayersIcon className="w-4 h-4" />}
      label="Model"
      description={modelDescription}
      path={modelAgent?.config_path ?? null}
    />
    <ConfigPath
      icon={<PackageIcon className="w-4 h-4" />}
      label="MCP"
      description={`${agent.key} · ${agent.format.toUpperCase()}`}
      path={mcpConfigPath}
      action={
        <IconButton
          title="编辑 MCP 配置文件路径"
          onClick={() => setEditingAgent(true)}
        >
          <EditIcon className="w-4 h-4" />
        </IconButton>
      }
    />
    <ConfigPath
      icon={<SparklesIcon className="w-4 h-4" />}
      label="Skills"
      description={skillsDescription}
      path={skillsConfigPath}
    />
  </div>
</section>
```

Change `ConfigPath.path` to `string | null`. Render a real path with the existing `<code title={path}>`; otherwise render:

```tsx
<span className="arca-agent-file-unavailable">不可用</span>
```

- [x] **Step 6: Implement the responsive three-column styles**

Change the grid and dividers:

```css
.arca-agent-file-map {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  overflow: hidden;
  border: 1px solid var(--border-hairline);
  border-radius: 8px;
  background: var(--surface-raised);
}
.arca-agent-file-unavailable {
  display: block;
  margin-top: 10px;
  color: var(--text-secondary);
  font-size: 10px;
}
```

Keep the existing `≤820px` single-column rule and top-border conversion. Ensure `.arca-agent-file-copy code` retains `overflow-wrap: anywhere`.

- [x] **Step 7: Run focused tests and production build**

Run:

```bash
cd desktop
npm test -- src/components/AgentSkillsSection.test.tsx src/lib/pinnedAgents.test.ts src/components/SkillReviewDialog.test.tsx src/components/SkillsView.test.tsx
npm run build
```

Expected: all named test files pass and the production build exits 0.

- [x] **Step 8: Commit Task 3**

```bash
git add desktop/src/lib/types.ts desktop/src/lib/pinnedAgents.test.ts desktop/src/components/SkillReviewDialog.test.tsx desktop/src/components/SkillsView.test.tsx desktop/src/components/AgentSkillsSection.test.tsx desktop/src/components/AgentView.tsx desktop/src/index.css
git commit -m "feat(desktop): show three config locations"
```

---

### Task 4: Align current documentation and verify the integrated source tree

**Files:**
- Modify: `docs/superpowers/specs/2026-07-16-skills-management-design.md:423-430`
- Modify: `docs/superpowers/plans/2026-07-17-agent-config-paths-and-skill-filters.md`

**Interfaces:**
- Consumes: completed Tasks 1-3.
- Produces: current design documentation matching the shipped UI contract and a verified, release-ready source commit.

- [x] **Step 1: Update the superseded sidebar contract**

Replace the old content-type navigation paragraph with:

```markdown
左侧 sidebar 只承载来源导航：

- 来源：GitHub、本地。

`content_kind` 继续作为 v1.2.16 settings、plan 与 recovery journal 的兼容字段，
Desktop 不展示、不计数、不筛选。Agent 配置位置的三类独立路径设计见
`2026-07-17-agent-config-paths-and-skill-filters-design.md`。
```

- [x] **Step 2: Run formatting and focused Rust verification**

Run:

```bash
cargo fmt --all -- --check
cargo test -p arca-core agents::tests
```

Expected: formatting check and all Agent module tests pass.

- [x] **Step 3: Run the complete Desktop suite and build**

Run:

```bash
cd desktop
npm test
npm run build
```

Expected: zero failed tests; TypeScript, agent-icon check, and Vite production build exit 0.

- [x] **Step 4: Run repository diff and compatibility checks**

Run from the repository root:

```bash
git diff --check
rg -n "内容类型|SkillContentFilter|contentKind" desktop/src --glob '!*.test.ts' --glob '!*.test.tsx'
rg -n "内容类型|SkillContentFilter|contentKind" desktop/src
rg -n "content_kind" core/src/skills
```

Expected: `git diff --check` exits 0; the production Desktop search returns no removed
filter UI identifiers and exits 1; the full Desktop search returns only the negative
`SkillsView.test.tsx` assertion that `queryByText("内容类型")` is absent; the Core search
still finds the compatibility field and classifier.

- [x] **Step 5: Record verification evidence and commit docs**

Mark completed checkboxes in this plan only after each command has run with the expected result, then commit the current documentation:

```bash
git add docs/superpowers/specs/2026-07-16-skills-management-design.md docs/superpowers/plans/2026-07-17-agent-config-paths-and-skill-filters.md
git commit -m "docs(skills): align configuration navigation"
```

- [x] **Step 6: Stop at the release boundary**

Report the tested source commit as ready for release. Do not install a source build, create a Preview bundle, publish a release, replace `/Applications/Arca.app`, or claim visual acceptance. A screenshot of the new UI requires a separately authorized official stable release followed by the `arca-ui-review` installed-app workflow.
