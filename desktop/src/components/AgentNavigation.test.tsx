import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AgentCapabilityView, AgentInfo } from "../lib/types";
import { mergeAgentInfos } from "../lib/agentCapabilities";
import { AgentNavigation } from "./AgentNavigation";

const pinnedAgentMocks = vi.hoisted(() => ({
  commit: vi.fn(),
}));

vi.mock("../hooks/usePinnedAgents", () => ({
  usePinnedAgents: () => ({
    agentIds: ["claude-code", "codex", "qoder"],
    ready: true,
    saving: false,
    commit: pinnedAgentMocks.commit,
  }),
}));

function agent(id: string, name: string): AgentInfo {
  return {
    id,
    name,
    format: "json",
    key: "mcpServers",
    has_global: true,
    has_project: false,
    enabled: true,
    supported_transports: ["stdio", "http"],
    global: `~/.${id}/settings.json`,
    project: null,
    skills_global_dir: null,
    docs: null,
    note: null,
    category: "coding",
    evidence: "official",
    verified_at: null,
    builtin: true,
  };
}

const agents = [
  agent("claude-code", "Claude Code"),
  agent("claude-desktop", "Claude Desktop"),
  agent("codex", "Codex"),
  agent("qoder", "Qoder"),
  agent("amp", "Amp"),
];

it("filters built-in, pinned and custom cards without duplicating Agents", () => {
  const custom = { ...agent("team-workbench", "Team Workbench"), builtin: false };
  const { container } = render(<AgentNavigation agents={[...agents, custom]} selectedAgentId="codex" onSelectAgent={vi.fn()} />);
  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);
  const picker = within(screen.getByRole("dialog", { name: "选择和置顶 Agent" }));
  expect(picker.getByRole("button", { name: "内置", pressed: true })).toBeVisible();
  expect(picker.getByRole("button", { name: "进入 Amp（amp）" })).toBeVisible();
  expect(picker.getAllByRole("button", { name: "进入 Codex（codex）" })).toHaveLength(1);
  expect(picker.queryByRole("button", { name: "进入 Team Workbench（team-workbench）" })).not.toBeInTheDocument();
  fireEvent.click(picker.getByRole("button", { name: "自定义" }));
  expect(picker.getByRole("button", { name: "进入 Team Workbench（team-workbench）" })).toBeVisible();
  expect(picker.queryByRole("button", { name: "进入 Amp（amp）" })).not.toBeInTheDocument();
  fireEvent.click(picker.getByRole("button", { name: "常用" }));
  expect(picker.getByRole("button", { name: "进入 Codex（codex）" })).toBeVisible();
  expect(picker.queryByRole("button", { name: "进入 Team Workbench（team-workbench）" })).not.toBeInTheDocument();
});

