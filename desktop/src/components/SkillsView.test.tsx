import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../lib/api";
import type { SkillsState } from "../hooks/useSkillsState";
import type {
  SkillDetail,
  SkillInventoryItem,
  ResourceNavigationRequest,
  SkillsInventory,
  UpdateCheckOutcome,
} from "../lib/types";
import {
  resolutionFixture,
  sharedTargetPlanFixture,
  skillDetailFixture,
  skillsInventoryFixture,
  skillsStateFixture,
} from "../test/skillsFixtures";
import App from "../App";
import { SkillsView } from "./SkillsView";

const appMocks = vi.hoisted(() => ({
  useInstallState: vi.fn(),
  useSkillsState: vi.fn(),
  useUpdater: vi.fn(),
  useCliTool: vi.fn(),
  usePinnedAgents: vi.fn(),
  getVersion: vi.fn(),
  agentViewProps: vi.fn(),
}));

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    getSkillDetail: vi.fn(),
    resolveGithubSkillSource: vi.fn(),
  };
});
vi.mock("../hooks/useInstallState", () => ({
  useInstallState: appMocks.useInstallState,
}));
vi.mock("../hooks/useSkillsState", async () => {
  const actual = await vi.importActual<typeof import("../hooks/useSkillsState")>(
    "../hooks/useSkillsState",
  );
  return { ...actual, useSkillsState: appMocks.useSkillsState };
});
vi.mock("../hooks/useUpdater", () => ({ useUpdater: appMocks.useUpdater }));
vi.mock("../hooks/useCliTool", () => ({ useCliTool: appMocks.useCliTool }));
vi.mock("../hooks/usePinnedAgents", () => ({
  usePinnedAgents: appMocks.usePinnedAgents,
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: appMocks.getVersion }));
vi.mock("./RegistryView", () => ({
  RegistryView: () => <div>registry-view</div>,
}));
vi.mock("./ModelsView", () => ({ ModelsView: () => <div>models-view</div> }));
vi.mock("./AgentView", () => ({
  AgentView: (props: {
    agentId: string;
    skillsState?: SkillsState;
    onOpenResource?: (request: ResourceNavigationRequest) => void;
  }) => {
    appMocks.agentViewProps(props);
    return (
      <div>
        agent-view:{props.agentId}
        <button
          type="button"
          onClick={() =>
            props.onOpenResource?.({ domain: "skill", kind: "detail", skillName: "review-changes" })
          }
        >
          查看 Agent Skill
        </button>
      </div>
    );
  },
}));
vi.mock("./AddAgentDialog", () => ({ AddAgentDialog: () => null }));
vi.mock("./RegistryEditPage", () => ({ RegistryEditPage: () => null }));
vi.mock("./UpdateBanner", () => ({ UpdateBanner: () => null }));

const stateWith = (
  inventory: SkillsInventory | null,
  overrides: Partial<SkillsState> = {},
): SkillsState => ({
  ...skillsStateFixture(),
  inventory,
  ...overrides,
});

const importedItem = (): SkillInventoryItem => ({
  ...skillsInventoryFixture().items[1],
  identity: "central:imported-legacy",
  name: "imported-legacy",
  description: "Imported local instructions",
  content_kind: "instructions",
  source: {
    kind: "imported",
    original_path: "~/.cursor/skills/imported-legacy",
    backup_path: "~/.arca/backups/skills/fixture/imported-legacy",
  },
});

const appAgent = (index: number) => ({
  id: `agent-${index}`,
  name: `Agent ${index}`,
  format: "json",
  key: "mcpServers",
  has_global: true,
  has_project: false,
  enabled: true,
  supported_transports: ["stdio" as const, "http" as const],
  global: `~/.agent-${index}/settings.json`,
  project: null,
  skills_global_dir: null,
  docs: null,
  note: null,
  category: "coding",
  evidence: "official",
  verified_at: null,
  builtin: true,
});

