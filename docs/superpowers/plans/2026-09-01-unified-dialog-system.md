# Unified Dialog System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Arca's visually divergent dialogs with one compact, flat, icon-led system while preserving necessary information and all existing behavior.

**Architecture:** Keep `Modal` as the accessibility and stacking authority, make `DialogShell` the visual authority for editor/picker/review dialogs, and align `ResourceInspector` with the same header/body/footer geometry. Add reusable inspector metric and icon-aware field primitives, then migrate Model, MCP, Skill, Provider, Agent, picker, and confirmation surfaces without changing Core or persistence behavior.

**Tech Stack:** React 19, TypeScript, Tauri, existing Arca SVG icon components, CSS design tokens, Vitest, Testing Library.

**Delivery constraint:** Work from live remote `main` in an isolated worktree. Do not create a local commit or run `git push`; extend `codex/unified-dialog-system` with GitHub Git Data API commits, create one PR, squash merge it, and verify the generated Stable release. The repository fast-delivery rule means tests and builds are not run locally unless the user explicitly changes that instruction; targeted tests are still updated as executable contracts and the signed Stable build is the compile gate.

---

## File map

- `desktop/src/components/DialogShell.tsx`: shared shell semantics, default kind icons, explicit shell variants.
- `desktop/src/components/DialogShell.test.tsx`: shell presets, regions, default icon, busy-close behavior.
- `desktop/src/components/ResourceWorkspace.tsx`: resource detail shell and reusable inspector metrics/fields.
- `desktop/src/components/ResourceWorkspace.test.tsx`: modal/focus behavior plus flat inspector structure.
- `desktop/src/components/ModelsView.tsx`: icon-led Model details and explicit Provider dialog variants.
- `desktop/src/components/ModelsView.test.tsx`: Model metric/field and Provider shell contracts.
- `desktop/src/components/RegistryView.tsx`: flat, icon-aware MCP details.
- `desktop/src/components/SkillInspector.tsx`: flat, icon-aware Skill details while retaining risk evidence.
- `desktop/src/components/AddAgentDialog.tsx`: explicit Agent editor shell variant.
- `desktop/src/components/SkillInstallDialog.tsx`: shared shell and flat step content.
- `desktop/src/components/SkillReviewDialog.tsx`: shared review/risk shell and flat groups.
- `desktop/src/components/ReviewDialog.tsx`: compact confirmation structure.
- `desktop/src/components/icons.tsx`: only missing metric icons needed by inspector fields.
- `desktop/src/index.css`: unified dialog tokens, geometry, flat inspector/editor/picker/review styling, responsive rules.
- `desktop/src/lib/dialogOverflowCss.test.ts`: body/list overflow contracts.
- `desktop/src/lib/dialogSystemCss.test.ts`: new structural CSS regression checks.
- `docs/superpowers/specs/2026-09-01-unified-dialog-system-design.md`: approved design, already committed on the feature branch.
- `docs/superpowers/plans/2026-09-01-unified-dialog-system.md`: this implementation plan.

---

### Task 1: Make DialogShell the visual authority

**Files:**
- Modify: `desktop/src/components/DialogShell.tsx`
- Modify: `desktop/src/components/DialogShell.test.tsx`
- Modify: `desktop/src/index.css`

- [ ] **Step 1: Add the failing shell contract**

Extend `DialogShell.test.tsx` with a test that requires a default semantic glyph, the shared region order, and an explicit variant hook:

```tsx
it.each([
  ["editor", "编辑"],
  ["picker", "选择"],
  ["review", "确认"],
] as const)("gives %s dialogs a semantic default glyph", (kind, label) => {
  render(
    <DialogShell kind={kind} title={`${label}资源`} onClose={() => undefined}>
      内容
    </DialogShell>,
  );

  const shell = screen.getByRole("dialog", { name: `${label}资源` }).firstElementChild!;
  expect(shell.querySelector(".arca-dialog-shell-glyph")).toBeInTheDocument();
  expect(shell).toHaveAttribute("data-dialog-kind", kind);
});
```

- [ ] **Step 2: Add default kind glyphs without changing call sites**

Import existing icons and derive the fallback only when the caller does not provide `leading`:

```tsx
import { CheckIcon, EditIcon, SearchIcon, XIcon } from "./icons";

const DEFAULT_LEADING: Record<DialogShellKind, ReactNode> = {
  editor: <EditIcon className="w-4 h-4" />,
  picker: <SearchIcon className="w-4 h-4" />,
  review: <CheckIcon className="w-4 h-4" />,
};

const effectiveLeading = leading ?? (
  <span className="arca-dialog-shell-glyph" aria-hidden="true">
    {DEFAULT_LEADING[kind]}
  </span>
);
```

Render `effectiveLeading` in the existing header. Preserve a caller-supplied danger glyph from `ReviewDialog`.

- [ ] **Step 3: Replace shell geometry with one flat rule set**

In `index.css`, define shared geometry once:

```css
:root {
  --arca-dialog-header-height: 56px;
  --arca-dialog-footer-height: 52px;
  --arca-dialog-inline: 18px;
  --arca-dialog-block: 16px;
}

.arca-dialog-shell {
  display: flex; width: 100%; max-height: inherit; min-height: 0;
  flex-direction: column; overflow: hidden; padding: 0;
}
.arca-dialog-shell-header {
  display: flex; min-height: var(--arca-dialog-header-height); flex: 0 0 auto;
  align-items: center; gap: 11px; padding: 10px var(--arca-dialog-inline);
  border-bottom: 1px solid var(--border-hairline); background: var(--surface-overlay);
}
.arca-dialog-shell-glyph {
  display: inline-flex; width: 32px; height: 32px; flex: 0 0 32px;
  align-items: center; justify-content: center; border-radius: 9px;
  background: color-mix(in srgb, var(--color-blue) 10%, transparent); color: var(--color-blue);
}
.arca-dialog-shell-body {
  flex: 1 1 auto; min-width: 0; min-height: 0; padding: var(--arca-dialog-block) var(--arca-dialog-inline);
  overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain;
}
.arca-dialog-shell-footer {
  display: flex; min-height: var(--arca-dialog-footer-height); flex: 0 0 auto;
  align-items: center; gap: 8px; margin: 0; padding: 9px var(--arca-dialog-inline);
  border-top: 1px solid var(--border-hairline); background: var(--surface-overlay);
}
```

Keep `picker` body overflow hidden so its result list owns the only scroll axis.

- [ ] **Step 4: Inspect the task diff**

Run:

```bash
git diff --check -- desktop/src/components/DialogShell.tsx desktop/src/components/DialogShell.test.tsx desktop/src/index.css
```

Expected: no whitespace errors. Do not run the test command under the current repository fast-mode instruction.

---

### Task 2: Flatten ResourceInspector and add reusable icon-led primitives

**Files:**
- Modify: `desktop/src/components/ResourceWorkspace.tsx`
- Modify: `desktop/src/components/ResourceWorkspace.test.tsx`
- Modify: `desktop/src/components/icons.tsx`
- Modify: `desktop/src/index.css`

- [ ] **Step 1: Add flat-inspector structure assertions**

Update the ResourceWorkspace harness to render metrics and an icon-aware field, then assert the structure:

```tsx
<ResourceInspector title="资源 A" avatar={<span>A</span>} onClose={closeInspector}>
  <InspectorMetrics>
    <InspectorMetric icon={<LayersIcon />} label="上下文" value="1M" />
  </InspectorMetrics>
  <InspectorField icon={<LinkIcon />} label="地址" mono action={<button aria-label="复制地址">复制</button>}>
    https://example.test
  </InspectorField>
</ResourceInspector>
```

```tsx
expect(screen.getByLabelText("资源指标")).toBeVisible();
expect(screen.getByText("1M")).toBeVisible();
expect(screen.getByRole("button", { name: "复制地址" })).toBeVisible();
```

- [ ] **Step 2: Add metric and icon-aware field primitives**

In `ResourceWorkspace.tsx`, add:

```tsx
export function InspectorMetrics({ children }: { children: ReactNode }) {
  return <div className="arca-inspector-metrics" aria-label="资源指标">{children}</div>;
}

export function InspectorMetric({ icon, label, value }: {
  icon: ReactNode;
  label: string;
  value: ReactNode;
}) {
  return (
    <div className="arca-inspector-metric" title={label}>
      <span className="arca-inspector-metric-icon" aria-hidden="true">{icon}</span>
      <span><strong>{value}</strong><small>{label}</small></span>
    </div>
  );
}
```

Extend `InspectorSection` with `icon?: ReactNode`. Extend `InspectorField` with `icon?: ReactNode`, `action?: ReactNode`, and `wide?: boolean`; render `data-wide={wide ? "true" : undefined}` on its root. Render visible short labels; icons do not replace ambiguous labels.

