# OpenHands CLI and SDK resource boundaries

The audited [OpenHands CLI 1.16.0 distribution](https://pypi.org/project/openhands/1.16.0/) pins [SDK 1.21.0](https://pypi.org/project/openhands-sdk/1.21.0/), tools 1.21.0 and workspace 1.11.1. All four complete wheels matched their official SHA-256 values; 497 RECORD digests across 501 archive members were checked. Static analysis parsed 453 Python modules without importing them. A complete ordinary source distribution needs no native REA session.

The console entry is `openhands = openhands_cli.entrypoint:main`. CLI version metadata comes from the `openhands` distribution, independently of the SDK version. Before argument parsing, the entry imports stores/SDK code and reads cwd `.env`. Arca therefore keeps an unbound OpenHands version unknown and suppresses an active version subprocess. Its launcher can discover the official `openhands` command; discovery does not start it.

The audited CLI requires Python **3.12 exactly**. No compatible interpreter or installed CLI was found in the bounded local PATH/Homebrew probes. The wheels were acquired for static inspection; no incomplete CLI installation, target import, runtime upgrade or target execution was performed. Other virtual environments and future installations remain unknown. There is no positive installed-wrapper binding in this change.

## CLI wiring

The TUI/headless constructor (`openhands_cli/setup.py:147`), ACP local constructor (`acp_impl/agent/local_agent.py:185`) and ACP remote constructor (`remote_agent.py:248`) pass neither a `plugins` argument nor keyword forwarding. SDK `Conversation.__new__` defaults that argument to `None`; `LocalConversation._ensure_plugins_loaded` loads plugins only when its saved plugin list is nonempty. The four first-party wheels contain no call to `load_installed_plugins`.

This establishes a bounded distinction: the dependency contains a native plugin subsystem, while ordinary new CLI sessions do not automatically select its installed-plugin ledger. Arca adds no OpenHands CLI plugin host from that ledger. Existing sessions, arbitrary import-time customization and remote service defaults were not inspected.

Skills use a separate, connected path. `AgentStore._build_agent_context` loads project Skills and sets `load_user_skills=True` and `load_public_skills=True`. User candidates are read in order from `~/.agents/skills`, `~/.openhands/skills`, legacy microagents and the enabled installed-Skills registry. Explicit project Skills win over auto-loaded names; user auto-loaded Skills override public collisions. Public Skills use the OpenHands/extensions repository and a separate Skills cache. Its marketplace filtering does not invoke `Plugin.load` or establish plugin activation.

CLI MCP configuration uses `<OPENHANDS_PERSISTENCE_DIR or ~/.openhands>/mcp.json`; ACP-provided definitions override matching names. The SDK's home-based Skills, hooks and plugin paths do not move with that CLI persistence override. Hook lookup selects the first existing working-directory `.openhands/hooks.json`, then the user file, without merging or falling back from a malformed selected file. Arca's existing global MCP/Skills writers are unchanged.

## SDK format and scanner limits

The SDK loader tries `.plugin/plugin.json`, then `.claude-plugin/plugin.json`; it does not query root `plugin.json`. Without a manifest it infers a directory name and version `1.0.0`. Its component loaders use fixed Skills, hooks, `.mcp.json`, agents and commands paths. Accepting extra manifest fields does not establish that component-path overrides are consumed or that the format fully conforms to Agent Plugins v1.

The SDK installed-plugin ledger lives below `~/.openhands/plugins/installed`; installed Skills have their own root and ledger. Native list/load helpers reconcile records and save metadata when their session exits normally. A read-only observer must parse confined metadata itself instead of calling those helpers. Ledger keys and `InstallationInfo.name` follow different validation paths; ambiguous names, unsafe paths and symlinks must remain diagnostics rather than arbitrary read targets. No actual user ledger, configuration, credential or component body was inspected.

ACP remote, cloud tasks, Docker-based serve and the web command's `uv run openhands` child can select separate servers or environments. Their effective plugins and policies remain unknown. No SDK format fact in this review proves that those contexts load the same resources. The isolated regression source exercises real launcher resolution and prevents a version lookup from executing a target bootstrap; it is retained unrun under fast delivery.
