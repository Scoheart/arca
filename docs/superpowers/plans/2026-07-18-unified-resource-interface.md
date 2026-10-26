# Arca Unified Resource Interface Implementation Plan

**Goal:** Implement the approved unified interface for top-level MCPs, Models,
and Skills, the Agent-scoped resource hub, and the editor, picker, and review
dialog system without changing core ownership or safety behavior.

**Design authority:**
`docs/superpowers/specs/2026-07-18-unified-resource-interface-design.md`

**Baseline:** branch `codex/unify-resource-ui`, based on Arca v1.2.20. The design
commit is `ccabb20`.

**Architecture:** Rust core remains the authority for resource discovery,
business state, plans, safe writes, and recovery. Domain React views retain
filtering and action decisions. Shared React components own geometry, keyboard
behavior, presentation slots, focus, and modal layout only.

**Tech stack:** React 19, TypeScript 5.8, Vitest 4, Testing Library, CSS,
Tauri 2, Rust workspace tests.

## Global constraints

- Preserve unknown fields, comments, formatting, backup behavior, permissions,
  atomic writes, plan/commit identity, and recovery behavior.
- Do not introduce a shared MCP/Model/Skill business model.
- Do not expose Keychain values or sensitive configuration in DOM, attributes,
  fixtures, logs, or screenshots.
- Keep global-only Agent configuration and user-level Skills scope.
- Keep the resource order `MCPs -> Models -> Skills` everywhere in scope.
- Preserve the existing sidebar resize persistence contract.
- Make every phase independently buildable and reviewable.
- Do not edit release-owned version files or generated lockfile versions.
- Do not replace `/Applications/Arca.app` without explicit user authorization.
- Stage only files named by the current task and inspect every staged diff.

## Common verification commands

Run focused tests while iterating, then use the phase gate:

```bash
cd desktop
npx vitest run src/components/ResourceCard.test.tsx
npm test
npm run build
```

When Tauri or shared Rust contracts are touched:

```bash
cargo fmt --check
cargo test --workspace
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```

All tests must isolate `HOME` and `ARCA_HOME` through the existing test helpers.

---

## Phase 1 — Shared primitives and workspace geometry

### Task 1.1: Lock the existing resource workspace contract

**Files:**

- Modify: `desktop/src/lib/resourceWorkspace.test.ts`
- Create: `desktop/src/components/ResourceWorkspace.test.tsx`
- Modify: `desktop/src/components/ResourceWorkspace.tsx`

**Steps:**

1. Add tests for the current shared sidebar width key, default/min/max bounds,
   persistence, and sensitive-value redaction.
2. Add component tests for status-tab roving focus, `Home`/`End`, active panel
   linkage, Inspector inertness, mask close, and focus restoration.
3. Add a nested modal/Inspector Escape case using the existing modal stack so
   the topmost layer consumes Escape exactly once.
4. Run the focused tests and confirm they fail only where the approved shared
   behavior is not yet implemented.
5. Make the smallest `ResourceWorkspace` fixes needed to establish the contract.

**Focused command:**

```bash
cd desktop
npx vitest run src/lib/resourceWorkspace.test.ts src/components/ResourceWorkspace.test.tsx
```

**Commit:** `test(ui): lock resource workspace behavior`

### Task 1.2: Add the four-slot ResourceCard primitive

**Files:**

- Create: `desktop/src/components/ResourceCard.tsx`
- Create: `desktop/src/components/ResourceCard.test.tsx`
- Modify: `desktop/src/index.css`

**Public interface:**

- `identity: ReactNode`
- `configuration?: ReactNode`
- `state?: ReactNode`
- `impact?: ReactNode`
- `selected?: boolean`
- `attention?: "warning" | "danger" | "shadowed"`
- `ariaLabel: string`
- `onOpen(): void`

**Steps:**

1. Write failing tests for the four named slots, Enter/Space activation,
   `aria-pressed`, selected state, and nested-control event isolation.
2. Implement a domain-neutral card with no resource classification logic.
3. Add shared CSS for `176px` minimum height, consistent padding, focus,
   selected state, configuration strip, state row, and impact footer.
4. Ensure attention styling is textual or icon-backed and never color-only.
5. Run focused tests and the production build.

**Focused command:**

```bash
cd desktop
npx vitest run src/components/ResourceCard.test.tsx
npm run build
```

**Commit:** `feat(ui): add shared resource card`

### Task 1.3: Add DialogShell above the existing modal primitive