- [ ] **Step 3: Remove the fixed Inspector height**

Replace the current 680 px surface and filled nested containers:

```css
.arca-workspace-inspector-surface {
  display: flex; width: 100%; max-height: inherit; min-width: 0; min-height: 0;
  overflow: hidden; border-radius: inherit;
}
.arca-resource-inspector {
  display: flex; width: 100%; max-height: inherit; min-height: 0;
  flex-direction: column; overflow: hidden; background: var(--surface-overlay);
}
.arca-resource-inspector-head {
  min-height: var(--arca-dialog-header-height); padding: 10px var(--arca-dialog-inline);
  border-bottom: 1px solid var(--border-hairline);
}
.arca-resource-inspector-body {
  flex: 0 1 auto; min-height: 0; max-height: calc(100vh - 140px);
  padding: var(--arca-dialog-block) var(--arca-dialog-inline); overflow-y: auto;
}
.arca-resource-inspector-footer {
  min-height: var(--arca-dialog-footer-height); margin: 0; padding: 9px var(--arca-dialog-inline);
  border-top: 1px solid var(--border-hairline); background: transparent;
}
.arca-inspector-section,
.arca-model-inspector-fields {
  padding: 0; border: 0; border-radius: 0; background: transparent;
}
```

- [ ] **Step 4: Add missing metric icons only**

Add `GaugeIcon` and `CalendarIcon` to `icons.tsx` using the same 24 × 24 stroked SVG contract as existing icons. Do not introduce another icon package.

- [ ] **Step 5: Inspect the task diff**

Run `git diff --check` for the four files. Expected: no whitespace errors.

---

### Task 3: Rebuild Model details around metrics and a flat field grid

**Files:**
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Modify: `desktop/src/i18n/index.ts`
- Modify: `desktop/src/index.css`

- [ ] **Step 1: Add Model detail contracts**

Extend the existing Model Inspector test to require metric values, essential labels, and copy buttons:

```tsx
expect(within(inspector).getByLabelText("资源指标")).toBeVisible();
expect(within(inspector).getByText("200K")).toBeVisible();
expect(within(inspector).getByText("32K")).toBeVisible();
expect(within(inspector).getByText("模型 ID")).toBeVisible();
expect(within(inspector).getByText("完整请求 URL")).toBeVisible();
expect(within(inspector).getByRole("button", { name: "复制模型 ID" })).toBeVisible();
```

- [ ] **Step 2: Render important metadata as metrics**

Import `InspectorMetrics`, `InspectorMetric`, `GaugeIcon`, `LayersIcon`, `SparklesIcon`, and `TerminalIcon`. Render metrics only when the value exists:

```tsx
<InspectorMetrics>
  {contextWindow && <InspectorMetric icon={<LayersIcon />} label={t("models.context")} value={formatTokens(contextWindow)} />}
  {maxOutputTokens && <InspectorMetric icon={<GaugeIcon />} label={t("models.outputLimit")} value={formatTokens(maxOutputTokens)} />}
  {capabilities.length > 0 && <InspectorMetric icon={<SparklesIcon />} label={t("models.capabilities")} value={capabilities.join(" · ")} />}
</InspectorMetrics>
```

Keep the description as one paragraph. Render Provider, protocol, reasoning, release date, model ID, URL, and environment variable in the flat field grid.

- [ ] **Step 3: Add accessible copy actions**

Inside `ModelInspector`, use the existing toast and browser clipboard API:

```tsx
const copyValue = async (label: string, value: string) => {
  await navigator.clipboard.writeText(value);
  toast.show({ kind: "success", msg: t("models.copiedValue", { label }) });
};
```

Pass icon-only buttons through `InspectorField.action` with `aria-label`, `title`, and `CopyIcon`. Add `models.copiedValue` in Chinese and English.

- [ ] **Step 4: Add responsive flat-grid CSS**

```css
.arca-model-inspector-fields {
  display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: 22px;
}
.arca-model-inspector-fields .arca-inspector-field[data-wide="true"] { grid-column: 1 / -1; }
@media (max-width: 620px) {
  .arca-model-inspector-fields { grid-template-columns: minmax(0, 1fr); }
}
```

Extend `InspectorField` with `wide?: boolean` in Task 2 so model ID and URL span both columns.

- [ ] **Step 5: Inspect the task diff**

