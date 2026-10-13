# Desktop app

This guide covers Arca: three central libraries, Agent launching and versions, local Trace, and network settings.

## Workspace overview


| Area | Purpose |
|---|---|
| Models / MCPs / Skills | Switch central libraries |
| Pinned Agent icons | Open frequent Agents; drag to reorder |
| Agent picker | Search CLI, Desktop, IDE, and Plugin entries |
| Trace | Browse local Agent sessions and tool events |
| Network / settings | Proxy, saved network records, appearance, and update preferences |
| Left navigation | Filter by Provider, MCP source, or Skill source |
| Main content | Search, add, inspect, and edit assets |

## Models

Add a Provider first: Base URL, credential source, account portal, and enabled protocol Endpoint Paths. Model-list URL and protocols appear directly in the form. The account portal is editable and opens through the adjacent button.

Then add a Model with its Provider, protocol, Model ID, and optional token settings. Select it in an Agent's Models tab. See [Models](/en/guide/models) for multi-model and per-conversation selection rules.

## MCPs


Add or paste a configuration, subscribe to a remote URL, or import a local file. Sources appear on the left; the main view shows names, transports, and commands or endpoints. Duplicate same-name, same-transport copies follow precedence while shadowed copies remain visible.

Adding a service does not run it. After assignment, the Agent loads it.

## Skills


The left navigation groups GitHub repositories, local folders, archives, and imports. Each Skill has a card with its name, up to three lines of description, and Agent icons.

Icons form a small fan that expands on hover or keyboard focus; clicking an icon opens the Agent. Details show the central content, actual copies, source, risk findings, and updates. See [Skills](/en/guide/skills).

## Agent workspace

Use the MCPs, Models, and Skills tabs to select existing central assets. Provider creation, Skill download, and new MCP entry forms belong in the libraries.

Launch controls show detected runtime and version information. CLI entries can use a chosen working directory and terminal; desktop apps launch through the system. **Edit configuration** manages launch settings and verified locations, while **Configuration** expands the selected capability's paths and docs.

The Agent picker uses a fan of cards with continuous page transitions and respects reduced-motion preferences.

## External changes and focused reviews

External additions, edits, and deletions remain observations. Choose an available adopt, restore, or detach action for the exact relationship. Reassigning does not silently overwrite external customization.

Ordinary additions, disabling, and low-risk updates report results directly. Deletion, local overwrites, and high-risk content use one focused review. Notifications enter from the upper right, show one at a time, and merge repeated messages.

## Trace and networking

Trace browses supported local sessions, event and tool details, or imported session files. Saved network records help inspect requests and responses. Arca proxy settings apply to its own network traffic. See [Trace and network records](/en/guide/traces).

[Watch the demo](/en/guide/demo) · [CLI automation](/en/guide/cli)