**Files:**

- Create: `desktop/src/components/DialogShell.tsx`
- Create: `desktop/src/components/DialogShell.test.tsx`
- Modify: `desktop/src/components/ui.tsx`
- Modify: `desktop/src/index.css`
- Modify if needed: `desktop/src/components/SkillReviewDialog.test.tsx`

**Boundary:** keep `Modal` as the low-level portal, focus trap, inert counter,
and modal-stack authority. `DialogShell` provides header/body/footer geometry,
size presets, pending-dismiss policy, and action layout.

**Steps:**

1. Add failing tests for `editor`, `picker`, and `review` size presets; fixed
   header/footer; scrollable body; initial title focus; topmost Escape; focus
   restore; and `busy` dismissal prevention.
2. Extend `Modal` only with the minimum hooks required for controlled overlay
   dismissal and responsive width. Do not fork its focus logic.
3. Implement `DialogShell` with `sm`, `md`, and `lg` presets, a `16px` viewport
   inset, optional subtitle/status, and named footer slots.
4. Preserve compatibility for existing `Modal` consumers until Phase 4.
5. Run modal stack tests, focused DialogShell tests, and build.

**Focused command:**

```bash
cd desktop
npx vitest run src/components/DialogShell.test.tsx src/components/SkillReviewDialog.test.tsx
npm run build
```

**Commit:** `feat(ui): add shared dialog shell`

### Task 1.4: Normalize workspace geometry without migrating domains

**Files:**

- Modify: `desktop/src/components/ResourceWorkspace.tsx`
- Modify: `desktop/src/index.css`
- Modify: `desktop/src/lib/skillWorkspaceCss.test.ts`

**Steps:**

1. Change the common toolbar to `56px`, filters to `40px`, content padding to
   `16px`, grid gap to `12px`, and grid columns to `minmax(250px, 1fr)`.
2. Remove Skills-only workspace toolbar, filter, sidebar-border, and grid-gap
   overrides. Retain only genuinely Skill-specific content styles.
3. Preserve the `224px` persisted sidebar default and existing resize bounds.
4. Update CSS contract tests to forbid domain-scoped workspace geometry.
5. Confirm that this task does not yet change card markup or resource actions.

**Phase 1 gate:**

```bash
cd desktop
npm test
npm run check:agent-icons
npm run build
```

**Commit:** `style(ui): normalize resource workspace geometry`

---

## Phase 2 — Top-level MCPs, Models, and Skills

### Task 2.1: Add shared resource state and empty-state primitives

**Files:**

- Create: `desktop/src/components/ResourceState.tsx`
- Modify: `desktop/src/components/ResourceWorkspace.tsx`
- Create: `desktop/src/components/ResourceState.test.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Extend the existing empty-state boundary into explicit `loading`, `empty`,
   `no-match`, `read-error`, and `recovery` presentations.
2. Keep exactly one primary action in true empty states.
3. Add clear-search or reset-filter action support for no-match states.
4. Add stable skeleton geometry so loading does not flash an empty state.
5. Test accessible status/alert roles and action naming.

**Commit:** `feat(ui): unify resource page states`

### Task 2.2: Migrate MCP cards and Inspector

**Files:**

- Create: `desktop/src/components/RegistryView.test.tsx`
- Modify: `desktop/src/components/RegistryView.tsx`
- Modify: `desktop/src/components/SourcesSidebar.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Write tests for MCP card slot mapping: name/transport/source, endpoint,
   effective/used/shadowed state, and Agent impact.
2. Test user-owned versus source-owned Inspector actions and redacted config.
3. Replace the custom MCP tile markup with `ResourceCard`.
4. Remove persistent repo/copy/edit/delete card controls. Move all actions to
   the standardized Inspector footer.
5. Replace shadowed decorative treatment with shared semantic attention and
   an explicit “被覆盖” label.
6. Preserve source filtering, status counts, ownership rules, and export/paste
   behavior unchanged.

**Focused command:**

```bash
cd desktop
npx vitest run src/components/RegistryView.test.tsx src/lib/resourceWorkspace.test.ts
```

**Commit:** `refactor(mcp): adopt shared resource interface`

### Task 2.3: Migrate Model cards and Inspector

**Files:**

- Create: `desktop/src/components/ModelsView.test.tsx`
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Test Model slot mapping: name/model ID, Base URL, protocol/reasoning,
   credential-presence state, and assigned Agents.
