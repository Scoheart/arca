import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import type { InstallState } from "../hooks/useInstallState";
import type { RegistryEntry } from "../lib/types";
import { RegistryEditPage } from "./RegistryEditPage";
import { ResourceWorkspace } from "./ResourceWorkspace";
import { ToastProvider } from "./Toast";

const source = await readFile(resolve(process.cwd(), "src/components/RegistryEditPage.tsx"), "utf8");

afterEach(cleanup);

it("routes central MCP changes through the shared asset plan", () => {
  expect(source).toMatch(/<DialogShell/);
  expect(source).toMatch(/kind="editor"/);
  expect(source).toMatch(/consumptionState\.planUpdate/);
  expect(source).toMatch(/consumptionState\.planDelete/);
  expect(source).not.toMatch(/upsertRegistry|deleteRegistry|resyncEntry/);
  expect(source).not.toMatch(/window\.confirm/);
  expect(source).not.toMatch(/<ModalHeader/);
});

it("hides manual tag editing while preserving existing asset tags", () => {
  expect(source).not.toMatch(/tagsText|标签（逗号分隔）|official, builtin/);
  expect(source).toMatch(/tags: existing\?\.tags \?\? \[\]/);
});

it("creates a manual override with custom env for a subscribed MCP", async () => {
  const user = userEvent.setup();
  const existing: RegistryEntry = {
    name: "source-backed-mcp",
    description: "Imported MCP",
    tags: ["official", "catalog"],
    origin: { kind: "remote", source: "arca-curated" },
    config: { stdio: { command: "npx", args: ["-y", "source-backed-mcp"] } },
  };
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "mcp-plan" });
  const state = {
    entries: [existing],
    customKeys: new Set<string>(),
  } as unknown as InstallState;
  const consumptionState = { planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <RegistryEditPage
        state={state}
        consumptionState={consumptionState}
        name={existing.name}
        transport="stdio"
        onBack={() => undefined}
      />
    </ToastProvider>,
  );

  expect(screen.getByRole("note")).toHaveTextContent("订阅内容和后续更新不会被修改");
  expect(screen.queryByRole("button", { name: "恢复默认" })).not.toBeInTheDocument();
  const addVariable = screen.getByRole("button", { name: "添加变量" });
  await user.click(addVariable);
  const inputs = screen.getAllByRole("textbox");
  await user.type(inputs.at(-2)!, "SOURCE_BACKED_API_KEY");
  await user.type(inputs.at(-1)!, "user-secret");
  await user.click(screen.getByRole("button", { name: "创建本地覆盖" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledTimes(1));
  expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "mcp",
    entry: expect.objectContaining({
      tags: ["official", "catalog"],
      origin: { kind: "manual", source: "manual" },
      config: expect.objectContaining({
        stdio: expect.objectContaining({
          env: { SOURCE_BACKED_API_KEY: "user-secret" },
        }),
      }),
    }),
  }));
});

it("renames an existing MCP through the shared plan while keeping its transport fixed", async () => {
  const user = userEvent.setup();
  const existing: RegistryEntry = {
    name: "old-name",
    description: "Rename me",
    tags: [],
    origin: { kind: "manual", source: "manual" },
    config: { stdio: { command: "rename-server" } },
  };
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "rename-plan" });
  const onBack = vi.fn();
  const state = {
    entries: [
      existing,
      {
        name: "new-name",
        description: "Same display name, another transport",
        tags: [],
        origin: { kind: "manual", source: "manual" },
        config: { http: { type: "http", url: "https://example.com/mcp" } },
      },
    ],
    customKeys: new Set(["old-name::stdio"]),
  } as unknown as InstallState;

  render(
    <ToastProvider>
      <RegistryEditPage
        state={state}
        consumptionState={{ planUpdate } as unknown as ConsumptionState}
        name={existing.name}
        transport="stdio"
        onBack={onBack}
      />
    </ToastProvider>,
  );

  const nameInput = screen.getByRole("textbox", { name: "名称" });
  expect(nameInput).toBeEnabled();
  expect(screen.getByRole("button", { name: "http / sse" })).toBeDisabled();
  await user.clear(nameInput);
  expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
  await user.type(nameInput, "new-name");
  await user.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledTimes(1));
  expect(planUpdate).toHaveBeenCalledWith({
    domain: "mcp",
    existing_key: "old-name::stdio",
    entry: expect.objectContaining({
      name: "new-name",
      config: { stdio: expect.objectContaining({ command: "rename-server" }) },
    }),
  });
  expect(onBack).not.toHaveBeenCalled();
});

it("rejects a rename collision and cancellation leaves the plan untouched", async () => {
  const user = userEvent.setup();
  const existing: RegistryEntry = {
    name: "old-name",
    description: "",
    tags: [],
    origin: { kind: "manual", source: "manual" },
    config: { stdio: { command: "rename-server" } },
  };
  const collision: RegistryEntry = {
    ...existing,
    name: "taken-name",
  };
  const planUpdate = vi.fn();
  const onBack = vi.fn();
  const state = {
    entries: [existing, collision],
    customKeys: new Set(["old-name::stdio", "taken-name::stdio"]),
  } as unknown as InstallState;

  render(
    <ToastProvider>
      <RegistryEditPage
        state={state}
        consumptionState={{ planUpdate } as unknown as ConsumptionState}
        name={existing.name}
        transport="stdio"
        onBack={onBack}
      />
    </ToastProvider>,
  );

  const nameInput = screen.getByRole("textbox", { name: "名称" });
  await user.clear(nameInput);
  await user.type(nameInput, "taken-name");
  await user.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByText("已存在同名同传输方式的 MCP: taken-name (stdio)")).toBeVisible();
  expect(planUpdate).not.toHaveBeenCalled();

  await user.click(screen.getByRole("button", { name: "取消" }));
  expect(onBack).toHaveBeenCalledTimes(1);
  expect(planUpdate).not.toHaveBeenCalled();
});

it("renders an existing MCP editor inside one resource dialog", () => {
  const existing: RegistryEntry = {
    name: "single-shell-mcp",
    description: "Single shell",
    tags: [],
    origin: { kind: "manual" },
    config: { stdio: { command: "npx", args: ["single-shell-mcp"] } },
  };
  const state = {
    entries: [existing],
    customKeys: new Set<string>(),
  } as unknown as InstallState;
  const consumptionState = {} as ConsumptionState;

  render(
    <ToastProvider>
      <ResourceWorkspace
        sidebar={<div />}
        query=""
        onQueryChange={() => undefined}
        searchPlaceholder="搜索 MCP"
        toolbarActions={null}
        inspector={
          <RegistryEditPage
            state={state}
            consumptionState={consumptionState}
            name={existing.name}
            entry={existing}
            transport="stdio"
            presentation="inspector"
            onBack={() => undefined}
          />
        }
      >
        <div />
      </ResourceWorkspace>
    </ToastProvider>,
  );

  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(screen.getByRole("complementary", { name: `${existing.name} 详情` })).toBeVisible();
  expect(screen.queryByRole("dialog", { name: "编辑 MCP" })).not.toBeInTheDocument();
});
