# CLI / TUI

`arca` is a native Rust CLI that shares `arca-core` and `~/.arca/` with the desktop app. MCPs, Models, and Skills all use the same model: central assets, Agent consumption relationships, and independently observed external state.

> Not installed yet? See [Installation · CLI](/en/guide/install#cli-tui-arca).

## Entry points

- `arca` opens the MCP terminal workspace for interactive catalog, source, and Agent relationship management.
- `arca <group> <command>` uses the scriptable asset and Agent commands.

Set `ARCA_NO_TUI=1` in scripts so an argument-free invocation prints help instead of entering the TUI.

## Unified command model

Use `arca mcp`, `arca model`, or `arca skill` to manage the corresponding
central library. Provider connections and catalogs live under `arca model provider`.

Use `arca agent list` to find an Agent ID and `arca agent status codex` to
see its MCPs, Models, and Skills together. To manage what an Agent uses, choose
`arca agent <mcp|model|skill>` and target it with `--agent`.

```text
arca mcp {list,show,add,save,delete,export,source,icon}
arca mcp source {list,subscribe,add-local,add-builtin,refresh,enable,disable,remove}
arca model {list,show,save,delete,import,provider,store,curl}
arca model provider {list,show,templates,catalog,save,delete,docs}
arca model store {list,save,check,delete,revisions,retire}
arca skill {list,show,inspect-source,install,import,update,remove,repair,check-updates}

arca agent {list,status,save,configure,enable,disable,run,launch,mcp,model,skill}
arca agent status <agent-id>
arca agent mcp {status,show,assign,unassign,enable,disable,enable-all,disable-all,converge}
arca agent model {status,assign,unassign,enable,disable,use,delivery,converge}
arca agent skill {status,assign,unassign,enable,disable,converge}
arca agent launch {show,configure,reset}
arca settings {show,terminal,locale,pins}
arca network proxy {show,set,clear}
arca status --agent <id> [--agent <id>] [--capability mcp|model|skill]
arca operation {apply,review} --file request.json
arca trace {list,show}
arca capture {list,show}
arca discover [mcp|model|skill]
arca workspace
arca upgrade
```

Existing scripts can still use the old relationship commands, such as
`arca model use PROFILE_ID --agent codex`. Both forms keep the same JSON output.

The three asset domains use the same command terminology:

| Command | Meaning |
|---|---|
| `list` / `show` | Query central assets |
| `status` | Compare desired and observed state, including ownership, enabled/current state, status, and available actions |
| `assign` | Add only the specified desired relationships for one Agent |
| `unassign` | Remove only the specified relationships without deleting central assets |
| `enable` / `disable` | Keep the relationship and change its desired enabled state |
| `converge` | Adopt, restore, or detach one exact observation |

Domain-specific commands preserve real differences: MCP has sources and exports; Model has Providers, credential delivery, and current selection; Skill has source inspection, installation, updates, and repair. Central asset authoring is available in both Desktop and the full CLI. Example IDs such as `work` refer to existing assets: obtain actual IDs from `list` or `status`.

## Batch reads and diagnosis

```bash
arca agent status codex --json
arca status --agent codex --agent opencode --capability mcp --json
arca agent launch show codex --json
arca trace list --agent codex --limit 20 --json
arca trace show SESSION_ID --event EVENT_ID --revision PAGE_REVISION --json
arca capture list --agent codex --json
arca capture show SESSION_ID --flow FLOW_ID --json
```

Trace pagination uses returned cursors; details bind the page revision. Capture `list/show` read saved records; `run` records an existing process or uses `--launch` to start a fresh instance with additional CA roots. `state` verifies owner and engine identities. Desktop and CLI share the Core workflow; see [network records](/en/guide/traces#network-records-and-proxy). JSON includes backend state and redacted diagnostics. Runtime detection is separate from configuration presence.

## Review and commit the original plan

```bash
arca operation review --file request.json --json
```

For example, `request.json` can enable an existing managed MCP:

```json
{
  "operation": "set_mcp_enabled",
  "request": {"agent_id": "opencode", "asset_key": "github::stdio", "enabled": true}
}
```

Keep this process alive. Its first NDJSON line has `phase: "review"`, a redacted `data.plan`, and `data.commit_decision`. After review, send the returned decision template as one JSON line to the same process's stdin, or send `{"action":"cancel"}`. EOF also cancels. The template binds the original operation ID, candidate hash, and any findings hash. Changed targets or mismatched decisions are rejected.

The final NDJSON line has `phase: "complete"`. Errors go to stderr as a JSON error document with nonzero exit. No-change operations complete immediately. `--file -`, `--yes`, `--dry-run`, and `--accept-risk` do not apply to the review session; stdin is reserved for the decision. Closing the process cancels it. Skill plans still expire after 24 hours.

For an already authorized operation, use `arca operation apply --file request.json --yes --json`. `--dry-run` previews and cancels a fresh plan; it is not a durable handoff token.

## Create Providers and Models

Save this as `provider.json`. Arca generates the ID when you create the record.

```json
{
  "name": "Work",
  "provider": "custom",
  "base_url": "https://api.example.com/v1",
  "protocols": {"openai-completions": {"endpoint_path": "/chat/completions"}},
  "auth_requirement": "required"
}
```

```bash
# Bash or Zsh, with WORK_API_KEY already available in the shell environment.
builtin printf '%s' "$WORK_API_KEY" |
  arca model provider save --file provider.json --store keychain --credential-stdin --yes
arca model provider list --json
arca model provider catalog PROVIDER_ID
```

Copy the generated Provider ID into `provider_id` in `model.json`:

```json
{
  "name": "Work model",
  "provider_id": "PASTE_PROVIDER_ID",
  "protocol": "openai-completions",
  "model": "your-model-id"
}
```

```bash
arca model save --file model.json --yes
arca model list --json
```

`provider catalog` lists models available from a saved Provider connection. `model list` lists the Model Profiles saved in Arca. The older `provider models` spelling and its JSON output still work.

For edits, pass the existing ID with `--id`. Both create and edit documents can omit the JSON `id` field. Use the redacted `provider show` output as a connection overview. Pass `--store` and `--credential-stdin` together to save a new key, with the JSON in a file and the key in a pipe. Omit both options to keep the existing credential. Switching stores requires a new key. Providers that allow unauthenticated access can use `--clear-credential` to remove a saved key. Explicit plaintext delivery requires `--confirm-plaintext` and a supported Agent.

## Credential stores

`keychain` is the built-in macOS Keychain store ID. For 1Password, grant a service account read and write access to the chosen vault and supply its token in the environment that runs Arca. Save the connection settings in `store.json` with your installed `op` path and the 26-character Vault ID. [Credential store setup](/en/guide/models#credential-stores)

```json
{
  "id": "work-vault",
  "name": "Work Vault",
  "backend": {
    "kind": "one-password",
    "command": "/opt/homebrew/bin/op",
    "vault_id": "REPLACE_WITH_VAULT_ID",
    "token_env": "OP_SERVICE_ACCOUNT_TOKEN"
  }
}
```

```bash
arca model store list
arca model store save --file store.json --yes
arca model store check work-vault
arca model store delete work-vault --dry-run
```

`check` verifies that the current process can access the vault. Once registered, choose it with `--store work-vault` when saving a Provider. Save the same ID and backend configuration with a new name to rename a connection. Update any Providers that still reference the connection before removing it.

Bitwarden and Vaultwarden share `backend.kind = "bitwarden"`. Set `command` to the `bw` path and include `server_url`, `appdata_dir`, `user_id`, `folder_id`, and `session_env`. Supply the unlocked session, then use the same `model store save` and `model store check` commands. [Bitwarden and Vaultwarden setup](/en/guide/models#bitwarden-and-vaultwarden)

Use the same save and check commands for the other backends. The JSON file contains connection metadata and environment variable names. [Setup for each store](/en/guide/models#credential-stores)

| `backend.kind` | Required fields | Optional fields |
| --- | --- | --- |
| `keepassxc` | `command`, `database_path`, `password_env` | `key_file` |
| `passbolt` | `command`, `server_url`, `user_id`, `private_key_path`, `server_fingerprint`, `password_env` | `folder_id` |
| `openbao` | `server_url`, `mount`, `prefix`, `token_env` | |

## Install and maintain Skills

```bash
arca skill inspect-source --github owner/repo
arca skill inspect-source --path /path/to/skills
arca skill inspect-source --archive /path/to/skills.zip
arca skill install --github owner/repo --name review-changes --dry-run
arca skill install --github owner/repo --name review-changes --yes
arca skill check-updates --yes
arca skill update review-changes --dry-run
arca skill repair review-changes --dry-run
arca skill remove review-changes --dry-run
```

Select exact names from source inspection. Installation creates the central copy; assignment is separate. Local replacement requires the applicable `--replace-local-changes` or `--replace-conflicts` intent. High-risk findings need `--accept-risk`; `--yes` alone does not accept risk. For multiple physical targets, enable/disable uses `--target <target-id>`.

## External changes and convergence

Agent files and user-level Skill directories are legitimate input sources. External additions, deletions, toggles, current-model changes, and field edits do not put Arca into a migration conflict. CLI status queries rescan immediately; Desktop refreshes from file-system events.

```bash
arca agent mcp converge github::stdio --agent claude-code adopt
arca agent mcp converge github::stdio --agent claude-code restore
arca agent mcp converge github::stdio --agent claude-code detach

arca agent model converge external-<candidate-id> --agent codex adopt
arca agent model converge work --agent codex restore
arca agent model converge work --agent codex detach

arca agent skill converge review-changes --agent codex adopt
arca agent skill converge review-changes --agent codex restore
arca agent skill converge review-changes --agent codex detach
```

| Action | Meaning |
|---|---|
| `adopt` | Accept the Agent's exact observed state into the central asset or desired state while preserving the Agent bytes |
| `restore` | Restore only the selected relationship to Arca's desired state |
| `detach` | Release Arca ownership; drifted or external content remains and is projected as external afterward |

Every convergence plan binds the full inventory revision at planning. A scoped status revision identifies its read result; it is not a substitute for the write plan revision. Core rescans after planning and verifies the candidate hash and target snapshot again at commit. A changed observation returns `observation_stale` instead of applying an old review to new content.

The old `reapply`, top-level `adopt`, and `migration review/resolve` routes are removed. The explicit convergence action is the intent; there is no second drift-confirmation token.

## Statuses

| Status | Meaning |
|---|---|
| `synced` | desired and observed state agree |
| `external-added` | present in the Agent but not managed by Arca |
| `external-changed` | managed fields, enabled state, or current Model changed externally |
| `external-removed` | desired relationship remains but its Agent target was removed |
| `unparseable` | one observed target cannot be parsed |
| `ambiguous` | one observed target has ambiguous identity or conflicting values |
| `unsupported` | the observed state cannot be adopted or represented losslessly |

The last three statuses isolate the affected asset. Ordinary external drift also affects only its relationship. A single MCP, Model, or Skill problem does not lock unrelated domains; only damaged shared settings or an unrecoverable incomplete transaction can make the whole workspace read-only.

In JSON status output, `capability_errors` means one capability is locally unavailable. `recovery_error` is reserved for a shared transaction recovery boundary. The former never blocks queries or mutations in other domains.

## Stable IDs and Agent selection

Mutations use exact stable IDs and never fuzzy-match display names:

| Domain | ID | Example |
|---|---|---|
| MCP | `name::transport` | `github::stdio` |
| Model | Profile ID or the external ID returned by `status` | `work` |
| Skill | Central Skill name | `review-changes` |
| Agent | Agent ID | `claude-code`, `codex` |

Relationship commands require one explicit `--agent <id>`. `assign` and `unassign` can process several exact asset IDs; `enable`, `disable`, `use`, and `converge` process one at a time.
MCP additionally supports `unassign --all`: after review, it clears every managed, disabled, and external MCP entry from that one Agent while leaving central MCP assets and every other Agent unchanged.

```bash
arca agent mcp assign github::stdio filesystem::stdio --agent claude-code
arca agent mcp unassign --all --agent qoder
arca agent skill unassign source-explainer --agent codex
arca agent model assign work backup --agent pi
arca agent model use work --agent pi
```

`github::stdio` and `github::http` are separate assets and must be named separately.

## MCP

```bash
arca mcp list
arca mcp show github::stdio
arca agent mcp status --agent claude-code
arca agent mcp assign github::stdio --agent claude-code
arca agent mcp disable github::stdio --agent claude-code
```

MCP-specific central operations:

```bash
arca mcp add github::stdio --command npx --arg -y --arg @example/server
arca mcp add docs::http --url https://mcp.example.com --http-type streamable-http
arca mcp delete github::stdio
arca mcp export
arca mcp export --out mcp.json --yes
```

`export --out` creates a new `0600` file and refuses to overwrite an existing target. A stdout export contains the complete MCP configuration by definition; other JSON projections remain redacted.

## Model

```bash
arca model list
arca model show work
arca agent model status --agent pi
arca agent model assign work backup --agent pi
arca agent model assign work --agent claude-code --replace
arca agent model disable backup --agent pi
arca agent model use work --agent pi
```

A multi-model Agent may retain several assigned Profiles but has at most one current Model. Capability validation keeps single-model Agents within their native constraints. Repeating `assign`, `enable`, or `use` is a desired-state no-op and never overwrites drift as a side effect; use exact `converge` when observed state must change.

External Models are projected by candidate identity rather than collapsed into “the current or first candidate.” Candidates that require unsafe credential conversion or have ambiguous identity remain `unsupported` or `ambiguous`; Arca does not guess.

## Skill

```bash
arca skill list
arca skill show review-changes
arca agent skill status --agent codex
arca agent skill assign review-changes source-explainer --agent codex
arca agent skill disable review-changes --agent codex
```

A Skill relationship links the central copy under `~/.arca/assets/skills/items/` into a verified user-level target. Several Agents may share one physical directory, so plans include the complete `affected_agent_ids`. `restore` rebuilds only a provably safe managed link; an external directory, regular file, or foreign symlink is never overwritten. `detach` preserves that external content.

## Read-only discovery

```bash
arca discover
arca discover mcp
arca discover model
arca discover skill
```

`discover` reports external observations and candidate details without creating ownership or editing Agent files. To change ownership, take the exact asset ID from `status` and run `converge`.

## Global options

| Option | Effect |
|---|---|
| `--json` | Emit a stable JSON envelope; mutations must also choose `--yes` or `--dry-run` |
| `--yes` | Skip the prompt without bypassing safety checks |
| `--dry-run` | Generate and display a plan, then cancel it without committing the requested mutation |
| `--no-color` | Disable ANSI colors |
| `--accept-risk` | Explicitly accept current high-risk Skill findings for a supported mutation |

`--yes` and `--dry-run` are mutually exclusive. Passing mutation-only options to a query is an error so automation cannot mistake an ignored flag for consent.

## Workspace and JSON

```bash
arca workspace
arca --json workspace
```

`workspace` returns one revision, Agent capabilities, central assets, desired relationships, observed inventory, and each row's `available_actions`. A success envelope looks like:

```json
{
  "schema_version": 1,
  "ok": true,
  "command": "model.status",
  "changed": false,
  "data": {}
}
```

Failures go to stderr with a stable `error.code` and redacted details. API keys, tokens, raw configurations, and private absolute paths are not included in normal status JSON.

## TUI keys

The argument-free TUI currently focuses on MCP:

| Key | Action |
|---|---|
| `1` / `2` / `3` | Registry / Sources / Agents |
| `↑` `↓` or `j` `k` | Move |
| `/` | Search |
| `i` / `a` | Install or add an MCP to an Agent |
| `Space` | Toggle a source, Agent, or MCP |
| `d` | Delete a central MCP or detach an Agent relationship |
| `Ctrl-R` | Rescan |
| `?` | Help |
| `q` | Quit |

TUI, CLI, and Desktop all invoke the same core planner; they do not implement separate write semantics.

## Updates

```bash
arca upgrade
```

A standalone or `cargo install` CLI can follow Stable with `arca upgrade`. The Desktop-bundled CLI updates with the app. Set `ARCA_NO_UPDATE_CHECK=1` to disable the daily version check after ordinary commands.

Next → [Supported agents](/en/guide/agents)
