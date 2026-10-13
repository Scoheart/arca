# Claude Desktop Direct Models Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let Arca safely create, select, observe, and clear one direct Claude Desktop third-party inference Profile, automatically disabling model-name verification only for non-Claude routes.

**Architecture:** Add a macOS-only Claude Desktop adapter under the authoritative Rust Model resource. The adapter owns a deterministic `Arca` config-library entry, reads the selected Provider credential from Keychain only during commit, writes the secret-bearing Profile as `0600`, and preserves/restores every external Claude Profile. This is direct Claude configuration; it does not add a local gateway, LaunchAgent, or background proxy. Existing Agent capability, consumption planning, inventory, React review, and remote PR/release paths remain the orchestration surfaces.

**Tech Stack:** Rust (`arca-core`, JSONC/serde, safe-write CAS, macOS Keychain), React 19 + TypeScript/Vitest, GitHub Git Data API, Arca Direct Stable.

---

## File map

- Create `core/src/resources/model/claude_desktop.rs`: Claude Desktop paths, route classification, lossless prepare/apply/observe/clear, credential export, permission tightening, and focused Rust tests.
- Modify `core/src/resources/model/mod.rs`: register the module and Claude Desktop capability; dispatch support, observation, apply, and clear to the adapter.
- Modify `core/src/assets/transaction.rs`: bind private-target discovery to the configured Claude Desktop fourth target and persist/restore its Keychain-backed rollback pre-state with crash-safe cleanup.
- Modify `core/src/safe_write.rs`: carry private transaction state with zeroizing/redacted values and hash/mode/identity-only durable write evidence and rollback guards.
- Modify `core/src/settings.rs`: persist the previously active external Claude Profile ID as typed operational state without mixing it into central Model assets.
- Modify `core/src/assets/planner.rs`: add the explicit plaintext-export review warning and ensure all four physical targets bind the plan hash.
- Modify `core/src/application/agents.rs`: retain the shared projection while testing the new capability is visible to every frontend.
- Modify `desktop/src/components/AssetOperationReviewDialog.tsx`: translate the new review warning into user-facing copy.
- Modify `desktop/src/components/AssetOperationReviewDialog.test.tsx`: cover the warning without adding a Claude-specific dialog.
- Modify `README.md`: list Claude Desktop among managed single-Profile Model Agents and disclose its credential-export exception.
- Retain `docs/superpowers/specs/2026-08-31-claude-desktop-direct-models-design.md` and this plan as delivery evidence.

## Task 1: Pure Claude Desktop projection

**Files:**
- Create: `core/src/resources/model/claude_desktop.rs`
- Modify: `core/src/resources/model/mod.rs`

- [ ] **Step 1: Write failing route and Profile projection tests**

Add `#[cfg(test)] mod tests` to the new module with isolated files under `TestHome`:

```rust
#[test]
fn non_claude_route_disables_verification() {
    let profile = anthropic_profile("qwen3.7-max", "Qwen 3.7 Max");
    let projected = projected_profile(&profile, "credential-for-test").unwrap();

    assert_eq!(projected["inferenceGatewayBaseUrl"], "https://max-ai.amap.com");
    assert_eq!(projected["modelDiscoveryEnabled"], false);
    assert_eq!(projected["unstableDisableModelVerification"], true);
    assert_eq!(projected["inferenceModels"][0]["name"], "qwen3.7-max");
    assert_eq!(projected["inferenceModels"][0]["labelOverride"], "Qwen 3.7 Max");
}

#[test]
fn claude_route_keeps_verification_enabled() {
    for route in ["claude-sonnet-4-6", "anthropic/claude-sonnet-4-6"] {
        let projected = projected_profile(
            &anthropic_profile(route, "Claude Sonnet 4.6"),
            "credential-for-test",
        )
        .unwrap();
        assert!(projected.get("unstableDisableModelVerification").is_none());
    }
}

#[test]
fn non_messages_endpoint_fails_closed() {
    let mut profile = anthropic_profile("qwen3.7-max", "Qwen");
    profile.endpoint_path = "/chat/completions".into();
    assert_eq!(
        projected_profile(&profile, "credential-for-test").unwrap_err(),
        "claude_desktop_endpoint_unsupported: expected an Anthropic /v1/messages endpoint"
    );
}
```

