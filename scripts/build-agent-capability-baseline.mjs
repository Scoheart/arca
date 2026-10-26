#!/usr/bin/env node

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const audited = JSON.parse(await readFile(resolve(root, "data/agents.json"), "utf8"));
const catalog = JSON.parse(await readFile(resolve(root, "data/agent-catalog.json"), "utf8"));
const discovery = JSON.parse(await readFile(resolve(root, "analysis/agent-capability-audit/catalog-source-discovery.json"), "utf8"));
const acp = JSON.parse(await readFile(resolve(root, "analysis/agent-capability-audit/acp-registry-source-discovery.json"), "utf8"));
const modelSource = await readFile(resolve(root, "core/src/resources/model/mod.rs"), "utf8");
const outputJson = resolve(root, "analysis/agent-capability-audit/coverage-baseline.json");
const outputMarkdown = resolve(root, "analysis/agent-capability-audit/coverage-baseline.md");

const sourceSnapshotDate = [
  ...Object.values(audited).flatMap((definition) => [
    definition.verified_at,
    definition.skills?.verified_at,
  ]),
  ...Object.values(catalog).map((definition) => definition.verified_at),
]
  .filter((date) => typeof date === "string" && /^\d{4}-\d{2}-\d{2}$/.test(date))
  .sort()
  .at(-1) ?? null;

const defaultPathsBlock = modelSource.match(/pub fn default_config_paths[\s\S]*?\n}\n\npub fn normalize_config_paths/)?.[0] ?? "";
if (defaultPathsBlock.length === 0) {
  throw new Error("Could not locate models::default_config_paths; refusing to emit an incomplete baseline");
}
const modelPaths = new Map();
for (const match of defaultPathsBlock.matchAll(/"([^"]+)"\s*=>\s*&\[([^\]]*)\]/g)) {
  modelPaths.set(match[1], [...match[2].matchAll(/"([^"]+)"/g)].map((path) => path[1]));
}
if (modelPaths.size === 0) {
  throw new Error("models::default_config_paths contained no parseable Agent mappings");
}

const listAgentsBlock = modelSource.match(/pub fn list_agents\(\) -> Vec<ModelAgentView> \{[\s\S]*?\n\}\n\nfn managed_agent_view/)?.[0] ?? "";
if (listAgentsBlock.length === 0) {
  throw new Error("Could not locate models::list_agents; refusing to emit an incomplete baseline");
}
const modelModes = new Map();
for (const match of listAgentsBlock.matchAll(/ModelAgentView\s*\{\s*id:\s*"([^"]+)"\.into\(\),[\s\S]*?mode:\s*"(managed|guided)"\.into\(\),/g)) {
  modelModes.set(match[1], match[2]);
}
for (const match of listAgentsBlock.matchAll(/managed_agent_view\(\s*&settings,\s*"([^"]+)"/g)) {
  modelModes.set(match[1], "managed");
}
if (modelModes.size === 0) {
  throw new Error("models::list_agents contained no parseable managed or guided Agent mappings");
}
const modelPathIds = [...modelPaths.keys()].sort();
const modelModeIds = [...modelModes.keys()].sort();
if (JSON.stringify(modelPathIds) !== JSON.stringify(modelModeIds)) {
  throw new Error(`Model path/mode definitions drifted: paths=${modelPathIds.join(",")} modes=${modelModeIds.join(",")}`);
}