const installStateForApp = (loading: boolean) => ({
  agents: Array.from({ length: 6 }, (_, index) => appAgent(index + 1)),
  loading,
  refreshAll: vi.fn().mockResolvedValue(undefined),
  refreshAgents: vi.fn().mockResolvedValue([]),
});

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.mocked(api.getSkillDetail).mockReset();
  vi.mocked(api.getSkillDetail).mockImplementation(async (identity) =>
    skillDetailFixture(identity.split(":").at(-1)),
  );
  vi.mocked(api.resolveGithubSkillSource).mockReset();
  vi.mocked(api.resolveGithubSkillSource).mockResolvedValue(resolutionFixture());
  appMocks.useInstallState.mockReset();
  appMocks.useInstallState.mockReturnValue(installStateForApp(true));
  appMocks.useSkillsState.mockReset();
  appMocks.useSkillsState.mockReturnValue(skillsStateFixture());
  appMocks.useUpdater.mockReset();
  appMocks.useUpdater.mockReturnValue({
    phase: { kind: "idle" },
    checkNow: vi.fn().mockResolvedValue("latest"),
    download: vi.fn(),
    restart: vi.fn(),
    dismiss: vi.fn(),
    later: vi.fn(),
  });
  appMocks.useCliTool.mockReset();
  appMocks.usePinnedAgents.mockReset();
  appMocks.usePinnedAgents.mockReturnValue({
    agentIds: Array.from({ length: 6 }, (_, index) => `agent-${index + 1}`),
    ready: true,
    saving: false,
    commit: vi.fn().mockResolvedValue(true),
  });
  appMocks.getVersion.mockReset();
  appMocks.getVersion.mockResolvedValue("1.2.14");
  appMocks.agentViewProps.mockReset();
});

it("keeps one card but routes copy-specific actions to the real inventory identity", async () => {
  const inventory = skillsInventoryFixture();
  const central = { ...inventory.items[0], states: ["managed" as const], content_hash: "central-content" };
  const external: SkillInventoryItem = { ...central, identity: "target:agents-user:review-changes",
    location: { kind: "agent_target", target_id: "agents-user", global_dir: "~/.agents/skills" },
    states: ["external"], source: null, content_hash: "external-content" };
  const broken: SkillInventoryItem = { ...central, identity: "target:claude-user:review-changes",
    location: { kind: "agent_target", target_id: "claude-user", global_dir: "~/.claude/skills" },
    states: ["broken_link"], content_hash: null };
  inventory.items = [central, external, broken];
  vi.mocked(api.getSkillDetail).mockImplementation(async (identity) => ({
    ...skillDetailFixture(), item: inventory.items.find((item) => item.identity === identity)!,
  }));
  const fixture = skillsStateFixture();
  const plan = vi.fn(fixture.plan);
  render(<SkillsView state={stateWith(inventory, { plan })} />);
  expect(screen.getAllByRole("button", { name: "打开 Skill review-changes 详情" })).toHaveLength(1);
  await userEvent.click(screen.getByRole("button", { name: "打开 Skill review-changes 详情" }));
  const inspector = await screen.findByRole("complementary", { name: "review-changes 详情" });
  expect(within(inspector).queryByRole("button", { name: "导入" })).not.toBeInTheDocument();
  expect(within(inspector).queryByRole("button", { name: "修复" })).not.toBeInTheDocument();
  await userEvent.click(within(inspector).getByRole("combobox", { name: "Skill 位置" }));
  await userEvent.click(screen.getByRole("option", { name: "~/.agents/skills" }));
  await waitFor(() => expect(api.getSkillDetail).toHaveBeenLastCalledWith(external.identity));
  await userEvent.click(within(inspector).getByRole("checkbox", { name: "备份并替换同名中央副本" }));
  expect(within(inspector).getByRole("checkbox", { name: "备份并替换同名中央副本" })).toBeChecked();
  await userEvent.click(within(inspector).getByRole("combobox", { name: "Skill 位置" }));
  await userEvent.click(screen.getByRole("option", { name: "~/.claude/skills" }));
  expect(within(inspector).getByRole("button", { name: "修复" })).toBeVisible();
  expect(within(inspector).queryByRole("button", { name: "导入" })).not.toBeInTheDocument();
  await userEvent.click(within(inspector).getByRole("combobox", { name: "Skill 位置" }));
  await userEvent.click(screen.getByRole("option", { name: "~/.agents/skills" }));
  expect(within(inspector).getByRole("checkbox", { name: "备份并替换同名中央副本" })).not.toBeChecked();
  await userEvent.click(within(inspector).getByRole("button", { name: "导入" }));
  expect(plan).toHaveBeenCalledWith({ operation: "import_skill", request: { identity: external.identity, replace_conflicts: false } });
});

it("reloads the selected Skill body when its observed content changes at the same identity", async () => {
  const inventory = skillsInventoryFixture();
  const item = inventory.items[0];
  vi.mocked(api.getSkillDetail).mockResolvedValueOnce({ ...skillDetailFixture(), item, skill_md: "first body" });
  const view = render(<SkillsView state={stateWith(inventory)} />);
  await userEvent.click(screen.getByRole("button", { name: `打开 Skill ${item.name} 详情` }));
  await screen.findByText("first body");
  const changed = { ...item, content_hash: "updated-content", updated_at: "2026-10-05T00:00:00Z" };
  vi.mocked(api.getSkillDetail).mockResolvedValueOnce({ ...skillDetailFixture(), item: changed, skill_md: "updated body" });
  view.rerender(<SkillsView state={stateWith({ ...inventory, items: inventory.items.map((value) => value.identity === item.identity ? changed : value) })} />);
  await screen.findByText("updated body");
  expect(screen.queryByText("first body")).not.toBeInTheDocument();
  expect(api.getSkillDetail).toHaveBeenCalledTimes(2);
});

