# Command recipes

Use exact IDs from the installed CLI's JSON output. The asset IDs below are
examples of existing assets, not instructions to create IDs with those names.
Check `arca <command> --help` for version-specific options.

## Read current state

```bash
arca agent list --json
arca status --agent codex --capability mcp --json
arca mcp list --json
arca model list --json
arca skill list --json
arca discover model --json
```

Use `arca workspace --json` only when the full cross-domain inventory is needed.
Read-only commands do not accept `--yes`, `--dry-run` or `--accept-risk`.

## Application launch settings

For ChatGPT use `codex-desktop`; use `codex` for Codex CLI.

```bash
arca agent launch show codex-desktop --json
```

For a complete new/replacement ChatGPT launch configuration, create
`launch.json` containing the following non-secret example. Confirm the actual
application path and proxy port for the user's machine.

```json
{
  "kind": "app",
  "path": "/Applications/ChatGPT.app",
  "args": [],
  "env": {
    "HTTP_PROXY": "http://127.0.0.1:6789",
    "NO_PROXY": "localhost,127.0.0.1"
  },
  "new_instance": false
}
```

```bash
arca agent launch configure codex-desktop --file launch.json --dry-run --json
arca agent launch configure codex-desktop --file launch.json --yes --json
arca agent launch show codex-desktop --json
```

`args` is an array of individual arguments, not one shell command string. `env`
maps names to strings. `new_instance` corresponds to Desktop's new-instance
switch. Application environment support and argument handling depend on the app;
saving these fields does not guarantee that it honors them. An existing process
may need to exit before a new launch receives the settings. Do not quit a user's
running app without authorization.

This replaces the whole launch target, including args and env. Omitted fields
default to empty arrays/maps and `false`, so do not treat it as a partial patch.
The redacted `show` output cannot recover the previous hidden values. If needed,
obtain the complete intended configuration before replacing it. Use `--file -`
instead of a file when piping a JSON document over stdin is appropriate.

Other supported target shapes:

```json
{"kind":"cli","command":"/absolute/path/to/codex","args":[],"env":{}}
```

```json
{"kind":"web","url":"https://example.com"}
```

Choose a target appropriate to the Agent; do not turn ChatGPT into a CLI Agent.
CLI targets resolve installed executable names or absolute paths. CLI launches
use the selected terminal and require a working directory. App/terminal launching
currently requires macOS.

```bash
# Set only the CLI Agent's default directory, preserving its configured target.
arca agent launch configure codex --default-directory ~/Code --dry-run --json
arca agent launch configure codex --default-directory ~/Code --yes --json

# Only if the user requested a launch:
arca agent run codex-desktop --yes --json
arca agent run codex --directory ~/Code --yes --json

# Reset restores automatic detection AND clears the configured default directory.
arca agent launch reset codex-desktop --dry-run --json
arca agent launch reset codex-desktop --yes --json
```

Omitting `--default-directory` preserves it; passing `--default-directory ''`
clears it. Configure a known installed terminal using:

```bash
arca settings terminal --json
arca settings terminal ghostty --dry-run --json
arca settings terminal ghostty --yes --json
```

Use the exact installed terminal ID from the query; `ghostty` is an example.

## MCP assets and relationships

```bash
arca mcp show github::stdio --json
arca mcp status --agent codex --json
arca mcp assign github::stdio --agent codex --dry-run --json
arca mcp assign github::stdio --agent codex --yes --json
arca mcp disable github::stdio --agent codex --yes --json
arca mcp unassign github::stdio --agent codex --yes --json
arca mcp status --agent codex --json
```

For creating a central MCP, inspect `arca mcp add --help` or
`arca mcp save --help`. For example, an authorized new HTTP MCP:

```bash
arca mcp add docs::http --url https://mcp.example.com --http-type streamable-http --dry-run --json
arca mcp add docs::http --url https://mcp.example.com --http-type streamable-http --yes --json
```

Adding a central asset and assigning it to an Agent are separate operations.
Source catalogs use `arca mcp source --help`. `mcp export` includes full config;
`export --out` is a write that creates a private new file and rejects overwrites.

