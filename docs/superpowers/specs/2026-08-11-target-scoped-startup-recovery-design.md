# Target-Scoped Startup Recovery Design

## Problem

Arca already rewrites an existing Agent configuration file in place, preserving its path and inode. However, startup recovery still treats a durable `target-incident` marker as an operation-wide failure: it skips physical reconciliation and records incidents for every Agent named by the old operation.

The observed legacy operation affected Claude Code, Grok Build, and Qoder, while its only unresolved mutation claim belongs to `~/.qoder/mcp.json`. Restarting Arca therefore keeps three incidents alive even though only Qoder requires review.

## Required behavior

- Existing Agent configuration files remain present and retain their inode while Arca changes owned fields.
- A startup recovery marker never broadens an already localized incident to peer Agents.
- MCP recovery checks each Agent independently against the persisted desired set and current enabled state.
- A converged Agent has its incident removed without rewriting its configuration.
- A drifted Agent keeps one target-scoped incident; unrelated Agents and capabilities remain writable.
- Legacy claim evidence is retired only when the safe-write recovery proof succeeds.
- When every target and lifecycle postcondition is verified, Arca durably finalizes and removes the stale operation journal.
- Unknown or externally changed bytes are never silently overwritten during startup recovery.

## Design

Add a read-only target reconciliation phase for marked operations. For MCP plans, reuse the same per-Agent convergence verifier used after normal writes, but do not call the writer. This phase updates only incident metadata: successful Agent targets are cleared and failed targets are retained.

Before classification, attempt the existing semantic mutation-intent recovery using the operation's rollback parent snapshots. A failed recovery remains localized and keeps its evidence. If no incidents remain, no mutation intents remain, and the complete operation postcondition verifies, write the normal commit marker and run the existing committed-operation cleanup path.

For Model, Skill, and Agent-capability markers, preserve the incident set already recorded by the target-scoped runtime path. Only synthesize plan-wide fallback incidents when a legacy marker has no corresponding incident metadata at all.

## Safety

- Startup classification reads observed Agent state but does not restore Arca desired bytes.
- Settings changes remain under the existing settings lock.
- Operation and claim directories are accepted only through existing UUID, anchored-parent, and mutation-intent validation.
- Cleanup occurs only after both target incidents and mutation intents are empty and lifecycle verification succeeds.
