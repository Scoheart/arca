# Arca GitHub Delivery and Automated Release Implementation Plan

> **For implementers:** Execute tasks in order. Complete each task's verification and commit before starting the next task. GitHub Secrets, repository settings, Ruleset activation, tag creation, Release publication, and installed-app replacement require separate authorization at the step that performs them.

**Goal:** Protect `main`, keep one installable Pre-release for every ordinary main merge, maintain one rolling Release PR, and turn an approved Release PR into an atomic stable Arca release without manual version synchronization or tag creation.

**Architecture:** Introduce one repository version source and a checked version invariant; keep CI, Release Please, and desktop packaging as separate workflows with narrow permissions. Release Please owns version proposals, Changelog, the stable tag, and a Draft Release. The desktop workflow owns signed assets and is the only component allowed to publish that Draft. GitHub Rulesets make `verify` and an up-to-date PR mandatory before `main` changes.

**Tech Stack:** GitHub Actions, GitHub Rulesets, Release Please, Conventional Commits, Node.js 24 LTS, npm, Rust 2021, Cargo, React 19, Vite 7, Vitest 4, Tauri 2, macOS hosted runners, `gh` CLI.

## Global Constraints

- Desktop, CLI, Core, Tauri, npm, and both Cargo lockfiles remain one SemVer release unit.
- Configure exactly one Release Please component targeting `main`; never enable `separate-pull-requests`.
- Ordinary main merges create Pre-releases; a Release PR merge creates only the stable build and must not create a duplicate Pre-release.
- Stable assets stay in a Draft Release until DMG, CLI, updater payload, `latest.json`, signatures, labels, and the required `verify` result are complete.
- Never move or overwrite a stable tag. A code defect after tag creation requires a new patch release.
- Keep the existing updater endpoint and CLI compatibility filename.
- Preserve existing failure-Issue and Copilot repair behavior.
- Never expose `RELEASE_PLEASE_TOKEN`, `COPILOT_PAT`, or Tauri signing material in logs, fixtures, documentation, commits, or artifacts.
- Do not enable an Active Ruleset, publish a stable Release, enable Immutable Releases, push, or replace `/Applications/Arca.app` without explicit authorization.
- Do not cancel older ordinary main builds: the approved product contract requires a package attempt for each ordinary main merge.

---

## File Structure

| File | Responsibility |
|---|---|
| `version.txt` | Human-readable and Release Please-managed release version source |
| `.release-please-manifest.json` | Release Please's last released version state |
| `release-please-config.json` | Single-component SemVer, Changelog, Draft Release, and extra-file configuration |
| `CHANGELOG.md` | Release Please-maintained stable release notes |
| `scripts/release-version.mjs` | Check version agreement and refresh generated lockfiles |
| `scripts/release-version.test.mjs` | Version parsing, mismatch, and release-merge classification tests |
| `scripts/workflow-contract.test.mjs` | Static workflow trigger, permission, pinning, and publication-order contracts |
| `.github/workflows/quality-monitor.yml` | Parallel PR/main/scheduled validation with one required `verify` result |
| `.github/workflows/release-please.yml` | Create/update the single Release PR and create stable tag plus Draft Release |
| `.github/workflows/build-desktop.yml` | Build Pre-release assets or complete an existing stable Draft Release |
| `.github/dependabot.yml` | GitHub Actions, npm, and Cargo update PRs |
| `.github/rulesets/main.json` | Auditable default-branch Ruleset payload |
| `.github/rulesets/tags.json` | Auditable stable/build tag immutability payload |
| `.github/rulesets/README.md` | Evaluate/apply/verify/activate and rollback commands |
| `AGENTS.md` | Stable release invariants for future agents |
| `README.md` | User-facing stable/Pre-release channel explanation |

---

### Task 1: Establish one release version invariant

**Files:**
- Modify: `.gitignore`
- Create: `version.txt`
- Create: `scripts/release-version.mjs`
- Create: `scripts/release-version.test.mjs`
- Modify: `core/Cargo.toml:1-4`
- Modify: `cli/Cargo.toml:1-4`
- Modify: `desktop/package.json:1-5`
- Modify: `desktop/package-lock.json`
- Create: `website/package-lock.json`
- Modify: `desktop/src-tauri/Cargo.toml:1-6`
- Modify: `desktop/src-tauri/tauri.conf.json:1-5`
- Regenerate: `Cargo.lock`
- Regenerate: `desktop/src-tauri/Cargo.lock`