it("reloads an external copy after an inventory refresh without a managed hash or timestamp", async () => {
  const inventory = skillsInventoryFixture();
  const item: SkillInventoryItem = { ...inventory.items[0], states: ["external"], source: null,
    content_hash: null, updated_at: null, identity: "target:agents-user:review-changes",
    location: { kind: "agent_target", target_id: "agents-user", global_dir: "~/.agents/skills" } };
  inventory.items = [item];
  vi.mocked(api.getSkillDetail).mockResolvedValueOnce({ ...skillDetailFixture(), item, skill_md: "external first body" });
  const view = render(<SkillsView state={stateWith(inventory)} />);
  await userEvent.click(screen.getByRole("button", { name: `打开 Skill ${item.name} 详情` }));
  await screen.findByText("external first body");
  vi.mocked(api.getSkillDetail).mockResolvedValueOnce({ ...skillDetailFixture(), item, skill_md: "external updated body" });
  view.rerender(<SkillsView state={stateWith({ ...inventory, items: [{ ...item }] })} />);
  await screen.findByText("external updated body");
  expect(screen.queryByText("external first body")).not.toBeInTheDocument();
  expect(api.getSkillDetail).toHaveBeenCalledTimes(2);
});

afterEach(cleanup);

describe("SkillsView", () => {
  it("applies a metadata-only update without an informational confirmation dialog", async () => {
    const inventory = skillsInventoryFixture();
    const plan = { ...sharedTargetPlanFixture(), kind: "update" as const, targets: [],
      warnings: ["Source revision changed; Skill content is unchanged."],
      skills: sharedTargetPlanFixture().skills.map((skill) => ({ ...skill, existing_states: ["managed" as const], replace_existing: false, files: [] })) };
    const commit = vi.fn().mockResolvedValue(inventory);
    render(<SkillsView state={stateWith(inventory, { plan: vi.fn().mockResolvedValue(plan), commit })} />);
    await userEvent.click(screen.getByRole("button", { name: "打开 Skill review-changes 详情" }));
    await userEvent.click(screen.getByRole("button", { name: "更新" }));
    await waitFor(() => expect(commit).toHaveBeenCalledWith(plan, null));
    expect(screen.queryByRole("dialog", { name: "确认 Skill 更改" })).not.toBeInTheDocument();
  });

  it("repairs missing content without confirming again when no existing copy is overwritten", async () => {
    const inventory = skillsInventoryFixture();
    inventory.items[0] = { ...inventory.items[0], states: ["managed", "missing"] };
    const plan = { ...sharedTargetPlanFixture(), kind: "repair" as const,
      skills: sharedTargetPlanFixture().skills.map((skill) => ({ ...skill, replace_existing: false })) };
    const commit = vi.fn().mockResolvedValue(inventory);
    render(<SkillsView state={stateWith(inventory, { plan: vi.fn().mockResolvedValue(plan), commit })} />);
    await userEvent.click(screen.getByRole("button", { name: "打开 Skill review-changes 详情" }));
    await userEvent.click(screen.getByRole("button", { name: "修复" }));
    await waitFor(() => expect(commit).toHaveBeenCalledWith(plan, null));
    expect(screen.queryByRole("dialog", { name: "确认 Skill 更改" })).not.toBeInTheDocument();
  });

  it("replaces the Skill Inspector with lifecycle confirmation instead of stacking dialogs", async () => {
    const user = userEvent.setup();
    const planned = {
      ...sharedTargetPlanFixture(),
      kind: "update" as const,
      requires_risk_override: true,
    };
    const plan = vi.fn().mockResolvedValue(planned);
    render(<SkillsView state={stateWith(skillsInventoryFixture(), { plan })} />);

    await user.click(screen.getByRole("button", { name: /打开 Skill review-changes 详情/ }));
    expect(await screen.findByRole("complementary", { name: "review-changes 详情" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "更新" }));

    expect(plan).toHaveBeenCalledWith({
      operation: "update_skill",
      request: {
        skill_name: "review-changes",
        replace_local_changes: false,
      },
    });
    expect(await screen.findByRole("dialog", { name: "确认 Skill 更改" })).toBeVisible();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(screen.queryByRole("complementary", { name: "review-changes 详情" })).not.toBeInTheDocument();
  });

  it("shows one card and one source count for duplicate target rows with the same name", () => {
    const inventory = skillsInventoryFixture();
    const base = inventory.items[1];
    const duplicateRows: SkillInventoryItem[] = [
      {
        ...base,
        identity: "target:agents-user:dws",
        name: "dws",
        location: {
          kind: "agent_target",
          target_id: "agents-user",
          global_dir: "~/.agents/skills",
        },
        source: {
          kind: "imported",
          original_path: "~/.agents/skills/dws",
          backup_path: "~/.arca/backups/skills/dws",
        },
        affected_agent_ids: ["codex", "cursor"],
      },
      {
        ...base,
        identity: "target:claude-user:dws",
        name: "dws",
        location: {
          kind: "agent_target",
          target_id: "claude-user",
          global_dir: "~/.claude/skills",
        },
        source: {
          kind: "imported",
          original_path: "~/.claude/skills/dws",
          backup_path: "~/.arca/backups/skills/dws-claude",
        },
        affected_agent_ids: ["claude-code"],
      },
    ];

    render(
      <SkillsView
        state={stateWith({ ...inventory, items: duplicateRows })}
      />,
    );

    expect(screen.getAllByRole("heading", { name: "dws" })).toHaveLength(1);
    expect(screen.getByRole("button", { name: /全部 Skills.*1/ })).toBeVisible();
    expect(screen.getByRole("button", { name: /^skills\s*1$/ })).toBeVisible();
  });

  it("renders the app-owned inventory inside the Skills workspace", () => {
    render(<SkillsView state={skillsStateFixture()} />);

    expect(
      screen.getByRole("heading", { name: "review-changes" }),
    ).toBeVisible();
    expect(screen.queryByRole("tablist", { name: "Skill 状态" })).not.toBeInTheDocument();
    expect(screen.getByRole("separator", { name: "调整侧边栏宽度" })).toBeVisible();
    expect(screen.getByRole("list", { name: "Skill 资产" })).toHaveClass("arca-skill-card-grid");
    expect(screen.getByText("Review repository changes")).toBeVisible();
    expect(screen.getByText("acme/skills")).toBeVisible();
    expect(screen.queryByText("风险")).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText("搜索 Skills")).toBeVisible();
  });

  it("omits inferred content-type navigation", () => {
    render(<SkillsView state={skillsStateFixture()} />);

    expect(screen.queryByText("内容类型")).not.toBeInTheDocument();
    expect(screen.getByText("来源")).toBeVisible();
    expect(screen.getByText("全部 Skills")).toBeVisible();
    expect(screen.getByText("acme/skills")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /说明型/ }),
    ).not.toBeInTheDocument();
  });

  it("combines source and query filters while keeping all status rows", async () => {
    const user = userEvent.setup();
    const inventory = skillsInventoryFixture();
    inventory.items.push(importedItem());
    render(<SkillsView state={stateWith(inventory)} />);

    await user.click(screen.getByRole("button", { name: /fixtures\s*1/ }));
    expect(screen.getByRole("heading", { name: "unassigned-skill" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "imported-legacy" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "review-changes" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tablist", { name: "Skill 状态" })).not.toBeInTheDocument();

    await user.type(screen.getByPlaceholderText("搜索当前来源的 Skills…"), "review");
    expect(screen.getByRole("button", { name: /acme\/skills\s*1/ })).toBeVisible();
    expect(screen.getByRole("button", { name: /fixtures\s*1/ })).toBeVisible();
    expect(screen.getByText("没有匹配项")).toBeVisible();
  });

  it("opens a selected card from the keyboard and loads detail only for the inspector", async () => {
    const user = userEvent.setup();
    render(<SkillsView state={skillsStateFixture()} />);
    expect(api.getSkillDetail).not.toHaveBeenCalled();

    const card = screen.getByRole("button", { name: /review-changes/ });
    expect(card).toHaveAttribute("aria-pressed", "false");
    card.focus();
    await user.keyboard(" ");

    expect(card).toHaveAttribute("aria-pressed", "true");
    expect(await screen.findByLabelText("review-changes 详情")).toBeVisible();
    await waitFor(() =>
      expect(api.getSkillDetail).toHaveBeenCalledWith("central:review-changes"),
    );
    expect(await screen.findByLabelText("SKILL.md 纯文本预览")).toBeVisible();
  });

  it("does not expose legacy assignment switches in the Inspector", async () => {
    const user = userEvent.setup();
    render(<SkillsView state={skillsStateFixture()} />);
    await user.click(screen.getByRole("button", { name: /review-changes/ }));
    const inspector = await screen.findByLabelText("review-changes 详情");
    expect(within(inspector).queryByRole("switch")).not.toBeInTheDocument();
  });

  it("moves focus into the inspector and returns it to the selected card", async () => {
    const user = userEvent.setup();
    render(<SkillsView state={skillsStateFixture()} />);

    const card = screen.getByRole("button", { name: /review-changes/ });
    card.focus();
    await user.keyboard(" ");
    const inspector = await screen.findByLabelText("review-changes 详情");
    await waitFor(() => expect(inspector).toHaveFocus());

    await user.click(
      within(inspector).getByRole("button", { name: "关闭详情" }),
    );
    await waitFor(() => expect(card).toHaveFocus());
  });

  it("discards a late detail after filtering closes the inspector and another item opens", async () => {
    const user = userEvent.setup();
    const first = deferred<SkillDetail>();
    const second = {
      ...skillDetailFixture("unassigned-skill"),
      skill_md: "second-selection",
    };
    vi.mocked(api.getSkillDetail)
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(second);
    render(<SkillsView state={skillsStateFixture()} />);

    await user.click(screen.getByRole("button", { name: /review-changes/ }));
    const firstInspector = await screen.findByLabelText("review-changes 详情");
    await waitFor(() => expect(firstInspector).toHaveFocus());
    await user.type(screen.getByPlaceholderText("搜索 Skills"), "unassigned");
    await waitFor(() =>
      expect(
        screen.queryByLabelText("review-changes 详情"),
      ).not.toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: /unassigned-skill/ }));
    expect(await screen.findByText("second-selection")).toBeVisible();

    first.resolve({ ...skillDetailFixture("review-changes"), skill_md: "stale-first" });
    await act(async () => {
      await first.promise;
    });
    expect(screen.queryByText("stale-first")).not.toBeInTheDocument();
    expect(screen.getByText("second-selection")).toBeVisible();
  });

  it("invalidates a pending detail request when the workspace unmounts", async () => {
    const pending = deferred<SkillDetail>();
    vi.mocked(api.getSkillDetail).mockReturnValueOnce(pending.promise);
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { unmount } = render(<SkillsView state={skillsStateFixture()} />);

    await userEvent.click(screen.getByRole("button", { name: /review-changes/ }));
    unmount();
    pending.resolve(skillDetailFixture("review-changes"));
    await act(async () => {
      await pending.promise;
    });

    expect(errorSpy).not.toHaveBeenCalled();
    errorSpy.mockRestore();
  });

  it("discards an old response when the same Skill is closed and reopened", async () => {
    const user = userEvent.setup();
    const first = deferred<SkillDetail>();
    const second = {
      ...skillDetailFixture("review-changes"),
      skill_md: "fresh-same-selection",
    };
    vi.mocked(api.getSkillDetail)
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(second);
    render(<SkillsView state={skillsStateFixture()} />);

    const card = screen.getByRole("button", { name: /review-changes/ });
    await user.click(card);
    const firstInspector = await screen.findByLabelText("review-changes 详情");
    await user.click(
      within(firstInspector).getByRole("button", { name: "关闭详情" }),
    );
    await user.click(card);
    expect(await screen.findByText("fresh-same-selection")).toBeVisible();

    first.resolve({
      ...skillDetailFixture("review-changes"),
      skill_md: "stale-same-selection",
    });
    await act(async () => {
      await first.promise;
    });
    expect(screen.queryByText("stale-same-selection")).not.toBeInTheDocument();
    expect(screen.getByText("fresh-same-selection")).toBeVisible();
  });

  it("keeps workspace chrome visible across loading, initial error, and recovery", async () => {
    const refresh = vi.fn().mockResolvedValue(skillsInventoryFixture());
    const { rerender } = render(
      <SkillsView
        state={stateWith(null, { loading: true, error: null, refresh })}
      />,
    );
    expect(screen.queryByRole("tablist", { name: "Skill 状态" })).not.toBeInTheDocument();
    expect(screen.getByText("正在读取 Skills…")).toBeVisible();
    expect(screen.getByRole("button", { name: "检查更新" })).toBeDisabled();

    rerender(
      <SkillsView
        state={stateWith(null, {
          loading: false,
          error: { code: "io", message: "读取失败，请检查目录权限。" },
          refresh,
        })}
      />,
    );
    expect(screen.getByText("读取 Skills 失败")).toBeVisible();
    expect(screen.getByText("读取失败，请检查目录权限。")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(refresh).toHaveBeenCalledOnce();

    const recovery = skillsInventoryFixture();
    recovery.recovery_error = "检测到未完成事务，请先恢复。";
    rerender(
      <SkillsView state={stateWith(recovery, { loading: false, error: null })} />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("上次 Skill 操作未完成");
    expect(screen.getByRole("alert")).toHaveTextContent("请重新打开 Arca，程序会自动重试恢复。");
    expect(screen.queryByText("检测到未完成事务，请先恢复。")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "review-changes" })).toBeVisible();
    expect(screen.getByRole("button", { name: "检查更新" })).toBeDisabled();
  });

  it("treats an app-owned recovery error as read-only", async () => {
    const refresh = vi.fn().mockResolvedValue(skillsInventoryFixture());
    const onIntentConsumed = vi.fn();
    render(
      <SkillsView
        state={stateWith(null, {
          loading: false,
          error: {
            code: "recovery_required",
            message: "journal recovery required",
          },
          refresh,
        })}
        onIntentConsumed={onIntentConsumed}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("上次 Skill 操作未完成");
    expect(screen.getByRole("alert")).toHaveTextContent("请重新打开 Arca，程序会自动重试恢复。");
    expect(screen.queryByText("journal recovery required")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "检查更新" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加 Skill" })).toBeDisabled();
    expect(screen.queryByRole("dialog", { name: "添加 Skill" })).not.toBeInTheDocument();
    expect(onIntentConsumed).not.toHaveBeenCalled();
    expect(refresh).not.toHaveBeenCalled();
  });

  it("closes and cancels a resolved install when the workspace enters recovery", async () => {
    const user = userEvent.setup();
    const inventory = skillsInventoryFixture();
    const cancel = vi.fn().mockResolvedValue(undefined);
    const initialState = stateWith(inventory, { cancel });
    const { rerender } = render(
      <SkillsView
        state={initialState}
        onIntentConsumed={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "添加 Skill" }));

    await user.type(screen.getByLabelText("仓库地址"), "acme/skills");
    await user.click(screen.getByRole("button", { name: "查找" }));
    expect(await screen.findByRole("checkbox", { name: "review-changes" })).toBeVisible();

    rerender(
      <SkillsView
        state={stateWith(inventory, {
          cancel,
          error: {
            code: "recovery_required",
            message: "journal recovery required",
          },
        })}
      />,
    );

    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "添加 Skill" }),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("resolve-fixture"));
    expect(cancel).toHaveBeenCalledOnce();
    expect(screen.getByRole("alert")).toHaveTextContent("上次 Skill 操作未完成");
  });

  it("keeps cached inventory under hook errors and runs only a manual metadata check", async () => {
    const pending = deferred<UpdateCheckOutcome>();
    const checkUpdates = vi.fn(() => pending.promise);
    render(
      <SkillsView
        state={stateWith(skillsInventoryFixture(), {
          error: { code: "rate_limited", message: "GitHub 暂时限流。", retry_at: "2026-07-17T08:00:00Z" },
          checkUpdates,
        })}
      />,
    );

    expect(screen.getByRole("heading", { name: "review-changes" })).toBeVisible();
    expect(screen.getByText("GitHub 暂时限流。")).toBeVisible();
    expect(screen.getByText(/2026-07-17T08:00:00Z/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "检查更新" }));
    expect(checkUpdates).toHaveBeenCalledWith(true);
    expect(screen.getByRole("button", { name: "检查中…" })).toBeDisabled();

    pending.resolve({
      performed: true,
      checked: 2,
      available: [],
      skipped_pinned: [],
      errors: {},
      checked_at: "2026-07-17T08:00:00Z",
    });
    await act(async () => {
      await pending.promise;
    });
    expect(screen.getByRole("button", { name: "检查更新" })).toBeEnabled();
  });

  it("settles a pending metadata check safely after the workspace unmounts", async () => {
    const pending = deferred<UpdateCheckOutcome>();
    const checkUpdates = vi.fn(() => pending.promise);
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { unmount } = render(
      <SkillsView
        state={stateWith(skillsInventoryFixture(), { checkUpdates })}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "检查更新" }));
    unmount();
    pending.resolve({
      performed: true,
      checked: 2,
      available: [],
      skipped_pinned: [],
      errors: {},
      checked_at: "2026-07-17T08:00:00Z",
    });
    await act(async () => {
      await pending.promise;
    });

    expect(errorSpy).not.toHaveBeenCalled();
    errorSpy.mockRestore();
  });

  it("distinguishes an empty inventory from filters with no matches", async () => {
    const empty = skillsInventoryFixture();
    empty.items = [];
    const { rerender } = render(<SkillsView state={stateWith(empty)} />);
    expect(screen.getByText("暂无 Skills")).toBeVisible();
    expect(screen.getAllByRole("button", { name: "添加 Skill" })).toHaveLength(1);

    rerender(<SkillsView state={skillsStateFixture()} />);
    await userEvent.type(screen.getByPlaceholderText("搜索 Skills"), "no-such-skill");
    expect(screen.getByText("没有匹配项")).toBeVisible();
    expect(screen.getByText("调整搜索或来源筛选后重试。")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "清除筛选" }));
    expect(screen.getByRole("heading", { name: "review-changes" })).toBeVisible();
  });

  it("waits for inventory, consumes a detail intent once, and opens only a managed central Skill", async () => {
    const onIntentConsumed = vi.fn();
    const intent = { id: 17, domain: "skill" as const, kind: "detail" as const, skillName: "review-changes" };
    const { rerender } = render(
      <SkillsView
        state={stateWith(null, { loading: true })}
        intent={intent}
        onIntentConsumed={onIntentConsumed}
      />,
    );

    expect(onIntentConsumed).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("review-changes 详情")).not.toBeInTheDocument();

    rerender(
      <SkillsView
        state={skillsStateFixture()}
        intent={intent}
        onIntentConsumed={onIntentConsumed}
      />,
    );
    expect(await screen.findByLabelText("review-changes 详情")).toBeVisible();
    expect(onIntentConsumed).toHaveBeenCalledOnce();
    expect(onIntentConsumed).toHaveBeenCalledWith(17);

    rerender(
      <SkillsView
        state={stateWith(skillsInventoryFixture())}
        intent={intent}
        onIntentConsumed={onIntentConsumed}
      />,
    );
    expect(onIntentConsumed).toHaveBeenCalledOnce();
  });

  it("consumes an unavailable detail intent with a visible notice", async () => {
    const skillName = "missing-skill";
    const inventory = skillsInventoryFixture();
    inventory.items.push({
      ...inventory.items[1],
      identity: "central:external-copy",
      name: "external-copy",
      states: ["external"],
      source: null,
    });
    const onIntentConsumed = vi.fn();

    render(
      <SkillsView
        state={stateWith(inventory)}
        intent={{ id: 23, domain: "skill", kind: "detail", skillName }}
        onIntentConsumed={onIntentConsumed}
      />,
    );

    expect(
      await screen.findByText(`未找到 Skill 资产“${skillName}”。`),
    ).toBeVisible();
    expect(onIntentConsumed).toHaveBeenCalledOnce();
    expect(screen.queryByLabelText(`${skillName} 详情`)).not.toBeInTheDocument();
  });

  it("opens an external Skill candidate as an importable asset", async () => {
    const inventory = skillsInventoryFixture();
    inventory.items.push({
      ...inventory.items[1],
      identity: "central:external-copy",
      name: "external-copy",
      states: ["external"],
      source: null,
    });

    render(
      <SkillsView
        state={stateWith(inventory)}
        intent={{ id: 23, domain: "skill", kind: "detail", skillName: "external-copy" }}
        onIntentConsumed={vi.fn()}
      />,
    );

    expect(await screen.findByLabelText("external-copy 详情")).toBeVisible();
    expect(screen.queryByRole("button", { name: "管理 Agent" })).not.toBeInTheDocument();
  });

  it.each(["missing", "broken_link"] as const)(
    "opens a central managed source whose current target state is %s",
    async (targetState) => {
      const inventory = skillsInventoryFixture();
      const skillName = `${targetState}-central-skill`;
      inventory.items.push({
        ...inventory.items[1],
        identity: `central:${skillName}`,
        name: skillName,
        states: [targetState],
      });
      const onIntentConsumed = vi.fn();

      render(
        <SkillsView
          state={stateWith(inventory)}
          intent={{ id: 29, domain: "skill", kind: "detail", skillName }}
          onIntentConsumed={onIntentConsumed}
        />,
      );

      expect(await screen.findByLabelText(`${skillName} 详情`)).toBeVisible();
      expect(onIntentConsumed).toHaveBeenCalledWith(29);
      expect(
        screen.queryByText(`未找到 Skill 资产“${skillName}”。`),
      ).not.toBeInTheDocument();
    },
  );

  it("opens a source-less central anomaly referenced by assignment settings", async () => {
    const inventory = skillsInventoryFixture();
    inventory.items.push({
      ...inventory.items[1],
      identity: "central:assigned-external-anomaly",
      name: "assigned-external-anomaly",
      states: ["external"],
      source: null,
      assigned_target_ids: ["agents-user"],
      affected_agent_ids: ["codex", "cursor", "gemini"],
    });
    const onIntentConsumed = vi.fn();

    render(
      <SkillsView
        state={stateWith(inventory)}
        intent={{
          id: 30,
          domain: "skill",
          kind: "detail",
          skillName: "assigned-external-anomaly",
        }}
        onIntentConsumed={onIntentConsumed}
      />,
    );

    expect(
      await screen.findByLabelText("assigned-external-anomaly 详情"),
    ).toBeVisible();
    expect(onIntentConsumed).toHaveBeenCalledWith(30);
  });

  it("clears an unavailable navigation notice after a manual Skill selection", async () => {
    render(
      <SkillsView
        state={skillsStateFixture()}
        intent={{ id: 30, domain: "skill", kind: "detail", skillName: "missing-skill" }}
        onIntentConsumed={vi.fn()}
      />,
    );
    expect(
      await screen.findByText("未找到 Skill 资产“missing-skill”。"),
    ).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: /review-changes/ }));
    expect(await screen.findByLabelText("review-changes 详情")).toBeVisible();
    expect(
      screen.queryByText("未找到 Skill 资产“missing-skill”。"),
    ).not.toBeInTheDocument();
  });

  it("has no Agent-scoped install navigation", () => {
    render(<SkillsView state={skillsStateFixture()} />);
    expect(screen.queryByText("为 Agent 添加 Skill")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "添加 Skill" })).toBeVisible();
  });
});