2. Replace the Model tile with `ResourceCard`.
3. Remove the protocol color rail; show protocol as a neutral classification.
4. Move edit/delete into the shared Inspector footer.
5. Preserve compatibility checks, guided mode, Keychain boolean handling, and
   apply semantics.
6. Cover loading and read-error behavior for both profile and Agent reads.

**Focused command:**

```bash
cd desktop
npx vitest run src/components/ModelsView.test.tsx
```

**Commit:** `refactor(models): adopt shared resource interface`

### Task 2.4: Migrate Skill cards and Inspector

**Files:**

- Modify: `desktop/src/components/SkillCard.tsx`
- Modify: `desktop/src/components/SkillCard.test.tsx`
- Modify: `desktop/src/components/SkillInspector.tsx`
- Modify: `desktop/src/components/SkillInspector.test.tsx`
- Modify: `desktop/src/components/SkillsView.tsx`
- Modify: `desktop/src/components/SkillsView.test.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Update tests to require the same four-slot `ResourceCard` structure.
2. Map description to identity secondary, source/revision to configuration,
   inventory/risk/update to state, and affected Agents to impact.
3. Remove Skills-only card surface, gap, selected, and hover rules now owned by
   `ResourceCard`.
4. Standardize Inspector section order: actionable issue, overview/source,
   configuration/content, Agent impact.
5. Keep lifecycle planning, recovery ownership, cancellation, and assignment
   semantics byte-for-byte equivalent where possible.
6. Run all existing Skills tests, not only the modified card tests.

**Focused command:**

```bash
cd desktop
npx vitest run src/components/SkillCard.test.tsx src/components/SkillInspector.test.tsx src/components/SkillsView.test.tsx src/hooks/useSkillsState.test.tsx
```

**Commit:** `refactor(skills): adopt shared resource interface`

### Task 2.5: Cross-domain resource review

**Files:**

- Create: `desktop/src/components/UnifiedResourceViews.test.tsx`
- Modify only when a failing test identifies a shared defect.

**Steps:**

1. Add a small contract test that renders representative MCP, Model, and Skill
   cards and verifies the same slot order and accessible activation.
2. Verify common toolbar/filter dimensions and absence of domain workspace
   overrides.
3. Verify card actions are Inspector-only across all domains.
4. Run the Phase 2 gate.

**Phase 2 gate:**

```bash
cd desktop
npm test
npm run check:agent-icons
npm run build
```

**Commit:** `test(ui): cover unified resource views`

---

## Phase 3 — Agent resource hub and typed navigation

### Task 3.1: Generalize typed resource navigation

**Files:**

- Modify: `desktop/src/lib/types.ts`
- Create: `desktop/src/lib/resourceNavigation.ts`
- Modify: `desktop/src/App.tsx`
- Create: `desktop/src/lib/resourceNavigation.test.ts`
- Modify: `desktop/src/components/SkillsView.test.tsx`

**Steps:**

1. Replace Skill-only request/intent types with a discriminated resource
   navigation request for MCP detail, Model detail, Skill detail, and domain
   install/add entry points.
2. Put request-to-intent construction and intent matching in pure helpers, and
   keep monotonically increasing intent IDs so a request is consumed once.
3. Route Agent-originated intents to the correct top-level view and preserve
   normal top-bar navigation behavior.
4. Add tests for intent creation, single consumption, missing resources, and
   switching between domains.
5. Keep domain views responsible for validating the requested resource.

**Commit:** `feat(ui): add typed resource navigation`

### Task 3.2: Normalize Agent identity and configuration locations

**Files:**

- Modify: `desktop/src/components/AgentView.tsx`
- Create: `desktop/src/components/AgentView.test.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Write tests for the exact MCPs, Models, Skills order.
2. Render configuration location tiles with domain, health/availability,
   format/protocol summary, path, and disclosure.
3. Remove repeated path copy from the lower resource controls.
4. Stack location tiles below the narrow breakpoint without horizontal scroll.
5. Preserve read-only/reference Agent behavior.

**Commit:** `refactor(agent): unify configuration locations`

### Task 3.3: Add AgentResourcePanel and MCP tab

**Files:**

- Create: `desktop/src/components/AgentResourcePanel.tsx`
- Create: `desktop/src/components/AgentResourcePanel.test.tsx`
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Add accessible MCPs, Models, Skills tabs with counts and roving focus.
2. Move existing installed MCPs into dense rows with identity, current-Agent
   state, switch, and disclosure.
