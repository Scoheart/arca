# Target-Scoped Startup Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent startup recovery from turning one Agent configuration incident into an operation-wide lock while safely retiring resolved legacy recovery evidence.

**Architecture:** Keep durable mutation-intent recovery as the file-safety authority, then classify MCP convergence independently per Agent without writing Agent files. Finalize the old operation only when every incident, claim, and lifecycle postcondition is verified.

**Tech Stack:** Rust, Arca core asset transactions, JSON/TOML Agent adapters, durable safe-write journals.

---

### Task 1: Reproduce the marked-operation short circuit

**Files:**
- Test: `core/src/assets/transaction.rs`

- [ ] **Step 1: Add a failing startup recovery test**

Create a committed desired MCP projection without its commit marker, add a target incident marker, and assert that startup recovery removes the resolved incident and staging directory.

- [ ] **Step 2: Verify the regression is red**

Run: `cargo test -p arca-core startup_recovery_finalizes_a_resolved_target_incident_marker -- --exact`

Expected: FAIL because the current marker branch immediately continues and leaves the operation root and incident intact.

### Task 2: Reconcile incidents per physical target

**Files:**
- Modify: `core/src/assets/transaction.rs`

- [ ] **Step 1: Add operation incident queries and cleanup**

Add helpers that determine whether an operation still owns incidents and remove only incidents whose `operation_id` matches the resolved operation.

- [ ] **Step 2: Classify MCP targets independently**

For every Agent in `DomainPlan::Mcp`, build its `before` and `after` sets and call `verify_agent_mcp_convergence`. Clear the matching target incident on success and record only that Agent on failure.

- [ ] **Step 3: Preserve already localized non-MCP incidents**

If a marked Model, Skill, or Agent-capability operation already has incident metadata, do not recreate incidents for every Agent. Use the existing plan-wide recorder only as a legacy fallback when no incident metadata exists.

### Task 3: Finalize fully converged legacy operations

**Files:**
- Modify: `core/src/assets/transaction.rs`

- [ ] **Step 1: Attempt safe claim reconciliation**

Load rollback snapshots, reconstruct their parent map, and call `recover_transaction_mutation_intents`. A failure keeps the operation unresolved and preserves evidence.

- [ ] **Step 2: Cross the durable completion boundary only after proof**

When the operation has no incidents, `ensure_no_transaction_mutation_intents` succeeds, and `verify_operation` succeeds, call `mark_operation_committed` and reuse `recover_pending_asset_operation` for credential and staging cleanup.

- [ ] **Step 3: Verify the focused behavior**

Run: `cargo test -p arca-core startup_recovery_finalizes_a_resolved_target_incident_marker -- --exact`

Expected: PASS, with the Agent config intact, the incident removed, and the operation staging directory absent.

### Task 4: Align the safety contract and deliver

**Files:**
- Modify: `AGENTS.md`
- Modify: `core/src/safe_write.rs`

- [ ] **Step 1: Document stable Agent file identity**

State that existing watched Agent configuration files use CAS-guarded in-place rewrites, while Arca-private files retain atomic replacement.

- [ ] **Step 2: Review and compile**

Run `git diff --check`, inspect the complete diff, and run the repository-required production build path. Do not expose real Agent configuration contents.

- [ ] **Step 3: Commit and publish**

Commit with `fix(core): localize startup target recovery`, push `main`, wait for Direct Stable, install the signed release, restart Arca, and verify that only the unresolved Qoder target remains incident-scoped.
