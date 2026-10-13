import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

async function read(relativePath) {
  return readFile(join(root, relativePath), "utf8");
}

function jobBlock(workflow, job, nextJob) {
  const start = workflow.indexOf(`\n  ${job}:`);
  assert.notEqual(start, -1, `missing ${job} job`);
  const end = nextJob ? workflow.indexOf(`\n  ${nextJob}:`, start + 1) : -1;
  return workflow.slice(start, end === -1 ? undefined : end);
}

test("quality workflow runs independent producer jobs", async () => {
  const workflow = await read(".github/workflows/quality-monitor.yml");
  const rust = jobBlock(workflow, "rust", "tauri");
  const tauri = jobBlock(workflow, "tauri", "desktop");
  const desktop = jobBlock(workflow, "desktop", "website");
  const website = jobBlock(workflow, "website", "verify");

  assert.match(rust, /cargo test --locked -p arca-core -p arca-cli/);
  assert.match(rust, /cargo fmt --all --check/);
  assert.match(rust, /cargo clippy --locked -p arca-core -p arca-cli --all-targets -- -D warnings/);
  assert.match(tauri, /runs-on:\s*macos-latest/);
  assert.match(
    tauri,
    /cargo fmt --all --manifest-path desktop\/src-tauri\/Cargo\.toml --check/,
  );
  assert.match(
    tauri,
    /working-directory:\s*desktop[\s\S]*bash scripts\/prepare-sidecar\.sh[\s\S]*cargo clippy/,
  );
  assert.doesNotMatch(tauri, /\btouch\s+.*binaries\/arca/);
  assert.match(
    tauri,
    /cargo clippy --locked --manifest-path desktop\/src-tauri\/Cargo\.toml --all-targets -- -D warnings/,
  );
  assert.match(
    tauri,
    /cargo test --locked --manifest-path desktop\/src-tauri\/Cargo\.toml/,
  );
  assert.match(desktop, /node-version:\s*24/);
  assert.match(desktop, /cache:\s*npm/);
  assert.match(desktop, /cache-dependency-path:\s*desktop\/package-lock\.json/);
  assert.match(desktop, /npm ci --no-audit --no-fund/);
  assert.match(desktop, /node scripts\/release-version\.mjs check/);
  assert.match(
    desktop,
    /node --test scripts\/workflow-contract\.test\.mjs scripts\/release-version\.test\.mjs/,
  );
  assert.match(website, /node-version:\s*24/);
  assert.match(website, /cache-dependency-path:\s*website\/package-lock\.json/);
  assert.match(website, /npm ci --no-audit --no-fund/);
  assert.doesNotMatch(workflow, /npm install/);
});

test("verify is the stable aggregate result", async () => {
  const workflow = await read(".github/workflows/quality-monitor.yml");
  const verify = jobBlock(workflow, "verify", "monitor");

  assert.match(verify, /name:\s*verify/);
  assert.match(verify, /if:\s*\$\{\{ always\(\) \}\}/);
  assert.match(verify, /needs:\s*\[rust, tauri, desktop, website\]/);
  for (const producer of ["rust", "tauri", "desktop", "website"]) {
    assert.match(verify, new RegExp(`needs\\.${producer}\\.result`));
  }
});

test("quality validates only stable tags, PRs, schedules, and manual runs", async () => {
  const workflow = await read(".github/workflows/quality-monitor.yml");
  const monitor = jobBlock(workflow, "monitor");

  assert.match(workflow, /push:\s*\n\s*tags:\s*\["v\*"\]/);
  assert.doesNotMatch(workflow, /push:\s*\n\s*branches:\s*\[main\]/);
  assert.match(workflow, /pull_request:\s*\n\s*branches:\s*\[main\]/);
  assert.match(workflow, /cancel-in-progress:\s*true/);
  assert.match(monitor, /needs\.verify\.result == 'failure'/);
  assert.match(monitor, /github\.event_name != 'pull_request'/);
  assert.match(monitor, /needs\.verify\.result == 'success'/);
  assert.match(monitor, /secrets\.COPILOT_PAT/);
});

