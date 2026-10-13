# Agent Trace

A read-only, local session viewer, independent from network capture and MCP / Model / Skill configuration.

## Navigation

Open **Trace** in the top bar, or use **Trace** on an Agent page to preselect its source. The workspace has three panes: sessions, conversation / tool events, and full native records. Unsupported Agents open an import-only filter rather than showing another Agent's sessions as their own.

- Session search: title, Agent name, project and file path.
- Event search: already-loaded previews, tool names and call IDs. It does not scan full raw bodies or unloaded history.
- Role filters: user, assistant, tool calls / results, and other events.
- Select a tool call to inspect its complete native JSON and its paired result.
- Copy or export the selected record and paired result; export is not a whole-session export.
- Refresh explicitly after the Agent appends to or rewrites a session.

## Sources and adapters

`data/trace-sources.json` owns discovery roots, aliases, extension filters and formats. Display names come from `data/agents.json` through Core, never a second frontend name table.

| Adapter | Default sources | Native handling |
| --- | --- | --- |
| Pi | `~/.pi/agent/sessions/` | Message content, toolCall / toolResult, compaction and branch summaries. Original entry IDs / parentId remain available; the timeline includes branches, not only the current context projection. |
| Codex CLI / ChatGPT | `~/.codex/sessions/`, `~/.codex/archived_sessions/` | response_item messages, function/custom tool calls and outputs. Mirrored event_msg messages and reasoning items are not double-counted. |
| Claude Code | `~/.claude/projects/` | user / assistant envelopes, tool_use and tool_result blocks, matched by tool_use_id. |
| Gemini CLI | `~/.gemini/tmp/**/session-*.json{,l}` | JSON messages or a journal with metadata, $set checkpoints and appended messages. Checkpoints replace messages instead of duplicating them. |
| Imported JSON / JSONL | Explicitly selected file | Known schemas are detected when identifiable. Unknown formats retain raw events without inferred tool semantics. |

Custom session roots, database-backed histories, remote hosts and cloud history APIs are not automatically discovered. Export a JSON / JSONL transcript from those Agents and import it. Generic format support is not a claim that every proprietary format can be normalized.

## Raw-data contract

Raw calls/results retain their native fields and nesting, including tool-result metadata, error flags and nested tool-call information when recorded by the Agent. There is no content summarization or silent truncation of the selected raw record. Conversational system / internal-reasoning fields are omitted, and common credential fields, bearer tokens, OAuth query secrets and explicitly identified passwords are masked **before IPC, copying or exporting**.

Redaction is conservative and is not a full privacy audit of arbitrary prose, files, screenshots or binary data. Raw previews are local React text nodes, not executable HTML. Do not share exported records without reviewing them.

Tools are paired by their native call ID, never by adjacency. For stream formats, pairing searches at most the following 1000 records / 32 MiB. A missing result explicitly states the bounded search scope; it is not proof that the Agent never produced a result. Opaque image content remains in raw JSON; there is no remote asset loading.

## Read and storage boundaries

- Core owns discovery, parsing, redaction, pagination, pairing and export. Tauri commands are thin blocking-worker/file-dialog adapters; React only presents records.
- Automatic discovery is bounded to eight directory levels, 5000 files and 25000 directory entries per source. File/entry limits are surfaced; deeper or custom locations require explicit import.
- JSONL reads 60 original records per page, even when some are omitted metadata. An empty page can therefore still have a continuation.
- A single JSONL record is limited to 8 MiB. Whole JSON / Gemini journal documents are limited to 32 MiB. Larger raw records are refused, not shortened.
- Pagination/detail requests bind the source length and modification revision. Changes require refresh rather than returning mixed snapshots.
- Unix reads anchor every path component with no-follow directory/file descriptors; symbolic-link records, non-regular files and path substitution are rejected.
- Agent files are opened read-only. No trace data is uploaded, no commands from logs are executed, and no configuration or Keychain entries are read or modified.
- Imported file references, discovered identities and password-redaction context remain in memory only, scoped to the current user directory.
- The detail cache holds at most six records in memory. No persistent trace cache or duplicate transcript is created under `~/.arca`.
- Export creates a new selected file with mode 0600 on Unix. Existing files, including transcripts, are never silently overwritten.

Regression fixtures in `core/tests/traces.rs` cover native adapters, exact ID pairing, password masking, source revisions, immutable source files, private export permissions and rejected symlinks. Frontend preview filters have independent tests in `desktop/src/lib/traces.test.ts`. Tests follow Arca's isolated HOME contract and only run when explicitly requested.
