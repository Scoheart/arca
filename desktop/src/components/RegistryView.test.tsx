import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import type { InstallState } from "../hooks/useInstallState";
import type { RegistryEntry } from "../lib/types";
import { RegistryView } from "./RegistryView";
import { ToastProvider } from "./Toast";

const apiMocks = vi.hoisted(() => ({
  listMcpIconPreferences: vi.fn().mockResolvedValue({}),
  setMcpBuiltinIcon: vi.fn(),
  importMcpIconDialog: vi.fn(),
  resetMcpIcon: vi.fn(),
}));

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return { ...actual, ...apiMocks };
});

const source = await readFile(resolve(process.cwd(), "src/components/RegistryView.tsx"), "utf8");

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  apiMocks.listMcpIconPreferences.mockResolvedValue({});
});

function registryState(overrides: Record<string, unknown> = {}) {
  return {
    entries: [],
    catalog: [],
    agents: [],
    installed: [],
    loading: false,
    registryError: null,
    agentsForServer: () => [],
    customKeys: new Set<string>(),
    sources: [],
    refreshRegistry: vi.fn().mockResolvedValue(undefined),
    rescan: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  } as unknown as InstallState;
}

it("maps MCP assets to the compact central index", () => {
  expect(source).toMatch(/className="arca-registry-workspace"/);
  expect(source).toMatch(/className="arca-asset-list arca-mcp-list"[\s\S]*?role="list"/);
  const row = source.slice(source.indexOf("function RegistryCard"), source.indexOf("function RegistryDetail"));
  expect(row).toMatch(/className="arca-asset-list-row arca-mcp-list-row"/);
  expect(row).toMatch(/arca-asset-list-identity/);
  expect(row).toMatch(/arca-asset-list-transport/);
  expect(row).toMatch(/arca-asset-list-source/);
  expect(row).toMatch(/arca-asset-list-status/);
  expect(row).toMatch(/transportOf\(entry\)\.toUpperCase\(\)/);
  expect(row).toMatch(/<McpAvatar[\s\S]*?assetKey=\{keyOf\(entry\)\}/);
  expect(row).toMatch(/centralAssets\.effective/);
  expect(row).toMatch(/centralAssets\.shadowed/);
  expect(row).not.toMatch(/<IconButton|<ResourceCard/);
});

it("renders effective and shadowed MCP rows and opens the existing Inspector", async () => {
  const effective: RegistryEntry = {
    name: "brave-search",
    description: "Web search",
    tags: ["search"],
    config: { http: { type: "http", url: "https://mcp.example.test/search" } },
    origin: { kind: "manual" },
  };
  const shadowed: RegistryEntry = {
    name: "filesystem-old",
    description: "Older subscribed copy",
    tags: [],
    config: { stdio: { command: "npx", args: ["filesystem"] } },
    origin: { kind: "remote", source: "team-catalog" },
  };
  const state = registryState({
    entries: [effective],
    catalog: [
      { entry: effective, in_effect: true },
      { entry: shadowed, in_effect: false },
    ],
    sources: [{
      id: "team-catalog",
      kind: "remote",
      name: "Team catalog",
      url: "https://mcp.example.test/catalog.json",
      path: null,
      format: "json",
      enabled: true,
      added_at: null,
      synced_at: null,
      server_count: 1,
      error: null,
      managed: false,
    }],
  });
  const user = userEvent.setup();

  render(
    <ToastProvider>
      <RegistryView state={state} onCreate={vi.fn()} />
    </ToastProvider>,
  );

  const list = screen.getByRole("list", { name: "MCP 资产" });
  expect(list).toHaveClass("arca-asset-list", "arca-mcp-list");
  expect(within(list).getByText("名称与连接")).toBeVisible();
  expect(within(list).getByText("传输")).toBeVisible();
  expect(within(list).getByText("来源")).toBeVisible();
  expect(within(list).getByText("状态")).toBeVisible();
  expect(within(list).getByText("生效")).toBeVisible();
  expect(within(list).getByText("被覆盖")).toBeVisible();
  expect(screen.queryByRole("button", { name: "粘贴配置" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "导出生效配置" })).toBeVisible();
  expect(screen.getByRole("button", { name: "添加 MCP" })).toBeVisible();
  expect(screen.queryByRole("tablist", { name: "MCP 状态" })).not.toBeInTheDocument();
  expect(screen.getByRole("separator", { name: "调整侧边栏宽度" })).toBeVisible();
  expect(screen.getByRole("button", { name: "添加订阅" })).toBeVisible();
  expect(screen.getByRole("button", { name: "导入配置" })).toBeVisible();
  expect(screen.getByRole("button", { name: /全部来源.*2/ })).toBeVisible();
  await user.click(screen.getByRole("button", { name: /Team catalog.*1/ }));
  expect(screen.getByRole("button", { name: /^打开 MCP filesystem-old 详情 ·/ })).toBeVisible();
  expect(screen.queryByRole("button", { name: /^打开 MCP brave-search 详情 ·/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /全部来源.*2/ }));

  await user.type(screen.getByPlaceholderText("搜索 MCP"), "not-present");
  expect(screen.getByText("没有匹配项")).toBeVisible();
  expect(screen.getByText("调整搜索或来源筛选后重试。")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "清除筛选" }));
  expect(screen.getByRole("button", { name: /^打开 MCP brave-search 详情 ·/ })).toBeVisible();
  expect(screen.getByRole("button", { name: /^打开 MCP filesystem-old 详情 ·/ })).toBeVisible();
  await user.click(screen.getByRole("button", { name: /^打开 MCP brave-search 详情 ·/ }));
  const inspector = await screen.findByRole("complementary", { name: "brave-search 详情" });
  expect(within(inspector).getByText("Web search")).toBeVisible();
  expect(within(inspector).getByText("https://mcp.example.test/search")).toBeVisible();
  expect(within(inspector).getByRole("button", { name: "图标" })).toBeVisible();

  apiMocks.setMcpBuiltinIcon.mockResolvedValue({
    "brave-search::http": { kind: "builtin", value: "database" },
  });
  await user.click(within(inspector).getByRole("button", { name: "图标" }));
  expect(screen.getByRole("dialog", { name: "选择 MCP 图标" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "选择内置图标：数据库" }));
  await waitFor(() => expect(apiMocks.setMcpBuiltinIcon).toHaveBeenCalledWith(
    "brave-search::http",
    "database",
  ));
  expect(document.querySelector('[data-mcp-icon="database"]')).not.toBeNull();
});

