# MiniMax Code Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add safe MiniMax Code MCP management and a credential-safe guided Models target to Arca.

**Architecture:** Reuse the audited Agent-definition pipeline for `~/.mavis/mcp.json` and the existing guided Model target pattern for `~/.mavis/config.yaml`. Do not add a MiniMax-specific writer because the existing explicit-type JSON codec matches the packaged runtime, while the model provider contract cannot consume Keychain credentials without plaintext persistence.

**Tech Stack:** Rust core tests and model target metadata, JSON Agent definitions, React/Vite icon assets, Markdown/VitePress documentation.

## Global Constraints

- Manage only the global MiniMax Code configuration.
- Preserve every configuration field outside the owned `mcpServers` subtree.
- Never write a model API key into `~/.mavis/config.yaml`.
- Use the official signed application bundle as configuration and icon evidence.
- Do not bump versions, commit, push, tag, or release in this implementation task.

---

### Task 1: Lock the configuration contract with failing tests

**Files:**
- Create: `core/tests/fixtures/minimax-code.json`
- Modify: `core/tests/agent_formats.rs`
- Modify: `core/src/models.rs`

**Interfaces:**
- Consumes: `AgentDefinition` loading and `apply_to_agent` fixture helpers; `list_agents` and `ensure_supported` model APIs.
- Produces: regression expectations for Agent ID `minimax-code`, MCP key `mcpServers`, and a guided model target.

- [x] Add a JSON fixture containing a root metadata field, stdio/HTTP/SSE servers, an unknown per-server field, and a disabled server.
- [x] Add an Agent-format test that applies a resource and verifies all unrelated fixture content remains.
- [x] Add model tests expecting a guided `minimax-code` target at `~/.mavis/config.yaml`, three supported protocols, and rejected automated assignment.
- [x] Run the focused tests and confirm they fail because MiniMax Code is not registered.

### Task 2: Register MiniMax Code and its Models guidance

**Files:**
- Modify: `data/agents.json`
- Modify: `core/src/models.rs`

**Interfaces:**
- Consumes: existing `explicit_type` codec and `ModelAgentView` guided-target pattern.
- Produces: writable MCP Agent metadata and read-only Models guidance for `minimax-code`.

- [x] Add the audited Agent definition with `~/.mavis/mcp.json`, `mcpServers`, JSON map layout, and stdio plus URL-based HTTP/SSE entries.
- [x] Add installed detection for `mavis`, `.mavis`, and `/Applications/MiniMax Code.app`.
- [x] Add a guided Model target and a precise error explaining that the current provider flow persists `apiKey` in plaintext YAML.
- [x] Run the focused tests and confirm they pass.

### Task 3: Add the official application icon

**Files:**
- Create: `desktop/src/assets/agents/minimax-code.png`
- Modify: `desktop/src/assets/agents/SOURCES.md`
- Modify: `desktop/src/components/brandIcons.tsx`

**Interfaces:**
- Consumes: official `icon.icns` from MiniMax Code `3.0.51` and the existing `AgentGlyph` asset loader.
- Produces: a full-bleed `minimax-code` icon that satisfies `check:agent-icons`.

- [x] Convert the official application icon to a 256 px PNG.
- [x] Record the signed bundle and official download page as the source.
- [x] Register MiniMax Code metadata and full-bleed rendering.
- [x] Run `npm run check:agent-icons` and confirm the new writable target has an asset.

### Task 4: Correct supported-Agent documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/agent-catalog.md`
- Modify: `website/guide/agents.md`
- Modify: `website/en/guide/agents.md`
- Modify: `website/en/guide/desktop.md`
- Modify: `website/guide/desktop.md`
- Modify: `website/guide/models.md`

**Interfaces:**
- Consumes: generated counts after adding one audited writable Agent not present in the discovery catalog.
- Produces: consistent public counts and a MiniMax Code row with distinct main/MCP paths and guided Models status.

- [x] Recompute audited, writable, merged, and overlap counts from the data files.
- [x] Update every current count reference.
- [x] Add MiniMax Code to the supported-Agent tables and explain that Models is guided because credentials would otherwise be stored in plaintext.
- [x] Search for stale old counts and MiniMax naming drift.

### Task 5: Verify the complete change

**Files:**
- Review: all changed files

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces: fresh evidence that the implementation is buildable, tested, documented, and scoped.

- [x] Run `cargo fmt --check`.
- [x] Run `cargo test --workspace`.
- [x] Run `(cd desktop && npm run check:agent-icons)`.
- [x] Run `(cd desktop && npm run build)`.
- [x] Run `(cd website && npm run build)`.
- [x] Inspect `git diff --check`, `git diff --stat`, and `git status --short`.
- [x] Detach the read-only MiniMax Code image and remove temporary downloaded/extracted artifacts.