3. Preserve enable/disable pending keys and supported-transport filtering.
4. Keep the current add MCP behavior behind the tab action temporarily; Phase 4
   replaces the anchored popover with a picker dialog.
5. Make row disclosure open the top-level MCP Inspector through typed navigation.

**Commit:** `feat(agent): add tabbed MCP resource panel`

### Task 3.4: Add Models and Skills tabs

**Files:**

- Modify: `desktop/src/components/AgentResourcePanel.tsx`
- Modify: `desktop/src/components/AgentResourcePanel.test.tsx`
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentSkillsSection.tsx`
- Modify: `desktop/src/components/AgentSkillsSection.test.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Move current/compatible Model controls into the Models tab.
2. Keep guided versus managed behavior and explicit apply review unchanged.
3. Adapt `AgentSkillsSection` content into the Skills tab without duplicating
   lifecycle planning or cancellation logic.
4. Make Model and Skill rows navigate to their top-level Inspectors.
5. Remove the old standalone Model, Skills, and MCP sections after parity tests
   pass.

**Phase 3 gate:**

```bash
cd desktop
npm test
npm run build
```

Run Tauri tests only if command signatures changed:

```bash
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```

**Commit:** `feat(agent): complete unified resource hub`

---

## Phase 4 — Dialog migration and review flows

### Task 4.1: Migrate editor dialogs

**Files:**

- Modify: `desktop/src/components/RegistryEditPage.tsx`
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/SubscribeDialog.tsx`
- Modify: `desktop/src/components/AddAgentDialog.tsx`
- Modify: `desktop/src/components/EnvEditor.tsx`
- Create: `desktop/src/components/RegistryEditPage.test.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Create: `desktop/src/components/SubscribeDialog.test.tsx`
- Create: `desktop/src/components/AddAgentDialog.test.tsx`

**Steps:**

1. Wrap each form in `DialogShell kind="editor"`.
2. Standardize header copy, field labels, inline validation, advanced sections,
   status area, and `Cancel + Save` footer.
3. Keep the MCP editor at `lg` width with a scrollable body; remove its
   app-covering special behavior.
4. Preserve transport conversion warnings, overwrite protection, unknown-field
   preservation, and credential rules.
5. Lock close/overlay/Escape while the save call is pending.

**Commit:** `refactor(ui): migrate editor dialogs`

### Task 4.2: Replace Agent MCP popover with a picker dialog

**Files:**

- Create: `desktop/src/components/ResourcePickerDialog.tsx`
- Create: `desktop/src/components/ResourcePickerDialog.test.tsx`
- Modify: `desktop/src/components/AgentResourcePanel.tsx`
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Implement search, empty results, selection state, result count, and explicit
   `Cancel + Add` actions using `DialogShell kind="picker"`.
2. Adapt the existing supported-transport and not-installed filters.
3. Remove anchored-popover and scrim CSS after behavior parity.
4. Test `900x600` height constraints through structural CSS assertions and
   scrollable-body behavior.

**Commit:** `feat(agent): add MCP picker dialog`

### Task 4.3: Migrate Skill install and review dialogs

**Files:**

- Modify: `desktop/src/components/SkillInstallDialog.tsx`
- Modify: `desktop/src/components/SkillInstallDialog.test.tsx`
- Modify: `desktop/src/components/SkillReviewDialog.tsx`
- Modify: `desktop/src/components/SkillReviewDialog.test.tsx`
- Modify: `desktop/src/index.css`

**Steps:**

1. Replace specialized outer shells with `DialogShell` while retaining the real
   install steps and full review evidence.
2. Keep nested risk confirmation as the topmost modal and preserve single Escape
   consumption.
3. Keep operation IDs, hashes, confirmation payloads, cancellation, commit, and
   recovery logic unchanged.
4. Remove duplicated header/footer/close/responsive CSS only after visual and
   interaction tests pass.

**Commit:** `refactor(skills): adopt shared dialog system`

### Task 4.4: Replace scoped window.confirm calls

**Files:**

