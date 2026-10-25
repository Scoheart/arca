# Agent launcher

The Agent picker and pinned-icon primary click continue to navigate Arca configuration. A compact split action in Agent details launches the runtime; pinned icons expose the same action in a context menu.

Agent details use one unframed workspace directly on the page, without an outer
background block, border or rounded shell. The icon sits above the centered name; credential,
edit and launch controls sit at the upper right. A single resource tab bar shares
one row with bulk actions, configuration and add controls. The current tab's paths
and documentation expand inline through the configuration button. Displayed home
paths use `~` while the opener receives the original target.

Resources use three columns, reducing to two on narrower desktop windows. MCPs
use short rows with name/transport beside the icon and fixed-width controls on
the right. Skills and Models separate the description from the footer; footer
controls occupy a nonshrinking grid column, so long text and shared-state badges
cannot push switches outside the card. External items retain their read-only and
convergence contracts. Model selection caveats remain visible. Preserve Arca's
existing palette: blue actions and selection, green enabled switches, and the
original semantic warning colors. The primary launch action is labeled briefly; missing runtimes offer
installation, and icon-only actions retain tooltips and accessible names.

Consumption panels render their existing controls into a shared toolbar slot,
keeping the same handlers and disabling conditions. The configuration section and
the resource list follow the same tab state. Changing Agent resets the open
configuration panel; changing resource tab updates its contents in place.

The configuration editor embeds the same controlled launch fields used by the
pinned-icon launcher. Launch-only edits call the existing launch-preference API.
Combined edits first review/commit capability paths, then persist launch settings.
No launch preference is written before review confirmation. If path commit
succeeds but launch save fails, the editor records the new path baseline, retains
the launch draft and reports the partial result; retry only saves the pending
launch settings. Configurable Agents expose launch settings through this editor
only; the workspace run menu keeps the CLI directory action. Installation docs are
a separate toolbar button before the credential selector. Desktop launchers with
no remaining run actions omit the dropdown arrow.

- Desktop / IDE: resolve an actual installed application and use the OS launcher, activating an existing instance.
- CLI: resolve an executable, select a working directory on first use, and launch in the globally selected terminal (Terminal, iTerm2, Ghostty, or Warp). Remember the per-Agent directory after successful dispatch; allow choosing another directory.
- Web: open the runtime website, not its documentation.
- Missing runtime: offer the existing official installation link. No automatic installation.
- Unmapped / custom Agent: configure an app path, executable plus separate arguments, or HTTP(S) URL. Saving these preferences never launches a process.

Core owns the built-in launch catalog, executable/application discovery, validation, persisted preferences and launch orchestration. Tauri commands run blocking work off the UI thread. The UI uses a shared launcher provider for detail and context-menu actions. Launch state does not mutate MCPs, Models, Skills or credentials. Agent detail shows an installed-version badge from the resolved launch target: `.app` versions come from `Info.plist`, while audited default CLI commands may run `--version` with a cleared environment, bounded output, and a two-second deadline. Custom CLI commands are not probed. The separate version query lets the launch action become ready without waiting for a CLI process.

Use exact process arguments for app/URL dispatch. CLI shell and AppleScript strings are encoded separately; commands, paths and arguments are treated as data. Runtime discovery only resolves paths; the limited version probe is the sole automatic CLI invocation. Never install packages during discovery. Terminal actions happen only on a user click.

The macOS bundle declares the Apple Events automation entitlement and a Terminal usage description. On first CLI launch, macOS may ask the user to allow Arca to control the selected terminal. A successful dispatch confirms that the launch request was sent, not that the Agent has authenticated or initialized successfully.

This iteration targets the shipped macOS desktop. Unsupported hosts report that explicitly. CLI entry points without verified metadata remain configurable rather than guessed. Existing installation probes supply declared command candidates; explicit data overrides distinguish IDE/desktop products and extension hosts.

Validation: user requested no test suites, formatters or preflight. Inspect the diff and compile the desktop for local delivery; do not launch third-party Agents as a test. No publication without a new release request and lane selection.

Devin web entry verified against https://docs.devin.ai/get-started/devin-intro (app.devin.ai).

Launch argument references: https://docs.augmentcode.com/cli/interactive (auggie); https://docs.openclaw.ai/cli/tui (openclaw tui).

Goose session entry: https://github.com/block/goose/blob/main/documentation/docs/quickstart.md (goose session).

