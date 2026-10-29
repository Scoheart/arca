# Hermes native plugin observations

Core observes Hermes metadata after binding the selected console entry to the reviewed editable installation. It never starts Hermes or Python, imports a module, calls `ep.load()`, runs a plugin/helper/hook, connects to MCP, or probes a version command. All reported enablement is unknown.

`data/native-plugin-hermes-runtime.json` records the complete normalized console-entry, editable `.pth`, finder and local `direct_url.json` templates, installed distribution metadata guards, and the union of 27 reviewed public source-file fingerprints. The source commit is NousResearch/hermes-agent `ff6a86cb529a372198b4b80d5e022e32a4a3f2cc`. A successful passive version label is the registered source declaration **0.8.0**; installed packaging metadata separately declares **0.7.0**. This does not establish future Python import resolution, interpreter behavior or the runtime dependency closure. Unknown binding always suppresses execution fallback.

The accepted installation shape is deliberately narrow: the reviewed Python 3.11 own-venv console entry, absolute selected-entry links, and source paths that can be represented safely in the exact templates. Changed, ambiguous, linked or unsupported metadata yields unknown. Core does not scan other `.pth` files or site hooks to resolve that uncertainty.

## Observed scopes

| Scope | Metadata shown | Limits |
| --- | --- | --- |
| Bundled memory providers | The eight source-directory identities in `data/native-plugin-hermes.json`: byterover, hindsight, holographic, honcho, mem0, openviking, retaindb and supermemory. Each has additional full-byte guards for its public `plugin.yaml` and `__init__.py`. | These are separate from general plugins. The directory name is the native identity. Availability and selected provider are unknown. Core does not call `discover_memory_providers`, which actually imports providers and calls availability code. |
| User native plugins | Immediate directories under the current global-home candidate's `plugins/`, including hidden directories. `plugin.yaml` precedes `plugin.yml`; malformed or unsafe YAML never falls back. Only name, version and declared `provides_tools`/`provides_hooks` are projected. | `__init__.py` is checked only for a confined ordinary-file presence. Its body is unread. Missing/unsafe init and unsupported manifest shapes produce references and an unavailable state. Valid siblings remain visible. |
| Python distribution entry points | Static `[hermes_agent.plugins]` names in immediate `.dist-info/entry_points.txt` files under the bound venv. Physical registration source and conflicts are retained. | Target values, package code and unrelated distribution metadata are not exported. Supported target grammar is checked as inert text; imports, registration and availability remain unknown. Other installation forms are unsupported. |
| Global Skills | `SKILL.md` filename candidates under the selected global-home candidate's `skills/`, recursively within finite bounds, excluding `.git`, `.github` and `.hub`. | No Markdown/frontmatter body is read. Names are filename candidates. They appear in `@global/resources`, independent of any plugin. No implicit `~/.agents/skills` fallback is added. |

Distinct physical plugin directories or distribution registrations remain distinct even when declared names collide. A declaration named `mcp`, a `mcp.json`, a plugin-local `SKILL.md`, or a declared hook does not prove a native contribution of another resource type. Tool and hook entries explicitly describe unverified declarations and have no native asset ID. This observer emits no MCP servers; the existing MCP reader owns the separate `config.yaml:mcp_servers` path.

## Global-home candidate

Core reads only the bounded profile-name metadata at fixed `~/.hermes/active_profile`. A valid existing named profile overrides the current `HERMES_HOME`; empty/default sticky metadata preserves it. Invalid, oversized, linked or missing named-profile targets make that scope unavailable, without borrowing the default home. A present empty, relative or unexpanded environment value is unsupported; Core does not trim it, expand `~`, resolve it against cwd or silently use a default.

This is a candidate for the current observer context. Explicit CLI profile flags and later dotenv overrides can change Hermes' effective paths. Core does not read `.env`, disabled lists, account data or any other settings value. `config.yaml` is retained as a path reference and watcher target; configured external Skills directories therefore remain unknown. It does not scan cwd, projects or ancestors.

Gateway `hooks/HOOK.yaml` plus executable handlers are a separate native subsystem, and general `PluginContext` hooks and memory-provider hooks are also distinct. They are not manufactured as shared resources by this observer. Hermes' native YAML version field is not Agent Plugins v1 conformance evidence.

## Read and watch boundaries

Private metadata reads have finite file/aggregate limits, no-follow confinement and file-identity checks before, during and after reading. Each Hermes scan owns its budget, so earlier Agents cannot exhaust its allowance. The public runtime binding additionally rechecks the complete sequence's identities. Neither scanning nor watching changes native files, installs packages, enables plugins, or promotes caches/history into installations.

Watch paths use the same metadata scan and cover explicit registration/template/source members, the sticky profile file, settings path, selected manifests/init presence and narrow resource directories/files. They exclude the whole Hermes home, source root, venv site root, package roots, auth and session trees. Unsupported binding retains only minimal selected-entry candidates. New distribution registrations require a later inventory rescan; watching known entry-point files does not claim recursive coverage of every future package.

Regression source uses `TestHome` and covers native YAML priority, malformed shapes, physical collisions, init absence, links, profile/environment precedence, declaration boundaries, private-value exclusion, entry-point conflicts/shape failures, filename-only Skills, watcher confinement and cross-Agent budget independence. Fast delivery leaves suite execution to an explicit validation request.