## Models and Providers

```bash
arca model provider templates --json
arca model provider list --json
arca model status --agent pi --json
arca model assign work --agent pi --dry-run --json
arca model assign work --agent pi --yes --json
arca model use work --agent pi --yes --json
arca model status --agent pi --json
```

For a new Provider, use a `provider.json` document like:

```json
{
  "id": "",
  "name": "Work",
  "provider": "custom",
  "base_url": "https://api.example.com/v1",
  "protocols": {"openai-completions": {"endpoint_path": "/chat/completions"}},
  "auth_requirement": "required",
  "api_key_source": {"kind": "env", "name": "WORK_API_KEY"}
}
```

```bash
arca model provider save --file provider.json --dry-run --json
arca model provider save --file provider.json --yes --json
arca model provider list --json
```

That document references a variable; it does not contain a secret. For Arca-managed
credentials use `{"kind":"arca-store"}` and `--credential-stdin` with a file
document. Read `arca model provider save --help` for the installed flow. Preserve
existing credentials unless the user specifically requests their removal.

Use the generated Provider ID to create `model.json`:

```json
{
  "id": "",
  "name": "Work model",
  "provider_id": "PASTE_EXACT_PROVIDER_ID",
  "protocol": "openai-completions",
  "model": "your-model-id"
}
```

```bash
arca model save --file model.json --dry-run --json
arca model save --file model.json --yes --json
arca model list --json
```

For creation leave `id` empty and omit `--id`; Core generates IDs. For editing,
specify the existing ID with `--id` and provide a complete intended document.
Provider `show` is redacted and cannot be round-tripped as an edit document.
Agent capabilities govern multi-model support and global current selection.

## Skills

Inspect exactly one source type and select candidate names returned by it:

```bash
arca skill inspect-source --path /absolute/path/to/skills --json
arca skill install --path /absolute/path/to/skills --name example-skill --dry-run --json
arca skill install --path /absolute/path/to/skills --name example-skill --yes --json
arca skill assign example-skill --agent codex --dry-run --json
arca skill assign example-skill --agent codex --yes --json
arca skill status --agent codex --json
```

Alternatively select `--github owner/repo` (a public source, optionally a GitHub
tree URL) or `--archive /path/to/skills.zip` instead of `--path`.

```bash
arca skill check-updates --yes --json
arca skill update example-skill --dry-run --json
arca skill repair example-skill --dry-run --json
arca skill remove example-skill --dry-run --json
```

`check-updates` writes the update cache, so it needs mutation confirmation.
Install/update plans can require `--accept-risk` after review and authorization;
never append it automatically. `--replace-conflicts` (install/import) and
`--replace-local-changes` (update) permit replacement and need an intentional
decision. Removing a central Skill also removes its managed assignments.

## Agent paths, preferences and proxy

For a requested Model configuration path change, `agent-patch.json` can contain:

```json
{"model":{"paths":["~/.codex/config.toml"]}}
```

```bash
arca agent configure codex --file agent-patch.json --dry-run --json
arca agent configure codex --file agent-patch.json --yes --json
arca settings show --json
arca network proxy show --json
arca network proxy set socks5://127.0.0.1:7890 --dry-run --json
arca network proxy set socks5://127.0.0.1:7890 --yes --json
arca network proxy show --json
```

`agent configure` accepts an `AgentConfigurationPatch`; this is distinct from
`agent launch configure`'s complete `LaunchTarget`. Arca's proxy URL must not
contain credentials and does not change Agent launch environment variables.

## External drift

After reading the exact observation and selecting the intended outcome:

```bash
arca mcp status --agent codex --json
arca mcp converge github::stdio --agent codex restore --dry-run --json
arca mcp converge github::stdio --agent codex restore --yes --json
arca mcp status --agent codex --json
```

Use `adopt` or `detach` instead of `restore` only when that matches the task.
Model and Skill domains use the same convergence verbs with their exact IDs.
On `observation_stale`, inspect the changed state before planning again.
