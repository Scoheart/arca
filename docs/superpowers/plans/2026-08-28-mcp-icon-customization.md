# MCP Icon Customization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace low-information MCP monograms with deterministic semantic icons while letting users select a built-in icon, upload a local image, or restore automatic behavior.

**Architecture:** Core owns `ui.mcp_icons`, validates asset keys and image payloads, and copies custom images into a private Arca-managed directory. Tauri exposes thin preference commands plus one native image picker. React owns the 18-icon catalog, deterministic inference, reusable MCP avatar, preference hook, and picker UI used by Registry and Agent views.

**Tech Stack:** Rust, serde, SHA-256, Tauri 2 asset protocol/dialog plugin, React 19, TypeScript, Vitest, Testing Library, CSS.

**Delivery constraint:** Prepare only in the isolated worktree, create one remote GitHub Data API commit, merge through a PR, and verify Direct Stable without installing or launching the App. Arca fast-delivery policy skips local tests/build unless the current user explicitly requests them.

---

### Task 1: Define the Core preference and import contract

**Files:**

- Modify: `core/src/settings.rs`
- Modify: `core/src/paths.rs`
- Modify: `core/src/safe_write.rs`
- Modify: `core/src/application/ui.rs`

- [ ] **Step 1: Add the persistent preference type**

Add to `core/src/settings.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpIconPreference {
    pub kind: String,
    pub value: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
```

Extend `UiSettings` with a defaulted, empty-skipping `BTreeMap<String, McpIconPreference>` named `mcp_icons`. Keep `UiSettings.extra` so unknown UI fields survive older/newer binary round-trips.

- [ ] **Step 2: Define the managed directory**

Add to `core/src/paths.rs`:

```rust
pub fn mcp_icons_dir() -> PathBuf {
    assets_dir().join("mcp-icons")
}
```

- [ ] **Step 3: Expose one safe content-addressed writer**

Add `ensure_private_file(path, bytes)` in `core/src/safe_write.rs`. It must create the parent with the existing durable private-directory helper, accept an existing regular file only when its bytes match, reject symlinks/non-files, and otherwise call the existing private no-replace writer.

- [ ] **Step 4: Add preference views and use cases**