Run `git diff --check` for the four files. Expected: no whitespace errors.

---

### Task 4: Flatten MCP and Skill inspectors without hiding risk data

**Files:**
- Modify: `desktop/src/components/RegistryView.tsx`
- Modify: `desktop/src/components/SkillInspector.tsx`
- Modify: `desktop/src/index.css`

- [ ] **Step 1: Apply icons to MCP detail groups and fields**

Use existing `LinkIcon`, `TerminalIcon`, `LayersIcon`, and `CopyIcon` with the Task 2 primitives:

```tsx
<InspectorSection title="连接" icon={<LinkIcon className="w-4 h-4" />}>
  <InspectorField icon={<NetworkIcon className="w-4 h-4" />} label="地址" mono>
    {endpoint.text}
  </InspectorField>
</InspectorSection>
```

Keep override warnings, descriptions, tags, configuration preview, and all actions. Make secondary repeated copy/customize controls icon-only with `aria-label` and `title`; keep Delete and Edit as icon plus short text.

- [ ] **Step 2: Apply icons to Skill detail groups**

Use `LinkIcon` for source/version, `LayersIcon` for state/risk, `FolderIcon` for files, and `TerminalIcon` for `SKILL.md`. Keep all risk findings, affected Agents, file hashes, truncation notices, errors, and raw content.

- [ ] **Step 3: Flatten visual groups in CSS**

Remove filled backgrounds and nested radii from ordinary Inspector sections, file trees, and state groups. Preserve a tinted background only for actionable warnings, errors, and high-risk evidence. Use hairline row separators and spacing for all other groups.

- [ ] **Step 4: Inspect the task diff**

Run `git diff --check` for the three files. Expected: no whitespace errors.

---

### Task 5: Remove feature-specific shell overrides from editors and pickers

**Files:**
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/AddAgentDialog.tsx`
- Modify: `desktop/src/components/SkillInstallDialog.tsx`
- Modify: `desktop/src/components/SkillReviewDialog.tsx`
- Modify: `desktop/src/index.css`
- Modify: `desktop/src/components/ModelsView.test.tsx`

- [ ] **Step 1: Replace descendant detection with explicit variants**

Set explicit `className` values at DialogShell call sites:

```tsx
<DialogShell className="arca-dialog-provider-catalog" kind="picker" size="lg" ... />
<DialogShell className="arca-dialog-provider-editor" kind="editor" size="wide" ... />
<DialogShell className="arca-dialog-agent-editor" kind="editor" size="lg" ... />
<DialogShell className="arca-dialog-skill-flow" kind="editor" size="md" ... />
<DialogShell className="arca-dialog-skill-review" kind="review" size="lg" ... />
```

Update `ModelsView.test.tsx` to require explicit class names and reject `:has(.arca-provider-catalog)` and `:has(.arca-provider-form)` selectors.

- [ ] **Step 2: Flatten Provider and Agent forms**

Keep the current fields and controls, but change form section styling to transparent groups with one top divider:

```css
.arca-provider-form-section {
  display: grid; min-width: 0; gap: 12px; margin-top: 16px; padding-top: 16px;
  border-top: 1px solid var(--border-hairline); background: transparent;
}
.arca-agent-create,
.arca-agent-capability-detail { background: transparent; border-radius: 0; }
```

Do not remove credential controls, protocol paths, request previews, validation, or Agent capability state.

- [ ] **Step 3: Flatten Skill workflow wrappers**

Keep the step state machine and nested risk modal. Remove duplicate `.arca-skill-dialog-body`, `.arca-skill-review-body`, and feature footer chrome where the shared shell already supplies it. Style source choices and candidate rows as flat rows rather than cards; preserve selected and risk states.

- [ ] **Step 4: Delete the old shell `:has(...)` rules**

Remove all Provider catalog, Provider form, and Agent create shell geometry selectors based on `:has`. Keep `:has` only where it expresses a local control relationship, such as a row containing a remove button.

- [ ] **Step 5: Inspect the task diff**

Run `git diff --check` for the five production files and the Model test file. Expected: no whitespace errors.

---

### Task 6: Compact confirmations and normalize picker rows

**Files:**
- Modify: `desktop/src/components/ReviewDialog.tsx`
- Modify: `desktop/src/components/ResourcePickerDialog.tsx`
- Modify: `desktop/src/components/ConsumptionPickerDialog.tsx`
- Modify: `desktop/src/index.css`
- Modify: `desktop/src/lib/dialogOverflowCss.test.ts`
- Create: `desktop/src/lib/dialogSystemCss.test.ts`

- [ ] **Step 1: Keep confirmation content to one impact block**

Preserve the semantic icon, title, impact sentence, error status, and two actions. Remove nested filled impact cards from confirmation-specific CSS; use a plain content block with an optional preserved-data line and a subtle status dot.

- [ ] **Step 2: Normalize picker rows**

Keep search, selected state, count, metadata, and empty states. Use transparent rows with hairline separators by default and one quiet selected fill. Ensure `.arca-picker-list` remains the only picker scroll axis.

- [ ] **Step 3: Add CSS regression tests**

Create `dialogSystemCss.test.ts`:

```ts
import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