**Interfaces:**
- `node scripts/release-version.mjs check` exits 0 only when every release-owned source and lockfile agrees with `version.txt`.
- `node scripts/release-version.mjs refresh-locks` regenerates npm/Cargo lock metadata from already-updated source manifests, then runs `check`.
- `isReleaseMerge(beforeState, afterState, commitTitle)` is a pure helper used by tests and later workflow classification.

- [x] **Step 1: Write failing Node tests for version agreement**

Cover these cases with `node:test` and temporary fixtures:

1. All JSON/TOML sources equal `version.txt` -> success.
2. `desktop/package-lock.json.version` differs -> mismatch reports its exact field.
3. `desktop/package-lock.json.packages[""].version` differs -> mismatch reports its exact field.
4. One Cargo package version differs -> mismatch reports the manifest path.
5. A stable tag version differs from the source -> fail closed.
6. `version.txt` changes and the commit title matches `chore(main): release X.Y.Z` -> release merge.
7. Only one signal matches -> not a release merge.

Run:

```bash
node --test scripts/release-version.test.mjs
```

Expected: FAIL because `scripts/release-version.mjs` does not exist.

- [x] **Step 2: Implement strict check mode**

Requirements:

- Accept only `MAJOR.MINOR.PATCH` in `version.txt`.
- Parse JSON with `JSON.parse`; require both npm lockfile version fields.
- Read the exact `[package] version` entry from each owned Cargo manifest and reject zero/multiple matches.
- Execute both Cargo metadata commands with `--locked --no-deps` so stale Cargo lockfiles fail:

```bash
cargo metadata --locked --no-deps --format-version 1
cargo metadata --locked --no-deps --format-version 1 \
  --manifest-path desktop/src-tauri/Cargo.toml
```

- Report every mismatch in one run; do not silently repair in `check` mode.

- [x] **Step 3: Implement generated-lock refresh mode**

`refresh-locks` must not invent a version. It reads source manifests already changed by Release Please, verifies they equal `version.txt`, then updates npm metadata in a temporary directory that never contains the developer machine's `node_modules`. A complete portable lockfile uses `npm version --allow-same-version --no-git-tag-version` so a release-only version bump cannot re-resolve dependencies. An incomplete lockfile is discarded and rebuilt once from `package.json` so platform-specific packages cannot remain pruned. The recovery path runs the equivalent of:

```bash
npm install --package-lock-only --ignore-scripts --no-audit --no-fund
cargo metadata --no-deps --format-version 1 >/dev/null
cargo metadata --no-deps --format-version 1 \
  --manifest-path desktop/src-tauri/Cargo.toml >/dev/null
node scripts/release-version.mjs check
```

The generated npm lock is copied back only after the temporary operation succeeds. `check` verifies that every declared regular or optional dependency resolves through the lockfile's Node package hierarchy. Generated files are updated only through these package managers; do not regex-edit lockfiles.

The repository previously ignored both npm lockfiles. Remove that broad ignore, keep the irrelevant root `/package-lock.json` ignored, and commit the Desktop and Website lockfiles before switching CI to `npm ci`.

- [x] **Step 4: Seed the current stable version**

Set `version.txt` to `1.2.18`, align the existing sources, run `refresh-locks`, and confirm the already-observed stale npm lockfile metadata is corrected.

- [x] **Step 5: Run focused and project verification**

```bash
node --test scripts/release-version.test.mjs
node scripts/release-version.mjs check
cargo test --locked -p arca-core -p arca-cli
cd desktop && npm ci --no-audit --no-fund && npm test && npm run build
```

Expected: all commands exit 0 and the working tree does not change after the final `check`.

- [x] **Step 6: Commit Task 1**

```bash
git add version.txt scripts/release-version.mjs scripts/release-version.test.mjs \
  core/Cargo.toml cli/Cargo.toml Cargo.lock \
  desktop/package.json desktop/package-lock.json \
  desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock \
  desktop/src-tauri/tauri.conf.json
git commit -m "build(release): centralize version metadata" \
  -m "Make Release Please and CI share one checked version invariant across Desktop, CLI, Core, Tauri, npm, and generated lockfiles."
```

---

### Task 2: Parallelize CI behind one stable required check