In `core/src/application/ui.rs` define:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpIconPreferenceView {
    pub kind: String,
    pub value: String,
    pub path: Option<String>,
}
```

Implement:

```rust
pub fn list_mcp_icon_preferences() -> Result<BTreeMap<String, McpIconPreferenceView>, String>;
pub fn set_mcp_builtin_icon(asset_key: String, icon_id: String) -> Result<BTreeMap<String, McpIconPreferenceView>, String>;
pub fn import_mcp_icon(asset_key: String, source_path: PathBuf) -> Result<BTreeMap<String, McpIconPreferenceView>, String>;
pub fn reset_mcp_icon(asset_key: String) -> Result<BTreeMap<String, McpIconPreferenceView>, String>;
```

Validate current Registry membership via the existing MCP application catalog. Detect PNG/JPEG/WebP by magic bytes, reject files over `1_048_576` bytes, hash content with SHA-256, write `<hash>.<ext>` under `mcp_icons_dir`, and persist only the relative filename.

- [ ] **Step 5: Add Core tests**

In the existing `application::ui` test module add isolated `TestHome` tests that prove:

- built-in preferences round-trip and preserve unknown UI/top-level fields;
- a valid tiny PNG is copied under `assets/mcp-icons`, returned as an absolute path, and private on Unix;
- JPEG/WebP signatures are accepted;
- SVG, oversized input, unsafe built-in IDs, missing Registry keys and unsafe stored filenames are rejected or omitted without mutation;
- reset removes only the requested mapping and missing custom files fall back by being omitted from the view.

Do not execute tests locally under active fast-delivery policy.

### Task 2: Expose the Tauri and TypeScript boundary

**Files:**

- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `desktop/src-tauri/tauri.conf.json`
- Modify: `desktop/src/lib/types.ts`
- Modify: `desktop/src/lib/api.ts`

- [ ] **Step 1: Add thin commands**

Expose list, built-in select and reset commands that delegate directly to Core. Add an async `import_mcp_icon_dialog(app, asset_key)` command using the existing `spawn_blocking + DialogExt` pattern and filters `png`, `jpg`, `jpeg`, `webp`; cancellation returns `None` without writing so the picker remains open.

- [ ] **Step 2: Register commands**

Add all four commands to `tauri::generate_handler!` in `desktop/src-tauri/src/lib.rs`.

- [ ] **Step 3: Scope custom image loading**

Enable `app.security.assetProtocol` in `tauri.conf.json` with the sole scope `$HOME/.arca/assets/mcp-icons/**` so `convertFileSrc` cannot read unrelated user paths.

- [ ] **Step 4: Add the frontend wire types**

Add:

```ts
export interface McpIconPreference {
  kind: "builtin" | "custom";
  value: string;
  path?: string;
}
export type McpIconPreferences = Record<string, McpIconPreference>;
```

Add `listMcpIconPreferences`, `setMcpBuiltinIcon`, `importMcpIconDialog`, and `resetMcpIcon` API wrappers. Every mutation returns the complete updated map.

### Task 3: Build the reusable MCP icon system

**Files:**

- Create: `desktop/src/components/McpIcon.tsx`
- Create: `desktop/src/components/McpIcon.test.tsx`
- Create: `desktop/src/hooks/useMcpIconPreferences.ts`

- [ ] **Step 1: Define the 18-icon catalog**

Create `MCP_ICON_OPTIONS` with stable IDs `mcp`, `search`, `browser`, `document`, `knowledge`, `files`, `database`, `terminal`, `code`, `api`, `cloud`, `automation`, `observability`, `map`, `communication`, `media`, `security`, and `ai`. Render restrained local line SVGs using currentColor; do not add a third-party icon dependency.

- [ ] **Step 2: Add deterministic inference and fallback**

Build searchable text from name, description, tags, command/args or URL. Match an explicit ordered keyword table and return an icon ID only on a match. Otherwise derive a two-character uppercase monogram from meaningful name segments.

- [ ] **Step 3: Add `McpAvatar`**

Render custom image, selected built-in, inferred built-in, or monogram in that priority. Custom paths use `convertFileSrc`; an image load failure hides the image and reveals the automatic fallback. Use one low-saturation avatar frame with `data-icon-source` and `data-icon-tone` attributes instead of seeded rainbow backgrounds.

- [ ] **Step 4: Add the preference hook**

`useMcpIconPreferences` loads the complete map once, exposes busy/error state, and replaces the local map with each mutation result. It provides `selectBuiltin(assetKey, iconId)`, `upload(assetKey)`, `reset(assetKey)`, and `refresh()`.

- [ ] **Step 5: Add focused component tests**

Cover keyword inference, two-letter fallback, selected built-in precedence, custom asset URL conversion, failed-image fallback, stable catalog IDs and preference-hook mutation replacement. Keep Tauri functions mocked and do not execute locally under fast-delivery policy.

### Task 4: Add the picker and connect all MCP surfaces

**Files:**

- Create: `desktop/src/components/McpIconPickerDialog.tsx`
- Create: `desktop/src/components/McpIconPickerDialog.test.tsx`
- Modify: `desktop/src/components/RegistryView.tsx`
- Modify: `desktop/src/components/RegistryView.test.tsx`
- Modify: `desktop/src/components/RegistryEditPage.tsx`
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentView.test.tsx`
- Modify: `desktop/src/components/ConsumptionPickerDialog.tsx`
- Modify: `desktop/src/i18n/index.ts`
- Modify: `desktop/src/index.css`

- [ ] **Step 1: Build the picker dialog**

Use `DialogShell kind="picker" size="md"`. Show the current preview, the recommended icon, an accessible 18-button grid, upload action, inline error, close action and conditional restore-auto action. Disable mutations while busy and close only after a successful selection/upload/reset.

- [ ] **Step 2: Integrate Registry**

Load preferences in `RegistryView`; replace `ResourceKindIcon`/`Avatar` for MCP rows and Inspector with `McpAvatar`. Add a footer action labelled “图标” that opens the picker for the selected stable asset key. Apply returned preferences immediately without refreshing the MCP catalog.

- [ ] **Step 3: Integrate Agent consumption cards**

Load the same preferences in `AgentView` and use `McpAvatar` for managed MCP rows. Do not expose picker or icon writes from the Agent page.

- [ ] **Step 4: Localize and style**

Add Simplified Chinese and English copy for picker title/subtitle, current/recommended/all sections, upload, restore, errors and all 18 icon names. Add compact low-saturation avatar tones, picker preview and a responsive icon grid that fits `900×600` without horizontal overflow.

- [ ] **Step 5: Add integration tests**

Verify Registry cards and Inspector use the same stable-key preference, the footer opens the picker, built-in/upload/reset actions call the correct API and update the avatar, and Agent rows read but cannot mutate preferences.

### Task 5: Audit and deliver remotely

**Files:**

- All files explicitly listed in Tasks 1–4
- Create: `docs/superpowers/specs/2026-08-28-mcp-icon-customization-design.md`
- Create: `docs/superpowers/plans/2026-08-28-mcp-icon-customization.md`

- [ ] **Step 1: Audit the exact diff**

Run `git diff --check`, inspect every changed/untracked path, verify the Tauri asset scope is limited to the managed icon directory, and confirm no version, changelog, lockfile, MCP config, Agent codec or credential surface changed. Do not run local tests/build under Arca fast-delivery policy.

- [ ] **Step 2: Commit and merge remotely**

Create remote branch `codex/mcp-icon-customization` from the current live `main`, commit the exact manifest through the GitHub Git Data API with `feat(mcp): add customizable asset icons`, compare every remote blob, create the PR, wait once for checks, inspect mergeability/files, then squash merge and delete the branch.

- [ ] **Step 3: Verify Direct Stable**

Track the merge-specific Direct Stable run, confirm release commit/tag/Draft identity and the main-scoped Desktop build, then run the release verifier once for DMG, CLI, updater and `latest.json` without `--install` or `--launch`.
