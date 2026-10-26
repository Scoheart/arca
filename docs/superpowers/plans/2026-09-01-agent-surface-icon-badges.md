# Agent Surface Icon Badges Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Distinguish built-in Agents that share a brand logo by adding small CLI, Desktop, IDE, or Web device-symbol badges to the shared `AgentGlyph` renderer.

**Architecture:** Keep vendor-logo aliases and product-surface metadata separate. `brandIcons.tsx` resolves both inputs, precomputes shared-logo collision groups, and wraps every existing glyph branch in one positioned container that may render a decorative surface badge. Page components remain unaware of individual Agent IDs and surface rules.

**Tech Stack:** React 19, TypeScript 7, Vite 8, Vitest 4, Testing Library, CSS

**Delivery override:** Do not create a local git commit or push. After focused verification, create the feature commit through the GitHub Git Data API, open a PR, merge it after required checks, and let Arca Direct Stable publish the next patch release.

---

## File Map

- Create `desktop/src/assets/agents/surfaces.json` — explicit presentation-only surface metadata.
- Create `desktop/src/components/brandIcons.test.tsx` — collision policy, badge identity, sizing, fallback, and accessibility tests.
- Modify `desktop/src/components/brandIcons.tsx` — surface parsing, collision detection, normalized glyph wrapper, and inline badge symbols.
- Modify `desktop/src/components/AgentNavigation.test.tsx` — prove the shared renderer surfaces the distinction in the real Agent picker.
- Modify `desktop/src/index.css` — wrapper positioning and theme-aware badge styling.
- Include `docs/superpowers/specs/2026-09-01-agent-surface-icon-badges-design.md` and this plan in the remote PR.

### Task 0: Prepare an Isolated Current-Main Worktree

**Files:**
- Carry forward: `docs/superpowers/specs/2026-09-01-agent-surface-icon-badges-design.md`
- Carry forward: `docs/superpowers/plans/2026-09-01-agent-surface-icon-badges.md`

- [ ] **Step 1: Create a clean isolated worktree from live `origin/main`**

Use the `using-git-worktrees` workflow to create a new worktree for `codex/agent-surface-icon-badges`. Do not modify or clean the existing Claude Desktop worktree, which contains the prior remote-only delivery files.

- [ ] **Step 2: Recreate the approved design and plan in the isolated worktree**

Use `apply_patch` to add the two approved documents with byte-identical contents. Confirm:

```bash
git status --short
```

Expected: only the design and plan documents are untracked; no source file is modified yet.

### Task 1: Lock the Surface Collision Contract with Tests

**Files:**
- Create: `desktop/src/components/brandIcons.test.tsx`

- [ ] **Step 1: Write the failing component tests**

Create `desktop/src/components/brandIcons.test.tsx` with the exact behavioral contract:

```tsx
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AgentGlyph } from "./brandIcons";

afterEach(cleanup);

function surfaceFor(container: HTMLElement): string | null {
  return container
    .querySelector<HTMLElement>("[data-agent-surface]")
    ?.getAttribute("data-agent-surface") ?? null;
}

it("distinguishes Claude and Qoder variants that share a logo", () => {
  const cases = [
    ["claude-code", "Claude Code", "cli"],
    ["claude-desktop", "Claude Desktop", "desktop"],
    ["qoder-cli", "Qoder CLI", "cli"],
    ["qoder", "Qoder Desktop", "ide"],
  ] as const;

  for (const [id, name, surface] of cases) {
    const view = render(<AgentGlyph id={id} name={name} size={30} />);
    expect(surfaceFor(view.container), id).toBe(surface);
    expect(view.container.querySelector("[data-agent-surface]"))
      .toHaveAttribute("aria-hidden", "true");
    expect(view.getByAltText(name)).toBeVisible();
    view.unmount();
  }
});

it("keeps unique, custom, and fallback Agent icons unbadged", () => {
  for (const [id, name] of [
    ["codex", "Codex"],
    ["cursor", "Cursor"],
    ["my-custom-agent", "My Custom Agent"],
  ]) {
    const view = render(<AgentGlyph id={id} name={name} size={30} />);
    expect(surfaceFor(view.container), id).toBeNull();
    view.unmount();
  }
});

it("uses the compact, regular, and large badge size tiers", () => {
  const cases = [
    [20, "10px"],
    [24, "10px"],
    [30, "12px"],
    [32, "12px"],
    [42, "14px"],
    [44, "14px"],
  ] as const;

  for (const [size, expected] of cases) {
    const view = render(<AgentGlyph id="claude-code" name="Claude Code" size={size} />);
    expect(view.container.querySelector<HTMLElement>("[data-agent-surface]"))
      .toHaveStyle({ width: expected, height: expected });
    view.unmount();
  }
});
```

