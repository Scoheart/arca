import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { HostMcpObservation, NativePluginObservation } from "../lib/types";
import { NativePluginsPanel } from "./NativePluginsPanel";
import { AgentConsumptionPanel } from "./AgentConsumptionPanel";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, values?: { name?: string }) => values?.name ? `${key}:${values.name}` : key }) }));
afterEach(cleanup);
const mcp: HostMcpObservation = {
  agent_id: "cursor", server_id: "plugin-fixture-example", name: "example", provider: "fixture",
  origin: "plugin", presence: "cache_only", versions: ["1.0.0"], source_paths: ["~/.cursor/plugins/cache/fixture/revision/mcp.json"],
  docs_url: "https://cursor.com/docs/plugins", source_fingerprint: "fixture",
};
const plugin: NativePluginObservation = {
  agent_id: "cursor", plugin_id: "fixture", name: "fixture", version: "1.0.0", revision: "revision-a",
  presence: "cache_only", enabled: null, source_paths: ["~/.cursor/plugins/cache/fixture/revision/.cursor-plugin/plugin.json"],
  components: [{ kind: "mcp", name: "example", source_path: mcp.source_paths[0], native_id: mcp.server_id }],
  docs_url: mcp.docs_url, source_fingerprint: "fixture",
};

it("keeps cache and enablement details in a read-only dialog, including every matching group snapshot", async () => {
  render(<NativePluginsPanel plugins={[plugin, { ...plugin, revision: "revision-b" }]} />);
  expect(screen.getAllByRole("button", { name: "nativePlugins.viewDetails:fixture" })).toHaveLength(1);
  expect(screen.queryByText("nativePlugins.cacheRuntime")).not.toBeInTheDocument();
  expect(screen.queryByText("revision-a")).not.toBeInTheDocument();
  fireEvent.change(screen.getByPlaceholderText("nativePlugins.search"), { target: { value: "revision-a" } });
  const trigger = screen.getByRole("button", { name: "nativePlugins.viewDetails:fixture" });
  fireEvent.click(trigger);
  const dialog = screen.getByRole("dialog", { name: "fixture" });
  expect(within(dialog).getByText("revision-a")).toBeInTheDocument();
  expect(within(dialog).getByText("revision-b")).toBeInTheDocument();
  expect(screen.getByText("nativePlugins.cacheRuntime")).toBeInTheDocument();
  expect(screen.getAllByText("nativePlugins.enablementUnknown")).toHaveLength(2);
  expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /delete|remove|install|删除|移除|安装/i })).not.toBeInTheDocument();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await waitFor(() => expect(trigger).toHaveFocus());
});

it("excludes host-only MCPs from every destructive and enablement callback", () => {
  const mutate = vi.fn();
  render(<AgentConsumptionPanel domain="mcp" title="MCPs" manageLabel="Add configured MCP" rows={[]} external={[]}
    hostMcps={[mcp]} present={() => ({ name: "unexpected mutable asset" })} onManage={() => undefined}
    onRemove={mutate} onConverge={mutate} onEnabledChange={mutate} onBulkRemove={mutate} bulkRemoveLabel="Delete all"
    onBulkEnabledChange={mutate} bulkToggleLabel="All" bulkEnabled={true} />);
  expect(screen.queryByText(mcp.server_id)).not.toBeInTheDocument();
  expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Delete all" })).not.toBeInTheDocument();
  const native = screen.getByRole("button", { name: "nativePlugins.viewDetails:example" }).closest("li")!;
  expect(within(native).queryByRole("button", { name: /delete|remove|删除|移除/i })).not.toBeInTheDocument();
  fireEvent.click(within(native).getByRole("button", { name: "nativePlugins.viewDetails:example" }));
  expect(within(screen.getByRole("dialog", { name: "example" })).getByText(mcp.server_id)).toBeInTheDocument();
  expect(mutate).not.toHaveBeenCalled();
});

it("disambiguates same-name plugins by searchable native ID and keeps App declarations read-only", () => {
  const first = { ...plugin, plugin_id: "same@first", name: "Same", components: [{ kind: "app" as const, name: "connector", source_path: "~/plugins/first/.app.json", native_id: null }] };
  render(<NativePluginsPanel plugins={[first, { ...first, plugin_id: "same@second" }]} />);
  expect(screen.getAllByRole("button", { name: "nativePlugins.viewDetails:Same" })).toHaveLength(2);
  fireEvent.change(screen.getByPlaceholderText("nativePlugins.search"), { target: { value: "same@second" } });
  expect(screen.getAllByRole("button", { name: "nativePlugins.viewDetails:Same" })).toHaveLength(1);
  expect(screen.getByText("same@second")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "nativePlugins.viewDetails:Same" }));
  expect(within(screen.getByRole("dialog", { name: "Same" })).getByText("nativePlugins.kind_app")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /delete|remove|install|删除|移除|安装/i })).not.toBeInTheDocument();
});