const css = await readFile(new URL("../index.css", import.meta.url), "utf8");

it("keeps dialog geometry shared and inspectors content-driven", () => {
  expect(css).toMatch(/--arca-dialog-header-height:\s*56px/);
  expect(css).toMatch(/\.arca-workspace-inspector-surface\s*\{[^}]*max-height:\s*inherit/);
  expect(css).not.toMatch(/\.arca-workspace-inspector-surface\s*\{[^}]*height:\s*min\(680px/);
});

it("does not style dialog shells by descendant feature detection", () => {
  expect(css).not.toMatch(/\.arca-dialog-shell:has\(\.arca-provider-(catalog|form)\)/);
  expect(css).not.toMatch(/\.arca-dialog-shell:has\(\.arca-agent-create\)/);
});
```

Update `dialogOverflowCss.test.ts` only if selector names change; retain its one-scroll-axis assertions.

- [ ] **Step 4: Inspect the task diff**

Run `git diff --check` for the six files. Expected: no whitespace errors.

---

### Task 7: Final consistency review and remote-only delivery

**Files:**
- Review every file listed in the File map.
- Do not modify release-owned versions or changelog files.

- [ ] **Step 1: Review the complete diff**

Run:

```bash
git status --short
git diff --check
git diff --stat
git diff -- desktop/src/components desktop/src/index.css desktop/src/lib docs/superpowers
```

Expected: only dialog-system files, this plan, and the approved design spec. Confirm no Core, persistence, Keychain, version, generated asset, or unrelated workspace changes.

- [ ] **Step 2: Confirm the fast-mode validation decision**

Do not run local `npm test`, `npm run build`, Cargo tests, formatters, changed-surface validator, or preflight under the current Arca `AGENTS.md`. Record them as intentionally skipped. The feature adds focused tests but relies on the signed Direct Stable build as the compile gate.

- [ ] **Step 3: Create the remote feature commit**

Create an exact manifest containing only intended files and update `codex/unified-dialog-system` with `gh-remote-commit`:

```text
feat(desktop): unify dialog presentation

Replace divergent dialog chrome with a flat, icon-led system that preserves necessary information and removes fixed Inspector whitespace.
```

Verify every remote blob SHA against `git hash-object` before opening the PR.

- [ ] **Step 4: Open and merge the PR**

Create a PR against live `main`, inspect its exact file list, wait once for required checks, require `MERGEABLE/CLEAN`, then squash merge with head-SHA protection and delete the remote branch.

- [ ] **Step 5: Verify the generated Stable release**

Watch the exact `Direct stable release` run for the merge SHA. Confirm the release commit, immutable tag, and Draft target share one SHA; confirm the Desktop build is dispatched from `main`.

Create an exact release-tag worktree and run the single-pass verifier without installation:

```bash
tag=$(gh release list --repo Scoheart/arca --limit 1 --json tagName --jq '.[0].tagName')
release_source=$(mktemp -d /tmp/arca-dialog-release.XXXXXX)
git worktree add --detach "$release_source" "$tag"
bash /Users/scoheart/Code/ai/.agents/skills/arca-release/scripts/verify-release.sh \
  --repo Scoheart/arca \
  --source-root "$release_source" \
  --wait \
  "$tag"
```

Expected: four assets, matching digests, signed App and updater, valid arm64 CLI, valid DMG, and published non-prerelease Release. Do not use `--install` because the user previously chose to update Arca themselves.

- [ ] **Step 6: Report visual acceptance boundary**

Report the release, PR, feature/release SHAs, build conclusion, verification result, skipped local checks, and preserved original checkout. State explicitly that `/Applications/Arca.app` was not replaced and final installed-UI review remains for the user's update.