The test helper constructs a real `ModelProfile` with `protocol: ModelProtocol::AnthropicMessages`, `base_url: "https://max-ai.amap.com"`, and `endpoint_path: "/v1/messages"`.

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test --locked -p arca-core claude_desktop
```

Expected: compilation fails because `claude_desktop` and `projected_profile` do not exist.

- [ ] **Step 3: Implement the minimal pure projection API**

Create these constants and functions:

```rust
pub(crate) const AGENT_ID: &str = "claude-desktop";
pub(crate) const PROFILE_ID: &str = "6d757800-0000-4000-8000-000000000001";
pub(crate) const PROFILE_NAME: &str = "Arca";

pub(crate) fn route_is_claude(route: &str) -> bool {
    route.rsplit('/').next().is_some_and(|segment| {
        segment.to_ascii_lowercase().starts_with("claude-")
    })
}

fn gateway_base_url(profile: &ModelProfile) -> Result<String, String> {
    if profile.protocol != ModelProtocol::AnthropicMessages {
        return Err(
            "claude_desktop_endpoint_unsupported: expected an Anthropic /v1/messages endpoint"
                .into(),
        );
    }
    protocol_client_base_url(&profile.base_url, &profile.protocol, &profile.endpoint_path)
        .map_err(|_| {
            "claude_desktop_endpoint_unsupported: expected an Anthropic /v1/messages endpoint"
                .into()
        })
}

pub(crate) fn projected_profile(
    profile: &ModelProfile,
    credential: &str,
) -> Result<serde_json::Value, String> {
    let mut value = serde_json::json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": gateway_base_url(profile)?,
        "inferenceGatewayApiKey": credential,
        "inferenceGatewayAuthScheme": "bearer",
        "modelDiscoveryEnabled": false,
        "inferenceModels": [{
            "name": profile.model,
            "labelOverride": profile.name,
        }],
        "coworkEgressAllowedHosts": ["*"],
        "disableDeploymentModeChooser": true,
    });
    if !route_is_claude(&profile.model) {
        value["unstableDisableModelVerification"] = serde_json::Value::Bool(true);
    }
    Ok(value)
}
```

Expose the child module from `model/mod.rs` with `mod claude_desktop;`.

Delivered implementation hardens this initial sketch: production projection uses a typed
`Serialize` carrier and returns only `Zeroizing<String>` JSON. The `serde_json::Value` helper is
compiled for fake-credential tests only, so a real Provider key never enters a cloneable or
debug-printable value tree.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run `cargo test --locked -p arca-core claude_desktop`.

Expected: the three projection tests pass with no network or real Keychain access.

## Task 2: Lossless four-target prepare and restore

**Files:**
- Modify: `core/src/resources/model/claude_desktop.rs`
- Modify: `core/src/settings.rs`

- [ ] **Step 1: Write failing prepare/clear tests**

Add tests that create:

```text
~/Library/Application Support/Claude/claude_desktop_config.json
~/Library/Application Support/Claude-3p/claude_desktop_config.json
~/Library/Application Support/Claude-3p/configLibrary/_meta.json
```

Use unrelated fields and entries:

```json
{
  "appliedId": "00000000-0000-4000-8000-000000157210",
  "entries": [
    {"id":"default-id","name":"Default"},
    {"id":"00000000-0000-4000-8000-000000157210","name":"CC Switch"}
  ],
  "future": {"keep": true}
}
```

Assertions:

```rust
assert_eq!(prepared.previous_applied_id.as_deref(), Some(CC_SWITCH_ID));
assert_eq!(prepared.files.len(), 4);
assert_eq!(json(&prepared.files[0])["mcpServers"]["filesystem"]["command"], "npx");
assert_eq!(json(&prepared.meta())["future"]["keep"], true);
assert_eq!(json(&prepared.meta())["appliedId"], PROFILE_ID);
assert!(json(&prepared.meta())["entries"].as_array().unwrap().iter()
    .any(|entry| entry["id"] == PROFILE_ID && entry["name"] == PROFILE_NAME));

let cleared = prepare_clear(&paths, Some(CC_SWITCH_ID)).unwrap();
assert_eq!(json(&cleared.meta())["appliedId"], CC_SWITCH_ID);
assert!(json(&cleared.meta())["entries"].as_array().unwrap().iter()
    .all(|entry| entry["id"] != PROFILE_ID));
assert!(cleared.profile().content.is_none());
```

Add separate failure tests for malformed JSON, a UUID collision whose entry name is not `Arca`, a missing remembered Profile with no `Default`, and a changed target observed through CAS.

- [ ] **Step 2: Run the focused test and verify RED**

Run `cargo test --locked -p arca-core claude_desktop`.

Expected: failures identify missing `prepare_apply`, `prepare_clear`, and runtime-state types.

- [ ] **Step 3: Add typed previous-Profile operational state**

In `settings.rs` add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelAgentRuntimeState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_applied_profile_id: Option<String>,
}
```

