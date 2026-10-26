# Resource Workspace Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move resource status filters above MCP/model grids, add a persistent resizable navigation column, and present details in a non-reflowing overlay drawer with sensitive values redacted.

**Architecture:** `ResourceWorkspace` remains the shared layout authority and gains a filter slot, sidebar resizing, and an inspector overlay layer. MCP and model pages keep their business filter state while rendering it through a shared `ResourceTabs` component. Pure UI persistence and redaction rules live in a small tested helper module.

**Tech Stack:** React 19, TypeScript 5.8, CSS, Node 24 built-in test runner, Vite 7, Tauri 2.

## Global Constraints

- Sidebar default/min/max widths are exactly `224px`, `184px`, and `340px`.
- Persist one shared width under `localStorage` key `arca.resourceWorkspace.sidebarWidth`.
- The sidebar is resizable but has no close or collapse action.
- The detail drawer overlays only the resource content area and never changes grid width or column count.
- Drawer width is `clamp(400px, 38vw, 520px)` and `min(520px, calc(100% - 32px))` on constrained windows.
- MCPs and Models use the same status-tab interaction.
- Sensitive field values never enter rendered DOM, title attributes, logs, or preview clipboard data.
- Preserve all pre-existing MiniMax Code working-tree changes and stage only files named by each task.

---

### Task 1: Add tested workspace UI helpers

**Files:**
- Create: `desktop/src/lib/resourceWorkspace.ts`
- Create: `desktop/src/lib/resourceWorkspace.test.ts`
- Modify: `desktop/package.json`

**Interfaces:**
- Produces: `SIDEBAR_WIDTH_STORAGE_KEY`, `DEFAULT_SIDEBAR_WIDTH`, `clampSidebarWidth(value)`, `parseSidebarWidth(value)`, and `redactSensitiveConfig(value)`.
- Consumed by: `ResourceWorkspace.tsx` and `RegistryView.tsx` in later tasks.

- [ ] **Step 1: Write failing helper tests**

```ts
import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_SIDEBAR_WIDTH,
  clampSidebarWidth,
  parseSidebarWidth,
  redactSensitiveConfig,
} from "./resourceWorkspace.ts";

test("sidebar width is clamped and invalid storage falls back", () => {
  assert.equal(clampSidebarWidth(100), 184);
  assert.equal(clampSidebarWidth(260), 260);
  assert.equal(clampSidebarWidth(500), 340);
  assert.equal(parseSidebarWidth(null), DEFAULT_SIDEBAR_WIDTH);
  assert.equal(parseSidebarWidth("invalid"), DEFAULT_SIDEBAR_WIDTH);
  assert.equal(parseSidebarWidth("312"), 312);
});

test("sensitive fields are recursively redacted without mutating input", () => {
  const source = {
    env: {
      API_TOKEN: "token-value",
      ALIBABA_CLOUD_ACCESS_KEY_SECRET: "secret-value",
      REGION: "cn-shenzhen",
    },
    nested: [{ password: "password-value", enabled: true }],
  };
  const result = redactSensitiveConfig(source);
  assert.deepEqual(result, {
    env: {
      API_TOKEN: "••••••••",
      ALIBABA_CLOUD_ACCESS_KEY_SECRET: "••••••••",
      REGION: "cn-shenzhen",
    },
    nested: [{ password: "••••••••", enabled: true }],
  });
  assert.equal(source.env.API_TOKEN, "token-value");
});
```

- [ ] **Step 2: Add and run the test script to verify failure**

Add to `desktop/package.json`:

```json
"test:unit": "node --test src/lib/*.test.ts"
```

Run: `cd desktop && npm run test:unit`

Expected: FAIL because `resourceWorkspace.ts` does not exist.

- [ ] **Step 3: Implement the pure helpers**

Create constants for the exact width bounds, parse finite stored numbers through the clamp function, and recursively copy arrays/objects. Treat a key as sensitive when its normalized name contains `token`, `secret`, `password`, `api_key`, `access_key`, `private_key`, or `credential`; replace the entire value with `••••••••` before descending.

