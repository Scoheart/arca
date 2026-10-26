# Agent Drag Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make pinned Agent rows and the top shortcut bar reorder live during dragging, while persisting only once on drop.

**Architecture:** Keep the existing HTML5 drag path and Tauri `dragDropEnabled: false` contract. Add a pure preview-order helper, then let `AgentNavigation` maintain ephemeral `previewIds` and drop-target state; both pinned UI surfaces derive from that same preview order, while the existing `commit` function remains the only persistence boundary.

**Tech Stack:** React 19, TypeScript, HTML5 DragEvent, CSS data attributes, Node built-in test runner, Tauri 2, Rust.

## Global Constraints

- Do not add a drag-and-drop dependency or replace the current HTML5 drag system.
- Dragover updates local React state only; only drop may call `commit` and write settings.
- The dropdown list and top pinned icon bar must use the same preview order.
- Cancelled drag, Escape, picker close, or drop outside a valid row must restore the persisted order.
- Keep the six-Agent limit, pin toggles, search behavior, keyboard `Option + Up/Down`, persistence format, and Agent information architecture unchanged.
- Preserve `prefers-reduced-motion`; visual state remains visible when motion is disabled.
- Do not touch the separate `feat/user-skills-management` working tree.

---

## File Map

- `desktop/src/lib/pinnedAgents.ts`: pure ordering model, including a placement-aware preview helper.
- `desktop/src/lib/pinnedAgents.test.ts`: ordering behavior and immutability coverage.
- `desktop/src/components/AgentNavigation.tsx`: one drag-session state machine and shared preview rendering.
- `desktop/src/index.css`: dragged-row and drop-target feedback.
- `desktop/src/lib/agentNavigationCss.test.ts`: CSS/state wiring regression checks.
- Cargo/npm/Tauri manifests and lockfiles: `1.2.15` release metadata only.

### Task 1: Model Placement-Aware Preview Ordering

**Files:**
- Modify: `desktop/src/lib/pinnedAgents.ts`
- Test: `desktop/src/lib/pinnedAgents.test.ts`

**Interfaces:**
- Produces: `export type PinnedDropPlacement = "before" | "after"`
- Produces: `previewPinnedAgentOrder(ids: string[], draggedId: string, targetId: string, placement: PinnedDropPlacement): string[]`
- Consumes: existing `movePinnedAgentBefore` and `movePinnedAgentAfter` helpers.

- [ ] **Step 1: Write the failing preview-order tests**

Add the import and tests:

```ts
import {
  previewPinnedAgentOrder,
  type PinnedDropPlacement,
} from "./pinnedAgents.ts";

test("drag preview follows before and after targets across multiple rows", () => {
  const ids = ["claude-code", "codex", "qoder", "pi"];
  const first = previewPinnedAgentOrder(ids, "qoder", "claude-code", "before");
  const second = previewPinnedAgentOrder(first, "qoder", "codex", "after");

  assert.deepEqual(first, ["qoder", "claude-code", "codex", "pi"]);
  assert.deepEqual(second, ["claude-code", "codex", "qoder", "pi"]);
  assert.deepEqual(ids, ["claude-code", "codex", "qoder", "pi"]);
});

test("drag preview preserves order for invalid and self targets", () => {
  const ids = ["claude-code", "codex", "qoder"];
  const placements: PinnedDropPlacement[] = ["before", "after"];

  for (const placement of placements) {
    assert.deepEqual(previewPinnedAgentOrder(ids, "codex", "codex", placement), ids);
    assert.deepEqual(previewPinnedAgentOrder(ids, "missing", "codex", placement), ids);
    assert.deepEqual(previewPinnedAgentOrder(ids, "codex", "missing", placement), ids);
  }
});
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cd desktop
node --test src/lib/pinnedAgents.test.ts
```

Expected: FAIL because `previewPinnedAgentOrder` and `PinnedDropPlacement` do not exist.

- [ ] **Step 3: Add the minimal placement-aware helper**

Append to `pinnedAgents.ts`:

```ts
export type PinnedDropPlacement = "before" | "after";

export function previewPinnedAgentOrder(
  ids: string[],
  draggedId: string,
  targetId: string,
  placement: PinnedDropPlacement,
): string[] {
  return placement === "after"
    ? movePinnedAgentAfter(ids, draggedId, targetId)
    : movePinnedAgentBefore(ids, draggedId, targetId);
}
```

