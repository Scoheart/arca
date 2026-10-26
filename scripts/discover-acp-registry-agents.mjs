#!/usr/bin/env node

import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(root, "analysis/agent-capability-audit/acp-registry-source-discovery.json");
const repository = "agentclientprotocol/registry";
const checkedAt = new Intl.DateTimeFormat("en-CA", {
  timeZone: "Asia/Shanghai",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
}).format(new Date());
const aliases = {
  "amp-acp": "amp",
  auggie: "augment",
  "claude-acp": "claude-code",
  "codex-acp": "codex",
  "github-copilot-cli": "copilot-cli",
  kilo: "kilo-code",
  kimi: "kimi-code",
  "pi-acp": "pi",
  qoder: "qoder-cli",
  vtcode: "vt-code",
};

async function github(path) {
  const suffix = path ? `/${path}` : "";
  const response = await fetch(`https://api.github.com/repos/${repository}${suffix}`, {
    headers: {
      accept: "application/vnd.github+json",
      "user-agent": "Arca-Agent-Capability-Audit/1.0",
      "x-github-api-version": "2022-11-28",
    },
  });
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  return response.json();
}

async function mapWithConcurrency(items, limit, mapper) {
  const results = new Array(items.length);
  let cursor = 0;
  const workers = Array.from(
    { length: Math.min(limit, items.length) },
    async () => {
      while (cursor < items.length) {
        const index = cursor;
        cursor += 1;
        results[index] = await mapper(items[index], index);
      }
    },
  );
  await Promise.all(workers);
  return results;
}

const repo = await github("");
const defaultBranch = repo.default_branch;
if (typeof defaultBranch !== "string" || defaultBranch.length === 0) {
  throw new Error("ACP Registry response has no default branch");
}

// Resolve the moving default branch once, then pin every subsequent read and
// audit URL to the same immutable commit snapshot.
const commit = await github(`commits/${encodeURIComponent(defaultBranch)}`);
const commitSha = commit.sha;
const treeSha = commit.commit?.tree?.sha;
if (!/^[0-9a-f]{40}$/i.test(commitSha) || !/^[0-9a-f]{40}$/i.test(treeSha)) {
  throw new Error("ACP Registry response has an invalid commit or tree SHA");
}

const tree = await github(`git/trees/${treeSha}?recursive=1`);
if (tree.truncated) throw new Error("ACP Registry tree response is truncated");
if (tree.sha !== treeSha) throw new Error("ACP Registry returned an unexpected tree SHA");
const manifests = tree.tree
  .filter((entry) => /^[^/]+\/agent\.json$/.test(entry.path) && entry.type === "blob")
  .sort((left, right) => left.path.localeCompare(right.path));

const records = await mapWithConcurrency(manifests, 6, async (entry) => {
  const manifestRawUrl = `https://raw.githubusercontent.com/${repository}/${commitSha}/${entry.path}`;
  const response = await fetch(manifestRawUrl, {
    headers: { "user-agent": "Arca-Agent-Capability-Audit/1.0" },
  });
  if (!response.ok) throw new Error(`${entry.path}: HTTP ${response.status}`);
  const manifest = await response.json();
  return {
    registryId: manifest.id,
    suggestedMuxId: aliases[manifest.id] ?? manifest.id,
    name: manifest.name,
    version: manifest.version,
    description: manifest.description,
    repository: manifest.repository ?? null,
    website: manifest.website ?? null,
    license: manifest.license ?? null,
    distributionKinds: Object.keys(manifest.distribution ?? {}).sort(),
    manifestUrl: `https://github.com/${repository}/blob/${commitSha}/${entry.path}`,
    manifestRawUrl,
    checkedAt,
  };
});

await mkdir(dirname(output), { recursive: true });
await writeFile(output, `${JSON.stringify({
  schemaVersion: 1,
  generatedAt: new Date().toISOString(),
  registryDefaultBranch: defaultBranch,
  registryCommit: commitSha,
  registryCommitUrl: `https://github.com/${repository}/commit/${commitSha}`,
  registryTree: treeSha,
  records,
}, null, 2)}\n`);
console.log(JSON.stringify({
  output,
  registryDefaultBranch: defaultBranch,
  registryCommit: commitSha,
  registryTree: treeSha,
  records: records.length,
}));
