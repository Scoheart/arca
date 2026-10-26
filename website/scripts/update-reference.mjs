#!/usr/bin/env node
// Regenerate public documentation metadata from the registry and a released CLI.
// Usage: node scripts/update-reference.mjs --arca /absolute/path/to/arca
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const website = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(website, "..");
const index = process.argv.indexOf("--arca");
if (index < 0 || !process.argv[index + 1]) throw new Error("Pass --arca /absolute/path/to/arca");
const cli = resolve(process.argv[index + 1]);
const sandbox = mkdtempSync(resolve(tmpdir(), "arca-doc-reference-"));
try {
  const env = { ...process.env, HOME: sandbox, ARCA_HOME: sandbox, ARCA_TEST_PROBE_ROOT: sandbox, ARCA_NO_UPDATE_CHECK: "1" };
  const query = (args) => {
    const value = JSON.parse(execFileSync(cli, ["--json", ...args], { env, encoding: "utf8" }));
    if (!value.ok) throw new Error(`Reference query failed: ${args.join(" ")}`);
    return value.data;
  };
  const version = readFileSync(resolve(root, "version.txt"), "utf8").trim();
  const cliVersion = execFileSync(cli, ["--version"], { env, encoding: "utf8" }).trim();
  if (!cliVersion.endsWith(version)) throw new Error(`CLI ${cliVersion} does not match ${version}`);
  const registry = JSON.parse(readFileSync(resolve(root, "data/agents.json"), "utf8"));
  const models = new Map(query(["agent", "list"]).map((agent) => [agent.identity.id, agent.capabilities.model]));
  const agents = Object.entries(registry).map(([id, item]) => {
    const model = models.get(id);
    return {
      id, name: item.name, category: item.category, docs: item.docs ?? null,
      mcp: item.global ? { path: item.global, format: item.format, key: item.key, transports: item.transports } : null,
      skills: item.skills ? { path: item.skills.global_dir, aliases: item.skills.aliases ?? [] } : null,
      model: model ? {
        mode: model.mode, protocols: model.supported_protocols,
        paths: model.config_paths, multiple: model.supports_multiple,
        globalSelection: model.supports_global_selection, credentialMode: model.credential_mode,
      } : null,
    };
  }).sort((a, b) => a.name.localeCompare(b.name, "en") || a.id.localeCompare(b.id));
  const providers = query(["model", "provider", "templates"]).providers.map((item) => ({
    id: item.id, name: item.name, category: item.category,
    baseUrl: item.default_base_url, protocols: Object.keys(item.protocols),
    docs: item.docs_url, portal: item.portal ?? null,
  }));
  const output = { version, agents, providers };
  const serialized = JSON.stringify(output, null, 2) + "\n";
  if (/\/Users\/|\/home\//.test(serialized)) throw new Error("Public metadata contains a private absolute path");
  writeFileSync(resolve(website, ".vitepress/reference.json"), serialized);
  console.log(`Reference updated: v${version}, ${agents.length} audited Agents, ${providers.length} Provider templates.`);
} finally {
  rmSync(sandbox, { recursive: true, force: true });
}