**Files:**
- Create: `scripts/workflow-contract.test.mjs`
- Modify: `.github/workflows/quality-monitor.yml:1-171`

**Interfaces:**
- Required status name remains `verify`.
- Producer jobs are `rust`, `desktop`, and `website`.
- Failure monitoring consumes the aggregate `verify` result and retains the current Issue/autofix lifecycle.

- [x] **Step 1: Write failing workflow contract tests**

Assert the quality workflow contains:

- Node 24 for Node jobs.
- `npm ci`, never `npm install`.
- lockfile-based `cache: npm` configuration for Desktop and Website.
- separate `rust`, `desktop`, and `website` jobs.
- an `if: always()` `verify` job that needs all three producers and fails unless each result is `success`.
- failure reporting that skips pull requests and runs after `verify`.
- the existing per-ref `cancel-in-progress: true` behavior for PR feedback.

Run:

```bash
node --test scripts/workflow-contract.test.mjs
```

Expected: FAIL against the current sequential workflow.

- [x] **Step 2: Split the verification jobs**

Implement:

- `rust`: checkout, stable Rust toolchain, Rust cache, locked core/CLI tests.
- `desktop`: checkout, Node 24, npm cache keyed by `desktop/package-lock.json`, `npm ci`, tests, version check, production build.
- `website`: checkout, Node 24, npm cache keyed by `website/package-lock.json`, `npm ci`, VitePress build.
- `verify`: a small Ubuntu aggregation job with `if: always()` and `needs: [rust, desktop, website]`.

Keep `verify` as the exact job ID and displayed check name used by the future Ruleset.

- [x] **Step 3: Move monitoring behind the aggregate result**

Use a separate `monitor` job with `if: always()` and `needs: [verify]`. Preserve:

- one open `[CI] Automated verification is failing` Issue;
- `ci-failure`, `autofix`, and `autofix-failed` labels;
- no Issue mutation for PR events;
- Copilot dispatch only when `COPILOT_PAT` is configured;
- closing the failure Issue after successful non-PR recovery.

- [x] **Step 4: Run local equivalence checks**

```bash
node --test scripts/workflow-contract.test.mjs
node scripts/release-version.mjs check
cargo test --locked -p arca-core -p arca-cli
cd desktop && npm ci --no-audit --no-fund && npm test && npm run build
cd ../website && npm ci --no-audit --no-fund && npm run build
```

- [x] **Step 5: Commit Task 2**

```bash
git add scripts/workflow-contract.test.mjs .github/workflows/quality-monitor.yml
git commit -m "ci(quality): parallelize required verification" \
  -m "Shorten PR feedback while preserving one stable verify gate and the existing automated failure lifecycle."
```

---

### Task 3: Add the single Release Please component

**Files:**
- Create: `release-please-config.json`
- Create: `.release-please-manifest.json`
- Create: `CHANGELOG.md`
- Create: `.github/workflows/release-please.yml`
- Modify: `scripts/workflow-contract.test.mjs`

**Interfaces:**
- One root package (`.`), component `arca`, `release-type: simple`.
- Release PR title: `chore(main): release ${version}`.
- Release PR branch is updated in place; `separate-pull-requests` is false.
- Stable GitHub Release is created as Draft and its tag is created immediately.

- [x] **Step 1: Extend workflow/config tests and verify RED**

Assert:

- `.release-please-manifest.json` starts at `1.2.18`.
- configuration has exactly one package key: `.`.
- `include-component-in-tag` is false and `include-v-in-tag` is true.
- `separate-pull-requests` is false.
- `draft` and `force-tag-creation` are true.
- source manifests are explicit `extra-files`; lockfiles are not directly text-edited.
- workflow triggers on `main` and `workflow_dispatch` only.
- Release Please receives `secrets.RELEASE_PLEASE_TOKEN`, not `github.token` or `COPILOT_PAT`.
- action references are full 40-character SHAs.

Run:

```bash
node --test scripts/workflow-contract.test.mjs
```

Expected: FAIL because the files do not exist.

- [x] **Step 2: Add the manifest configuration**

Configure `extra-files` for:

- `core/Cargo.toml` -> `$.package.version`
- `cli/Cargo.toml` -> `$.package.version`
- `desktop/package.json` -> `$.version`
- `desktop/src-tauri/Cargo.toml` -> `$.package.version`
- `desktop/src-tauri/tauri.conf.json` -> `$.version`

