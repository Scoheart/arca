# Arca Unified Resource Interface Design

## Status

Approved design for implementation planning. The scope includes the top-level
MCPs, Models, and Skills workspaces, Agent-scoped resource management, and the
editing, picking, and review dialogs used by those surfaces.

The selected direction is a balanced resource workspace: keep the overview and
low migration risk of the current card grid, add the scan efficiency of dense
Agent rows, and retain focused detail through an overlay Inspector instead of a
permanent split pane.

## Context and evidence

The source and the installed `/Applications/Arca.app` version 1.2.18 were
reviewed before this design. The current product already has useful shared
primitives, but the visible system is only partly unified:

- MCPs, Models, and Skills all render through `ResourceWorkspace`, but Skills
  overrides the workspace surface, toolbar height, filter height, grid gap, and
  card treatment.
- MCP cards use a generic bordered tile, Model cards add a permanent protocol
  rail, and Skill cards use a borderless raised surface with a larger minimum
  height.
- MCP and Model cards expose persistent direct actions while Skill cards only
  open an Inspector.
- MCP, Model, and Skill metadata appear in different orders even when they
  answer the same questions: what is this, how is it configured, what state is
  it in, and which Agents does it affect?
- The top-level resource order is MCPs, Models, Skills. The Agent configuration
  location order is Models, MCPs, Skills, while its management sections are
  Models, Skills, MCPs.
- The Agent page duplicates three distinct section layouts in a long scrolling
  page instead of acting as a scoped projection of the top-level resources.
- Ordinary `Modal`, the MCP editor page, the Model form, Skill install, and
  Skill review use different shells, action placement, error placement, and
  responsive rules. Several destructive actions still use `window.confirm`.
- MCPs and Models have little component-level UI coverage compared with Skills.

The result is structurally related screens that still require users to relearn
hierarchy and action placement when they change domains.

## Goals

1. Establish one resource interaction grammar across MCPs, Models, and Skills.
2. Preserve domain semantics instead of forcing unlike data into one model.
3. Make scanning efficient at both `1200x820` and `900x600`.
4. Make Agent pages a current-Agent projection of the top-level resources.
5. Unify editor, picker, and review dialog behavior without weakening existing
   safety or recovery contracts.
6. Keep Rust core as the only authority for discovery, codecs, plans, writes,
   and recovery.
7. Deliver the change in independently reviewable, reversible phases.

## Non-goals

- Do not change the Rust resource data models merely to simplify presentation.
- Do not duplicate core orchestration, status classification, or write logic in
  React.
- Do not reintroduce project-scoped configuration writes.
- Do not expose secrets, Keychain values, or unredacted sensitive configuration
  in rendered UI, logs, fixtures, or screenshots.
- Do not replace the existing safe-write, backup, plan/commit, conflict, or
  recovery behavior.
- Do not turn Arca into a permanent two-pane desktop inspector; the design must
  remain usable at `900x600`.
- Do not redesign the website, CLI, or TUI in this implementation program.

## Design principles

### Unify cognition, not domain data

Every resource surface answers the same questions in the same order:

1. Identity: what resource is this?
2. Configuration: where or how is it configured?
3. State: is it usable, assigned, stale, overridden, or unsafe?
4. Impact: which Agents are affected?
5. Action: what can the user safely do next?

MCP transport, Model protocol, and Skill risk remain domain-specific content.
They occupy shared presentation slots rather than becoming a shared business
enum.

### One global resource order

Use `MCPs -> Models -> Skills` everywhere:

- top-level navigation;
- Agent configuration locations;
- Agent resource tabs;
- empty-state and picker ordering where multiple domains appear;
- user-facing documentation updated by the implementation.

### Opaque, restrained surfaces

Use the existing opaque macOS-like neutral palette. The Arca gold/coral/magenta
gradient remains exclusive to the wordmark. System blue is the interactive
accent. Green, amber, red, and purple communicate semantic state only; protocol,
transport, and domain identity do not receive persistent decorative color rails.

### Progressive disclosure

Cards and Agent rows support identification and scanning. Inspectors and dialogs
own detailed configuration and actions. Destructive, risky, or multi-target
operations remain explicit review steps.

## Information architecture

### Top-level resource workspace

All three top-level resource pages use the same structure:

