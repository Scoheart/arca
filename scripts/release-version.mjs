#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { copyFile, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SEMVER_PATTERN = /^\d+\.\d+\.\d+$/;
const STABLE_TAG_PATTERN = /^v(\d+\.\d+\.\d+)$/;
const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const SOURCE_FIELDS = [
  ["core/Cargo.toml", "cargo"],
  ["cli/Cargo.toml", "cargo"],
  ["desktop/package.json", "json", ["version"]],
  ["desktop/src-tauri/Cargo.toml", "cargo"],
  ["desktop/src-tauri/tauri.conf.json", "json", ["version"]],
];

const GENERATED_FIELDS = [
  ["desktop/package-lock.json", "json", ["version"]],
  ["desktop/package-lock.json", "json", ["packages", "", "version"]],
];

function displayPath(root, path) {
  return relative(root, path).replaceAll("\\", "/");
}

function formatJsonField(path) {
  return path
    .map((part, index) => {
      if (part === "") return '[""]';
      return index === 0 ? part : `.${part}`;
    })
    .join("");
}

function jsonValueAt(value, path) {
  let current = value;
  for (const part of path) {
    if (
      current === null ||
      typeof current !== "object" ||
      !Object.hasOwn(current, part)
    ) {
      return undefined;
    }
    current = current[part];
  }
  return current;
}

export function readCargoPackageVersion(contents, path = "Cargo.toml") {
  let inPackage = false;
  const versions = [];

  for (const line of contents.split(/\r?\n/)) {
    const section = line.match(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/);
    if (section) {
      inPackage = section[1] === "package";
      continue;
    }
    if (!inPackage) continue;

    const version = line.match(
      /^\s*version\s*=\s*"([^"]+)"\s*(?:#.*)?$/,
    );
    if (version) versions.push(version[1]);
  }

  if (versions.length !== 1) {
    throw new Error(
      `${path} must contain exactly one [package] version; found ${versions.length}`,
    );
  }
  return versions[0];
}

export function nextReleaseVersion(version, releaseType) {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!match) {
    throw new Error(`invalid semantic version: ${JSON.stringify(version)}`);
  }
  const index = ["major", "minor", "patch"].indexOf(releaseType);
  if (index < 0) {
    throw new Error("release type must be major, minor, or patch");
  }
  const parts = match.slice(1).map(BigInt);
  parts[index] += 1n;
  parts.fill(0n, index + 1);
  return parts.join(".");
}

export function updateCargoPackageVersion(
  contents,
  version,
  path = "Cargo.toml",
) {
  if (!SEMVER_PATTERN.test(version)) {
    throw new Error(`invalid Cargo package version: ${JSON.stringify(version)}`);
  }

  let inPackage = false;
  let updates = 0;
  const refreshed = contents
    .split(/(\r?\n)/)
    .map((line) => {
      const section = line.match(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/);
      if (section) {
        inPackage = section[1] === "package";
        return line;
      }
      if (!inPackage) return line;

      const match = line.match(
        /^(\s*version\s*=\s*)"[^"]+"(\s*(?:#.*)?)$/,
      );
      if (!match) return line;
      updates += 1;
      return `${match[1]}"${version}"${match[2]}`;
    })
    .join("");

  if (updates !== 1) {
    throw new Error(
      `${path} must contain exactly one [package] version; found ${updates}`,
    );
  }
  return refreshed;
}

async function readExpectedVersion(root, mismatches) {
  const path = join(root, "version.txt");
  try {
    const version = (await readFile(path, "utf8")).trim();
    if (!SEMVER_PATTERN.test(version)) {
      mismatches.push(
        `version.txt must contain MAJOR.MINOR.PATCH, received ${JSON.stringify(version)}`,
      );
      return null;
    }
    return version;
  } catch (error) {
    mismatches.push(`version.txt cannot be read: ${error.message}`);
    return null;
  }
}

async function inspectField(root, definition, expected, mismatches) {
  const [relativePath, kind, fieldPath] = definition;
  const path = join(root, relativePath);
  let actual;

  try {
    const contents = await readFile(path, "utf8");
    if (kind === "cargo") {
      actual = readCargoPackageVersion(contents, relativePath);
    } else {
      const value = JSON.parse(contents);
      actual = jsonValueAt(value, fieldPath);
      if (typeof actual !== "string") {
        throw new Error(
          `${relativePath}.${formatJsonField(fieldPath)} must be a string`,
        );
      }
    }
  } catch (error) {
    mismatches.push(`${displayPath(root, path)}: ${error.message}`);
    return;
  }

  if (actual !== expected) {
    const field =
      kind === "cargo"
        ? relativePath
        : `${relativePath}.${formatJsonField(fieldPath)}`;
    mismatches.push(`${field} is ${actual}; expected ${expected}`);
  }
}