it("keeps MCP workspace actions visible through loading, error retry, and empty states", async () => {
  const user = userEvent.setup();
  const retry = vi.fn().mockResolvedValue(undefined);
  const loadingView = render(
    <ToastProvider>
      <RegistryView state={registryState({ loading: true })} onCreate={vi.fn()} />
    </ToastProvider>,
  );

  expect(screen.getByRole("status", { name: "正在读取 MCP…" })).toBeVisible();
  expect(screen.getByRole("button", { name: "添加 MCP" })).toBeVisible();
  loadingView.unmount();

  const errorView = render(
    <ToastProvider>
      <RegistryView
        state={registryState({ registryError: "registry unavailable" })}
        onCreate={vi.fn()}
        onRetryLoad={retry}
      />
    </ToastProvider>,
  );
  expect(screen.getByRole("alert")).toHaveTextContent("读取 MCP 失败");
  expect(screen.getByRole("alert")).toHaveTextContent("registry unavailable");
  await user.click(screen.getByRole("button", { name: "重试" }));
  expect(retry).toHaveBeenCalledTimes(1);
  errorView.unmount();

  render(
    <ToastProvider>
      <RegistryView state={registryState()} onCreate={vi.fn()} />
    </ToastProvider>,
  );
  expect(screen.getByText("暂无 MCP")).toBeVisible();
  expect(screen.getByText("添加订阅、导入配置或新建 MCP")).toBeVisible();
});

it("keeps mutations and redacted configuration in the Inspector", () => {
  const inspector = source.slice(source.indexOf("function RegistryDetail"));
  expect(inspector).toMatch(/redactSensitiveConfig\(entry\.config\)/);
  expect(inspector).toMatch(/onCopy/);
  expect(inspector).toMatch(/onEdit/);
  expect(inspector).toMatch(/onDelete/);
});

it("switches an editable MCP to the shared form inside the same Inspector shell", () => {
  expect(source).toMatch(/presentation="inspector"/);
  expect(source).toMatch(/setEditingDetail\(true\)/);
  expect(source).not.toMatch(/onEdit: \(name: string, transport: Transport\)/);
});

it("routes deletion through the central lifecycle planner", () => {
  expect(source).toMatch(/consumptionState\.planDelete/);
  expect(source).not.toMatch(/forgetEntry|deleteMcp|uninstall/);
});

it("keeps the source filter while removing MCP status tabs", () => {
  expect(source).toMatch(/<SourcesSidebar[\s\S]*selectedId=\{selectedSource\}/);
  expect(source).toMatch(/selectedSource|sourceScoped/);
  expect(source).not.toMatch(/<ResourceTabs|statusFilter|statusCounts|MCP 状态/);
  expect(source).toMatch(/调整搜索或来源筛选|清除筛选/);
});

it("offers editing for source-owned MCPs while keeping deletion user-owned", () => {
  expect(source).toMatch(/Every catalog copy can be edited/);
  expect(source).toMatch(/onEdit=\{\s*consumptionState/);
  expect(source).toMatch(/const deletable = useCallback\(\(entry: RegistryEntry\) => isUserOwned\(entry\)/);
  expect(source).not.toMatch(/const editable = useCallback/);
});
