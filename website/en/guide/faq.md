# FAQ

## Do Desktop and CLI share data?

Yes. They share `~/.arca/`, assets, relationships, and write rules. The full CLI manages Models, Providers, MCPs, and Skills; the argument-free TUI focuses on MCPs. Assets do not need to be created twice.

## Why can an Agent launch without a configuration capability?

Launching, runtime versions, configuration presence, and writable capabilities are separate facts. Only verified paths and native formats are writable; other capabilities remain read-only or guided. An installed plugin host does not prove the plugin exists.

## Does Arca overwrite my existing settings?

It edits managed fields and preserves unrelated content, comments, and policies. External additions and edits remain observations. Invalid formats, concurrent changes, and unsafe overwrites stop the affected write. Choose an available adopt, restore, or detach action.

## Why did only some Agents synchronize?

Physical targets synchronize independently and completed targets stay complete. Inspect the failed relationship and repair that target; other Agents do not need to be redone.

## Disable, remove use, or delete?

Disable keeps the relationship and central asset. Remove use detaches one Agent and safely removes its managed target. Central deletion reviews all consumers and cleans the asset; Skill content moves to backup. External files are preserved.

## How are same-named MCPs or Skills handled?

MCP identity is `name::transport`; same-named stdio and HTTP entries are distinct. Central Skills use their name. Importing an existing central name retains consumers without automatically overwriting conflicting external copies. Details distinguish actual copies and content.

## Why does an Agent fail authentication when a key is in Keychain?

Check its supported delivery. An environment reference needs the variable in the launch environment; some clients require explicitly reviewed export to native private configuration. Verify the Provider, protocol, Model ID, and plan. Keep keys out of logs. See [Models](/en/guide/models).

## Project Skills, private repositories, or editing SKILL.md?

Arca manages verified user-level targets. Authenticated private Git sources and editing `SKILL.md` inside Arca are not supported. Import local folders and archives directly. See [Skills](/en/guide/skills).

## Does Trace upload or run content?

Trace reads local sessions or explicitly imported files. Preview does not upload or execute them. Review code and business content before sharing.

## How do updates work?

Check for a Stable update in Desktop settings. A standalone CLI uses `arca upgrade`; a bundled CLI updates with the app. Install the app in `/Applications` before updating instead of running it from a read-only DMG. See [Installation](/en/guide/install).

## Which platforms are released?

Official prebuilt Desktop and CLI artifacts currently target macOS Apple Silicon. Source compilation on another platform is not the same as completed platform validation.

[GitHub Issues](https://github.com/Scoheart/mux/issues) · [Demo](/en/guide/demo)