1. A resizable category sidebar.
2. A toolbar with search and page-level actions.
3. A fixed status-tab row.
4. A scrollable resource grid.
5. An overlay Inspector attached to the content region.

The sidebar only contains stable category navigation:

- MCPs: source;
- Models: protocol;
- Skills: source.

Status remains in tabs above the grid. This prevents the sidebar from mixing
taxonomy with transient state and preserves the current source/protocol mental
model.

Use one shared workspace geometry:

- top bar: retain the current `56px` application bar;
- default sidebar: retain `224px`, with the existing persisted resize behavior;
- workspace toolbar: `56px` minimum height;
- status row: `40px` height;
- content padding: `16px` horizontally with `12px` grid gaps;
- resource column: `minmax(250px, 1fr)`;
- Inspector: overlay the resource content without changing grid columns.

At `1200x820`, the primary target is three grid columns. At `900x600`, the
primary target is two grid columns with no horizontal scrolling. An open
Inspector may cover the grid but must not reflow it.

### Shared resource card anatomy

Introduce a presentational `ResourceCard` with four slots and no domain
business logic:

1. `identity`: avatar, name, and primary domain identifier;
2. `configuration`: a compact inset strip;
3. `state`: semantic badges and classifications;
4. `impact`: Agent usage plus a disclosure affordance.

Domain mappings are:

| Domain | Identity secondary | Configuration | State | Impact |
| --- | --- | --- | --- | --- |
| MCP | transport and source | endpoint or command | effective, used, shadowed | installed Agents |
| Model | model ID | Base URL | protocol, reasoning, credential presence | assigned Agents |
| Skill | short description | source and revision | inventory states, risk, update | affected Agents |

The default minimum card height is `176px`. Identity text and configuration
values truncate with accessible titles where safe. Skill descriptions may use a
two-line clamp. Rows in a grid stretch to equal height.

Cards have one primary interaction: open the Inspector. Persistent edit, copy,
and delete icons are removed from MCP and Model cards so all three domains use
the same interaction. This is a deliberately reversible decision: installed-app
testing must measure whether the extra click materially harms common MCP copy or
edit workflows. If it does, a shared overflow action can be added to all three
domains; domain-specific always-visible toolbars must not return.

### Shared Inspector

Retain the overlay behavior and shared focus restoration. Standardize its
structure:

- header: avatar, title, classification, close control;
- body: actionable warning or risk first, followed by overview,
  configuration, and Agent impact;
- footer left: destructive action when the resource is writable;
- footer right: optional secondary action and exactly one primary action.

Examples include `Copy + Edit`, `Open source + Update`, and `Manage + Apply`.
Read-only source-owned resources omit unavailable mutations rather than showing
disabled controls without explanation.

The Inspector must retain opaque surfaces, background inertness, overlay-only
masking, top-level Escape handling, and secret redaction.

## Agent resource hub

Replace the current long sequence of Model, Skills, and MCP sections with two
stable layers.

### Layer 1: Agent identity and configuration locations

The Agent header shows identity, detection status, and refresh or edit actions.
Below it, three configuration-location tiles appear in the global order MCPs,
Models, Skills. Each tile shows:

- domain name;
- availability or health summary;
- format or protocol summary where relevant;
- user-level path;
- a disclosure affordance for any permitted path management.

Paths are shown once in this layer instead of repeated throughout the resource
controls. At narrow widths the tiles stack without horizontal scrolling.

### Layer 2: Agent resource panel

Add `AgentResourcePanel` with MCPs, Models, and Skills tabs. The panel represents
only the selected Agent:

- MCPs: dense installed-resource rows, enable state, add action;
- Models: current model, compatible choices, apply action, and link to Models;
- Skills: dense assigned-resource rows, assignment state, install action, and
  links to Skill details.

Dense rows absorb the scan efficiency of the high-density design direction.
Rows use identity, current-Agent state, scoped action, and disclosure. They do
not duplicate the full resource editor.

Selecting a row sends a typed navigation intent to the matching top-level view
and opens its Inspector. Extend the existing Skill navigation concept to MCPs
and Models rather than adding an unrelated routing mechanism.

Agent switches continue to call existing core-backed assignment or enable
operations. Operations that already require a plan or review keep that
requirement.

## Dialog system