- Create: `desktop/src/components/ReviewDialog.tsx`
- Create: `desktop/src/components/ReviewDialog.test.tsx`
- Modify: `desktop/src/components/RegistryView.tsx`
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/SourcesSidebar.tsx`
- Modify: `desktop/src/components/RegistryEditPage.tsx`

**Steps:**

1. Add compact review variants for MCP deletion, Model deletion/application,
   source deletion, and transport replacement.
2. Show exact resource, targets, retained/removed effects, backup behavior, and
   the explicit action verb.
3. Start the existing mutation only from the review dialog action.
4. Keep the dialog open on failure, preserve context, and expose retry or cancel.
5. Assert that no `window.confirm` remains in the scoped desktop source.

**Focused command:**

```bash
cd desktop
npx vitest run src/components/ReviewDialog.test.tsx src/components/RegistryView.test.tsx src/components/ModelsView.test.tsx
rg 'window\.confirm' src
```

Expected: the tests pass and the final search returns no scoped matches.

**Phase 4 gate:**

```bash
cd desktop
npm test
npm run check:agent-icons
npm run build
cd ..
cargo fmt --check
cargo test --workspace
cargo test --manifest-path desktop/src-tauri/Cargo.toml
```

**Commit:** `refactor(ui): unify destructive reviews`

---

## Phase 5 — Integrated verification and installed-app review

### Task 5.1: Full static and isolated integration verification

**Files:** modify only when a failure proves a product defect.

**Steps:**

1. Run all Desktop tests, production build, and icon checks.
2. Run the root Rust workspace and Tauri test suites.
3. Confirm test processes use isolated `HOME`/`ARCA_HOME` and do not touch real
   Keychain or Skills.
4. Search rendered fixtures and source snapshots for secret-like values.
5. Run `git diff --check` and review the complete branch diff by phase.

**Commands:**

```bash
cd desktop
npm test
npm run check:agent-icons
npm run build
cd ..
cargo fmt --check
cargo test --workspace
cargo test --manifest-path desktop/src-tauri/Cargo.toml
git diff --check origin/main...HEAD
```

### Task 5.2: Build the review artifact without replacing the installed app

**Steps:**

1. Follow `../../../memory/reference/arca-ui-review.md` and the project release
   contracts.
2. Build/package from the committed branch only after static verification passes.
3. Verify bundle ID, architecture, sidecar version, and deep strict codesign for
   the review artifact.
4. Do not name or launch a source target bundle as the installed application.
5. Stop and request explicit authorization before replacing
   `/Applications/Arca.app`.

### Task 5.3: Installed-app acceptance after authorization

**Steps:**

1. Atomically stage and replace `/Applications/Arca.app` under the UI review
   playbook, with a verified rollback copy.
2. Inspect only the exact installed path with Computer Use.
3. Capture MCPs, Models, Skills, an Agent page, and all three dialog kinds at
   `1200x820` and `900x600` in light and dark themes.
4. Verify grid column targets, no horizontal scroll, non-reflowing Inspector,
   fixed dialog header/footer, keyboard focus, topmost Escape, and reduced motion.
5. Confirm screenshots, DOM, titles, logs, and fixtures contain no sensitive
   configuration values.
6. Exercise common MCP copy/edit workflows. If Inspector-only actions are
   materially slower, add one shared overflow action across all resource cards;
   do not restore domain-specific persistent toolbars.
7. Restore the previous installed app immediately if bundle, signature, sidecar,
   launch, or core behavior verification fails.

### Task 5.4: Documentation and final delivery

**Files:**

- Modify as required: `README.md`
- Modify as required: `website/guide/desktop.md`
- Modify as required: `website/guide/agents.md`
- Modify as required: `website/guide/models.md`
- Modify as required: `website/guide/skills.md`
- Modify as required: screenshots under `website/public/img/`

**Steps:**

1. Document the global resource order and Agent resource tabs.
2. Update only screenshots captured from the accepted installed app.
3. Run the website production build.
4. Re-run Desktop smoke tests after any copy or navigation change.
5. Inspect final commits, keep the worktree clean, and report unpushed status.
6. Push or open a PR only with explicit user authorization.

**Final commands:**

```bash
cd website
npm ci
npm run build
cd ../desktop
npm test
npm run build
```

**Suggested final commit:** `docs(arca): document unified resource interface`

## Completion evidence

The implementation is complete only when the handoff includes:

- phase-by-phase commit list;
- focused and full test results;
- installed app version, exact path, signature, sidecar, and screenshot evidence;
- `1200x820` and `900x600` results for both themes;
- the final decision on Inspector-only card actions;
- confirmation that no real configuration, Skills, Keychain value, or secret was
  read by tests or included in artifacts;
- clean Arca and parent worktrees, or an explicit list of preserved unrelated
  changes;
- push/PR/release status stated accurately.
