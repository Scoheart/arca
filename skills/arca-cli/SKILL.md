---
name: arca-cli
description: Use Arca's CLI to inspect and configure AI Agents, application launch paths, arguments and environment variables, MCP servers, model providers and profiles, Skills, and shared preferences. Trigger for Arca/arca CLI tasks, Agent launch configuration, or requests to manage these resources through Arca (Arca 命令行、启动配置、代理、MCP、模型和技能管理).
compatibility: Requires an installed arca CLI and shell access to the user's Arca environment. Application and terminal launching currently require macOS.
---

# Arca CLI

Arca Desktop and `arca` use the same Core and user workspace (`~/.arca/` by
default). Use the CLI for supported changes instead of editing Arca's private
files or an Agent's managed configuration directly.

## Start with the installed CLI

1. Run `command -v arca`, `arca --version`, and `arca --help`. If it is missing
   from PATH on macOS, check `~/.local/bin/arca`, the Desktop-provided entry.
   If unavailable, report the missing CLI; install or upgrade only when requested.
2. Confirm that commands run on the intended machine and user workspace. A cloud
   checkout cannot configure the user's Mac through its local filesystem. Preserve
   the intended environment; do not silently change `HOME` or `ARCA_HOME`.
3. Use `arca agent list --json` to resolve exact Agent IDs and capabilities.
   ChatGPT is `codex-desktop`; Codex CLI is `codex`. Display names are not IDs.
4. Read the relevant subcommand's `--help`. Installed help takes precedence if
   this skill and the installed CLI differ. Do not invent unsupported flags.

Never invoke bare `arca` in automation: it opens the interactive MCP TUI.
Prefer explicit commands with `--json`; `ARCA_NO_TUI=1` also disables that entry.

## Choose the task

Read [the command recipes](references/commands.md) for syntax and JSON examples.

| User intent | Entry point |
| --- | --- |
| Set an app path, launch arguments, environment variables or new-instance switch | `agent launch show/configure/reset` |
| Start an app or CLI Agent | `agent run` |
| Change where Arca reads an Agent's configuration | `agent configure` |
| Manage MCP assets and Agent assignments | `mcp list/show/status`, then the specific mutation |
| Manage Providers, Models and current selection | `model provider`, `model list/show/status` |
| Install, assign, update or remove Skills | `skill inspect-source/install`, then `skill assign` |
| Configure Arca's own network proxy | `network proxy show/set/clear` |
| Select the terminal, locale or pinned Agents | `settings` |
| Diagnose external changes | Scoped `status`, then explicit `converge` |
| Inspect saved conversations or traffic | `trace list/show` or `capture list/show` |

Arca's network proxy and an Agent's launch environment are separate settings.
For example, `network proxy set` does not set ChatGPT's `HTTP_PROXY`.

## Inspect, plan, apply, verify

1. Query current state with `--json`. Prefer scoped reads such as
   `arca mcp status --agent codex --json` over the entire `workspace`.
   Take asset IDs, candidate IDs, target IDs and available actions from output.
2. For a change, generate a plan with `--dry-run --json`, inspect its effects,
   and check every affected Agent or shared physical target. Some preference
   previews show intent only; full validation happens when applying.
3. Apply the authorized change with `--yes --json`. `--yes` and `--dry-run`
   are mutually exclusive; neither belongs on a read-only command. Existing
   task authorization is sufficient for changes within that scope. Clarify
   missing targets or broader destructive effects before proceeding.
4. Query the relevant status or preference again. Report what was saved and
   what remains pending; a successful launch request is not proof of an app's
   behavior or network connectivity.

Prefer one precise operation at a time. Do not automatically use bulk clears,
replacement flags, plaintext credential delivery or risk overrides to make an
error disappear.

## Resource semantics

- `assign` adds named Agent relationships; it does not remove other assignments.
- `unassign` removes a relationship; central asset deletion is a separate action.
  MCP/Model `unassign --all` can also clear external entries: it is a bulk clear.
- `enable`/`disable` preserve relationships. Model `use` changes current selection
  only where the Agent supports it; assignment alone does not imply current.
- MCP IDs include transport, such as `github::stdio` or `github::http`.
- Skills install into the central library first; assignment is separate. Only
  verified user-level directories are managed. Shared directories can affect
  several Agents. When status lists multiple targets, use the exact `--target`
  for Skill enable/disable operations.
- `discover` reads external configuration without importing it. Choose `adopt`
  (accept external state), `restore` (restore Arca state), or `detach` (release
  ownership and retain external content) from the user's intended outcome.
  Repeating `assign` does not reconcile external drift.

## Output, failure and credentials

- Successful JSON has `ok`, `command`, `changed`, and `data`. Also inspect
  backend warnings. Errors go to stderr with a nonzero exit and `error.code`.
- `pending_convergence` can mean central state was already saved. Read its
  `changed`, `operation_id` and `target_incidents`, then query status and recover
  only affected targets. Do not recreate assets or roll back successful targets.
- For `observation_stale`, re-read state and plan again. For unparseable,
  ambiguous or unsupported targets, report the specific limitation instead of
  manually overwriting their files.
- A Skill's `risk_confirmation_required` needs its findings reviewed and explicit
  authorization for that risk before using `--accept-risk`. `--yes` alone does
  not accept those findings. Replacement flags require intentional replacement.
- Keep credentials out of arguments, logs, committed files and pasted output.
  For Provider secrets use the supported `--credential-stdin` flow; it cannot
  share stdin with `--file -`. Ordinary status/show output is redacted and is
  not a complete editable document. MCP export can contain full configuration.
- Launch configuration replaces a complete target. `launch show` exposes argument
  counts and environment names, not values: do not reconstruct hidden values or
  silently erase existing settings when asked to change a single field.
- If another person or Agent must approve the exact plan, use
  `operation review --file request.json --json`. Keep that process alive, inspect
  its review event, and send the returned `data.commit_decision` template as one
  JSON line on the same stdin. EOF or `{"action":"cancel"}` cancels. This mode
  does not accept `--yes`, `--dry-run`, `--accept-risk` or `--file -`.

This skill grants no additional access. Start apps, record traffic, read
conversation content, or change credentials only within the user's task scope.
