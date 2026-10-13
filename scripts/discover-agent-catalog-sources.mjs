#!/usr/bin/env node

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const catalog = JSON.parse(await readFile(resolve(root, "data/agent-catalog.json"), "utf8"));
const output = resolve(root, "analysis/agent-capability-audit/catalog-source-discovery.json");
const ignoredGithubOwners = new Set(["glama-ai", "punkpeye"]);
const checkedAt = process.env.ARCA_AUDIT_DATE || new Intl.DateTimeFormat("en-CA", {
  timeZone: "Asia/Shanghai",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
}).format(new Date());
if (!/^\d{4}-\d{2}-\d{2}$/.test(checkedAt)) {
  throw new Error(`ARCA_AUDIT_DATE must use YYYY-MM-DD, received: ${checkedAt}`);
}

function decodeHtml(value) {
  return value
    .replaceAll("&amp;", "&")
    .replaceAll("&quot;", '"')
    .replaceAll("&#x27;", "'")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">");
}

function githubRepos(html) {
  const urls = [...html.matchAll(/https:\/\/github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)/g)]
    .map((match) => ({ owner: match[1], repo: match[2].replace(/\.$/, "") }))
    .filter(({ owner, repo }) => !ignoredGithubOwners.has(owner.toLowerCase()) && repo !== "sponsors")
    .map(({ owner, repo }) => `https://github.com/${owner}/${repo}`);
  return [...new Set(urls)].sort();
}

function meta(html, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = html.match(new RegExp(`<meta[^>]+(?:name|property)=["']${escaped}["'][^>]+content=["']([^"']*)["']`, "i"))
    ?? html.match(new RegExp(`<meta[^>]+content=["']([^"']*)["'][^>]+(?:name|property)=["']${escaped}["']`, "i"));
  return match ? decodeHtml(match[1]) : null;
}

function externalSites(html) {
  const urls = [...html.matchAll(/<a\s+href="(https?:\/\/[^"#?]+)"/g)]
    .map((match) => decodeHtml(match[1]).replace(/\/$/, ""))
    .filter((url) => !url.includes("glama.ai") && !url.includes("github.com"));
  return [...new Set(urls)].sort();
}

async function fetchWithRetry(url) {
  let lastError;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      const response = await fetch(url, { headers: { "user-agent": "Arca-Agent-Capability-Audit/1.0" } });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return await response.text();
    } catch (error) {
      lastError = error;
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 250 * (attempt + 1)));
    }
  }
  throw lastError;
}

const entries = Object.entries(catalog).sort(([left], [right]) => left.localeCompare(right));
const results = new Array(entries.length);
let cursor = 0;

async function worker() {
  while (cursor < entries.length) {
    const index = cursor;
    cursor += 1;
    const [id, definition] = entries[index];
    const base = {
      id,
      name: definition.name,
      catalogUrl: definition.docs,
      existingEvidence: definition.evidence,
      checkedAt,
    };
    if (!definition.docs?.startsWith("https://glama.ai/")) {
      results[index] = {
        ...base,
        discoveryStatus: "official-link-present",
        description: null,
        githubCandidates: definition.docs?.includes("github.com/") ? [definition.docs] : [],
        externalSites: definition.docs ? [definition.docs] : [],
        error: null,
      };
      continue;
    }
    try {
      const html = await fetchWithRetry(definition.docs);
      results[index] = {
        ...base,
        discoveryStatus: "catalog-page-read",
        description: meta(html, "description"),
        githubCandidates: githubRepos(html),
        externalSites: externalSites(html),
        error: null,
      };
    } catch (error) {
      results[index] = {
        ...base,
        discoveryStatus: "fetch-failed",
        description: null,
        githubCandidates: [],
        externalSites: [],
        error: String(error?.message ?? error),
      };
    }
  }
}

await Promise.all(Array.from({ length: 8 }, () => worker()));
await mkdir(dirname(output), { recursive: true });
await writeFile(output, `${JSON.stringify({ schemaVersion: 1, generatedAt: new Date().toISOString(), records: results }, null, 2)}\n`);

const counts = results.reduce((summary, record) => {
  summary[record.discoveryStatus] = (summary[record.discoveryStatus] ?? 0) + 1;
  if (record.githubCandidates.length > 0) summary.withGithubCandidates += 1;
  return summary;
}, { withGithubCandidates: 0 });
console.log(JSON.stringify({ output, records: results.length, ...counts }));