test("one explicit main release request creates one Draft and dispatches one main-scoped build", async () => {
  const workflow = await read(".github/workflows/direct-stable-release.yml");

  assert.doesNotMatch(workflow, /\n  push:/);
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /release_type:[\s\S]*required:\s*true/);
  assert.match(workflow, /options:\s*\[patch, minor, major\]/);
  assert.match(workflow, /test "\$GITHUB_REF" = "refs\/heads\/main"/);
  assert.match(workflow, /actions:\s*write/);
  assert.match(workflow, /cancel-in-progress:\s*false/);
  assert.doesNotMatch(workflow, /fast-lane|ends_at|ARCA_FAST_LANE_UNTIL/i);
  assert.match(workflow, /secrets\.RELEASE_PLEASE_TOKEN/);
  assert.match(workflow, /git rev-parse origin\/main/);
  assert.match(workflow, /current.*SOURCE_SHA/);
  assert.match(workflow, /commit_title.*chore\(main\): release/);
  assert.match(workflow, /release-version\.mjs prepare-direct --source "\$SOURCE_SHA" --bump "\$RELEASE_TYPE"/);
  assert.match(workflow, /commit -m "chore\(main\): release \$version"/);
  assert.match(workflow, /git push origin HEAD:main/);
  assert.match(
    workflow,
    /gh api --method POST "repos\/\$GITHUB_REPOSITORY\/releases"/,
  );
  assert.match(workflow, /-F draft=true/);
  assert.match(workflow, /-F generate_release_notes=true/);
  assert.match(workflow, /release_id=\$\(jq -er '\.id'/);
  assert.match(workflow, /git tag "\$RELEASE_TAG" "\$RELEASE_SHA"/);
  assert.match(workflow, /git push origin "refs\/tags\/\$RELEASE_TAG"/);
  assert.match(workflow, /gh workflow run build-desktop\.yml/);
  assert.match(workflow, /--ref main/);
  assert.match(workflow, /-f "stable_tag=\$RELEASE_TAG"/);
  assert.equal(
    workflow.match(/gh workflow run build-desktop\.yml/g)?.length,
    1,
    "Direct Stable must dispatch exactly one desktop build",
  );
  assert.ok(
    workflow.indexOf('gh api --method POST "repos/$GITHUB_REPOSITORY/releases"') <
      workflow.indexOf('git push origin "refs/tags/$RELEASE_TAG"'),
    "Draft must exist before its immutable tag is pushed",
  );
  assert.ok(
    workflow.indexOf('git push origin "refs/tags/$RELEASE_TAG"') <
      workflow.indexOf("gh workflow run build-desktop.yml"),
    "tag and Draft provenance must exist before the desktop build is dispatched",
  );
  assert.match(workflow, /test "\$target" = "\$RELEASE_SHA"/);
  assert.match(workflow, /test "\$\(jq -r '\.tag_name'/);
  assert.match(workflow, /test "\$\(jq -r '\.target_commitish'/);
  assert.doesNotMatch(workflow, /release-please-manifest|fast-lane/i);
  assert.doesNotMatch(workflow, /git (?:push|tag)[^\n]*(?:--force|-f)/);
  assert.doesNotMatch(workflow, /--clobber/);
});

test("desktop workflow builds stable tags only from the reusable main cache scope", async () => {
  const workflow = await read(".github/workflows/build-desktop.yml");

  assert.doesNotMatch(workflow, /push:\s*\n\s*tags:\s*\["v\*"\]/);
  assert.match(workflow, /workflow_dispatch:[\s\S]*stable_tag:/);
  assert.match(workflow, /test "\$GITHUB_REF" = "refs\/heads\/main"/);
  assert.match(workflow, /group:\s*stable-desktop-\$\{\{ inputs\.stable_tag \}\}/);
  assert.match(workflow, /RELEASE_TAG:\s*\$\{\{ inputs\.stable_tag \}\}/);
  assert.doesNotMatch(workflow, /PRERELEASE_TAG_REGEX|mode=prerelease|-build\./);
  assert.doesNotMatch(workflow, /\n  classify:/);
  assert.match(workflow, /node-version:\s*24/);
  assert.match(workflow, /cache-dependency-path:\s*desktop\/package-lock\.json/);
  assert.match(workflow, /npm ci --no-audit --no-fund/);
  assert.match(workflow, /\^v\[0-9\]\+\\\.\[0-9\]\+\\\.\[0-9\]\+\$/);
  assert.match(workflow, /\.target_commitish == \$sha/);
  assert.match(workflow, /"\$draft_matches" = 1/);
  assert.match(workflow, /publish-release-assets\.sh/);
  assert.match(workflow, /shared-key:\s*stable-desktop/);
  assert.match(workflow, /save-if:\s*\$\{\{ github\.ref == 'refs\/heads\/main' \}\}/);
  assert.match(workflow, /path:\s*\.delivery/);
  assert.match(workflow, /\.delivery\/\.github\/scripts\/publish-release-assets\.sh/);
  assert.match(workflow, /steps\.source\.outputs\.sha/);
  assert.doesNotMatch(workflow, /cancel-in-progress:\s*true/);
});

test("quality restores release caches on tags and only writes reusable main caches", async () => {
  const workflow = await read(".github/workflows/quality-monitor.yml");
  const rust = jobBlock(workflow, "rust", "tauri");
  const tauri = jobBlock(workflow, "tauri", "desktop");
  const reusableWrite =
    /save-if:\s*\$\{\{ github\.ref == 'refs\/heads\/main' && \(github\.event_name == 'schedule' \|\| github\.event_name == 'workflow_dispatch'\) \}\}/;

  assert.match(rust, reusableWrite);
  assert.match(tauri, reusableWrite);
  assert.match(tauri, /shared-key:\s*stable-desktop/);
  assert.doesNotMatch(workflow, /save-if:[^\n]*github\.event_name == 'push'/);
});

test("desktop release packaging reuses the CLI already built for the sidecar", async () => {
  const workflow = await read(".github/workflows/build-desktop.yml");
  const sidecar = await read("desktop/scripts/prepare-sidecar.sh");
  const publisher = await read(".github/scripts/publish-release-assets.sh");

  assert.match(sidecar, /cargo build --release --locked -p arca-cli/);
  assert.match(sidecar, /\.\.\/target\/release\/arca/);
  assert.doesNotMatch(workflow, /cargo build --release -p arca-cli/);
  assert.match(workflow, /CLI=target\/release\/arca/);
  assert.match(workflow, /tar -C target\/release/);
  assert.match(workflow, /desktop\/src-tauri -> target/);
  assert.match(workflow, /^\s+\. -> target$/m);
  assert.match(workflow, /tauri build -- --bundles app,dmg -- --locked/);
  assert.match(publisher, /make_latest=legacy/);
  assert.match(publisher, /releases\/latest/);
  assert.match(publisher, /sort_by\(\.tag_name/);
});

test("CLI and desktop use the same fast release profile", async () => {
  const cliManifest = await read("Cargo.toml");
  const desktopManifest = await read("desktop/src-tauri/Cargo.toml");

  for (const manifest of [cliManifest, desktopManifest]) {
    assert.match(manifest, /\[profile\.release\]/);
    assert.match(manifest, /codegen-units\s*=\s*256/);
    assert.match(manifest, /opt-level\s*=\s*2/);
    assert.match(manifest, /strip\s*=\s*true/);
  }
});

test("every repository Action uses an immutable commit", async () => {
  const workflowDirectory = join(root, ".github", "workflows");
  const workflowNames = (await readdir(workflowDirectory)).filter((name) =>
    name.endsWith(".yml"),
  );

  for (const workflowName of workflowNames) {
    const workflow = await read(`.github/workflows/${workflowName}`);
    for (const match of workflow.matchAll(/^\s*uses:\s*([^\s#]+).*$/gm)) {
      const action = match[1];
      if (action.startsWith("./")) continue;
      if (action.startsWith("docker://")) {
        assert.match(action, /@sha256:[0-9a-f]{64}$/);
        continue;
      }
      const separator = action.lastIndexOf("@");
      assert.notEqual(separator, -1, `${workflowName}: ${action}`);
      assert.match(
        action.slice(separator + 1),
        /^[0-9a-f]{40}$/,
        `${workflowName}: ${action}`,
      );
    }
  }
});

function ruleByType(ruleset, type) {
  const rule = ruleset.rules.find((candidate) => candidate.type === type);
  assert.ok(rule, `${ruleset.name}: missing ${type} rule`);
  return rule;
}

test("stable tag Ruleset allows creation but blocks mutation", async () => {
  const ruleset = JSON.parse(await read(".github/rulesets/tags.json"));

  assert.equal(ruleset.target, "tag");
  assert.equal(ruleset.enforcement, "evaluate");
  assert.deepEqual(ruleset.bypass_actors, []);
  assert.deepEqual(ruleset.conditions.ref_name.include, ["refs/tags/v*"]);
  const types = ruleset.rules.map((rule) => rule.type);
  assert.ok(!types.includes("creation"));
  for (const type of ["update", "deletion", "non_fast_forward"]) {
    assert.ok(types.includes(type));
  }
  assert.equal(
    ruleByType(ruleset, "update").parameters.update_allows_fetch_and_merge,
    false,
  );
});