```ts
export const SIDEBAR_WIDTH_STORAGE_KEY = "arca.resourceWorkspace.sidebarWidth";
export const DEFAULT_SIDEBAR_WIDTH = 224;
export const MIN_SIDEBAR_WIDTH = 184;
export const MAX_SIDEBAR_WIDTH = 340;
export const REDACTED_VALUE = "••••••••";

const SENSITIVE_KEY = /(token|secret|password|api[_-]?key|access[_-]?key|private[_-]?key|credential)/i;

export function clampSidebarWidth(value: number): number {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, value));
}

export function parseSidebarWidth(value: string | null): number {
  if (value === null || value.trim() === "") return DEFAULT_SIDEBAR_WIDTH;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? clampSidebarWidth(parsed) : DEFAULT_SIDEBAR_WIDTH;
}

export function redactSensitiveConfig(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(redactSensitiveConfig);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value).map(([key, child]) => [
      key,
      SENSITIVE_KEY.test(key) ? REDACTED_VALUE : redactSensitiveConfig(child),
    ])
  );
}
```

- [ ] **Step 4: Run tests and TypeScript build**

Run: `cd desktop && npm run test:unit && npm run build`

Expected: both commands exit 0.

- [ ] **Step 5: Commit the helper boundary**

```bash
git add desktop/package.json desktop/src/lib/resourceWorkspace.ts desktop/src/lib/resourceWorkspace.test.ts
git commit -m "test(ui): cover workspace persistence and redaction"
```

### Task 2: Refactor the shared workspace shell

**Files:**
- Modify: `desktop/src/components/ResourceWorkspace.tsx`
- Modify: `desktop/src/index.css`

**Interfaces:**
- Consumes: width constants and parsing helpers from `desktop/src/lib/resourceWorkspace.ts`.
- Produces: `ResourceWorkspace.filters`, `ResourceWorkspace.onInspectorClose`, reusable `ResourceTabs<T>`, persistent resize behavior, and non-reflowing inspector overlay markup.

- [ ] **Step 1: Add the shared status-tab component**

Implement a generic component with this public shape:

```ts
export interface ResourceTabOption<T extends string> {
  value: T;
  label: string;
  count: number;
}

export function ResourceTabs<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: ResourceTabOption<T>[];
  onChange: (value: T) => void;
})
```

Render a `role="tablist"` container and `role="tab"` buttons with `aria-selected`, stable count labels, and no conditional removal of zero-count options.

- [ ] **Step 2: Add persistent pointer-based sidebar resizing**

Initialize width with `parseSidebarWidth(localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY))`. On pointer down, track the initial pointer and width; on pointer move apply `clampSidebarWidth(startWidth + deltaX)`; on pointer up save the final width and restore body selection/cursor styles. Remove window listeners during cleanup and on component unmount.

Pass the value through the workspace style variable:

```tsx
<div
  className="arca-workspace"
  style={{ "--arca-workspace-sidebar-width": `${sidebarWidth}px` } as React.CSSProperties}
>
```

Place a `role="separator"`, `aria-orientation="vertical"` resize handle at the right edge of `WorkspaceSidebar`. Support ArrowLeft/ArrowRight in 8px increments and Home/End for minimum/maximum width, persisting keyboard changes immediately.

- [ ] **Step 3: Add filter and inspector overlay slots**

Extend `ResourceWorkspace` with:

```ts
filters?: ReactNode;
inspector?: ReactNode;
onInspectorClose?: () => void;
```

Render `filters` between the toolbar and content. When an inspector exists, render an absolutely positioned layer containing a full-content mask button and the inspector. The mask calls `onInspectorClose`; the inspector remains responsible for Escape and stops pointer propagation through its opaque surface.

- [ ] **Step 4: Update shared CSS without changing card dimensions**

Use the CSS variable for `grid-template-columns`. Add fixed filter-row dimensions and bottom border. Make the resize handle a 7px absolute hit target with a 1px visible line on hover/focus/drag. Make the inspector layer absolute with `inset: 0`, the mask cover the same box, and the inspector absolute on the right with the approved responsive widths. Remove the old wide-window flex width and the `max-width:1080px` positioning exception.

- [ ] **Step 5: Build and inspect generated layout CSS**

Run: `cd desktop && npm run build`

Expected: TypeScript and Vite build exit 0; no unused props or invalid style-property errors.

- [ ] **Step 6: Commit the shared workspace refactor**

```bash
git add desktop/src/components/ResourceWorkspace.tsx desktop/src/index.css
git commit -m "feat(ui): add resizable resource workspace shell"
```

### Task 3: Move MCP and model status filters into tabs

**Files:**
- Modify: `desktop/src/components/SourcesSidebar.tsx`
- Modify: `desktop/src/components/RegistryView.tsx`
- Modify: `desktop/src/components/ModelsView.tsx`