## Workspace settings

The top bar has one Settings entry. Its dialog groups file editor and default CLI terminal, appearance/language/network, and rescan/update actions. Existing palette and update banners remain unchanged. Network configuration temporarily replaces the settings dialog and returns to it on close.

`settings.json` stores the global `cli_terminal` ID independently of each Agent’s launch target and working directory. The Core lists installed adapters, rejects unavailable choices, and resolves the preference on every CLI launch; missing applications do not silently fall back. Default remains macOS Terminal. App and web Agents are unaffected.

Terminal and iTerm2 use a fresh AppleScript session. Ghostty uses its native surface-configuration AppleScript API (1.3+); Warp opens a private, uniquely named executable `.command` handoff which removes itself before running the Agent. Commands retain shell-quoted arguments and working directories. Discovery and preference changes never execute Agent commands.

Adapter references: [Ghostty AppleScript](https://ghostty.org/docs/features/applescript), [Warp executable scripts](https://docs.warp.dev/terminal/more-features/files-and-links).

## Unified configuration and default directory

The Agent page no longer expands resource paths inline. Its Edit dialog owns MCP/Model/Skills paths, file/folder open actions, credential delivery, and launch settings. CLI launch settings include a per-Agent default working directory with a native folder chooser. A configured default takes priority over the last-used directory; an explicit one-off directory takes priority for that launch only. Clearing the default resumes the remembered-directory behavior. Core validates and stores the default together with the launch target. Editing only the directory preserves automatic target discovery.

## Application arguments

Application launch targets accept an optional argument array and a `new_instance` boolean, both backwards-compatible defaults. The editor keeps app arguments separate from CLI arguments and treats each non-empty line as one literal argument. Core validates argument count, size and NUL bytes, then uses `/usr/bin/open [-n] -a <app> [--args <arguments>...]` with process arguments rather than shell interpolation. New-instance mode is opt-in; the target application decides which arguments and multi-instance behavior it supports. Existing applications may only activate without consuming new arguments when new-instance mode is off.

## Launch environment

App and CLI targets accept an optional `env` string map (empty for older settings). The editor exposes editable name/value rows and rejects duplicates, invalid names, NUL values, and oversized values. Core repeats validation before save/launch. Values are literal, including spaces, quotes and equals signs. App launches pass each entry with `open --env NAME=VALUE` before `--args`; terminal CLI launches use shell-quoted command-scoped assignments. Existing parent environment is inherited for unspecified names; no system environment, shell startup file, or global terminal profile is changed. Ordinary launch variables are stored with launch preferences; Provider API credentials remain independently managed in Keychain.

For ZCode, select `/Applications/ZCode.app` and add `NODE_USE_SYSTEM_CA` with value `1`. The launch environment only applies to new processes; restart the App or request a new instance. Recognition of this Node setting depends on ZCode's bundled runtime.

The unified editor uses persistent Launch and Configuration Files tabs with a shared save footer. Switching tabs retains all drafts. Path labels sit above compact input/open-action controls; unsupported capabilities show a status row. Startup arguments use a smaller textarea and environment variables remain editable rows, reducing the default dialog height without removing settings.

## Named launch profiles

Each Agent supports 1–32 named launch profiles. Every profile independently stores its target, literal arguments, environment, new-instance option and default CLI working directory. Adding a profile copies the current draft; switching profiles retains all unsaved drafts, and deleting a custom profile selects the immutable default. The default profile is always empty: no target override, arguments, environment, new-instance option or default working directory. It cannot be edited, renamed or deleted; selecting it uses automatic runtime discovery. The shared Save action atomically saves all profiles and makes the selected profile the one used for subsequent launches. Cancel discards the whole draft.

Legacy single-target preferences are read as an editable custom profile alongside the empty default without writing a migration during discovery. Older mutable default profiles are likewise preserved as custom profiles. The first profile save stores the collection and active ID, and removes the superseded target/default-directory fields. Existing CLI configure commands update the active custom profile, or create a custom profile when the empty default is active. CLI reset selects the default while preserving custom profiles. Runtime discovery, launch, capture, network discovery and Instructions launch-context checks all use the active profile through Core. An editor revision excludes the remembered one-off CLI directory, but rejects concurrent changes to profiles or their active selection. The launch editor has no Restore Default button; choosing the immutable default is the only default-launch selection. Each launch still validates the selected runtime before dispatch.