- [ ] **Step 4: Run the focused test and verify GREEN**

Run:

```bash
cd desktop
node --test src/lib/pinnedAgents.test.ts
```

Expected: all pinned-Agent tests pass.

- [ ] **Step 5: Commit the ordering model**

```bash
git add desktop/src/lib/pinnedAgents.ts desktop/src/lib/pinnedAgents.test.ts
git commit -m "test(ui): model Agent drag preview order"
```

### Task 2: Render One Live Preview Across Both Agent Surfaces

**Files:**
- Modify: `desktop/src/components/AgentNavigation.tsx`
- Modify: `desktop/src/index.css`
- Test: `desktop/src/lib/agentNavigationCss.test.ts`

**Interfaces:**
- Consumes: `previewPinnedAgentOrder(...)` and `PinnedDropPlacement` from Task 1.
- Produces: ephemeral `previewIds`, `dropTarget`, `orderedPinnedAgents`, `previewAtRow`, `finishDrop`, and `clearDragPreview` inside `AgentNavigation`.
- Produces DOM states: `data-dragging="true"` and `data-drop-position="before|after"`.

- [ ] **Step 1: Add failing source/CSS regression assertions**

Extend `agentNavigationCss.test.ts`:

```ts
test("drag preview drives both pinned surfaces and exposes target styling", () => {
  assert.match(component, /const \[previewIds, setPreviewIds\]/);
  assert.match(component, /const orderedPinnedAgents/);
  assert.equal(
    (component.match(/orderedPinnedAgents\.map/g) ?? []).length,
    2,
    "top shortcuts and pinned rows must share the preview order",
  );
  assert.match(component, /previewPinnedAgentOrder/);
  assert.match(component, /data-drop-position=/);

  const row = declarations(css, ".arca-agent-picker-row");
  assert.match(row, /position:\s*relative/);
  assert.match(css, /\.arca-agent-picker-row\[data-drop-position="before"\]::before/);
  assert.match(css, /\.arca-agent-picker-row\[data-drop-position="after"\]::after/);
});

test("reduced motion disables drag-preview transitions", () => {
  const reducedStart = css.indexOf("@media (prefers-reduced-motion: reduce)");
  assert.notEqual(reducedStart, -1);
  assert.match(css.slice(reducedStart), /\.arca-agent-picker-row\s*\{[^}]*transition:\s*none/);
});
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cd desktop
node --test src/lib/agentNavigationCss.test.ts
```

Expected: FAIL because the preview state, shared render order, target pseudo-elements, and reduced-motion override are absent.

- [ ] **Step 3: Add the drag-session state and shared render order**

In `AgentNavigation.tsx`, import `useCallback`, `PinnedDropPlacement`, and `previewPinnedAgentOrder`. Add:

```ts
interface AgentDropTarget {
  id: string;
  placement: PinnedDropPlacement;
}

const [previewIds, setPreviewIds] = useState<string[] | null>(null);
const [dropTarget, setDropTarget] = useState<AgentDropTarget | null>(null);

const previewOrder = draggedId && previewIds ? previewIds : pinnedIds;
const pinnedById = useMemo(
  () => new Map(sections.pinned.map((agent) => [agent.id, agent])),
  [sections.pinned],
);
const orderedPinnedAgents = previewOrder.flatMap((id) => {
  const agent = pinnedById.get(id);
  return agent ? [agent] : [];
});

const clearDragPreview = useCallback(() => {
  setDraggedId(null);
  setPreviewIds(null);
  setDropTarget(null);
}, []);
```

When `open` becomes false, call `clearDragPreview()` from the existing picker effect so Escape, outside click, and trigger close all restore the persisted order.

- [ ] **Step 4: Replace drop-only calculation with live dragover calculation**

Replace `dropAtRow` with:

```ts
const previewAtRow = (event: DragEvent<HTMLDivElement>, targetId: string) => {
  event.preventDefault();
  if (!ready || saving || !draggedId || targetId === draggedId) return;
  const bounds = event.currentTarget.getBoundingClientRect();
  const placement: PinnedDropPlacement =
    event.clientY >= bounds.top + bounds.height / 2 ? "after" : "before";
  setDropTarget((current) =>
    current?.id === targetId && current.placement === placement
      ? current
      : { id: targetId, placement },
  );
  setPreviewIds((current) => {
    const base = current ?? pinnedIds;
    const next = previewPinnedAgentOrder(base, draggedId, targetId, placement);
    return sameOrder(base, next) ? base : next;
  });
};

const finishDrop = (event: DragEvent<HTMLDivElement>) => {
  event.preventDefault();
  if (!ready || saving || !draggedId) return;
  const next = previewIds ?? pinnedIds;
  const movedId = draggedId;
  clearDragPreview();
  saveOrder(next, movedId);
};
```

Wire sortable rows to `onDragOver={event => previewAtRow(event, agent.id)}` and `onDrop={finishDrop}`. Set `data-drop-position` only when the row matches `dropTarget.id`.

- [ ] **Step 5: Initialize and cancel the preview from the drag handle**

Update `onDragStart`:

```ts
event.dataTransfer.effectAllowed = "move";
event.dataTransfer.setData("text/plain", agent.id);
const row = event.currentTarget.closest(".arca-agent-picker-row");
if (row instanceof HTMLElement) {
  const bounds = row.getBoundingClientRect();
  event.dataTransfer.setDragImage(row, 18, Math.min(bounds.height / 2, 24));
}
setDraggedId(agent.id);
setPreviewIds(pinnedIds);
setDropTarget(null);
```

Use `onDragEnd={clearDragPreview}`. Replace both `sections.pinned.map(...)` render sites with `orderedPinnedAgents.map(...)`; keep the count based on `sections.pinned.length`.

- [ ] **Step 6: Add restrained dragged and drop-target styles**

Change/add CSS:

```css
.arca-agent-picker-row {
  position: relative;
  transition: background-color .12s ease, opacity .12s ease, transform .12s ease;
}
.arca-agent-picker-row[data-dragging="true"] {
  background: color-mix(in srgb, var(--color-blue) 8%, transparent);
  opacity: .52;
  transform: scale(.985);
}
.arca-agent-picker-row[data-drop-position="before"]::before,
.arca-agent-picker-row[data-drop-position="after"]::after {
  position: absolute;
  right: 8px;
  left: 8px;
  z-index: 1;
  height: 2px;
  border-radius: 2px;
  background: var(--color-blue);
  content: "";
  pointer-events: none;
}
.arca-agent-picker-row[data-drop-position="before"]::before { top: -1px; }
.arca-agent-picker-row[data-drop-position="after"]::after { bottom: -1px; }
```

Extend the existing reduced-motion query:

```css
@media (prefers-reduced-motion: reduce) {
  .arca-agent-picker { animation: none; }
  .arca-agent-picker-row { transition: none; }
}
```

- [ ] **Step 7: Run desktop tests and build**

Run:

```bash
cd desktop
npm run test:unit
npm run check:agent-icons
npm run build
```

Expected: all unit tests pass, all configurable Agent icons validate, and Vite production build succeeds without TypeScript errors.

- [ ] **Step 8: Commit the live UI behavior**

```bash
git add desktop/src/components/AgentNavigation.tsx desktop/src/index.css desktop/src/lib/agentNavigationCss.test.ts
git commit -m "feat(ui): preview Agent order while dragging"
```

### Task 3: Validate, Release, and Install v1.2.15

