# Agent Trace and network records

Trace presents supported Agents' local sessions, messages, and tool events. Network records present saved requests and responses. Both support inspection and debugging.

## Agent Trace

1. Open **Trace**, select an Agent and a local session.
2. Read the timeline and expand tool calls and results.
3. Inspect event details or import a supported local session file.

Sessions load by page and details bind to the page revision. If the file changes, read it again instead of applying old event positions to new content. Damaged, incomplete, or oversized input returns an error.

Native formats depend on the current adapters; a launchable Agent does not automatically have a Trace adapter. Imported third-party content is data, not an execution instruction.

## Network records and proxy

The **Network** menu exposes proxy configuration and records. Arca's proxy covers its own remote sources, GitHub Skills, and updates; it does not rewrite every Agent's system proxy.

Inspect saved requests, responses, and individual flows. CLI Capture commands read saved content without starting capture or treating the current CLI process as desktop capture-state authority. Actual capture follows the desktop environment setup.

## Read from another Agent

```bash
arca trace list --agent codex --limit 20 --json
arca trace show SESSION_ID --json
arca trace show SESSION_ID --event EVENT_ID --revision PAGE_REVISION --json
arca trace show --file /path/to/session.jsonl --json
arca capture list --agent codex --json
arca capture show SESSION_ID --limit 100 --json
arca capture show SESSION_ID --flow FLOW_ID --json
```

Continue with the returned cursor and use that page's revision for details. Capture's incremental cache avoids reparsing unchanged history.

## Content and redaction

Trace establishes credential-redaction context before returning content. Paging and individual detail reads follow the same rule. Context plus current input is bounded to 32 MiB / 100000 records; missing trusted context stops body output.

Session and network bodies can still contain code, business material, or personal information. Review actual content before sharing. The website demo uses public examples only. Importing a file does not upload it.

[CLI review and automation](/en/guide/cli#review-and-commit-the-original-plan)
