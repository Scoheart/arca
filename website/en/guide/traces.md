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

Open **Capture** from an Agent page. The workbench uses that Agent directly, with no second Agent selector, and shows only its processes and saved sessions. Choose a fresh instance or an existing process. A fresh supported runtime receives additional capture CA roots and capture binds to that instance. Select direct egress, the Arca proxy, or a custom HTTP proxy. Clients using the system certificate store still require system trust for the capture CA.

Desktop and CLI share Core's discovery, launch, forwarding, redaction, and stop workflow. `capture list/show` read saved records, `capture environment` inspects prerequisites, and `capture state` verifies process identities. `capture run` records in the foreground for a bounded duration and stops afterward; losing its owner also ends capture.

## Read from another Agent

```bash
arca trace list --agent codex --limit 20 --json
arca trace show SESSION_ID --json
arca trace show SESSION_ID --event EVENT_ID --revision PAGE_REVISION --json
arca trace show --file /path/to/session.jsonl --json
arca capture list --agent codex --json
arca capture show SESSION_ID --limit 100 --json
arca capture show SESSION_ID --flow FLOW_ID --json
arca capture environment --json
arca capture run --agent pi --target TARGET_ID --seconds 60 --json
arca capture run --agent pi --launch --directory /path/to/project --seconds 60 --json
arca capture state SESSION_ID --json
```

Continue with the returned cursor and use that page's revision for details. Capture's incremental cache avoids reparsing unchanged history.

On first capture, official mitmproxy installs and registers Mitmproxy Redirector. It may be absent from System Settings before registration. Approve the macOS installation and activation prompts; Arca waits up to three minutes and continues when the engine is ready. After registration, find it under System Settings → General → Login Items & Extensions → Network Extensions.

Existing processes cannot receive new CA roots at runtime, and proxy settings or existing connections may bypass capture. Prefer a fresh process after a TLS failure. An unresponsive launcher returns an error after 10 seconds; ordinary engine startup with an initialized extension retains its 12-second limit and preserves a redacted diagnosis on failure. Process capture currently requires macOS and official mitmproxy. System permissions still require user approval.

## Content and redaction

Trace establishes credential-redaction context before returning content. Paging and individual detail reads follow the same rule. Context plus current input is bounded to 32 MiB / 100000 records; missing trusted context stops body output.

Session and network bodies can still contain code, business material, or personal information. Review actual content before sharing. The website demo uses public examples only. Importing a file does not upload it.

[CLI review and automation](/en/guide/cli#review-and-commit-the-original-plan)
