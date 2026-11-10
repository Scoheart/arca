import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { McpConfigurationView, McpInspectionRequest } from "../lib/types";
import { McpConfigurationDialog } from "./McpConfigurationDialog";

const { inspect } = vi.hoisted(() => ({ inspect: vi.fn() }));
vi.mock("../lib/api", () => ({ inspectMcpConfiguration: inspect }));
const request: McpInspectionRequest = { agent_id: "codex", source: { kind: "configured", key: "docs::stdio" } };
const view: McpConfigurationView = { agent_id: "codex", server_id: "docs", documents: [{
  source_path: "~/.codex/config.toml", configuration: { command: "fixture-command", env: { TOKEN: "[已隐藏]" } }, unavailable_reason: null,
}] };

beforeEach(() => inspect.mockReset().mockResolvedValue(view));
afterEach(cleanup);

it("reads once in StrictMode and offers only a read-only configuration and close action", async () => {
  const close = vi.fn();
  render(<StrictMode><McpConfigurationDialog name="docs" request={request} onClose={close} /></StrictMode>);
  const dialog = screen.getByRole("dialog", { name: "docs" });
  expect(await within(dialog).findByText(/fixture-command/)).toBeVisible();
  expect(inspect).toHaveBeenCalledTimes(1);
  expect(inspect).toHaveBeenCalledWith(request);
  expect(within(dialog).getByText("~/.codex/config.toml")).toBeVisible();
  expect(within(dialog).queryByRole("textbox")).not.toBeInTheDocument();
  expect(within(dialog).queryByRole("button", { name: /保存|删除|移除|收录|切换/ })).not.toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  expect(close).toHaveBeenCalledTimes(1);
});

it("keeps read errors private and retries the exact same observation", async () => {
  inspect.mockRejectedValueOnce(new Error("native-line-with-private-value")).mockResolvedValueOnce(view);
  render(<McpConfigurationDialog name="docs" request={request} onClose={vi.fn()} />);
  const retry = await screen.findByRole("button", { name: "重试" });
  expect(screen.queryByText(/native-line-with-private-value/)).not.toBeInTheDocument();
  await userEvent.click(retry);
  expect(await screen.findByText(/fixture-command/)).toBeVisible();
  expect(inspect.mock.calls).toEqual([[request], [request]]);
});

it("does not render a late response after the selected Agent changes", async () => {
  let resolveOld!: (value: McpConfigurationView) => void;
  inspect.mockReturnValueOnce(new Promise<McpConfigurationView>(resolve => { resolveOld = resolve; }));
  const { rerender } = render(<McpConfigurationDialog name="old" request={request} onClose={vi.fn()} />);
  await waitFor(() => expect(inspect).toHaveBeenCalledTimes(1));
  const next: McpInspectionRequest = { agent_id: "cursor", source: { kind: "configured_name", name: "new" } };
  inspect.mockResolvedValueOnce({ ...view, agent_id: "cursor", server_id: "new", documents: [{
    source_path: "~/.cursor/mcp.json", configuration: { command: "current-command" }, unavailable_reason: null,
  }] });
  rerender(<McpConfigurationDialog name="new" request={next} onClose={vi.fn()} />);
  expect(await screen.findByText(/current-command/)).toBeVisible();
  await act(async () => { resolveOld(view); });
  expect(screen.queryByText(/fixture-command/)).not.toBeInTheDocument();
  expect(screen.getByText(/current-command/)).toBeVisible();
});

it("rejects a response belonging to another Agent", async () => {
  inspect.mockResolvedValueOnce({ ...view, agent_id: "cursor" });
  render(<McpConfigurationDialog name="docs" request={request} onClose={vi.fn()} />);
  expect(await screen.findByRole("button", { name: "重试" })).toBeVisible();
  expect(screen.queryByText(/fixture-command/)).not.toBeInTheDocument();
});

it("explains a host-managed source with no independent declaration instead of rendering an empty object", async () => {
  const reason = "此来源由宿主代码生成连接，没有可独立读取的 MCP 声明配置。";
  inspect.mockResolvedValueOnce({ ...view, documents: [{ source_path: "/Applications/Fixture.app/resource.js",
    configuration: null, unavailable_reason: reason }] });
  render(<McpConfigurationDialog name="docs" request={request} onClose={vi.fn()} />);
  expect(await screen.findByText(reason)).toBeVisible();
  expect(screen.queryByLabelText("已解析的 MCP 配置")).not.toBeInTheDocument();
  expect(screen.getByText("/Applications/Fixture.app/resource.js")).toBeVisible();
});
