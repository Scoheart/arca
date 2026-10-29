import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AgentGlyph } from "./brandIcons";

afterEach(cleanup);

function surfaceFor(container: HTMLElement): string | null {
  return container
    .querySelector<HTMLElement>("[data-agent-surface]")
    ?.getAttribute("data-agent-surface") ?? null;
}

it("distinguishes shared-brand product surfaces with localized names", () => {
  const cases = [
    ["cursor", "Cursor", "ide"],
    ["cursor-cli", "Cursor CLI", "cli"],
    ["codex-ide", "Codex", "plugin"],
    ["chatgpt", "ChatGPT", "web"],
    ["claude-code", "Claude Code", "cli"],
    ["claude-desktop", "Claude Desktop", "desktop"],
    ["qoder-cli", "Qoder CLI", "cli"],
    ["qoder", "Qoder IDE", "ide"],
    ["opencode", "OpenCode", "cli"],
    ["opencode-desktop", "OpenCode Desktop", "desktop"],
    ["openclaw", "OpenClaw", "cli"],
    ["openclaw-desktop", "OpenClaw", "desktop"],
    ["codex", "Codex CLI", "cli"],
    ["codex-desktop", "Codex Desktop", "desktop"],
    ["factory-droid", "Factory Droid", "cli"],
    ["factory-desktop", "Factory", "desktop"],
  ] as const;

  for (const [id, name, surface] of cases) {
    const view = render(<AgentGlyph id={id} name={name} size={30} />);
    expect(surfaceFor(view.container), id).toBe(surface);
    expect(view.container.querySelector("[data-agent-surface]")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(view.getByAltText(name)).toBeVisible();
    const labels = { cli: "命令行", desktop: "桌面应用", ide: "IDE 编辑器", plugin: "IDE 插件", web: "Web 应用" };
    expect(view.getByRole("img", { name: `${name} · ${labels[surface]}` })).toHaveAttribute("title", `${name} · ${labels[surface]}`);
    view.unmount();
  }
});

it("keeps unique, custom, and fallback Agent icons unbadged", () => {
  for (const [id, name] of [
    ["qoder-desktop", "Qoder Desktop"],
    ["freebuff", "Freebuff"],
    ["my-custom-agent", "My Custom Agent"],
  ]) {
    const view = render(<AgentGlyph id={id} name={name} size={30} />);
    expect(surfaceFor(view.container), id).toBeNull();
    view.unmount();
  }
});

it("uses brand assets for Cline Desktop and Freebuff", () => {
  for (const [id, name] of [
    ["cline-desktop", "Cline Desktop"],
    ["freebuff", "Freebuff"],
  ]) {
    const view = render(<AgentGlyph id={id} name={name} size={30} />);
    expect(view.getByAltText(name)).toBeVisible();
    view.unmount();
  }
});

it("scales badges for consumer lists, pinned bars, and Agent headers", () => {
  const cases = [
    [16, "10px"],
    [20, "12px"],
    [24, "12px"],
    [30, "15px"],
    [32, "15px"],
    [42, "19px"],
    [44, "19px"],
    [52, "19px"],
    [64, "24px"],
  ] as const;

  for (const [size, expected] of cases) {
    const view = render(
      <AgentGlyph id="claude-code" name="Claude Code" size={size} />,
    );
    expect(
      view.container.querySelector<HTMLElement>("[data-agent-surface]"),
    ).toHaveStyle({ width: expected, height: expected });
    view.unmount();
  }
});