- [ ] **Step 2: Run the focused test and confirm the missing behavior**

Run:

```bash
cd desktop
npm test -- brandIcons.test.tsx
```

Expected: FAIL because no glyph contains `[data-agent-surface]`.

### Task 2: Add Explicit Surface Metadata and Collision Resolution

**Files:**
- Create: `desktop/src/assets/agents/surfaces.json`
- Modify: `desktop/src/components/brandIcons.tsx:1-105`
- Test: `desktop/src/components/brandIcons.test.tsx`

- [ ] **Step 1: Add the initial explicit surface map**

Create `desktop/src/assets/agents/surfaces.json`:

```json
{
  "claude-code": "cli",
  "claude-desktop": "desktop",
  "qoder-cli": "cli",
  "qoder": "ide"
}
```

- [ ] **Step 2: Parse surface metadata and precompute collision logo keys**

At the top of `brandIcons.tsx`, import the new JSON next to `aliases.json` and define the closed surface type:

```tsx
import iconAliases from "../assets/agents/aliases.json";
import agentSurfaces from "../assets/agents/surfaces.json";

type AgentSurface = "cli" | "desktop" | "ide" | "web";

const ICON_ALIASES: Record<string, string> = iconAliases;
const SURFACE_VALUES = new Set<AgentSurface>(["cli", "desktop", "ide", "web"]);
const AGENT_SURFACES: Record<string, string> = agentSurfaces;

function resolvedLogoKey(id: string): string {
  return ICON_ALIASES[id] ?? id;
}

function declaredSurface(id: string): AgentSurface | null {
  const value = AGENT_SURFACES[id];
  return SURFACE_VALUES.has(value as AgentSurface) ? value as AgentSurface : null;
}

const COLLIDING_LOGO_KEYS = (() => {
  const surfacesByLogo = new Map<string, Set<AgentSurface>>();
  for (const id of Object.keys(AGENT_SURFACES)) {
    const surface = declaredSurface(id);
    const logoKey = resolvedLogoKey(id);
    if (!surface || !LOGOS[logoKey]) continue;
    const surfaces = surfacesByLogo.get(logoKey) ?? new Set<AgentSurface>();
    surfaces.add(surface);
    surfacesByLogo.set(logoKey, surfaces);
  }
  return new Set(
    [...surfacesByLogo.entries()]
      .filter(([, surfaces]) => surfaces.size > 1)
      .map(([logoKey]) => logoKey),
  );
})();

function visibleSurface(id: string): AgentSurface | null {
  const logoKey = resolvedLogoKey(id);
  if (!COLLIDING_LOGO_KEYS.has(logoKey)) return null;
  return declaredSurface(id);
}
```

Place the collision computation after `LOGOS` and `ICON_ALIASES` are initialized so it never reads an uninitialized binding.

- [ ] **Step 3: Run the focused test to retain the red state**

Run:

```bash
cd desktop
npm test -- brandIcons.test.tsx
```

Expected: FAIL because metadata is available but `AgentGlyph` does not render the badge yet.

### Task 3: Normalize AgentGlyph and Render Device Symbols

**Files:**
- Modify: `desktop/src/components/brandIcons.tsx:88-170`
- Modify: `desktop/src/index.css:415-520`
- Test: `desktop/src/components/brandIcons.test.tsx`

- [ ] **Step 1: Add exact badge sizing and inline SVG symbols**

Add these helpers before `AgentGlyph`:

```tsx
function surfaceBadgeSize(size: number): number {
  if (size <= 24) return 10;
  if (size <= 36) return 12;
  return 14;
}

function AgentSurfaceBadge({ surface, size }: { surface: AgentSurface; size: number }) {
  const badgeSize = surfaceBadgeSize(size);
  return (
    <span
      className="arca-agent-surface-badge"
      data-agent-surface={surface}
      aria-hidden="true"
      style={{ width: badgeSize, height: badgeSize }}
    >
      <svg viewBox="0 0 12 12" fill="none" focusable="false">
        {surface === "cli" && (
          <>
            <path d="M2.25 3.25 4.5 5.5 2.25 7.75" />
            <path d="M5.5 8h4" />
          </>
        )}
        {surface === "desktop" && (
          <>
            <rect x="1.5" y="2" width="9" height="6.75" rx="1.25" />
            <path d="M4 10h4" />
          </>
        )}
        {surface === "ide" && (
          <>
            <rect x="1.5" y="1.75" width="9" height="8.5" rx="1.25" />
            <path d="M5 2v8M6.75 4h2M6.75 6h2" />
          </>
        )}
        {surface === "web" && (
          <>
            <circle cx="6" cy="6" r="4.5" />
            <path d="M1.75 6h8.5M6 1.75c1.35 1.25 2 2.67 2 4.25S7.35 9 6 10.25C4.65 9 4 7.58 4 6s.65-3 2-4.25Z" />
          </>
        )}
      </svg>
    </span>
  );
}
```

- [ ] **Step 2: Refactor the rendering branches behind one wrapper**

Within `AgentGlyph`, keep the existing base-tile decisions but assign their JSX to `baseGlyph`. Return one positioned wrapper:

```tsx
const surface = logo ? visibleSurface(id) : null;

return (
  <span
    className="arca-agent-glyph"
    data-agent-id={id}
    style={{ width: size, height: size }}
  >
    <span className="arca-agent-glyph-base" style={{ width: size, height: size }}>
      {baseGlyph}
    </span>
    {surface && <AgentSurfaceBadge surface={surface} size={size} />}
  </span>
);
```

Use the wrapper for the wide-tile, full-bleed, mark-only, and monogram branches. Move each branch's existing radius, background, border, image size, and object-fit styles into `baseGlyph` unchanged. Do not add a badge to the monogram fallback.

- [ ] **Step 3: Add the shared badge CSS**

Add the following near the Agent navigation styles in `desktop/src/index.css`:

```css
.arca-agent-glyph {
  position: relative;
  display: inline-flex;
  flex: 0 0 auto;
  overflow: visible;
  vertical-align: middle;
}
.arca-agent-glyph-base {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: inherit;
}
.arca-agent-surface-badge {
  position: absolute;
  right: -2px;
  bottom: -2px;
  z-index: 2;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  border-radius: 4px;
  color: #fff;
  box-shadow: 0 0 0 2px var(--surface-app);
}
.arca-agent-surface-badge[data-agent-surface="cli"] { background: #202733; }
.arca-agent-surface-badge[data-agent-surface="desktop"] { background: var(--color-blue); }
.arca-agent-surface-badge[data-agent-surface="ide"] { background: #7656d6; }
.arca-agent-surface-badge[data-agent-surface="web"] { background: #159b91; }
.arca-agent-surface-badge > svg { width: 78%; height: 78%; }
.arca-agent-surface-badge :is(path, rect, circle) {
  stroke: currentColor;
  stroke-width: 1.35;
  stroke-linecap: round;
  stroke-linejoin: round;
  vector-effect: non-scaling-stroke;
}
```

- [ ] **Step 4: Run the component test and make it green**

Run:

```bash
cd desktop
npm test -- brandIcons.test.tsx
```

Expected: all three tests PASS.

### Task 4: Verify the Distinction in Agent Navigation

**Files:**
- Modify: `desktop/src/components/AgentNavigation.test.tsx:15-55`
- Test: `desktop/src/components/AgentNavigation.test.tsx`

- [ ] **Step 1: Add Claude Desktop to the fixture list**

Extend the existing `agents` fixture without changing pinned-order expectations:

```tsx
const agents = [
  agent("claude-code", "Claude Code"),
  agent("claude-desktop", "Claude Desktop"),
  agent("codex", "Codex"),
  agent("qoder", "Qoder"),
  agent("amp", "Amp"),
];
```