and to `Settings`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub model_agent_runtime: Option<BTreeMap<String, ModelAgentRuntimeState>>,
```

Add helpers that read, set, and remove only the `claude-desktop` record while preserving unknown settings fields.

- [ ] **Step 4: Implement lossless preparation**

Define:

```rust
pub(crate) struct PreparedClaudeDesktop {
    pub files: Vec<PreparedModelFile>, // three ordinary, non-secret targets
    pub profile: PreparedClaudeDesktopPrivateFile, // Zeroizing, non-Debug/non-Clone
    pub previous_applied_id: Option<String>,
}

pub(crate) fn default_paths() -> Vec<String>;
pub(crate) fn prepare_apply(
    paths: &[PathBuf],
    profile: &ModelProfile,
    credential: &str,
) -> Result<PreparedClaudeDesktop, String>;
pub(crate) fn prepare_clear(
    paths: &[PathBuf],
    remembered: Option<&str>,
) -> Result<PreparedClaudeDesktop, String>;
```

Use `jsonc_parser`/serde JSON values to preserve unknown semantic fields. `prepare_apply` sets `deploymentMode` only, adds/reuses the exact Arca entry, selects it, and builds the private Arca profile. `prepare_clear` restores the remembered entry or the entry named `Default`, removes only the Arca entry, and returns `content: None` for the Arca-owned Profile file. A non-Arca UUID collision returns `claude_desktop_profile_collision`.

- [ ] **Step 5: Run the focused test and verify GREEN**

Run `cargo test --locked -p arca-core claude_desktop`.

Expected: preparation, preservation, restore fallback, collision, malformed JSON, and CAS tests pass.

## Task 3: Credential-aware commit and authoritative observation

**Files:**
- Modify: `core/src/resources/model/claude_desktop.rs`
- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/src/assets/transaction.rs`
- Modify: `core/src/safe_write.rs`

- [ ] **Step 1: Write failing commit, permission, observation, and clear tests**

Use `TestHome` and the existing isolated credential store. Save a Provider credential through the test Keychain path, then assert:

```rust
let result = apply_profile("claude-desktop", &profile.id).unwrap();
assert!(result.restart_required);
assert_eq!(result.files.len(), 4);
assert_eq!(profile_json["inferenceGatewayApiKey"], "credential-for-test");
#[cfg(unix)]
assert_eq!(fs::metadata(profile_path).unwrap().permissions().mode() & 0o777, 0o600);
assert_eq!(observe_profile("claude-desktop", &profile).unwrap(), ModelObservedState::Synced);

clear_profile("claude-desktop", &profile.id).unwrap();
assert!(!profile_path.exists());
assert_eq!(meta_json["appliedId"], CC_SWITCH_ID);
assert!(meta_json["entries"].as_array().unwrap().iter()
    .any(|entry| entry["id"] == CC_SWITCH_ID));
```

Also assert that missing/non-UTF-8 credentials return stable error codes and never include credential bytes in the error.

- [ ] **Step 2: Run the focused test and verify RED**

Run `cargo test --locked -p arca-core claude_desktop`.

Expected: the capability is unsupported or the dispatch branch is missing.

- [ ] **Step 3: Implement the Claude Desktop transaction**

In the adapter, add:

```rust
pub(crate) fn apply(
    paths: &[PathBuf],
    profile: &ModelProfile,
    credential: zeroize::Zeroizing<Vec<u8>>,
) -> Result<ModelApplyResult, ModelTargetError>;
pub(crate) fn clear(
    paths: &[PathBuf],
    remembered: Option<&str>,
) -> Result<(), ModelTargetError>;
pub(crate) fn observe(
    paths: &[PathBuf],
    profile: &ModelProfile,
) -> Result<ModelObservedState, String>;
pub(crate) fn observe_active(
    settings: &Settings,
    paths: &[PathBuf],
) -> ObservedActiveModel;
```

Commit the two ordinary Claude JSON files and `_meta.json` with `write_if_unchanged` so existing regular files preserve inode. Commit the configured fourth target—the Arca Profile—with `write_private_if_unchanged` so it is atomically published as `0600`; do not restrict this to the default literal path. Do not call `backup_config` for the secret-bearing Arca Profile. Roll back changed files in reverse order with the same normal/private writer distinction. The outer transaction extension in `transaction.rs` persists a nonsecret private-target ledger before Keychain pre-state, stores the private pre-state under an operation/path-bound Keychain subject, and cleans it on persist failure, cancel, success, rollback, startup/no-manifest recovery, and resolved incidents. Filesystem rollback manifests, mutation intents, and write evidence carry only path/mode/identity/version/hash/reference metadata; private Profile bytes never enter them.

