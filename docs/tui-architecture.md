# Arca TUI — Architecture Design

Status: **historical design, implemented and superseded**. This document records
the original TUI design and is not a CLI reference. Use `arca --help`, the
README, or `website/*/guide/cli.md` for the current command contract.

The TUI remains the no-argument `arca` experience, wrapped around the same
`arca-core`; non-interactive resource operations now live under `arca mcp`,
`arca model`, and `arca skill`.

---

## 1. Goals & non-goals

**Goals**

- Full parity with the desktop app's operations, on the current catalog model
  (sources → aggregated registry; `name::transport` identity; provenance;
  enable/disable snapshots; per-agent installs).
- Zero business logic in the TUI. Every mutation goes through `arca-core`, so the
  TUI and desktop can never diverge — the same rule that motivated the whole
  Rust migration.
- Keyboard-first, fast, legible at 80×24, graceful up to full-screen.
- Testable core loop: state transitions are pure and unit-tested; rendering is
  snapshot-tested with ratatui's `TestBackend`.

**Non-goals (v1)**

- Mouse support (ratatui supports it; defer — keyboard-first).
- Project-scope installs and per-agent override patches. These exist in the
  command surface but have **no live desktop caller** either; the TUI mirrors the
  desktop and ships global-scope install only in v1. The core API keeps the
  parameters so it's a UI-only follow-up.
- A file browser widget for local-source import (v1 uses a path text field;
  a browse-widget is a later polish).

---

## 2. Phase 0 — complete `arca-core` first (prerequisite)

The TUI cannot call Tauri commands. Today a large amount of orchestration lives
**only** in `desktop/src-tauri/src/commands.rs`, not in `arca-core`. Building the
TUI against `arca-core` as-is would force re-implementing that logic in the CLI —
recreating the exact dual-implementation divergence we just eliminated.

So the first milestone is to **lift the remaining orchestration into `arca-core`**,
exactly as `ops.rs` already did for install/uninstall/import. Desktop commands
then become one-line delegators (as `apply_install`/`uninstall` already are), and
the TUI calls the same functions.

What moves, and where it lands:

| Desktop-only logic in `commands.rs` | New home in `arca-core` |
|---|---|
| `upsert_registry_entry` + `propagate_edit_to_installs` | `ops::upsert_entry(entry) -> Result<()>` (captures prev, writes manual source, propagates to clean installs) |
| `list_custom_registry_keys` | already thin over `registry::user_override_keys()` — expose directly |
| `import_pasted_config` + `extract_servers` | `ops::import_pasted(text) -> Result<Vec<String>>` |
| `list_sources` → `SourceView{…, server_count, error, managed}` | `sources::list_views() -> Vec<SourceView>` (move `SourceView` into core) |
| `subscribe_source` / `add_local_source` / `add_builtin_collection` | `sources::subscribe(url,name)`, `sources::add_local(path,name)`, `sources::add_official()` |
| `refresh_source` / `set_source_enabled` / `remove_source` | `sources::refresh(id)`, `sources::set_enabled(id,on)`, `sources::remove(id)` |
| `scan_installed` → `InstalledMcp{…, customized, enabled}` (merges disabled store) | `ops::scan_installed(project_dir) -> Vec<InstalledMcp>` (move `InstalledMcp` into core) |
| `disable_mcp` / `enable_mcp` / `delete_mcp` | `ops::disable(req)`, `ops::enable(req)`, `ops::delete(req)` |
| `put_agent` / `collapse_home` / `add_agent` / `update_agent` | `agents::put(id,def,overwrite)`, and expose `collapse_home` |
| `list_agents` → `AgentInfo` | `agents::list_infos() -> Vec<AgentInfo>` (move `AgentInfo` into core) |

`add_local_source_dialog` (native file picker) is desktop-only and *stays* in
desktop — the TUI substitutes a path text-field feeding `sources::add_local`.

**This phase ships with no UI change**: desktop tests stay green, desktop commands
just delegate. It's mechanical, test-covered, and independently valuable — it
finishes the core-consolidation the migration started. Only after it lands does
TUI screen work begin.