Do not list npm or Cargo lockfiles in `extra-files`. After Release Please updates the source manifests, `refresh-locks` regenerates their version metadata with the package managers and commits only those generated files back to the same Release PR.

Use Changelog sections for breaking changes, features, fixes, and dependencies; hide internal docs/test/chore/ci entries by default.

- [x] **Step 3: Add the Release Please workflow**

Workflow permissions are limited to:

```text
contents: write
pull-requests: write
issues: write
```

Behavior:

1. Fail with a clear message when `RELEASE_PLEASE_TOKEN` is missing.
2. Run the SHA-pinned Release Please Action.
3. If a Release PR was created or updated, checkout that PR head with the same token.
4. Run Node 24 and stable Rust setup.
5. Run `node scripts/release-version.mjs refresh-locks` and then `check`.
6. If generated lockfiles changed, commit only those files to the Release PR branch using one-shot `git -c user.name=... -c user.email=... commit`; never change repository git config.
7. Push with `RELEASE_PLEASE_TOKEN` so PR CI runs.

The Release Please action should create/update the Release PR and, after it merges, create the stable tag plus Draft Release. It must not publish desktop assets.

- [x] **Step 4: Validate configuration without external mutation**

```bash
node --test scripts/workflow-contract.test.mjs
node scripts/release-version.mjs check
python3 -m json.tool release-please-config.json >/dev/null
python3 -m json.tool .release-please-manifest.json >/dev/null
```

Do not add the Secret or run the workflow in this task.

- [x] **Step 5: Commit Task 3**

```bash
git add release-please-config.json .release-please-manifest.json CHANGELOG.md \
  .github/workflows/release-please.yml scripts/workflow-contract.test.mjs
git commit -m "ci(release): add single Release PR automation" \
  -m "Keep one rolling release proposal for main and create only a stable tag plus Draft Release after explicit approval."
```

---

### Task 4: Make desktop Pre-release and Stable publication atomic

**Files:**
- Modify: `.github/workflows/build-desktop.yml:1-288`
- Create: `.github/scripts/wait-for-verify.sh`
- Create: `.github/scripts/publish-release-assets.sh`
- Create: `.github/scripts/release-scripts.test.sh`
- Modify: `scripts/workflow-contract.test.mjs`

**Interfaces:**
- `wait-for-verify.sh <repo> <sha>` succeeds only when GitHub Actions check `verify` for the exact SHA succeeds; it fails on failure, missing check after timeout, or ambiguous non-Actions sources.
- `publish-release-assets.sh <tag> <asset...>` only targets an existing Draft Release. Existing same-name assets must have identical SHA-256 content or the script fails.
- Stable publication changes `draft` to false only after assets and labels are complete.

- [x] **Step 1: Write failing shell and workflow contract tests**

Use a fake `gh` executable earlier on `PATH` to cover:

1. `verify` success for the exact SHA.
2. pending check followed by success.
3. failed, missing, or wrong-app check -> failure.
4. missing Draft Release -> failure.
5. identical existing asset -> reuse.
6. different existing asset with the same name -> failure, never overwrite.
7. Draft is published only after all labels succeed.

Extend the Node workflow test to require:

- Node 24 and `npm ci` with `desktop/package-lock.json` cache input.
- stable tag regex `^v[0-9]+\.[0-9]+\.[0-9]+$`.
- release-merge detection before a main Pre-release is built.
- no cancellation of ordinary main builds.
- no `latest.json` on Pre-releases.
- Stable publication uses the existing Draft rather than creating a second Release.

Run:

```bash
bash .github/scripts/release-scripts.test.sh
node --test scripts/workflow-contract.test.mjs
```

Expected: FAIL because the helpers and new state machine do not exist.

- [x] **Step 2: Implement exact-SHA verification gating**

`wait-for-verify.sh` polls the Checks API with bounded retries shorter than ten minutes. Select check runs where:

- `name == "verify"`
- `head_sha == requested SHA`
- `app.slug == "github-actions"`

Success requires exactly one current successful result. A timeout or any terminal non-success result exits non-zero.

- [x] **Step 3: Implement idempotent Draft asset publication**

The helper must:

1. Resolve the GitHub Release by exact tag.
2. Assert `draft == true` and `prerelease == false`.
3. Upload absent assets.
4. For an existing asset, download it and compare SHA-256; reuse only identical bytes.
5. Apply all human-readable labels.
6. Re-query the Release and assert the exact required asset set.
7. PATCH `draft=false` as the final mutation.

Never use `--clobber` for stable assets.

- [x] **Step 4: Refactor build classification and setup**

Keep triggers for `main`, stable tags, and `workflow_dispatch`. Add manual inputs for `prerelease` and `stable-retry`; a stable retry must run with a selected existing `vX.Y.Z` ref.

Classification:

- ordinary `main` push -> Pre-release;
- Release PR merge detected by both version transition and fixed commit title -> skip Pre-release;
- strict stable tag -> Stable path;
- every other tag/ref -> fail closed.

Upgrade setup to Node 24, `actions/setup-node` npm caching with `desktop/package-lock.json`, `npm ci`, stable Rust, and the two existing Rust target caches.

- [x] **Step 5: Preserve and tighten artifact verification**

Keep all existing checks:

- Tauri App and DMG exist.
- manifest, App bundle, and CLI versions agree.
- `codesign --verify --deep --strict` succeeds.
- `hdiutil verify` succeeds.
- mounted DMG contains an App that passes the same version/signature checks.

For stable tags, also require `v$EXPECTED == github.ref_name` before any upload.

- [x] **Step 6: Implement the two publication branches**

Pre-release:

- derive `v<version>-build.<zero-padded run_number>`;
- package DMG and CLI;
- wait for exact-SHA `verify`;
- create one GitHub Pre-release using `gh release create` and the default `GITHUB_TOKEN`;
- label its two assets;
- never create updater assets or `latest.json`.

Stable:

- package updater payload, installer, CLI, and `latest.json`;
- wait for exact-SHA `verify`;
- require the Draft created by Release Please;
- upload/label/verify assets idempotently;
- publish the Draft last.

- [x] **Step 7: Run local verification**

```bash
bash -n .github/scripts/wait-for-verify.sh
bash -n .github/scripts/publish-release-assets.sh
bash .github/scripts/release-scripts.test.sh
node --test scripts/workflow-contract.test.mjs scripts/release-version.test.mjs
node scripts/release-version.mjs check
cd desktop && npm ci --no-audit --no-fund && npm test && npm run build
```

Do not create a tag or Release during local verification.

- [x] **Step 8: Commit Task 4**

```bash
git add .github/workflows/build-desktop.yml \
  .github/scripts/wait-for-verify.sh \
  .github/scripts/publish-release-assets.sh \
  .github/scripts/release-scripts.test.sh \
  scripts/workflow-contract.test.mjs
git commit -m "ci(release): make desktop publishing atomic" \
  -m "Keep Pre-releases automatic while withholding stable updates until the exact commit and every signed asset are verified."
```

---

### Task 5: Pin the Actions supply chain and automate updates

**Files:**
- Create: `.github/dependabot.yml`
- Modify: `.github/workflows/build-desktop.yml`
- Modify: `.github/workflows/quality-monitor.yml`
- Modify: `.github/workflows/issue-autofix.yml`
- Modify: `.github/workflows/repair-review-notify.yml`
- Modify: `.github/workflows/release-please.yml`
- Modify: `scripts/workflow-contract.test.mjs`

- [x] **Step 1: Add a failing full-SHA policy test**

Scan all `.github/workflows/*.yml` `uses:` entries. Allow only:

- local paths beginning `./`;
- Docker references pinned by digest;
- repository actions pinned to exactly 40 lowercase hexadecimal characters.

Reject `@main`, `@stable`, `@v4`, `@v5`, and other mutable refs.

- [x] **Step 2: Resolve and verify immutable action SHAs**

For every current action, resolve the intended upstream release tag with `gh api`, follow annotated tags to the commit object, and verify the commit belongs to the expected upstream repository. Pin that commit and keep the human version in a comment:

```yaml
uses: actions/checkout@<40-char-sha> # v5
```

Apply this to checkout, setup-node, cache, Rust toolchain, Rust cache, Release Please, and every remaining third-party action.

- [x] **Step 3: Add Dependabot update groups**

Configure weekly updates for:

- `github-actions` at `/`;
- npm at `/desktop` and `/website`;
- Cargo at `/` and `/desktop/src-tauri`.

Group compatible minor/patch updates by ecosystem. Limit concurrent PRs to avoid flooding the single-maintainer queue.