Extend `safe_write.rs` so private carriers are `Zeroizing` and redacted in debug output, while durable `TransactionPathState` and write evidence remain hash-only. Hash/mode/identity CAS guards must refuse rollback after an external edit and retain evidence for recovery when ownership or cleanup cannot be proven.

Wrap `read_credential(profile_id)` in `Zeroizing<Vec<u8>>`, validate UTF-8 without logging it, and read it only inside the apply branch after operation confirmation.

Retain the focused `private_transaction` tests as delivery evidence: configured-path classifier isolation, hash-only snapshots and repeated writes/removals, new-target rollback, external-edit refusal, missing Keychain pre-state, no-manifest startup/cancel cleanup, terminal cleanup, persistence-failure cleanup, real Claude-plan recovery, overridden fourth-target startup cleanup, and resolved-incident cleanup.

- [ ] **Step 4: Register the capability and dispatch branches**

In `default_config_paths`, return `claude_desktop::default_paths()` for `claude-desktop`.

Add this `ModelAgentView` to `list_agents()`:

```rust
ModelAgentView {
    id: "claude-desktop".into(),
    name: "Claude Desktop".into(),
    mode: "managed".into(),
    storage_authority: ModelStorageAuthority::ArcaMapping,
    installed: agent_installed(&[], &[], &["/Applications/Claude.app"]),
    config_path: claude_desktop_path,
    config_paths: claude_desktop_paths,
    docs: "https://support.claude.com/".into(),
    assigned_profile: assignments.get("claude-desktop").cloned(),
    assigned_profiles: assigned_profiles("claude-desktop"),
    active_profile: assignments.get("claude-desktop").cloned(),
    supports_multiple: false,
    credential_mode: "keychain-export".into(),
    supported_protocols: vec![ModelProtocol::AnthropicMessages],
    note: "Exports the selected Provider credential into Claude Desktop's private third-party inference Profile; restart Claude Desktop after applying.".into(),
}
```

Add `claude-desktop` to `ensure_supported`, `observe_profile`, `observe_profile_consumption`, `observe_external_model`, `observe_active_model_for_settings`, `apply_profile_consumption_with_credential_presence`, and `clear_profile_consumption`. Leave clear-all on `ArcaMapping`, so the shared transaction removes only the one managed relationship and calls adapter clear.

- [ ] **Step 5: Run the focused test and verify GREEN**

Run `cargo test --locked -p arca-core claude_desktop`.

Expected: all Claude Desktop adapter and dispatch tests pass, including permission and secret-redaction assertions.

## Task 4: Planning warning and shared React review

**Files:**
- Modify: `core/src/assets/planner.rs`
- Modify: `core/src/application/agents.rs`
- Modify: `desktop/src/components/AssetOperationReviewDialog.tsx`
- Modify: `desktop/src/components/AssetOperationReviewDialog.test.tsx`

- [ ] **Step 1: Write failing core planning tests**

Add a planner test that adds an Anthropic Profile to Claude Desktop and asserts:

```rust
assert!(plan.warnings.contains(&
    "claude-desktop: model_credential_export_plaintext".to_string()
));
assert_eq!(plan.target_files, vec![
    "~/Library/Application Support/Claude/claude_desktop_config.json",
    "~/Library/Application Support/Claude-3p/claude_desktop_config.json",
    "~/Library/Application Support/Claude-3p/configLibrary/_meta.json",
    "~/Library/Application Support/Claude-3p/configLibrary/6d757800-0000-4000-8000-000000000001.json",
]);
assert!(!serde_json::to_string(&plan).unwrap().contains("credential-for-test"));
```

Add an application projection assertion that `list_agent_capabilities()` exposes `claude-desktop` with `arca-mapping`, `keychain-export`, one supported protocol, and `supports_multiple == false`.

- [ ] **Step 2: Run the focused Rust test and verify RED**

Run `cargo test --locked -p arca-core claude_desktop`.

Expected: warning and capability assertions fail before planner registration.

- [ ] **Step 3: Implement the planner warning and projection coverage**

When a Model domain plan adds or activates a Claude Desktop Profile, append exactly:

```rust
"claude-desktop: model_credential_export_plaintext"
```

Do not add the warning to unrelated Agents, removals, or no-op plans. Keep credentials out of the plan and warning text.

- [ ] **Step 4: Write the failing React warning test**

In `AssetOperationReviewDialog.test.tsx`, render a Model-add plan with that warning and assert:

```tsx
expect(screen.getByText(
  "Claude Desktop：将把所选 Provider 的 API Key 写入 Claude Desktop 的私有配置文件（权限 0600）",
)).toBeVisible();
expect(screen.queryByText("credential-for-test")).not.toBeInTheDocument();
```

- [ ] **Step 5: Run the focused React test and verify RED**

Run:

```bash
cd desktop
npm test -- AssetOperationReviewDialog.test.tsx
```

Expected: the raw warning code renders because `warningCopy` has no label yet.

- [ ] **Step 6: Add the shared warning copy and verify GREEN**

Add to `warningCopy`:

```ts
model_credential_export_plaintext:
  "将把所选 Provider 的 API Key 写入 Claude Desktop 的私有配置文件（权限 0600）",
```

Run the same focused Vitest command once. Expected: the review test passes without changing dialog structure.

## Task 5: Documentation, final verification, and remote-only delivery

**Files:**
- Modify: `README.md`
- Add/modify only the files listed in the File map.

- [ ] **Step 1: Update user-facing support boundaries**

Change the reusable Models summary so the single-Profile sentence names Claude Desktop alongside Claude Code and Codex. Add a short note that Claude Desktop accepts only Anthropic Messages-compatible Providers and exports the selected credential into its private third-party inference Profile.

- [ ] **Step 2: Inspect the exact prepared diff**

Run:

```bash
git diff --check
git status --short
git diff -- core/src/resources/model/claude_desktop.rs \
  core/src/resources/model/mod.rs \
  core/src/settings.rs \
  core/src/assets/planner.rs \
  core/src/application/agents.rs \
  desktop/src/components/AssetOperationReviewDialog.tsx \
  desktop/src/components/AssetOperationReviewDialog.test.tsx \
  README.md \
  docs/superpowers/specs/2026-08-31-claude-desktop-direct-models-design.md \
  docs/superpowers/plans/2026-08-31-claude-desktop-direct-models.md
```

Expected: only the intended Claude Desktop adapter, warning, docs, spec, and plan are present; no version, changelog, lockfile, generated output, secret, or unrelated file appears.

- [ ] **Step 3: Run one decisive final validation only if authorized**

The focused red/green tests above are the implementation evidence. Under Arca fast mode, do not repeat them and do not run full local suites. If final local validation is explicitly authorized, run exactly once:

```bash
bash /Users/scoheart/Code/ai/.agents/skills/arca-release/scripts/validate-change.sh \
  --repo /Users/scoheart/.config/superpowers/worktrees/arca/claude-desktop-direct-models \
  --base origin/main
```

- [ ] **Step 4: Create one remote feature commit and update PR #146**

Arca normally delivers through its direct-main policy. This change uses the PR #146 path because the user explicitly requested remote-only commit, PR, and merge delivery; keep the local checkout untouched while performing those remote operations.

Build an exact TSV manifest and use `gh-remote-commit.sh` against `codex/claude-desktop-direct-models`. Commit message:

```text
feat(models): support Claude Desktop direct models

Create a reversible Arca-owned inference Profile and opt non-Claude model routes out of Claude Desktop verification while keeping external Profiles intact.
```

Verify every remote blob SHA matches `git hash-object`, compare `main...branch`, and mark PR #146 ready only after the exact file list and mergeability are confirmed.

- [ ] **Step 5: Squash merge and verify Stable**

Squash merge PR #146 and delete its remote branch. Do not pull the result into the user's diverged local checkout. Locate the exact Direct Stable run for the merge SHA and wait until its immutable tag is recorded. Resolve the release tag from the post-merge `main` commit before invoking the verifier:

```bash
release_sha="$(gh api repos/Scoheart/arca/git/ref/heads/main --jq '.object.sha')"
stable_tag="$(gh api 'repos/Scoheart/arca/releases?per_page=20' --paginate \
  | jq -r --arg sha "$release_sha" '.[] | select(.target_commitish == $sha) | .tag_name' \
  | head -n 1)"
test -n "$stable_tag" && test "$stable_tag" != "null"
```

Then run the single-pass release verifier:

```bash
bash /Users/scoheart/Code/ai/.agents/skills/arca-release/scripts/verify-release.sh \
  --repo Scoheart/arca \
  --source-root /Users/scoheart/.config/superpowers/worktrees/arca/claude-desktop-direct-models \
  --wait \
  "$stable_tag"
```

Do not add `--install` because the user previously said they will update Arca themselves. Report the PR, merge SHA, release SHA/tag, Direct Stable/Desktop conclusions, four verified assets, skipped broad validation, and unchanged original checkout.