it("searches across categories, retains pin state, and opens an exact custom match", () => {
  const onSelectAgent = vi.fn();
  const custom = { ...agent("code-workbench", "Code Workbench"), builtin: false };
  const { container } = render(<AgentNavigation agents={[...agents, custom]} selectedAgentId="codex" onSelectAgent={onSelectAgent} />);
  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);
  const picker = within(screen.getByRole("dialog", { name: "选择和置顶 Agent" }));
  const search = picker.getByRole("searchbox", { name: "搜索 Agent" });
  fireEvent.change(search, { target: { value: "code" } });
  expect(picker.getByRole("button", { name: "进入 Code Workbench（code-workbench）" })).toBeVisible();
  expect(picker.getByRole("button", { name: "取消置顶 Codex" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.change(search, { target: { value: "no-such-agent" } });
  expect(picker.getByText("未找到匹配的 Agent")).toBeVisible();
  expect(picker.queryByRole("button", { name: /^进入 / })).not.toBeInTheDocument();
  fireEvent.change(search, { target: { value: "code-workbench" } });
  fireEvent.keyDown(search, { key: "Enter" });
  expect(onSelectAgent).toHaveBeenCalledWith("code-workbench");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("distinguishes shared Claude logos in the Agent picker", () => {
  const { container } = render(
    <AgentNavigation
      agents={agents}
      selectedAgentId="codex"
      onSelectAgent={vi.fn()}
    />,
  );

  fireEvent.click(
    container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!,
  );

  const picker = within(screen.getByRole("dialog", { name: "选择和置顶 Agent" }));
  const codeRow = picker.getByText("Claude Code").closest(".arca-agent-hand-select");
  const desktopRow = picker
    .getByText("Claude Desktop")
    .closest(".arca-agent-hand-select");
  expect(codeRow?.querySelector("[data-agent-surface='cli']")).not.toBeNull();
  expect(
    desktopRow?.querySelector("[data-agent-surface='desktop']"),
  ).not.toBeNull();
});

beforeEach(() => {
  pinnedAgentMocks.commit.mockReset();
  pinnedAgentMocks.commit.mockResolvedValue(true);
});

afterEach(cleanup);

it("keeps Dock drag sources reachable and commits the dropped order", async () => {
  render(<AgentNavigation agents={agents} selectedAgentId="codex" onSelectAgent={vi.fn()} />);
  const dock = screen.getByRole("navigation", { name: "置顶 Agent" });
  const source = within(dock).getByRole("button", { name: "Qoder" });
  const target = within(dock).getByRole("button", { name: "Claude Code" });
  vi.spyOn(target, "getBoundingClientRect").mockReturnValue({
    top: 0, bottom: 36, left: 0, right: 36, width: 36, height: 36, x: 0, y: 0, toJSON: () => ({}),
  });
  const dataTransfer = { effectAllowed: "none", dropEffect: "none", setData: vi.fn() } as unknown as DataTransfer;
  expect(source).toHaveAttribute("draggable", "true");
  fireEvent.dragStart(source, { dataTransfer });
  expect(dataTransfer.setData).toHaveBeenCalledWith("text/plain", "qoder");
  fireEvent.dragOver(target, { clientX: 0, dataTransfer });
  expect(source).toBeInTheDocument();
  expect(source).toHaveAttribute("draggable", "true");
  fireEvent.drop(target, { dataTransfer });
  await waitFor(() => expect(pinnedAgentMocks.commit).toHaveBeenCalledWith(["qoder", "claude-code", "codex"]));
});

it("uses a dedicated crossed pin for unpinning and keeps the action wired", async () => {
  const { container } = render(
    <AgentNavigation
      agents={agents}
      selectedAgentId="codex"
      onSelectAgent={vi.fn()}
    />,
  );

  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);

  const unpin = screen.getByRole("button", { name: "取消置顶 Claude Code" });
  const pin = screen.getByRole("button", { name: "置顶 Amp" });
  expect(screen.queryByText("JSON · claude-code")).not.toBeInTheDocument();
  expect(screen.queryByText("JSON · amp")).not.toBeInTheDocument();
  expect(unpin.querySelector('[data-icon="pin-off"]')).not.toBeNull();
  expect(unpin.querySelector('[data-icon="pin"]')).toBeNull();
  expect(pin.querySelector('[data-icon="pin"]')).not.toBeNull();
  expect(pin.querySelector('[data-icon="pin-off"]')).toBeNull();

  fireEvent.click(unpin);
  await waitFor(() => {
    expect(pinnedAgentMocks.commit).toHaveBeenCalledWith(["codex", "qoder"]);
  });
  expect(await screen.findByRole("status")).toHaveTextContent(/Claude Code.*取消置顶/);
});

it("opens the custom Agent creation card as one native action", async () => {
  const onAddAgent = vi.fn();
  const { container } = render(
    <AgentNavigation
      agents={agents}
      selectedAgentId="codex"
      onSelectAgent={vi.fn()}
      onAddAgent={onAddAgent}
    />,
  );

  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);

  fireEvent.click(screen.getByRole("button", { name: "自定义" }));
  const addAgent = screen.getByRole("button", { name: "添加自定义 Agent" });
  expect(addAgent).toHaveClass("arca-agent-hand-select");
  expect(addAgent.querySelector("button")).toBeNull();
  fireEvent.click(addAgent);
  await waitFor(() => expect(onAddAgent).toHaveBeenCalledOnce());
  expect(
    screen.queryByRole("dialog", { name: "选择和置顶 Agent" }),
  ).not.toBeInTheDocument();
});

it("lists and opens a projection-only Model Agent", () => {
  const projection: AgentCapabilityView = {
    identity: {
      id: "model-only",
      name: "Projection Model",
      enabled: true,
      builtin: false,
      category: "coding-agent",
      evidence: "official",
    },
    installed: true,
    capabilities: {
      model: {
        mode: "managed",
        storage_authority: "arca-mapping",
        installed: true,
        config_paths: ["~/.model-only/config.json"],
        assigned_profiles: [],
        supports_multiple: false,
        supports_global_selection: true,
        credential_mode: "guided",
        supported_protocols: ["openai-responses"],
      },
    },
  };
  const onSelectAgent = vi.fn();
  const { container } = render(
    <AgentNavigation
      agents={mergeAgentInfos([], [projection])}
      selectedAgentId={null}
      onSelectAgent={onSelectAgent}
    />,
  );

  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);
  fireEvent.change(screen.getByRole("searchbox", { name: "搜索 Agent" }), { target: { value: "model-only" } });
  fireEvent.keyDown(screen.getByRole("searchbox"), { key: "Enter" });
  expect(onSelectAgent).toHaveBeenCalledWith("model-only");
});
