# Native global instructions

Open an Agent and select **Instructions**. Arca shows the Agent's existing local global Prompt / Rules files, reads their complete source, and saves your explicit edits to the same file. Source and Markdown preview are separate views. The page uses the current Agent context without another Agent selector.

This feature is a native-file editor, independent of central MCP / Model / Skill assets. There is no central Prompt copy, asset assignment, generated managed block, delete, rename, rule toggle, collection import or cross-Agent synchronization. Shared paths refer to the same original file.

## Directory preferences

In **Agent → Edit → Configuration files**, configure the **Instructions** directories alongside MCP, Model and Skills paths. Type a directory or use the folder picker; each row shows the native filenames or rule patterns it covers. Fixed-file rows select the containing directory. Rule rows select the rule directory itself (for example `~/.claude/rules`), and mode-rule rows select the parent of the native `rules-*` directories.

Built-in native paths are defaults, not mandatory editor locations. Preferences are stored per Agent and stable native source ID in `agent_config_paths.<agent>.instruction_paths` in Arca settings. Restore default removes the override and resumes native environment/platform resolution. Opening configuration and saving directory preferences never create, copy, move, delete or rewrite Agent files; changing these editor locations does not configure the Agent runtime to load them.

Native `context.fileName` settings still determine fixed filenames; selecting an Instructions directory does not relocate the Agent's `settings.json`.

Discovery, sharing metadata, reads, edit plans and explicit missing-file initialization all use these same effective locations. Directory changes refresh the editor; an old draft remains available, while a pending write to a source no longer selected fails closed. Preferences have revision conflict protection independent of central asset projection. Incomplete native contracts remain read-only/guided even at custom locations.

## Existing files

- Fixed files, native rule-directory files and compatibility candidates remain separate. Native frontmatter, conditional activation, priority and file names stay under the original Agent's control.
- Codex's non-empty `AGENTS.override.md` has precedence over `AGENTS.md`; Arca displays both and initially selects a preferred candidate. An empty file remains an existing file.
- Existing `SYSTEM.md`, `APPEND_SYSTEM.md` or `SOUL.md` are labeled by their native role rather than treated as interchangeable guidance. They are never automatically generated as a replacement system prompt.
- Markdown previews do not execute HTML, commands, includes or remote images. Editing uses the full UTF-8 source, not a shortened preview. Files above the 1 MiB editor budget or unsupported encodings can be opened in the preferred external editor.
- Unsaved drafts stay in session memory when moving between Agent/resource pages. Refreshing or changing files requires resolving the current draft. Closing Arca with drafts requests a discard decision. Prompt contents never enter browser storage.

## Missing files

Only a complete scan with no existing supported fixed file, rule file or compatibility fallback can offer **Create global instructions**. Creation requires an explicit click followed by saving the content you enter. Viewing, refreshing and preparing an edit never initialize Agent files or folders.

Arca creates one verified native entry point, never a set of optional files. For Codex it creates ordinary `AGENTS.md`, not a higher-priority override. For supported context filename settings, it resolves the native configured name. A file appearing before commit blocks initialization; ordinary Save never turns into Create after an external deletion.

Unreadable paths, symlinks, incomplete scans, unknown roots or launcher profiles are not treated as absence. Products without a complete initialization contract remain guided rather than receiving guessed files. Cursor / TRAE / IBM Bob Markdown candidates are inspection-only pending a precise native format contract. OpenHands Skills are not flattened into a Prompt editor.

## Saving and recovery

Core owns discovery and opaque file references, preparation, and commit. Changes bind the full file bytes, mode, inode and parent identity. Concurrent edits or replacements preserve your draft and refuse an old-snapshot overwrite. Existing files are rewritten in place to preserve their inode and native watchers; original BOM and unchanged-line endings are retained.

Before changing an existing document, Core retains a private Keychain snapshot using its existing secure snapshot facility. Durable leaf-write claims are scoped to that document under `$ARCA_HOME/instruction-edits/`; they contain fingerprints and namespace evidence, not Prompt text. Interrupted or ambiguous edits block that document only, leaving the central asset capabilities available. Unknown namespace entries are never deleted to bypass recovery.

A successful save means the original local file was written. Native activation, selected backend, exclusions, project priority, capacity and reload timing still determine what a particular request receives. Arca reports new-session/native-reload guidance and does not call a saved file "runtime verified". Desktop/remote backends can use another home; custom launch contexts that cannot be safely resolved remain guided.

## Registry and boundaries

`data/instruction-files.json` is the native contract source. Agent names remain in `data/agents.json`. The registry records confirmed paths, directory patterns, native conditions and source links from the supplied October 16 audit. Support for reading, editing and initial creation is deliberately distinct; an audited identity is not a promise that every native backend or profile is writable.

`core/src/application/instructions.rs` owns the file workflow; Tauri commands only forward typed requests and the desktop supplies the editor. No Instruction variants are added to central `AssetRef`, consumption relationships or asset catalogs.

Regression fixtures cover read-only discovery, explicit initialization, no upsert on Save, concurrent changes, existing-rule/override preservation, BOM/newline retention and inode preservation. Validation uses an isolated HOME / ARCA_HOME; the repository's fast-delivery policy controls whether suites run.