**Interfaces:**
- Consumes: `ResourceTabs`, overlay close callback, and `redactSensitiveConfig`.
- Produces: source-only MCP sidebar, protocol-only Models sidebar, synchronized detail closing, and redacted MCP config preview.

- [ ] **Step 1: Remove status responsibilities from `SourcesSidebar`**

Delete `McpStatusFilter`, `McpStatusCounts`, and the three status-related props. Remove the status `SidebarSection`; retain all source listing, subscribe, import, refresh, enable, and removal behavior unchanged.

- [ ] **Step 2: Render MCP status tabs and close stale detail**

Define MCP status types locally in `RegistryView.tsx`. Pass four stable options to `ResourceTabs` through `ResourceWorkspace.filters`. Wrap query, source, and status setters so each sets `detail` to `null` before updating its filter. Pass `onInspectorClose={() => setDetail(null)}`.

Keep status counts scoped to the selected source. Preserve the existing rule that `used` and `unused` only count effective entries while `shadowed` counts overridden copies.

- [ ] **Step 3: Redact the MCP detail preview**

Import `redactSensitiveConfig` and replace the preview expression with:

```tsx
<pre className="arca-config-preview">
  {JSON.stringify(redactSensitiveConfig(entry.config), null, 2)}
</pre>
```

Do not change `copyConfig`; it continues copying the source configuration directly and never reads from rendered DOM.

- [ ] **Step 4: Render model status tabs and retain protocol navigation**

Remove the Models status section from `WorkspaceSidebar`. Pass `all`, `assigned`, and `unassigned` options to `ResourceTabs` through `filters`. Wrap query, protocol, and status changes to close `selectedProfileId`; pass `onInspectorClose={() => setSelectedProfileId(null)}`.

- [ ] **Step 5: Run unit tests and build**

Run: `cd desktop && npm run test:unit && npm run build`

Expected: both commands exit 0; MCPs and Models compile against the same workspace interfaces.

- [ ] **Step 6: Commit the view migration**

```bash
git add desktop/src/components/SourcesSidebar.tsx desktop/src/components/RegistryView.tsx desktop/src/components/ModelsView.tsx
git commit -m "feat(ui): move resource status filters above grids"
```

### Task 4: Verify responsive and interaction behavior

**Files:**
- Modify only if verification finds an issue: `desktop/src/components/ResourceWorkspace.tsx`, `desktop/src/index.css`, `desktop/src/components/RegistryView.tsx`, `desktop/src/components/ModelsView.tsx`

**Interfaces:**
- Consumes: completed shared workspace and migrated resource views.
- Produces: verified UI behavior at required window sizes with no secrets visible.

- [ ] **Step 1: Run all focused static verification**

Run:

```bash
cd desktop
npm run test:unit
npm run check:agent-icons
npm run build
```

Expected: all commands exit 0.

- [ ] **Step 2: Update only the installed application for review**

Follow `../../skills/tool/arca-ui-review/SKILL.md`: compare source, latest stable release, and `/Applications/Arca.app`; build/package and replace the installed app only when required. Do not launch a dev server or preview bundle.

- [ ] **Step 3: Verify at `1200x820`**

Confirm status tabs appear above the grid, MCP sidebar contains only sources, Models sidebar contains only protocols, dragging changes width without collapse, and opening the drawer does not change grid column count. Confirm mask click and Escape close the drawer and a top-level dialog consumes Escape first.

- [ ] **Step 4: Verify at `900x600`**

Confirm all top-bar actions remain visible, sidebar stays within `184–340px`, drawer width leaves the 16px inset, card text does not overlap, and no horizontal overflow appears.

- [ ] **Step 5: Verify persistence and redaction**

Set the sidebar width to a non-default value, restart `/Applications/Arca.app`, and verify it restores. Open an MCP containing environment credentials and confirm no original token, secret, password, or key value exists in visible text or captured DOM.

- [ ] **Step 6: Run final diff and worktree checks**

Run:

```bash
git diff --check
git status --short
```

Expected: no whitespace errors; pre-existing MiniMax files remain present and unchanged except for their original working-tree state.

- [ ] **Step 7: Commit any verification-only corrections**

If Step 3-5 required source corrections, stage only the named UI files and commit:

```bash
git commit -m "fix(ui): polish resource workspace responsiveness"
```

If no corrections were needed, do not create an empty commit.