function resolveLockPackage(packages, parentPath, dependency) {
  let base = parentPath;
  while (true) {
    const candidate = base
      ? `${base}/node_modules/${dependency}`
      : `node_modules/${dependency}`;
    if (Object.hasOwn(packages, candidate)) return candidate;
    if (!base) return null;

    const parentMarker = base.lastIndexOf("/node_modules/");
    base = parentMarker === -1 ? "" : base.slice(0, parentMarker);
  }
}

function npmLockClosureMismatches(lock, relativePath) {
  const mismatches = [];
  const packages = lock.packages;
  if (packages === null || typeof packages !== "object") {
    return [`${relativePath}: packages must be an object`];
  }

  for (const [packagePath, metadata] of Object.entries(packages)) {
    for (const group of ["dependencies", "optionalDependencies"]) {
      const dependencies = metadata?.[group];
      if (dependencies === null || typeof dependencies !== "object") continue;

      for (const dependency of Object.keys(dependencies)) {
        if (!resolveLockPackage(packages, packagePath, dependency)) {
          mismatches.push(
            `${relativePath}: ${packagePath || "<root>"} ${group} entry ${dependency} is missing from the portable lockfile`,
          );
        }
      }
    }
  }
  return mismatches;
}

async function inspectNpmLockClosure(root, mismatches) {
  const relativePath = "desktop/package-lock.json";
  const path = join(root, relativePath);

  try {
    const lock = JSON.parse(await readFile(path, "utf8"));
    mismatches.push(...npmLockClosureMismatches(lock, relativePath));
  } catch (error) {
    mismatches.push(`${relativePath}: ${error.message}`);
  }
}

export async function collectVersionMismatches(
  root = REPOSITORY_ROOT,
  { includeGenerated = true, stableTag } = {},
) {
  const mismatches = [];
  const expected = await readExpectedVersion(root, mismatches);
  if (!expected) return mismatches;

  const definitions = includeGenerated
    ? [...SOURCE_FIELDS, ...GENERATED_FIELDS]
    : SOURCE_FIELDS;
  await Promise.all(
    definitions.map((definition) =>
      inspectField(root, definition, expected, mismatches),
    ),
  );
  if (includeGenerated) await inspectNpmLockClosure(root, mismatches);

  if (stableTag !== undefined) {
    const match = stableTag.match(STABLE_TAG_PATTERN);
    if (!match) {
      mismatches.push(
        `stable tag must match vMAJOR.MINOR.PATCH, received ${JSON.stringify(stableTag)}`,
      );
    } else if (match[1] !== expected) {
      mismatches.push(`stable tag ${stableTag} does not match version ${expected}`);
    }
  }

  return mismatches;
}

function run(command, args, cwd, stdio = "inherit") {
  execFileSync(command, args, { cwd, stdio });
}

export function updateCargoLockPackageVersions(
  contents,
  packageNames,
  version,
  path = "Cargo.lock",
) {
  if (!SEMVER_PATTERN.test(version)) {
    throw new Error(`invalid Cargo package version: ${JSON.stringify(version)}`);
  }

  const expected = new Set(packageNames);
  const updates = new Map(packageNames.map((name) => [name, 0]));
  const blocks = contents.split(/(?=^\[\[package\]\]\r?$)/m);

  const refreshed = blocks.map((block) => {
    const name = block.match(/^name[ \t]*=[ \t]*"([^"]+)"[ \t]*$/m)?.[1];
    if (!expected.has(name) || /^source[ \t]*=/m.test(block)) return block;

    const versions = [
      ...block.matchAll(/^version[ \t]*=[ \t]*"([^"]+)"[ \t]*$/gm),
    ];
    if (versions.length !== 1) {
      throw new Error(
        `${path} local package ${name} must contain exactly one version`,
      );
    }
    updates.set(name, updates.get(name) + 1);
    return block.replace(
      /^version[ \t]*=[ \t]*"[^"]+"[ \t]*$/m,
      `version = "${version}"`,
    );
  });

  for (const [name, count] of updates) {
    if (count !== 1) {
      throw new Error(
        `${path} must contain exactly one local ${name} package; found ${count}`,
      );
    }
  }
  return refreshed.join("");
}