- [x] **Step 4: Run supply-chain checks**

```bash
node --test scripts/workflow-contract.test.mjs
python3 -m json.tool release-please-config.json >/dev/null
```

- [x] **Step 5: Commit Task 5**

```bash
git add .github/dependabot.yml .github/workflows scripts/workflow-contract.test.mjs
git commit -m "ci(security): pin Actions and schedule updates" \
  -m "Use immutable upstream commits while retaining automated reviewable dependency refreshes."
```

---

### Task 6: Codify and evaluate GitHub repository governance

**Files:**
- Create: `.github/rulesets/main.json`
- Create: `.github/rulesets/tags.json`
- Create: `.github/rulesets/README.md`
- Modify: `scripts/workflow-contract.test.mjs`

**External state:** GitHub merge settings and Repository Rulesets. Do not apply or activate without explicit authorization.

- [x] **Step 1: Add failing Ruleset payload tests**

Validate the committed JSON requires:

Main:

- target default branch;
- pull request required with zero approvals;
- required `verify` status from GitHub Actions;
- strict/up-to-date branch requirement;
- resolved conversations;
- linear history;
- blocked force pushes and deletion;
- no routine bypass actor.

Tags:

- target `v*`;
- block update, force push, and deletion after creation;
- allow the release automation to create a new tag.

Initial enforcement should be `evaluate`, not `active`. GitHub currently limits Evaluate mode to Enterprise plans; if the API rejects it for the repository plan, the only safe fallback is `disabled`, never an automatic fallback to `active`.

- [x] **Step 2: Add auditable payloads and runbook**

The runbook must contain exact commands to:

1. GET current repository merge settings and Rulesets into timestamped local evidence.
2. POST the Evaluate Rulesets from committed JSON.
3. PATCH merge settings to squash-only, use the PR title deterministically for the squash commit, and delete merged branches.
4. Confirm the effective rules from the API.
5. Promote a named Ruleset from Evaluate or Disabled to Active with the current REST `PUT` endpoint.
6. Disable—not delete—the Ruleset for emergency rollback.

Do not embed IDs, tokens, or current API responses in committed files.

- [x] **Step 3: Validate locally**

```bash
python3 -m json.tool .github/rulesets/main.json >/dev/null
python3 -m json.tool .github/rulesets/tags.json >/dev/null
node --test scripts/workflow-contract.test.mjs
```

- [x] **Step 4: Commit Task 6 code before applying it**

```bash
git add .github/rulesets scripts/workflow-contract.test.mjs
git commit -m "docs(release): codify repository governance" \
  -m "Keep main and tag protection reviewable, reproducible, and reversible before changing GitHub state."
```

- [ ] **Step 5: With authorization, apply Evaluate mode and merge settings**

Before mutation, re-read live state. Apply the committed payloads with `gh api`, then verify:

```bash
gh api repos/Scoheart/arca --jq '{allow_squash_merge,allow_merge_commit,allow_rebase_merge,delete_branch_on_merge,squash_merge_commit_title,squash_merge_commit_message}'
gh api repos/Scoheart/arca/rulesets
```

Expected: Rulesets are visible but not yet blocking; squash-only and branch deletion settings match the plan.

---

### Task 7: Document, roll out, and prove the pipeline

**Files:**
- Modify: `AGENTS.md`
- Modify: `README.md`
- Modify: `.github/copilot-instructions.md`
- Modify: `docs/superpowers/specs/2026-07-17-release-pipeline-design.md`
- Modify: `docs/superpowers/plans/2026-07-17-release-pipeline.md` checkboxes during execution

**External state:** Secret creation, branch push, PR creation/merge, workflow dispatch, Ruleset activation, first stable Release, Immutable Releases, and installed App replacement. Each needs current authorization.

- [x] **Step 1: Update durable repository guidance**

Document:

- feature branches and PRs are mandatory;
- `verify` is the merge gate;
- ordinary main merges produce Pre-releases;
- one Release PR accumulates the next stable version;
- merging it creates the stable Draft/tag flow;
- failed Drafts do not change the update channel;
- no agent may merge a Release PR or publish/replace the App without authorization.

README should explain stable versus Pre-release downloads without exposing maintainer credentials or internal recovery details.

- [x] **Step 2: Run the complete local suite**

