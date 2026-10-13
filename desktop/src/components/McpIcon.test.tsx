import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { McpIconPreference, RegistryEntry } from "../lib/types";
import { inferMcpIcon, McpAvatar, mcpMonogram, MCP_ICON_OPTIONS } from "./McpIcon";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

afterEach(cleanup);

function entry(name: string, description = "", tags: string[] = []): RegistryEntry {
  return {
    name,
    description,
    tags,
    config: { stdio: { command: "npx", args: [name] } },
  };
}

it("ships a stable semantic MCP icon catalog", () => {
  expect(MCP_ICON_OPTIONS.map((option) => option.id)).toEqual([
    "mcp", "search", "browser", "document", "knowledge", "files", "database",
    "terminal", "code", "api", "cloud", "automation", "observability", "map",
    "communication", "media", "security", "ai",
  ]);
});

it("infers meaningful icons and keeps a two-letter fallback", () => {
  expect(inferMcpIcon(entry("brave-search"))).toBe("search");
  expect(inferMcpIcon(entry("alibaba_cloud_observability", "metrics and logs")))
    .toBe("observability");
  expect(inferMcpIcon(entry("team-docs", "", ["knowledge-base"]))).toBe("knowledge");
  expect(inferMcpIcon(entry("opaque-service"))).toBeNull();
  expect(mcpMonogram("ali-employee-assistant")).toBe("AE");
  expect(mcpMonogram("@scope/brave-search")).toBe("BS");
});

it("prefers user choices and falls back when a custom image cannot load", () => {
  const builtin: McpIconPreference = { kind: "builtin", value: "database" };
  const view = render(
    <McpAvatar
      assetKey="brave-search::stdio"
      entry={entry("brave-search")}
      preference={builtin}
      size={34}
    />,
  );
  expect(view.container.firstElementChild).toHaveAttribute("data-icon-source", "builtin");
  expect(view.container.querySelector('[data-mcp-icon="database"]')).not.toBeNull();

  const custom: McpIconPreference = {
    kind: "custom",
    value: "custom.png",
    path: "/Users/test/.arca/assets/mcp-icons/custom.png",
  };
  view.rerender(
    <McpAvatar
      assetKey="brave-search::stdio"
      entry={entry("brave-search")}
      preference={custom}
      size={34}
    />,
  );
  const image = view.container.querySelector("img") as HTMLImageElement;
  expect(image.src).toContain("asset:///Users/test/.arca/assets/mcp-icons/custom.png");
  fireEvent.error(image);
  expect(view.container.firstElementChild).toHaveAttribute("data-icon-source", "auto");
  expect(view.container.querySelector('[data-mcp-icon="search"]')).not.toBeNull();
});