async function refreshCargoLocks(root, version) {
  const locks = [
    ["Cargo.lock", ["arca-core", "arca-cli"]],
    ["desktop/src-tauri/Cargo.lock", ["desktop", "arca-core"]],
  ];

  await Promise.all(
    locks.map(async ([relativePath, packages]) => {
      const path = join(root, relativePath);
      const contents = await readFile(path, "utf8");
      const refreshed = updateCargoLockPackageVersions(
        contents,
        packages,
        version,
        relativePath,
      );
      if (refreshed !== contents) await writeFile(path, refreshed);
    }),
  );
}

function cargoLockErrors(root) {
  const commands = [
    ["cargo", ["metadata", "--locked", "--no-deps", "--format-version", "1"]],
    [
      "cargo",
      [
        "metadata",
        "--locked",
        "--no-deps",
        "--format-version",
        "1",
        "--manifest-path",
        "desktop/src-tauri/Cargo.toml",
      ],
    ],
  ];
  const errors = [];

  for (const [command, args] of commands) {
    try {
      run(command, args, root, "pipe");
    } catch (error) {
      const details = error.stderr?.toString().trim() || error.message;
      errors.push(`${command} ${args.join(" ")} failed: ${details}`);
    }
  }
  return errors;
}

async function failOnMismatches(mismatches) {
  if (mismatches.length === 0) return;
  for (const mismatch of mismatches) console.error(`- ${mismatch}`);
  throw new Error(`release version check failed with ${mismatches.length} error(s)`);
}