describe("App Skills routing", () => {
  it("keeps navigation usable while fresh MCP state is still loading", async () => {
    render(<App />);
    expect(screen.getByText("registry-view")).toBeVisible();
    expect(screen.getByRole("button", { name: "MCPs" })).toBeVisible();
    expect(screen.getByRole("button", { name: /Models/ })).toBeVisible();
    expect(screen.getByRole("navigation", { name: "置顶 Agent" })).toBeVisible();
    expect(appMocks.useSkillsState).toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Skills" }));
    expect(
      await screen.findByRole("heading", { name: "review-changes" }),
    ).toBeVisible();
    expect(appMocks.useSkillsState.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("keeps MCP, Models, and Agent routes explicit after adding Skills", async () => {
    const user = userEvent.setup();
    appMocks.useInstallState.mockReturnValue(installStateForApp(false));
    render(<App />);

    expect(screen.getByText("registry-view")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /Models/ }));
    expect(await screen.findByText("models-view")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "MCPs" }));
    expect(screen.getByText("registry-view")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Agent 1" }));
    expect(await screen.findByText("agent-view:agent-1")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Skills" }));
    expect(
      await screen.findByRole("heading", { name: "review-changes" }),
    ).toBeVisible();
  });

  it("uses two Escape presses to close an Agent picker above a Skill inspector", async () => {
    const user = userEvent.setup();
    appMocks.useInstallState.mockReturnValue(installStateForApp(false));
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Skills" }));
    await user.click(screen.getByRole("button", { name: /review-changes/ }));
    expect(await screen.findByLabelText("review-changes 详情")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "选择 Agent" }));
    expect(screen.getByRole("dialog", { name: "选择和置顶 Agent" })).toBeVisible();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "选择和置顶 Agent" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("review-changes 详情")).toBeVisible();

    await user.keyboard("{Escape}");
    expect(screen.queryByLabelText("review-changes 详情")).not.toBeInTheDocument();
  });

  it("routes typed Agent requests through the sole app-owned Skills state", async () => {
    const user = userEvent.setup();
    const skillsState = skillsStateFixture();
    appMocks.useInstallState.mockReturnValue(installStateForApp(false));
    appMocks.useSkillsState.mockReturnValue(skillsState);
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Agent 1" }));
    expect(await screen.findByText("agent-view:agent-1")).toBeVisible();
    expect(appMocks.agentViewProps).toHaveBeenLastCalledWith(
      expect.objectContaining({
        agentId: "agent-1",
        skillsState,
        onOpenResource: expect.any(Function),
      }),
    );
    const openResource = appMocks.agentViewProps.mock.calls.at(-1)?.[0]
      .onOpenResource as (request: ResourceNavigationRequest) => void;

    await user.click(screen.getByRole("button", { name: "查看 Agent Skill" }));
    const inspector = await screen.findByLabelText("review-changes 详情");
    expect(inspector).toBeVisible();
    await user.click(
      within(inspector).getByRole("button", { name: "关闭详情" }),
    );
    expect(screen.queryByLabelText("review-changes 详情")).not.toBeInTheDocument();

    act(() => openResource({ domain: "skill", kind: "detail", skillName: "review-changes" }));
    expect(await screen.findByLabelText("review-changes 详情")).toBeVisible();
    expect(
      appMocks.useSkillsState.mock.results
        .filter((result) => result.type === "return")
        .every((result) => result.value === skillsState),
    ).toBe(true);
  });
});
