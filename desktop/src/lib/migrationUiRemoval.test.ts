import { access, readFile } from "node:fs/promises";
import { expect, it } from "vitest";

const relativeFile = (path: string) => new URL(path, import.meta.url);

it("has no central external-configuration migration surface or entry point", async () => {
  const sources = await Promise.all([
    "../App.tsx",
    "../components/Layout.tsx",
    "../components/RegistryView.tsx",
    "../components/ModelsView.tsx",
    "../components/SkillsView.tsx",
    "../components/AgentView.tsx",
  ].map((path) => readFile(relativeFile(path), "utf8")));
  const combined = sources.join("\n");

  expect(combined).not.toMatch(
    /MigrationDialog|MigrationBanner|onOpenMigration|migrationCount|让 Arca 管理|已识别的外部配置/,
  );
  expect(sources[1]).toMatch(/onClick=\{\(\) => void handleRescan\(\)\}/);
});

it("deletes the migration-only components and presentation helper", async () => {
  for (const path of [
    "../components/MigrationDialog.tsx",
    "../components/MigrationBanner.tsx",
    "./migration.ts",
  ]) {
    await expect(access(relativeFile(path))).rejects.toThrow();
  }
});