Create a shared `DialogShell` responsible for:

- opaque backdrop and surface;
- header, scrollable body, and fixed footer geometry;
- focus trapping and focus restoration;
- topmost-only Escape handling;
- preventing dismissal while an irreversible commit is pending;
- responsive width and a minimum `16px` viewport inset.

The shell supports three task types.

### Editor dialogs

Used by MCP, Model, source, and Agent-path editing. They share field labels,
validation placement, advanced disclosure, status messaging, and `Cancel +
Save` footer semantics.

The MCP editor migrates from an app-covering edit page to this shell unless
implementation discovery proves that a field set cannot fit safely. The body
scrolls independently and advanced environment fields remain available without
making the default form dense.

### Picker dialogs

Used by Agent MCP assignment, Skill installation, and resource/source selection.
They share search, list rows, selection controls, result counts, empty results,
and `Cancel + Add/Install` actions. This replaces the narrow anchored Agent MCP
popover, which is vulnerable to constrained-window clipping.

### Review dialogs

Used by Skill plans, Model application, destructive deletion, and conflict
replacement. They show the exact operation, targets, affected files or fields,
risk level, backup behavior, and recovery implications.

Replace user-facing `window.confirm` calls in the scoped interfaces with this
review shell. Core confirmation requirements remain authoritative. A simple
delete review may be compact; a Skill plan may retain its existing multi-section
detail. Step indicators appear only for real multi-stage workflows such as Skill
installation.

Dialog action labels use specific verbs: `Save`, `Add`, `Install`, `Apply`,
`Update`, `Delete`, or `Remove`. Do not use generic `OK`, `Confirm`, or `Submit`.
Success toasts use the same verb as the initiating action.

## Component boundaries

The shared React layer contains presentation and interaction primitives only:

- `ResourceWorkspace`: geometry, category area, toolbar, tabs, Inspector layer;
- `ResourceCard`: four-slot card and keyboard behavior;
- `ResourceInspector`: shared header, sections, footer, focus behavior;
- `AgentResourcePanel`: tabs and current-Agent projection shell;
- `DialogShell`: focus, layering, responsive geometry, footer layout;
- small primitives for resource status, configuration strips, empty/error
  states, and dense Agent rows.

Domain views keep business derivation:

- `RegistryView`: origin ownership, effective/shadowed state, usage, MCP actions;
- `ModelsView`: protocol compatibility, credential presence, assignment;
- `SkillsView`: inventory state, risk, update, recovery, lifecycle plans;
- `AgentView`: selected Agent and domain-scoped adapters.

Do not create a shared resource business model or duplicate core decisions in a
presentation adapter. Shared components receive rendered slots or narrow
presentation props.

## Data flow

### Read path

`Rust core -> Tauri command -> domain hook/view -> derived filters and counts ->
shared presentation components`

No new cross-domain global store is required. Each top-level view owns its
query, category, status, selection, and async state. Typed navigation intents
may initialize those states when arriving from an Agent page.

### Write path

All mutations follow the same visible transaction:

1. Edit a local draft and run local validation.
2. Request a core plan when the domain requires one.
3. Review targets, risk, backup, and impact.
4. Call the existing core commit or safe-write operation.
5. Refresh only the affected domain and Agent data while preserving the user's
   current page, category, query, and selection when still valid.

Configuration writes do not use optimistic UI. Keychain secrets never enter
React state after saving; the UI receives only credential-presence state.

## Loading, empty, error, and recovery states

Use one state vocabulary across the scoped surfaces:

- `Loading`: render a stable workspace or dialog skeleton so empty state does
  not flash.
- `Empty`: explain what is absent and provide exactly one appropriate primary
  action.
- `No match`: retain the page structure and offer clear-search or reset-filter.
- `Read error`: show the failed domain, a concise reason, and a retry action.
- `Operation error`: keep the Inspector or dialog open, preserve the draft, and
  show the next corrective action.
- `Recovery required`: show a blocking banner, make scoped mutations read-only,
  and route into the existing recovery workflow.
- `Concurrent change`: preserve the draft, stop the write, explain that source
  data changed, and require refresh or re-review.

Pending state disables only controls that could conflict with the operation.
Navigation and read-only inspection remain available unless core recovery rules
require a broader lock.

