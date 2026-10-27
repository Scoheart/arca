import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { HostMcpObservation, NativePluginObservation } from "../lib/types";
import { NativePluginsPanel } from "./NativePluginsPanel";
import { AgentConsumptionPanel } from "./AgentConsumptionPanel";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
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

it("shows cache provenance and unknown enablement without mutation controls", () => {
  render(<NativePluginsPanel plugins={[plugin, { ...plugin, revision: "revision-b" }]} />);
  expect(screen.getAllByRole("heading", { level: 4 })).toHaveLength(1);
  expect(screen.getByText("nativePlugins.cacheRuntime")).toBeInTheDocument();
  expect(screen.getAllByText("nativePlugins.enablementUnknown")).toHaveLength(2);
  expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /delete|remove|install|删除|移除|安装/i })).not.toBeInTheDocument();
});

it("excludes host-only MCPs from every destructive and enablement callback", () => {
  const mutate = vi.fn();
  render(<AgentConsumptionPanel domain="mcp" title="MCPs" manageLabel="Add configured MCP" rows={[]} external={[]}
    hostMcps={[mcp]} present={() => ({ name: "unexpected mutable asset" })} onManage={() => undefined}
    onRemove={mutate} onConverge={mutate} onEnabledChange={mutate} onBulkRemove={mutate} bulkRemoveLabel="Delete all"
    onBulkEnabledChange={mutate} bulkToggleLabel="All" bulkEnabled={true} />);
  expect(screen.getByText(mcp.server_id)).toBeInTheDocument();
  expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Delete all" })).not.toBeInTheDocument();
  const native = screen.getByText(mcp.server_id).closest("li")!;
  expect(within(native).queryByRole("button", { name: /delete|remove|删除|移除/i })).not.toBeInTheDocument();
  expect(mutate).not.toHaveBeenCalled();
});