**Files:**
- Modify: `Cargo.lock`
- Modify: `cli/Cargo.toml`
- Modify: `core/Cargo.toml`
- Modify: `desktop/package.json`
- Modify: `desktop/src-tauri/Cargo.lock`
- Modify: `desktop/src-tauri/Cargo.toml`
- Modify: `desktop/src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: completed Task 1 and Task 2 commits.
- Produces: stable annotated tag `v1.2.15`, four verified Release assets, and installed `/Applications/Arca.app` version `1.2.15`.

- [ ] **Step 1: Run the complete pre-release verification suite**

Run from the repository root:

```bash
cargo fmt --check
cargo test --workspace
(cd desktop && npm run test:unit && npm run check:agent-icons && npm run build)
bash desktop/scripts/prepare-sidecar.sh
(cd desktop/src-tauri && cargo test)
(cd website && npm run build)
git diff --check
```

Expected: all commands pass. If the nested worktree triggers Cargo workspace ancestry detection, create a detached worktree under `/tmp`, run the Tauri commands there, and do not alter Cargo workspace membership.

- [ ] **Step 2: Bump all version sources to 1.2.15**

Change only the package versions for `arca-core`, `arca-cli`, and `desktop` from `1.2.14` to `1.2.15` in the seven listed manifests/lockfiles. Verify:

```bash
rg -n '1\.2\.14|1\.2\.15' Cargo.lock core cli desktop/package.json desktop/src-tauri
```

Expected: package source-of-truth entries show `1.2.15`; no Arca package remains at `1.2.14`.

- [ ] **Step 3: Re-run focused release checks and commit**

```bash
(cd desktop && npm run test:unit && npm run build)
cargo test --workspace
git diff --check
git add Cargo.lock cli/Cargo.toml core/Cargo.toml desktop/package.json \
  desktop/src-tauri/Cargo.lock desktop/src-tauri/Cargo.toml desktop/src-tauri/tauri.conf.json
git commit -m "chore(release): bump version to 1.2.15"
```

Expected: tests/build pass and the release metadata is isolated in one commit.

- [ ] **Step 4: Push main and require the pre-release workflow gate**

```bash
git fetch origin main
git rebase origin/main
git push -u origin codex/agent-drag-preview
git push origin HEAD:main
SHA=$(git rev-parse HEAD)
MAIN_RUN_ID=$(SHA="$SHA" gh run list --workflow build-desktop.yml --branch main --limit 5 \
  --json databaseId,headSha --jq '.[] | select(.headSha == env.SHA) | .databaseId' | head -1)
test -n "$MAIN_RUN_ID"
gh run watch "$MAIN_RUN_ID" --exit-status
```

Expected: clean runner builds and verifies the pre-release DMG before any stable tag is created.

- [ ] **Step 5: Create and push the annotated stable tag**

```bash
git tag -a v1.2.15 -m "Arca v1.2.15"
git push origin v1.2.15
TAG_RUN_ID=$(gh run list --workflow build-desktop.yml --limit 10 \
  --json databaseId,headBranch --jq '.[] | select(.headBranch == "v1.2.15") | .databaseId' | head -1)
test -n "$TAG_RUN_ID"
gh run watch "$TAG_RUN_ID" --exit-status
```

Expected: stable workflow publishes `latest.json`, desktop updater tarball, Apple Silicon DMG, and CLI tarball.

- [ ] **Step 6: Re-download and verify every formal asset**

Download to a unique temporary directory and compare every file against GitHub's `sha256:` digest. Verify:

```bash
gh release view v1.2.15 --repo Scoheart/arca --json assets,isDraft,isPrerelease,url
```

Then verify `latest.json` version/URL/signature, `arca --version`, arm64 architecture, DMG checksum, `CFBundleShortVersionString=1.2.15`, `CFBundleIdentifier=com.scoheart.arca`, bundled `arca 1.2.15`, and `codesign --verify --deep --strict` for updater and DMG apps.

- [ ] **Step 7: Transactionally update and inspect the installed app**

Follow `../../skills/tool/arca-ui-review/SKILL.md`: stage the verified app under `/Applications`, stop only `/Applications/Arca.app/Contents/MacOS/desktop`, retain a rollback copy until post-install verification, launch only `/Applications/Arca.app`, and confirm version `1.2.15` and a live on-screen CGWindow.

Attempt one real drag verification in the installed app. Confirm list and top shortcuts reorder together before drop, cancel restores, and a completed drop survives restart. If Computer Use again returns `cgWindowNotFound` with an on-screen native window, stop retries and report that UI automation mapping blocks the physical drag check; do not substitute a preview build.

- [ ] **Step 8: Record durable release knowledge and workspace state**

Update the parent repository's current-day Arca daily brief, `memory/MEMORY.md`, and `memory/reference/arca-registry-release.md` with the stable release and verified preview-state contract. Run:

```bash
python3 scripts/workspace-state.py capture
```

Commit only the memory and generated workspace snapshot files; preserve unrelated parent changes.