## Accessibility and responsive behavior

- Preserve Enter and Space activation for cards and rows.
- Tabs implement roving focus with arrows, Home, and End.
- Opening an Inspector or dialog moves focus inside; closing restores the
  originating control when it still exists.
- Only the topmost modal layer handles Escape.
- Status is never expressed by color alone.
- Icon-only controls have accessible names and visible focus.
- Text truncation must not expose redacted values through `title` attributes.
- `prefers-reduced-motion` removes nonessential transforms and transitions.
- `1200x820` and `900x600` must have no horizontal application scrolling,
  clipped primary action, or dialog content overlap.
- Validate both light and dark themes because the application exposes a theme
  toggle.

## Verification strategy

### Pure and component tests

- resource card slot rendering and accessible activation;
- workspace tabs, counts, and keyboard behavior;
- Inspector focus entry, background inertness, mask close, and focus restore;
- topmost-only Escape with nested review dialogs;
- DialogShell pending-dismiss protection and responsive class contracts;
- Agent resource tabs and domain ordering;
- typed Agent-to-resource navigation for MCP, Model, and Skill;
- loading, empty, no-match, read-error, operation-error, and recovery states;
- sensitive-value redaction, including attributes and preview content.

MCP and Model views receive direct component coverage rather than relying on CSS
string tests alone. Existing Skills lifecycle tests remain the safety baseline.

### Build and core verification

Run checks proportional to each phase, expanding to the full contract before
handoff:

- Desktop unit and component tests;
- Desktop production build and icon checks;
- Tauri command and integration tests touched by navigation or dialogs;
- `cargo fmt --check` and `cargo test --workspace` when shared contracts change;
- existing Skill plan/commit/recovery integration tests unchanged and passing.

All test environments isolate `HOME` and `ARCA_HOME` and must not read real user
configuration, Skills, or Keychain.

### Installed-app review

Final UI acceptance uses only `/Applications/Arca.app` under the Arca UI review
playbook. Replacing the installed application requires separate explicit user
authorization.

At `1200x820` and `900x600`, verify:

- common toolbar, tabs, card anatomy, and domain order;
- three and two grid-column targets respectively;
- Inspector opening does not reflow the grid;
- Agent tabs remain visible and their primary actions do not clip;
- all three dialog types keep header and footer visible while body scrolls;
- keyboard focus, Escape layering, and reduced motion;
- light and dark theme contrast;
- no sensitive value appears in DOM, screenshot, tooltip, or logs.

The always-visible card-action removal is specifically reviewed with common MCP
copy and edit tasks. If the extra Inspector step is materially slower, introduce
one shared overflow action across all domains in a follow-up adjustment.

## Delivery phases

The approved scope is too broad for one reviewable implementation patch. Deliver
it as five sequential phases, each with its own implementation plan and
regression gate:

1. Shared primitives and geometry: tokens, `ResourceCard`, workspace cleanup,
   `DialogShell` foundation, and tests without domain behavior changes.
2. Top-level resources: migrate MCPs, Models, and Skills to the shared card and
   Inspector grammar.
3. Agent resource hub: configuration locations, tabs, dense rows, and typed
   cross-navigation.
4. Dialog migration: editor, picker, and review shells; remove scoped
   `window.confirm` and anchored picker inconsistencies.
5. Integrated installed-app review: responsive, theme, accessibility, safety,
   and the reversible card-action decision.

Each phase must leave the app buildable and preserve existing core behavior.
Behavior and visual changes should not be combined into one unreviewable commit.

## Acceptance criteria

- MCPs, Models, and Skills visibly share one workspace, card, Inspector, empty,
  and error-state grammar.
- Domain-specific information remains legible and is not reduced to generic
  labels.
- The resource order is MCPs, Models, Skills everywhere in scope.
- Agent management uses one tabbed resource panel and no longer requires the
  current three-section long scroll.
- Editor, picker, and review dialogs share geometry, focus, error, and action
  behavior.
- All writes continue through existing core authority and safety contracts.
- `1200x820` and `900x600` pass installed-app review without horizontal scroll,
  clipped primary actions, Inspector reflow, or dialog overlap.
- Sensitive values remain absent from UI artifacts.
- The change is delivered in independently testable phases with a documented
  fallback for card action discoverability.