---

## 3. Where the TUI lives

Inside the existing `cli` crate as a module tree — one `arca` binary, no new crate:

```
cli/
  src/
    main.rs            # clap dispatch; no subcommand → tui::run()
    tui/
      mod.rs           # run(): terminal setup/teardown, the event loop
      model.rs         # Model (the whole state tree) + sub-states
      message.rs       # Msg enum (all input-derived + async-result events)
      update.rs        # update(&mut Model, Msg) -> Vec<Effect>   (pure, tested)
      effect.rs        # Effect enum + the worker runner (blocking core calls)
      view/            # pure render fns, one file per screen
        mod.rs         # view(&Model, &mut Frame); chrome + modal stacking
        registry.rs
        sources.rs
        agents.rs
        editor.rs
        modals.rs      # detail, paste, subscribe, confirm, install-targets, help
        widgets.rs     # shared: list w/ scroll indicators, multi-select, form field
        theme.rs       # colors, provenance/transport glyphs
        chrome.rs      # breathing border, logo shimmer (optional polish layer)
      keymap.rs        # key → Msg translation, per focus context
```

New dependencies (minimal, no async runtime):

- `ratatui` — rendering.
- `crossterm` — terminal backend + input (ratatui's default; already transitive).
- `fuzzy-matcher` (SkimMatcherV2) — replaces fuse.js for search. Optional; a
  plain case-insensitive substring filter is an acceptable v1 fallback.

No `tokio`. `arca-core` is synchronous (`ureq` blocking); the effect runner uses
plain `std::thread` + `mpsc`.

---

## 4. Architecture pattern — The Elm Architecture (TEA)

Four pure pieces plus one impure runner:

- **Model** — the entire application state. One struct, owned by the loop.
- **Msg** — every event that can change the model: a key press (already
  translated by the keymap), a tick, or the result of an async effect.
- **update(&mut Model, Msg) -> Vec<Effect>** — the only place state changes.
  Pure and synchronous: it mutates the model and returns side effects to run. No
  I/O here. This is what unit tests drive.
- **view(&Model, &mut Frame)** — pure render. Reads the model, draws widgets.
  Never mutates.
- **Effect + EffectRunner** — the impure edge. `update` returns `Effect`s
  describing I/O (network fetch, disk write via core); the runner executes them
  off the UI thread and posts a result `Msg` back.

Why TEA over immediate-mode component state: the app is multi-screen with modals,
async source fetches, and a "mutate → re-read → re-render" cycle. Centralized
state + pure transitions makes that cycle explicit and testable, and avoids
scattered `RefCell`/callback state that ratatui's immediate-mode invites.

The loop:

```rust
fn run() -> Result<()> {
    let mut term = setup_terminal()?;                 // raw mode, alt screen
    let (tx, rx) = mpsc::channel::<Msg>();
    spawn_input_thread(tx.clone());                   // crossterm events -> Msg::Key/Resize
    let runner = EffectRunner::new(tx.clone());       // worker(s) -> Msg::*Done
    let mut model = Model::new();
    for eff in update(&mut model, Msg::Init) {         // initial data load
        runner.spawn(eff);
    }
    while !model.should_quit {
        term.draw(|f| view(&model, f))?;
        // recv_timeout drives animation ticks when something is animating
        let msg = match rx.recv_timeout(model.tick_interval()) {
            Ok(m) => m,
            Err(Timeout) => Msg::Tick,
            Err(Disconnected) => break,
        };
        for eff in update(&mut model, msg) {
            runner.spawn(eff);
        }
    }
    restore_terminal(term)
}
```

A single `mpsc` channel multiplexes input-thread events and effect results, so
the loop blocks on exactly one `recv_timeout`. The timeout is the animation frame
interval when a spinner/shimmer is active, and effectively infinite (a long idle)
when nothing animates — so an idle TUI consumes zero CPU.

---

## 5. The effect model (async without a runtime)

```rust
enum Effect {
    LoadAll,                                  // read_registry + list_infos + scan + sources
    ReloadRegistry, ReloadSources, ReloadInstalled, ReloadAgents,
    Subscribe { url: String, name: Option<String> },   // network — slow
    RefreshSource { id: String },                      // network — slow
    AddLocal { path: String, name: Option<String> },
    SetSourceEnabled { id: String, on: bool },
    RemoveSource { id: String },
    UpsertEntry(RegistryEntry),
    DeleteEntry { name: String, transport: String },
    ImportPaste(String),
    Install(InstallRequest), Uninstall(InstallRequest),
    Enable(InstallRequest), Disable(InstallRequest), Delete(InstallRequest),
    PutAgent { id: String, def: AgentDefinition, overwrite: bool },
}
```

Each effect maps 1:1 to a `arca-core` call. The runner owns a small worker thread
(one is enough; a bounded pool if we later parallelize multi-source refresh) that
pulls effects off a queue, calls core, and sends a result `Msg` — e.g.
`Effect::Subscribe` → `Msg::SourceMutated(Result<(), String>)`. On success the
handler for that `Msg` enqueues the matching `Reload*` effect, so the model always
re-reads authoritative state from core after a mutation rather than optimistically
guessing (the desktop's optimistic-then-rescan trick is unnecessary here — local
re-reads are sub-millisecond).

Split of work by latency:

- **Network** (`Subscribe`, `RefreshSource`, `AddLocal` reading a remote-ish
  path): always on the worker; the triggering entry shows a spinner via
  `Model.inflight` until its result `Msg` arrives.
- **Local disk** (all reads, and writes like install/upsert/enable): fast enough
  to run on the worker too, uniformly — keeps `update` pure and the UI
  never-blocking even on a slow disk. Reads triggered by `Init`/reloads are also
  effects, so first paint shows a loading state and fills in as results land.

Enum effects (not boxed closures) keep everything `Send` trivially and make the
effect surface auditable and mockable in tests.

---

## 6. State tree (the Model)

```rust
struct Model {
    // ---- authoritative caches (filled by effects, re-read after mutations) ----
    registry:    Vec<RegistryEntry>,
    custom_keys: HashSet<String>,        // name::transport with a user override
    sources:     Vec<SourceView>,
    agents:      Vec<AgentInfo>,
    installed:   Vec<InstalledMcp>,      // includes disabled rows (enabled=false)
    loaded:      LoadFlags,              // which caches have arrived (loading UI)

    // ---- navigation ----
    screen: Screen,                      // Registry | Sources | Agents
    modals: Vec<Modal>,                  // stack; top captures input
    editor: Option<EditorState>,         // full-page editor (replaces screen body)

    // ---- per-screen UI state ----
    registry_ui: RegistryUi,             // query, origin_filter, cursor, scroll, focus
    sources_ui:  ListUi,                 // cursor
    agents_ui:   AgentsUi,               // selected agent id, cursor, add-popover state

    // ---- feedback / async ----
    inflight: HashSet<InflightId>,       // spinners (which source/cell is busy)
    toast:    Option<Toast>,             // ephemeral status: text, level, expires_at_tick

    // ---- animation ----
    tick: u64,
    should_quit: bool,
}

enum Screen { Registry, Sources, Agents }

enum Modal {
    Detail { key: String },                        // read-only entry view + copy
    Paste(TextArea),                               // paste JSON/TOML
    Subscribe(SubscribeForm),                      // url + name
    AddLocal(PathForm),                            // local-source path + name
    AddAgent(AgentForm),                           // create/edit agent def
    InstallTargets(InstallWizard),                 // scope step -> multi-agent step
    Confirm(ConfirmSpec),                          // destructive ops (y/n or 3-way)
    Help,                                          // keybinding cheatsheet overlay
}
```

`Screen` is the persistent body; `editor` and `modals` overlay it. Input routes
top-down: `Help`/topmost modal → `editor` if present → current `Screen`. Global
keys (quit, help, screen-switch) are checked first *unless* a text input has
focus (so you can type `q` / digits into a field).

The caches are the single source of truth for rendering; UI state is purely
cursor/scroll/focus/form-draft. After any mutation effect completes, the relevant
`Reload*` refills the cache and the view reflects reality — no manual cache
patching, no drift.

---

## 7. The core UX decision — apply model

Two lineages inform this:

- **Old TS TUI**: a *staged* model. You toggled a working `selections` set across
  a project/global/registry/agents matrix, it computed a `diff` against disk, and
  an explicit **review-and-apply** step (`Ctrl+S` → confirm screen listing
  `+add`/`-remove`) wrote everything at once.
- **Current desktop**: *immediate* per-action mutation. Toggling a cell installs/
  uninstalls right then; enable/disable/delete are instant; the UI re-scans.

**Decision (locked): immediate-apply, matching the desktop**, with a `Confirm`
modal gate on *destructive* actions only (delete catalog entry, remove source,
hard-delete an install). Rationale:

- The current model is source-based and toggle-centric (enable/disable snapshots,
  per-agent switches). Those are inherently immediate; a staging layer fights the
  data model.
- One mental model across both front-ends. The staged matrix was built for the
  old registry shape that no longer exists.
- Every write is already backed up (timestamped) by the applier, so "undo" is
  recoverable without a staging buffer.

What we **keep** from the staged TUI is its *feel*, not its buffering: the
multi-select idiom, the scope→targets wizard for installs, the diff-style
coloring (`+`/`-`), and the confirm step — just scoped to one action at a time.

---

## 8. Screens & routing — feature-parity map

Three persistent screens (switchable with `1`/`2`/`3` or `Tab`), a full-page
editor, and a modal stack. Every desktop capability maps to one of these.

### 8.1 Registry (`1`) — the catalog

- **Shows**: search box, count, origin-filter segmented row
  (`全部|订阅|本地|手动|探索`), and a scrolling list of entries. Each row:
  provenance glyph + name + `TransportPill` + origin tag + usage badge
  (`N agents` / 未使用, derived from `installed`) + endpoint summary.
- **Data**: `registry` filtered by `registry_ui.query` (fuzzy over name+desc+tags)
  and `origin_filter` (client-side `bucketOf`). Usage from `installed`.
- **Keys**: `↑↓` move · `/` focus search · `Enter` → Detail modal · `i` → install
  (opens InstallTargets wizard) · `e` → editor (edit) · `n` → editor (new) ·
  `d` → Confirm(delete entry) · `p` → Paste modal · `y` copy config JSON to
  clipboard · filter keys or a filter-focus toggle for the origin row.
- **Effects**: install → `Install`; edit/new save → `UpsertEntry`; delete →
  `DeleteEntry`; paste → `ImportPaste`.

### 8.2 Sources (`2`) — where the catalog comes from

- **Shows**: enabled-server total, and a list of source cards sorted
  remote → local → managed(manual/discovered). Each: name, kind badge, server
  count, enabled indicator, location (url/path/description), error line.
- **Keys**: `↑↓` move · `Space`/`Enter` toggle enabled (`SetSourceEnabled`) ·
  `r` refresh (`RefreshSource`, or read-only `LoadAll` re-detection for the
  discovered managed source; manual has none) · `d` → Confirm(remove) for non-managed
  (`RemoveSource`) · `s` → Subscribe modal · `l` → AddLocal modal · `o` subscribe
  official (Subscribe prefilled with the GitHub raw URL).
- **Managed rules**: `manual`/`discovered` hide delete; `manual` hides refresh;
  `discovered` refresh = re-scan only. It never adopts detected Agent state.

### 8.3 Agents (`3`) — per-agent install manager

- **Shows**: a left column list of agents (enabled state, warning glyph if no
  path) and, for the selected agent, its config-path card + an installed-MCP list
  (including dimmed disabled rows), sorted by name then transport.
- **Keys** (agent list focus): `↑↓` select agent · `Space` toggle agent
  enabled (`PutAgent` with flipped `enabled`) · `e` edit config path
  (AddAgent modal, edit mode) · `a` (top bar) add agent (AddAgent, create).
- **Keys** (install list focus, `→`/`Tab` to enter it): `↑↓` move · `Space`
  enable/disable the row (`Enable`/`Disable`) · `d` → Confirm(hard delete)
  (`Delete`) · `a` add MCP → a search popover of not-installed entries → pick →
  `Install` (scope global, this agent).
- **Empty/guard**: add-MCP disabled when the agent has no global path.

### 8.4 Editor (full page) — create / edit a catalog entry

Replaces the screen body (not a modal — it's a big form). Fields mirror the
desktop `RegistryEditPage`:

- name (editable only if new or custom), description, tags (comma), transport
  segmented (`stdio | http/sse`).
- stdio: command, args (one per line), env (KEY=value rows).
- http: type (presets `http|sse|streamable-http` + free text), url, headers rows.
- **Navigate-vs-edit** interaction (from the old TUI): `↑↓` move between fields;
  `Enter` enters inline edit; `Enter`/`Esc` leaves edit; the transport field
  toggles instead of editing and swaps the field set live. Inline validation
  (name required; command/url required per transport; `name::transport` collision
  guard) with clear-on-keystroke errors.
- `Ctrl+S` save (`UpsertEntry`, + `DeleteEntry` old key on rename); `r` revert to
  default for custom entries (`DeleteEntry`); `Esc` back.

### 8.5 Modals

| Modal | Purpose | Keys | Effect |
|---|---|---|---|
| Detail | read-only entry + pretty config JSON | `y` copy · `e` edit · `Esc` | — |
| Paste | textarea for JSON/TOML blob | `Ctrl+S` submit · `Esc` | `ImportPaste` |
| Subscribe | url + optional name | `Enter` submit · `Esc` | `Subscribe` |
| AddLocal | path + optional name | `Enter` submit · `Esc` | `AddLocal` |
| AddAgent | id/format/key/global/project | `Ctrl+S` · `Esc` | `PutAgent` |
| InstallTargets | scope step → multi-agent select | wizard (below) | `Install` |
| Confirm | destructive gate | `y`/`Enter`/`n`/`Esc` | the pending effect |
| Help | keybinding cheatsheet | any/`Esc` | — |

**InstallTargets wizard** (preserved from the old apply panel): step 1 pick scope
(`global` in v1; `project`/`both` reserved), step 2 multi-select agents
(`Space` toggle, `Ctrl+A` all, `Enter` apply if ≥1, `Esc` steps back then out).

---

## 9. Input handling & keymap

`keymap.rs` translates raw `crossterm` `KeyEvent` → `Msg`, parameterized by the
current *input context* (which the model derives): `TextInput` (a field/search
has focus) vs `Navigation`. In `TextInput` context, printable keys and editing
keys become `Msg::Input(char)` / `Msg::Backspace` etc., and only `Esc`/`Ctrl`-
combos are global; in `Navigation` context, letters are shortcuts.

Global (Navigation context): `1/2/3` screens · `Tab`/`Shift-Tab` cycle
screens/focus · `?` Help · `q` / `Ctrl-C` quit · `Esc` pop modal / leave editor.

Every screen renders a **context footer** (like the old TUI) listing its live
bindings, so the UI is self-documenting; `?` opens the full cheatsheet.

---

## 10. Rendering, layout, theme

- **Layout**: `ratatui::Layout` constraints. Adaptive list height from terminal
  rows, `[min,max]` clamped, with always-reserved scroll-indicator lines
  (`↑ N more` / `↓ N more`) so layout never jumps — ported from the old TUI's
  windowing. CJK double-width-aware truncation via `unicode-width` for
  descriptions/paths.
- **Shared widgets** (`widgets.rs`): `scroll_list` (windowed list + indicators +
  cursor `›`), `multi_select` (`◉/○`, Space/Ctrl-A), `form_field`
  (label + navigate/edit state), `pill`/`tag` renderers.
- **Theme** (`theme.rs`): the old color language — cyan=focus, green=success/add,
  red=danger/remove, yellow=warn/edit, magenta=global/custom, blue=cursor/target,
  dim=hints. Provenance glyphs (cloud/folder/hand/compass) and transport pills.
  Honors `NO_COLOR`.
- **Chrome/animation** — **deferred past v1 (decided)**. v1 ships plain: static
  borders, a static one-line wordmark, no shimmer/breathing. The color language,
  glyphs, and layout above are all v1; only the *animated* chrome (BreathingBorder,
  shimmer-gradient logo, traveling-shimmer modal border from the old TUI) is out.
  When revisited it lands in `chrome.rs`, driven by `Model.tick`, behind a
  `--plain`/`ARCA_PLAIN` escape hatch with a static fallback for small/headless
  terminals — so it stays purely additive and never blocks functionality.

---

## 11. Error handling & feedback

- **Toast/status line**: a single-line ephemeral message with level
  (info/success/error), auto-expiring on a `tick` deadline. Every effect result
  `Msg` sets it (green "✓ subscribed: N servers", red "✗ fetch failed: …").
- **Inflight spinners**: `Model.inflight` keyed by the busy entity (source id,
  install cell) drives a `dots` spinner on that row; the rest of the UI stays
  interactive during a network fetch.
- **Loading**: `LoadFlags` show per-cache skeletons on first paint until the
  initial `LoadAll` results arrive.
- **Panics**: a panic hook restores the terminal (leave alt-screen, disable raw
  mode) before printing, so a crash never wedges the user's shell.

---

## 12. Testing strategy

- **update() unit tests** (the bulk): construct a `Model`, feed a `Msg`, assert
  the new state and the returned `Vec<Effect>`. Covers navigation, focus, wizard
  steps, form validation, filter/search, and that each action emits the right
  effect. No terminal, no I/O.
- **Render snapshots**: `ratatui::backend::TestBackend` + a fixed-size buffer;
  assert the rendered `Buffer` for representative models (empty state, populated
  list, modal open). Catches layout regressions.
- **Effect boundary**: effects are an enum, so tests assert *which* effect is
  requested; the runner's actual core calls are already covered by `arca-core`'s
  own tests. A thin fake runner can feed canned result `Msg`s to test the
  post-mutation reload cycle.
- **No new integration tests against real `~/.arca`** — the Phase-0 core functions
  carry the behavioral tests (same `$HOME`-isolation gotcha applies).

---

## 13. Implementation phases

0. **Core completion** (§2) — lift orchestration into `arca-core`; desktop
   delegates; all existing tests stay green. *No UI yet.*
1. **Loop skeleton** — terminal setup/teardown, panic hook, event loop, Model,
   Msg, empty `update`/`view`, quit. Renders a static frame.
2. **Read-only browsing** — `LoadAll` effect + the three screens rendering caches
   (Registry list w/ search+filter, Sources list, Agents list). Detail modal.
   Navigation, focus, footers. No mutations.
3. **Install flow** — InstallTargets wizard + Agent-screen add/enable/disable/
   delete; Confirm modal. The core toggle loop end-to-end.
4. **Catalog editing** — full-page Editor (create/edit/revert), Paste modal.
5. **Sources management** — Subscribe/AddLocal/refresh/toggle/remove/official,
   with inflight spinners for network effects.
6. **Agents management** — AddAgent create/edit, config-path editing, enable.
   **This is the end of v1** (full functional parity, plain styling).

Post-v1 (deferred, decided out of v1): animated chrome (breathing border,
logo/modal shimmer), `--plain` toggle, and any small-terminal animation
fallbacks. The help cheatsheet and static theming ship within phases 2–6.

Each phase is independently shippable; `arca` prints help until phase 2 wires the
no-arg launch (so we can merge incrementally without exposing a half-built TUI —
gate it behind `ARCA_TUI=1` until phase 3, then make it the default).

---

## 14. Decisions (locked)

1. **Apply model** — **immediate-apply with confirm-on-destructive** (§7). No
   staging buffer; effects fire per action and the model re-reads from core.
2. **v1 scope** — **full parity**. Phases 0–6: browse + install + editing +
   sources + agents, plain styling.
3. **Animated chrome** — **out of v1**. v1 is plain (static borders/wordmark);
   the breathing/shimmer aesthetic is a deferred, purely-additive follow-up (§10).
</content>
