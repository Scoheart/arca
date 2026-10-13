import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

const relativeFile = (path: string) => new URL(path, import.meta.url);

it("uses trash icons for destructive remove controls while keeping X for close and clear", async () => {
  const [consumption, envEditor, configuration, inspector, navigation, dialog, workspace] =
    await Promise.all([
      readFile(relativeFile("../components/AgentConsumptionPanel.tsx"), "utf8"),
      readFile(relativeFile("../components/EnvEditor.tsx"), "utf8"),
      readFile(relativeFile("../components/AgentConfigurationDialog.tsx"), "utf8"),
      readFile(relativeFile("../components/SkillInspector.tsx"), "utf8"),
      readFile(relativeFile("../components/AgentNavigation.tsx"), "utf8"),
      readFile(relativeFile("../components/DialogShell.tsx"), "utf8"),
      readFile(relativeFile("../components/ResourceWorkspace.tsx"), "utf8"),
    ]);

  for (const source of [consumption, envEditor, configuration, inspector]) {
    expect(source).toContain("TrashIcon");
    expect(source).not.toContain("<XIcon");
  }
  expect(consumption).toMatch(
    /aria-label=\{removeLabel\?\.\(presentation\.name\)[\s\S]*?<TrashIcon/,
  );
  expect(navigation).toContain('aria-label="清除搜索"');
  expect(navigation).toMatch(/aria-label="清除搜索"[\s\S]*?<XIcon/);
  expect(dialog).toMatch(/aria-label=\{closeLabel\}[\s\S]*?<XIcon/);
  expect(workspace).toMatch(/aria-label="关闭详情"[\s\S]*?<XIcon/);
  expect(dialog).not.toMatch(/arca-dialog-shell-close[\s\S]*?<TrashIcon/);
  expect(workspace).not.toMatch(/arca-inspector-close[\s\S]*?<TrashIcon/);
});
