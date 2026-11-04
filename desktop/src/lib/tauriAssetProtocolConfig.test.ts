import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { expect, it } from "vitest";

it("keeps the Tauri asset protocol config and Cargo feature in lockstep", async () => {
  const root = resolve(process.cwd());
  const config = JSON.parse(await readFile(resolve(root, "src-tauri/tauri.conf.json"), "utf8"));
  const manifest = await readFile(resolve(root, "src-tauri/Cargo.toml"), "utf8");
  const tauriDependency = manifest.match(/^tauri\s*=.*$/m)?.[0] ?? "";

  expect(config.app.security.assetProtocol.enable).toBe(true);
  const scopes = config.app.security.assetProtocol.scope as string[];
  expect(scopes.length).toBeGreaterThan(0);
  for (const scope of scopes) {
    // Brand compatibility may expose the former icon directory, never other
    // Agent files, credential stores, or a broad HOME wildcard.
    expect(scope).toMatch(/^\$HOME\/\.(?:arca|mux)\/assets\/mcp-icons\/\*\*$/);
  }
  expect(tauriDependency).toContain('"protocol-asset"');
});

