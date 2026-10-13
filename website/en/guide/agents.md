# Supported Agents

This searchable table comes from the released Arca registry and Core capability output. Search by name or exact ID and filter capabilities. Product forms retain separate identities; CLI, Desktop, IDE, and Plugin distinguish same-named entries.

<AgentReference lang="en" />

## What support means

MCP paths, Skill directories, and Model capabilities are verified independently. One capability does not imply another. An entry without a confirmed path and native format is not writable. Discovery-only candidates are a research backlog, not automatic configuration support.

- MCP shows verified user-level global files.
- Skills shows the primary user-level directory; compatibility read paths and shared targets appear in actual reviews.
- Models distinguishes managed configuration from official guidance. See [Models](/en/guide/models) for protocols, current selection, and credentials.

## Shared configuration and separate entries

**Codex CLI and its desktop entry** share Codex configuration and user-level Skills; Model writing is managed through the CLI identity. **Cursor IDE and CLI** share MCPs and Skills with separate launch entries.

**OpenCode CLI and Desktop** share native configuration and both manage Models. **Qoder IDE** has its own MCP file; **Qoder Desktop and CLI** share settings but select Models differently. **QoderWork** has independent user configuration. Use the exact identity rather than only the display name.

A shared file or directory review lists all affected Agents; Arca does not treat one physical target as several unrelated files.

## Installation and versions

Runtime detection is separate from an existing configuration file. macOS app versions come from bundle metadata; audited default CLI commands use bounded version queries. Custom CLI commands are not run automatically for detection.

A launchable plugin host does not prove the plugin is installed. Unconfirmed facts remain unknown. `arca agent launch show <agent-id> --json` returns the same launch and version information.

## Skills

Skill assignment uses verified user-level directories and targets available on the current machine. Agents sharing a directory form one impact group. Codex's primary directory is `~/.agents/skills`; another product's compatibility directory is not automatically a Codex write contract.

## Read-only entries and native guidance

Products without a stable user-level configuration contract remain read-only or guided. Pi's native MCP contract applies to 0.99.0+; earlier versions need their appropriate extension. Claude desktop's local MCP file accepts stdio; remote connections use its native Connectors.

See the repository's [Agent Catalog methodology](https://github.com/Scoheart/mux/blob/main/docs/agent-catalog.md).