const discoveryById = new Map(discovery.records.map((record) => [record.id, record]));
const acpById = new Map(acp.records.map((record) => [record.suggestedMuxId, record]));
const githubEvidence = (url) => typeof url === "string" && /^https:\/\/github\.com\//i.test(url);
const ids = [...new Set([...Object.keys(audited), ...Object.keys(catalog), ...acpById.keys()])].sort();
const records = ids.map((id) => {
  const acpRecord = acpById.get(id);
  const definition = audited[id] ?? catalog[id] ?? {
    name: acpRecord.name,
    evidence: "acp-registry",
    docs: acpRecord.repository ?? acpRecord.website ?? acpRecord.manifestUrl,
  };
  const source = discoveryById.get(id);
  const registeredEvidenceUrls = [
    definition.docs,
    audited[id]?.skills?.docs,
  ].filter((url) => typeof url === "string" && url.length > 0);
  return {
    id,
    name: definition.name,
    audited: Boolean(audited[id]),
    inCatalog: Boolean(catalog[id]),
    inAcpRegistry: Boolean(acpRecord),
    mcp: audited[id]?.global
      ? { state: "writable", global: audited[id].global, project: audited[id].project, format: audited[id].format, key: audited[id].key, layout: audited[id].layout, codec: audited[id].codec, transports: audited[id].transports }
      : { state: audited[id] ? "read-only" : "discovery-only", global: null, project: null, format: "unknown", key: null, layout: null, codec: null, transports: [] },
    models: modelPaths.has(id)
      ? { state: "supported-or-guided", mode: modelModes.get(id), paths: modelPaths.get(id) }
      : { state: "not-integrated", mode: null, paths: [] },
    skills: audited[id]?.skills
      ? { state: "supported", globalDir: audited[id].skills.global_dir, aliases: audited[id].skills.aliases ?? [] }
      : { state: "not-integrated", globalDir: null, aliases: [] },
    currentEvidence: definition.evidence ?? null,
    currentDocs: definition.docs ?? null,
    registeredEvidenceUrls,
    githubCandidates: [...new Set([
      ...(source?.githubCandidates ?? []),
      ...(acpRecord?.repository?.startsWith("https://github.com/") ? [acpRecord.repository] : []),
      ...registeredEvidenceUrls.filter(githubEvidence),
    ])],
    externalSites: source?.externalSites ?? [],
    acpIdentity: acpRecord ?? null,
  };
});

const summary = {
  identities: records.length,
  audited: records.filter((record) => record.audited).length,
  catalogEntries: Object.keys(catalog).length,
  catalogOnly: records.filter((record) => !record.audited).length,
  acpRegistry: records.filter((record) => record.inAcpRegistry).length,
  acpOnly: records.filter((record) => record.inAcpRegistry && !record.audited && !record.inCatalog).length,
  mcpWritable: records.filter((record) => record.mcp.state === "writable").length,
  modelTargets: modelModes.size,
  modelManaged: [...modelModes.values()].filter((mode) => mode === "managed").length,
  modelGuided: [...modelModes.values()].filter((mode) => mode === "guided").length,
  skillsIntegrated: records.filter((record) => record.skills.state === "supported").length,
  withGithubCandidates: records.filter((record) => record.githubCandidates.length > 0).length,
};

await mkdir(dirname(outputJson), { recursive: true });
await writeFile(outputJson, `${JSON.stringify({ schemaVersion: 1, sourceSnapshotDate, summary, records }, null, 2)}\n`);

const lines = [
  "# Agent capability coverage baseline",
  "",
  "> Generated from current Arca sources. This is an implementation baseline, not research proof.",
  ...(sourceSnapshotDate ? [`> Source snapshot date: ${sourceSnapshotDate}.`] : []),
  "",
  ...Object.entries(summary).map(([key, value]) => `- ${key}: ${value}`),
  "",
  "| ID | Name | Audited | ACP | MCP | Models | Skills | GitHub candidates |",
  "|---|---|---:|---:|---|---|---|---:|",
  ...records.map((record) => `| ${record.id} | ${record.name.replaceAll("|", "\\|")} | ${record.audited ? "yes" : "no"} | ${record.inAcpRegistry ? "yes" : "no"} | ${record.mcp.state} | ${record.models.state} | ${record.skills.state} | ${record.githubCandidates.length} |`),
];
await writeFile(outputMarkdown, `${lines.join("\n")}\n`);
console.log(JSON.stringify({ outputJson, outputMarkdown, summary }));
