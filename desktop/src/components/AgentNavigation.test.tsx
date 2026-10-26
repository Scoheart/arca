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

it("groups unpinned Agents by origin and keeps pinned Agents out of both groups", () => {
  const custom = { ...agent("team-workbench", "Team Workbench"), builtin: false };
  const { container } = render(
    <AgentNavigation agents={[...agents, custom]} selectedAgentId="codex" onSelectAgent={vi.fn()} />,
  );
  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);

  const picker = within(screen.getByRole("dialog", { name: "选择和置顶 Agent" }));
  const customGroup = within(picker.getByRole("group", { name: "自定义 Agent" }));
  const builtinGroup = within(picker.getByRole("group", { name: "内置 Agent" }));
  expect(customGroup.getByText("Team Workbench")).toBeVisible();
  expect(customGroup.queryByText("Amp")).not.toBeInTheDocument();
  expect(builtinGroup.getByText("Amp")).toBeVisible();
  expect(builtinGroup.queryByText("Team Workbench")).not.toBeInTheDocument();
  expect(builtinGroup.queryByText("Codex")).not.toBeInTheDocument();
  expect(picker.getAllByText("Codex")).toHaveLength(1);
  expect(picker.queryByText("全部 Agent")).not.toBeInTheDocument();
});

it("groups search matches, retains pin state, and hides empty categories", () => {
  const onSelectAgent = vi.fn();
  const custom = { ...agent("code-workbench", "Code Workbench"), builtin: false };
  const { container } = render(
    <AgentNavigation agents={[...agents, custom]} selectedAgentId="codex" onSelectAgent={onSelectAgent} />,
  );
  fireEvent.click(container.querySelector<HTMLButtonElement>(".arca-agent-picker-trigger")!);
  const picker = within(screen.getByRole("dialog", { name: "选择和置顶 Agent" }));
  const search = picker.getByRole("searchbox", { name: "搜索 Agent" });

  fireEvent.change(search, { target: { value: "code" } });
  expect(within(picker.getByRole("group", { name: "自定义 Agent" })).getByText("Code Workbench")).toBeVisible();
  expect(within(picker.getByRole("group", { name: "内置 Agent" })).getByRole("button", { name: "取消置顶 Codex" })).toBeEnabled();
  expect(picker.getAllByText("Codex")).toHaveLength(1);

  fireEvent.change(search, { target: { value: "workbench" } });
  expect(picker.queryByRole("group", { name: "内置 Agent" })).not.toBeInTheDocument();
  expect(picker.getByRole("group", { name: "自定义 Agent" })).toBeVisible();

  fireEvent.change(search, { target: { value: "no-such-agent" } });
  expect(picker.queryByRole("group", { name: "自定义 Agent" })).not.toBeInTheDocument();
  expect(picker.getByText("未找到匹配项")).toBeVisible();

  fireEvent.change(search, { target: { value: "workbench" } });
  fireEvent.click(picker.getByText("Code Workbench").closest("button")!);
  expect(onSelectAgent).toHaveBeenCalledWith("code-workbench");
  expect(screen.queryByRole("dialog", { name: "选择和置顶 Agent" })).not.toBeInTheDocument();
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
  const codeRow = picker.getByText("Claude Code").closest(".arca-agent-picker-select");
  const desktopRow = picker
    .getByText("Claude Desktop")
    .closest(".arca-agent-picker-select");
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

it("keeps the HTML drag source hit-testable and commits the dropped order", async () => {
  const { container } = render(
    <AgentNavigation
      agents={agents}
      selectedAgentId="codex"
      onSelectAgent={vi.fn()}
    />,
  );

  const trigger = container.querySelector<HTMLButtonElement>(
    ".arca-agent-picker-trigger",
  );
  expect(trigger).not.toBeNull();
  fireEvent.click(trigger!);

  const sourceHandle = screen.getByRole("button", {
    name: "调整 Qoder 的置顶顺序",
  });
  const sourceSlot = sourceHandle.closest(".arca-agent-picker-slot");
  const targetHandle = screen.getByRole("button", {
    name: "调整 Claude Code 的置顶顺序",
  });
  const targetSlot = targetHandle.closest(".arca-agent-picker-slot");
  expect(sourceSlot).not.toBeNull();
  expect(targetSlot).not.toBeNull();

  vi.spyOn(targetSlot!, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 48,
    left: 0,
    right: 360,
    width: 360,
    height: 48,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  const dataTransfer = {
    effectAllowed: "none",
    dropEffect: "none",
    setData: vi.fn(),
    setDragImage: vi.fn(),
  } as unknown as DataTransfer;

  fireEvent.dragStart(sourceHandle, { dataTransfer });
  expect(sourceSlot).toHaveAttribute("data-drag-source", "true");
  expect(targetSlot).toHaveAttribute("data-sorting", "true");

  fireEvent.dragOver(targetSlot!, { clientY: 1, dataTransfer });
  fireEvent.drop(targetSlot!, { clientY: 1, dataTransfer });

  await waitFor(() => {
    expect(pinnedAgentMocks.commit).toHaveBeenCalledWith([
      "qoder",
      "claude-code",
      "codex",
    ]);
  });
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
  expect(screen.getByText("Claude Code 已取消置顶")).toHaveAttribute("aria-live", "polite");
});

it("uses the entire custom Agent footer as one native action", () => {
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

  const addAgent = screen.getByRole("button", { name: "添加自定义 Agent" });
  expect(addAgent).toHaveClass("arca-agent-picker-footer");
  expect(addAgent.querySelector("button")).toBeNull();
  fireEvent.click(addAgent);
  expect(onAddAgent).toHaveBeenCalledOnce();
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
      builtin: true,
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
  fireEvent.click(screen.getByText("Projection Model").closest("button")!);
  expect(onSelectAgent).toHaveBeenCalledWith("model-only");
});
