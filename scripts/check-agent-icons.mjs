import { readFileSync, readdirSync } from "node:fs";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const assetDir = resolve(root, "desktop/src/assets/agents");
// Match core::agents::builtin_agents: audited definitions override broad catalog entries.
const agents = {
  ...JSON.parse(readFileSync(resolve(root, "data/agent-catalog.json"), "utf8")),
  ...JSON.parse(readFileSync(resolve(root, "data/agents.json"), "utf8")),
};
const aliases = JSON.parse(readFileSync(resolve(assetDir, "aliases.json"), "utf8"));
const extensions = new Set([".png", ".svg", ".webp", ".ico"]);
const assets = new Set(
  readdirSync(assetDir)
    .filter((file) => extensions.has(extname(file)))
    .map((file) => file.slice(0, -extname(file).length))
);

const missingAliasTargets = Object.entries(aliases)
  .filter(([, target]) => !assets.has(target))
  .map(([id, target]) => `${id} -> ${target}`);
// Include read-only and Skills-only entries; they also appear in the directory.
const builtin = Object.entries(agents).filter(([, agent]) => agent.builtin);
const missingIcons = builtin
  .filter(([id]) => !assets.has(aliases[id] ?? id))
  .map(([id, agent]) => `${id} (${agent.name})`);

if (missingAliasTargets.length) {
  console.error(`Icon aliases reference missing assets:\n- ${missingAliasTargets.join("\n- ")}`);
  process.exit(1);
}

if (missingIcons.length) {
  console.log(`Built-in agents hidden until an icon is supplied:\n- ${missingIcons.join("\n- ")}`);
}
console.log(`Audited ${builtin.length} built-in agents: ${builtin.length - missingIcons.length} visible, ${missingIcons.length} hidden.`);