```bash
node --test scripts/release-version.test.mjs scripts/workflow-contract.test.mjs
bash .github/scripts/release-scripts.test.sh
node scripts/release-version.mjs check
cargo fmt --check
cargo test --locked --workspace
cd desktop && npm ci --no-audit --no-fund && npm test && npm run build
bash scripts/prepare-sidecar.sh
cargo test --manifest-path src-tauri/Cargo.toml
cd ../website && npm ci --no-audit --no-fund && npm run build
git diff --check
```

Expected: all commands exit 0 and generated lockfiles remain clean.

- [x] **Step 3: Commit documentation and final plan state**

```bash
git add AGENTS.md README.md .github/copilot-instructions.md \
  docs/superpowers/specs/2026-07-17-release-pipeline-design.md \
  docs/superpowers/plans/2026-07-17-release-pipeline.md
git commit -m "docs(release): document automated delivery" \
  -m "Make the new PR, Pre-release, Release PR, stable publication, and authorization boundaries discoverable to users and agents."
```

- [ ] **Step 4: With authorization, configure the dedicated token**

Create a fine-grained token restricted to `Scoheart/arca` with only Contents, Pull requests, and Issues read/write. Store it as `RELEASE_PLEASE_TOKEN`; never print or read it back. Confirm only the Secret name exists.

- [ ] **Step 5: With authorization, push and open the implementation PR**

Use a release-eligible squash title such as:

```text
feat(release): automate verified desktop delivery
```

Wait for every PR check. Review the workflow permissions, Release Please configuration, version diff, and generated scripts before merge.

- [ ] **Step 6: Verify one rolling Release PR before activation**

After the implementation PR merges:

1. Confirm an ordinary main Pre-release was created from the exact merge SHA.
2. Confirm exactly one Release PR exists with `autorelease: pending`.
3. Confirm its version files, lockfiles, and Changelog agree.
4. Confirm a subsequent eligible main merge updates the same PR number rather than opening a second one.
5. Confirm an out-of-date Release PR cannot merge once Ruleset enforcement is active.

- [ ] **Step 7: With authorization, activate main and tag Rulesets**

Re-read live state, patch Evaluate -> Active, then use a disposable documentation PR to prove direct `main` updates are blocked and `verify` is required. Roll back by disabling the Ruleset if the Release Please PR cannot update or merge.

- [ ] **Step 8: With authorization, perform the first automated stable release**

Merge the reviewed Release PR. Verify in order:

1. one stable `vX.Y.Z` tag exists at the Release PR merge SHA;
2. one Draft Release exists for that tag;
3. Stable build and exact-SHA `verify` succeed;
4. required DMG, CLI, updater, signature, and `latest.json` assets exist and labels are correct;
5. Draft becomes the newest non-prerelease only after assets are complete;
6. CLI `arca upgrade` resolves the compatibility filename;
7. `/Applications/Arca.app` installs and reports the released version;
8. the updater reads the new signed `latest.json` successfully.

- [ ] **Step 9: Enable Immutable Releases after the successful proof**

Only after the first automated stable release is complete, enable GitHub Immutable Releases and verify published assets and tags can no longer be changed. Draft creation and asset upload must remain possible before publication.

- [ ] **Step 10: Measure the outcome**

Record at least three successful runs for each path:

- PR `verify` wall time;
- ordinary main Pre-release wall time;
- stable Release wall time;
- cache restore/save duration;
- Rust/Tauri build duration.

Acceptance targets:

- PR feedback is faster than the previous 1 minute 40 seconds to 2 minute sequential baseline.
- warm macOS build remains at or below the previous 3 to 5 minute range.
- no duplicate stable/Pre-release build for a Release PR merge.
- no incomplete stable Release becomes `releases/latest`.

---

## Completion Checklist

- [x] All seven code/document tasks are committed independently.
- [x] Arca working tree is clean.
- [ ] Parent workspace state is captured and checked.
- [ ] Implementation branch is pushed and reviewed through a PR.
- [ ] `RELEASE_PLEASE_TOKEN` exists with the documented minimum permissions.
- [ ] One rolling Release PR was observed across multiple eligible main merges.
- [ ] Main/tag Rulesets are Active and verified.
- [ ] First automated Stable Release completed without exposing a partial update.
- [ ] Immutable Releases are enabled only after the stable proof.
- [ ] Before/after duration evidence is recorded.
