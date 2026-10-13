import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  collectVersionMismatches,
  nextReleaseVersion,
  updateCargoPackageVersion,
  updateCargoLockPackageVersions,
} from "./release-version.mjs";

async function writeJson(path, value) {
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`);
}

async function createFixture(overrides = {}) {
  const root = await mkdtemp(join(tmpdir(), "arca-release-version-"));
  await mkdir(join(root, "core"), { recursive: true });
  await mkdir(join(root, "cli"), { recursive: true });
  await mkdir(join(root, "desktop", "src-tauri"), { recursive: true });

  const version = overrides.version ?? "1.2.18";
  await writeFile(join(root, "version.txt"), `${version}\n`);
  await writeFile(
    join(root, "core", "Cargo.toml"),
    `[package]\nname = "arca-core"\nversion = "${overrides.core ?? version}"\n`,
  );
  await writeFile(
    join(root, "cli", "Cargo.toml"),
    `[package]\nname = "arca-cli"\nversion = "${overrides.cli ?? version}"\n`,
  );
  await writeJson(join(root, "desktop", "package.json"), {
    name: "desktop",
    version: overrides.packageJson ?? version,
  });
  await writeJson(join(root, "desktop", "package-lock.json"), {
    name: "desktop",
    version: overrides.lockRoot ?? version,
    lockfileVersion: 3,
    packages: {
      "": {
        name: "desktop",
        version: overrides.lockPackage ?? version,
      },
      ...overrides.lockPackages,
    },
  });
  await writeFile(
    join(root, "desktop", "src-tauri", "Cargo.toml"),
    `[package]\nname = "desktop"\nversion = "${overrides.tauriCargo ?? version}"\n`,
  );
  await writeJson(join(root, "desktop", "src-tauri", "tauri.conf.json"), {
    productName: "Arca",
    version: overrides.tauriConfig ?? version,
  });

  return root;
}

test("all release-owned version fields agree", async () => {
  const root = await createFixture();
  assert.deepEqual(await collectVersionMismatches(root), []);
});

test("refreshes only local Cargo lock packages", () => {
  const lock = `version = 4

[[package]]
name = "arca-core"
version = "1.2.18"

[[package]]
name = "desktop"
version = "1.2.18"

[[package]]
name = "desktop"
version = "9.9.9"
source = "registry+https://github.com/rust-lang/crates.io-index"
`;

  assert.equal(
    updateCargoLockPackageVersions(
      lock,
      ["arca-core", "desktop"],
      "1.2.19",
    ),
    lock
      .replace('name = "arca-core"\nversion = "1.2.18"', 'name = "arca-core"\nversion = "1.2.19"')
      .replace('name = "desktop"\nversion = "1.2.18"', 'name = "desktop"\nversion = "1.2.19"'),
  );
  assert.throws(
    () => updateCargoLockPackageVersions(lock, ["arca-cli"], "1.2.19"),
    /exactly one local arca-cli package; found 0/,
  );
});

test("uses the explicitly selected semantic release scope", () => {
  assert.equal(nextReleaseVersion("1.8.239", "patch"), "1.8.240");
  assert.equal(nextReleaseVersion("1.8.239", "minor"), "1.9.0");
  assert.equal(nextReleaseVersion("1.8.239", "major"), "2.0.0");
  assert.equal(nextReleaseVersion("12.0.99999999999999999999", "patch"), "12.0.100000000000000000000");
  assert.throws(() => nextReleaseVersion("v1.8.5", "patch"), /invalid semantic version/);
  for (const scope of [undefined, "", "auto", "toString"]) {
    assert.throws(() => nextReleaseVersion("1.8.239", scope), /release type must be/);
  }
});

test("updates only the Cargo package version", () => {
  const manifest = `[package]
name = "arca"
version = "1.8.5" # release-owned

[dependencies]
version = "9.9.9"
`;
  assert.equal(
    updateCargoPackageVersion(manifest, "1.8.6"),
    manifest.replace('version = "1.8.5"', 'version = "1.8.6"'),
  );
  assert.throws(
    () => updateCargoPackageVersion("[dependencies]\nversion = \"1.0.0\"\n", "1.8.6"),
    /exactly one \[package\] version; found 0/,
  );
});

test("reports both npm lockfile fields independently", async () => {
  const root = await createFixture({
    lockRoot: "1.2.16",
    lockPackage: "1.2.17",
  });
  const mismatches = await collectVersionMismatches(root);

  assert.ok(
    mismatches.some((message) =>
      message.includes("desktop/package-lock.json.version"),
    ),
  );
  assert.ok(
    mismatches.some((message) =>
      message.includes('desktop/package-lock.json.packages[""].version'),
    ),
  );
});

test("reports transitive npm packages missing from a portable lockfile", async () => {
  const root = await createFixture({
    lockPackages: {
      "node_modules/bundler": {
        version: "1.0.0",
        optionalDependencies: {
          "bundler-linux-x64": "1.0.0",
        },
      },
      "node_modules/bundler-linux-x64": {
        version: "1.0.0",
        optional: true,
        dependencies: {
          "bundler-runtime": "1.0.0",
        },
      },
    },
  });
  const mismatches = await collectVersionMismatches(root);

  assert.ok(
    mismatches.some(
      (message) =>
        message.includes("bundler-runtime") &&
        message.includes("node_modules/bundler-linux-x64"),
    ),
  );
});

test("reports the manifest path when a Cargo package differs", async () => {
  const root = await createFixture({ cli: "1.2.17" });
  const mismatches = await collectVersionMismatches(root);

  assert.ok(
    mismatches.some(
      (message) =>
        message.includes("cli/Cargo.toml") && message.includes("1.2.17"),
    ),
  );
});

test("rejects a stable tag that does not match the version source", async () => {
  const root = await createFixture();
  const mismatches = await collectVersionMismatches(root, {
    stableTag: "v1.2.17",
  });

  assert.ok(mismatches.some((message) => message.includes("stable tag")));
});

test("rejects malformed version.txt values", async () => {
  const root = await createFixture({ version: "v1.2.18" });
  const mismatches = await collectVersionMismatches(root);

  assert.ok(mismatches.some((message) => message.includes("version.txt")));
});