async function refreshNpmLock(root) {
  const desktop = join(root, "desktop");
  const packageJson = join(desktop, "package.json");
  const packageLock = join(desktop, "package-lock.json");
  const temporary = await mkdtemp(join(tmpdir(), "arca-desktop-lock-"));

  try {
    await copyFile(packageJson, join(temporary, "package.json"));
    const manifest = JSON.parse(await readFile(packageJson, "utf8"));
    if (!SEMVER_PATTERN.test(manifest.version)) {
      throw new Error("desktop/package.json must contain a semantic version");
    }
    let rebuild = true;
    try {
      const current = JSON.parse(await readFile(packageLock, "utf8"));
      const closureErrors = npmLockClosureMismatches(
        current,
        "desktop/package-lock.json",
      );
      if (closureErrors.length === 0) {
        await copyFile(packageLock, join(temporary, "package-lock.json"));
        rebuild = false;
      } else {
        console.warn(
          `Rebuilding a portable desktop lockfile because ${closureErrors.length} dependency entry or entries are missing.`,
        );
      }
    } catch (error) {
      console.warn(`Rebuilding unreadable desktop lockfile: ${error.message}`);
    }

    if (rebuild) {
      run(
        "npm",
        [
          "install",
          "--package-lock-only",
          "--ignore-scripts",
          "--no-audit",
          "--no-fund",
        ],
        temporary,
      );
    } else {
      run(
        "npm",
        [
          "version",
          manifest.version,
          "--allow-same-version",
          "--no-git-tag-version",
          "--ignore-scripts",
        ],
        temporary,
        "ignore",
      );
    }
    await copyFile(join(temporary, "package-lock.json"), packageLock);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

async function check(root, stableTag) {
  const mismatches = await collectVersionMismatches(root, { stableTag });
  mismatches.push(...cargoLockErrors(root));
  await failOnMismatches(mismatches);
  console.log("Release version metadata is consistent.");
}

async function refreshLocks(root) {
  await failOnMismatches(
    await collectVersionMismatches(root, { includeGenerated: false }),
  );
  const expected = (await readFile(join(root, "version.txt"), "utf8")).trim();
  await refreshNpmLock(root);
  await refreshCargoLocks(root, expected);
  await check(root);
}

function updateJsonString(contents, fieldPath, value, path) {
  const parsed = JSON.parse(contents);
  let parent = parsed;
  for (const field of fieldPath.slice(0, -1)) {
    if (
      parent === null ||
      typeof parent !== "object" ||
      !Object.hasOwn(parent, field)
    ) {
      throw new Error(`${path}.${formatJsonField(fieldPath)} does not exist`);
    }
    parent = parent[field];
  }
  const finalField = fieldPath.at(-1);
  if (
    parent === null ||
    typeof parent !== "object" ||
    typeof parent[finalField] !== "string"
  ) {
    throw new Error(`${path}.${formatJsonField(fieldPath)} must be a string`);
  }
  parent[finalField] = value;
  return `${JSON.stringify(parsed, null, 2)}\n`;
}

function changelogCommits(root, previousVersion, sourceSha) {
  const range = `v${previousVersion}..${sourceSha}`;
  const output = execFileSync(
    "git",
    ["log", "--reverse", "--format=%H%x09%s", range],
    { cwd: root, encoding: "utf8" },
  ).trim();
  if (!output) throw new Error(`no commits found in ${range}`);

  return output.split("\n").map((line) => {
    const separator = line.indexOf("\t");
    if (separator !== 40) {
      throw new Error(`unexpected git log entry in ${range}`);
    }
    const sha = line.slice(0, separator);
    const title = line.slice(separator + 1).replaceAll("[", "\\[");
    return `* ${title} ([${sha.slice(0, 7)}](https://github.com/Scoheart/arca/commit/${sha}))`;
  });
}

export async function prepareDirectRelease(root, sourceSha, releaseType) {
  if (!/^[0-9a-f]{40}$/.test(sourceSha)) {
    throw new Error("prepare-direct requires a full lowercase commit SHA");
  }
  execFileSync("git", ["cat-file", "-e", `${sourceSha}^{commit}`], {
    cwd: root,
    stdio: "pipe",
  });

  const versionPath = join(root, "version.txt");
  const previousVersion = (await readFile(versionPath, "utf8")).trim();
  if (!SEMVER_PATTERN.test(previousVersion)) {
    throw new Error("version.txt must contain MAJOR.MINOR.PATCH");
  }
  const version = nextReleaseVersion(previousVersion, releaseType);

  for (const [relativePath, kind, fieldPath] of SOURCE_FIELDS) {
    const path = join(root, relativePath);
    const contents = await readFile(path, "utf8");
    const refreshed =
      kind === "cargo"
        ? updateCargoPackageVersion(contents, version, relativePath)
        : updateJsonString(contents, fieldPath, version, relativePath);
    await writeFile(path, refreshed);
  }
  await writeFile(versionPath, `${version}\n`);
  // Keep published documentation metadata on the same generated release version.
  const referencePath = join(root, "website/.vitepress/reference.json");
  try {
    const reference = await readFile(referencePath, "utf8");
    await writeFile(referencePath, updateJsonString(reference, ["version"], version, referencePath));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }

  const changelogPath = join(root, "CHANGELOG.md");
  const changelog = await readFile(changelogPath, "utf8");
  const heading = "# Changelog\n";
  if (!changelog.startsWith(heading)) {
    throw new Error("CHANGELOG.md must start with # Changelog");
  }
  if (changelog.includes(`## [${version}]`)) {
    throw new Error(`CHANGELOG.md already contains ${version}`);
  }
  const date = new Date().toISOString().slice(0, 10);
  const commits = changelogCommits(root, previousVersion, sourceSha);
  const entry = [
    "",
    `## [${version}](https://github.com/Scoheart/arca/compare/v${previousVersion}...v${version}) (${date})`,
    "",
    "### Changes",
    "",
    ...commits,
    "",
  ].join("\n");
  await writeFile(changelogPath, `${heading}${entry}${changelog.slice(heading.length)}`);

  await refreshLocks(root);
  console.log(`Prepared direct stable version ${version}.`);
  return version;
}

function parseArguments(argv) {
  const [command, ...args] = argv;
  let root = REPOSITORY_ROOT;
  let stableTag;
  let sourceSha;
  let releaseType;

  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === "--root" && args[index + 1]) {
      root = resolve(args[++index]);
    } else if (args[index] === "--tag" && args[index + 1]) {
      stableTag = args[++index];
    } else if (args[index] === "--source" && args[index + 1]) {
      sourceSha = args[++index];
    } else if (args[index] === "--bump" && args[index + 1]) {
      releaseType = args[++index];
    } else {
      throw new Error(`unknown argument: ${args[index]}`);
    }
  }
  return { command, root, stableTag, sourceSha, releaseType };
}

async function main() {
  const { command, root, stableTag, sourceSha, releaseType } = parseArguments(
    process.argv.slice(2),
  );
  if (command === "check") {
    await check(root, stableTag);
  } else if (command === "refresh-locks") {
    if (stableTag) throw new Error("refresh-locks does not accept --tag");
    await refreshLocks(root);
  } else if (command === "prepare-direct") {
    if (sourceSha === undefined) {
      throw new Error("prepare-direct requires --source");
    }
    if (releaseType === undefined) {
      throw new Error("prepare-direct requires --bump <patch|minor|major>");
    }
    await prepareDirectRelease(root, sourceSha, releaseType);
  } else {
    throw new Error(
      "usage: node scripts/release-version.mjs <check|refresh-locks|prepare-direct> [options]",
    );
  }
}

const isEntrypoint =
  process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url;
if (isEntrypoint) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