- [ ] **Step 2: Add a picker-level integration assertion**

Append this test:

```tsx
it("distinguishes shared Claude logos in the Agent picker", () => {
  const { container } = render(
    <AgentNavigation
      agents={agents}
      selectedAgentId="claude-code"
      onSelectAgent={vi.fn()}
    />,
  );

  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);

  const codeRow = screen.getByText("Claude Code").closest(".arca-agent-picker-select");
  const desktopRow = screen.getByText("Claude Desktop").closest(".arca-agent-picker-select");
  expect(codeRow?.querySelector("[data-agent-surface='cli']")).not.toBeNull();
  expect(desktopRow?.querySelector("[data-agent-surface='desktop']")).not.toBeNull();
});
```

- [ ] **Step 3: Run both test files in one invocation**

Run:

```bash
cd desktop
npm test -- brandIcons.test.tsx AgentNavigation.test.tsx
```

Expected: both test files PASS; no existing navigation test regresses.

### Task 5: Focused Quality and Visual Verification

**Files:**
- Verify: `desktop/src/assets/agents/surfaces.json`
- Verify: `desktop/src/components/brandIcons.tsx`
- Verify: `desktop/src/components/brandIcons.test.tsx`
- Verify: `desktop/src/components/AgentNavigation.test.tsx`
- Verify: `desktop/src/index.css`

- [ ] **Step 1: Run the icon asset contract and focused tests together**

Run:

```bash
cd desktop
npm run check:agent-icons
npm test -- brandIcons.test.tsx AgentNavigation.test.tsx
```

Expected: the asset check exits 0 and both test files PASS.

- [ ] **Step 2: Compile the desktop frontend**

Run:

```bash
cd desktop
npm run build
```

Expected: TypeScript and Vite build successfully with no new warning or error.

- [ ] **Step 3: Inspect the actual installed-size contexts**

Run the Arca desktop review workflow against the development build and inspect:

- pinned bar at 30px;
- collapsed Agent trigger at 24px;
- Agent picker rows at 32px;
- Agent detail header at 44px;
- resource-consumer stack at 20px;
- both light and dark themes.

Expected: CLI and Desktop/IDE symbols remain recognizable; the badge does not obscure the brand mark, clip, shift layout, or introduce a badge on unrelated Agents.

- [ ] **Step 4: Run final static checks**

Run:

```bash
git diff --check
git status --short
```

Expected: no whitespace errors and only the five implementation files plus the design and plan documents are changed.

### Task 6: Remote-Only PR, Merge, and Stable Release

**Files:**
- Deliver the exact seven-file manifest from Task 5.

- [ ] **Step 1: Base the remote feature branch on the live main head**

Read the current `main` ref through the GitHub API immediately before creating the remote commit. If `main` changed after local implementation, compare overlapping files and incorporate the new content before uploading blobs.

- [ ] **Step 2: Create and verify the remote feature commit**

Use the GitHub Git Data API with commit subject:

```text
feat(desktop): distinguish shared Agent logos
```

Upload only:

```text
desktop/src/assets/agents/surfaces.json
desktop/src/components/brandIcons.tsx
desktop/src/components/brandIcons.test.tsx
desktop/src/components/AgentNavigation.test.tsx
desktop/src/index.css
docs/superpowers/specs/2026-09-01-agent-surface-icon-badges-design.md
docs/superpowers/plans/2026-09-01-agent-surface-icon-badges.md
```

Verify every remote blob SHA against `git hash-object` before updating the feature ref.

- [ ] **Step 3: Open the PR and wait for required checks**

The PR body must summarize the collision-only policy and list the focused test/build evidence. Confirm the PR file list contains exactly the seven paths above and GitHub reports it mergeable.

- [ ] **Step 4: Squash merge and verify Direct Stable**

After required checks pass, squash merge the PR and delete the remote feature branch. Wait for `Direct stable release` and the dispatched macOS desktop build to succeed. Verify the published release contains exactly `latest.json`, the updater app archive, the Apple Silicon DMG, and the CLI archive.

- [ ] **Step 5: Do not update the user's local Arca installation**

Report the PR, merge commit, Stable tag, workflow results, and focused test evidence. Leave local Arca source and `/Applications/Arca.app` unchanged.
