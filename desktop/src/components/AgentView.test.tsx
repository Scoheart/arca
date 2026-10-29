import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import type { InstallState } from "../hooks/useInstallState";
import type { SkillsState } from "../hooks/useSkillsState";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import type {
  AgentCapabilityView,
  AgentInfo,
  ModelAdoptionCandidate,
  ModelAgentView,
  ModelProfileView,
} from "../lib/types";
import { assetOperationPlanFixture } from "../test/consumptionFixtures";
import { AgentView } from "./AgentView";
import { ToastProvider } from "./Toast";

const apiMocks = vi.hoisted(() => ({
  listNativeResources: vi.fn().mockResolvedValue({ native_plugin_agents: [], host_mcps: [], native_plugins: [], unavailable: false }),
  listModelAgents: vi.fn().mockResolvedValue([]),
  listModelProfiles: vi.fn().mockResolvedValue([]),
  setAgentCredentialDelivery: vi.fn().mockResolvedValue({ agent: "pi", profile: "" }),
  listMcpIconPreferences: vi.fn().mockResolvedValue({
    "github::stdio": { kind: "builtin", value: "database" },
  }),
}));
const openerMocks = vi.hoisted(() => ({
  openPath: vi.fn().mockResolvedValue(undefined),
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn().mockResolvedValue("/Users/test"),
}));
vi.mock("@tauri-apps/plugin-opener", () => openerMocks);

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return { ...actual, ...apiMocks };
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("shows an Agent-level credential strategy next to config locations", async () => {
  const source = await readFile(resolve(process.cwd(), "src/components/AgentView.tsx"), "utf8");
  expect(source).toContain("setAgentCredentialDelivery");
  expect(source).toContain("凭据策略");
  expect(source).toContain("明文配置");
  expect(source).toContain("plaintextConfirmation");
  expect(source).toContain("available_deliveries");
  expect(source).toContain("ProviderGlyph");
  expect(source).toContain("modelProviderIcon");
  expect(source).not.toContain("setModelCredentialDelivery");
  expect(source).not.toContain("自动适配（推荐）");
  expect(source).not.toContain("需要 ENV");
});

const skillsOnlyAgent: AgentInfo = {
  id: "cortex-code",
  name: "Cortex Code",
  format: "unknown",
  key: "",
  has_global: false,
  has_project: false,
  enabled: true,
  supported_transports: [],
  global: null,
  project: null,
  skills_global_dir: "~/.snowflake/cortex/skills",
  docs: "https://docs.snowflake.com/",
  note: "只管理 Skills。",
  category: "coding-agent",
  evidence: "official-source",
  verified_at: "2026-07-22",
  builtin: true,
};

const skillsOnlyProjection: AgentCapabilityView = {
  identity: {
    id: skillsOnlyAgent.id,
    name: skillsOnlyAgent.name,
    enabled: true,
    builtin: true,
    category: skillsOnlyAgent.category,
    evidence: skillsOnlyAgent.evidence,
    docs: skillsOnlyAgent.docs,
    note: skillsOnlyAgent.note,
    verified_at: skillsOnlyAgent.verified_at,
  },
  installed: true,
  capabilities: {
    skill: {
      installed: true,
      target_id: "cortex-user",
      global_dir: skillsOnlyAgent.skills_global_dir!,
      alias_dirs: [],
      affected_agent_ids: [skillsOnlyAgent.id],
    },
  },
};

const state = {
  entries: [],
  agents: [skillsOnlyAgent],
  installed: [],
  refreshAgents: vi.fn().mockResolvedValue([skillsOnlyAgent]),
  rescan: vi.fn().mockResolvedValue([]),
} as unknown as InstallState;

const skillsState = {
  inventory: {
    items: [],
    agents: [{
      id: skillsOnlyAgent.id,
      name: skillsOnlyAgent.name,
      target_id: "cortex-user",
      global_dir: skillsOnlyAgent.skills_global_dir,
      affected_agent_ids: [skillsOnlyAgent.id],
      docs: skillsOnlyAgent.docs,
      evidence: skillsOnlyAgent.evidence,
      verified_at: skillsOnlyAgent.verified_at,
    }],
    targets: [],
    recovery_error: null,
  },
  loading: false,
  error: null,
  refresh: vi.fn(),
} as unknown as SkillsState;

const emptyConsumptionState = {
  agents: [],
  inventory: {
    revision: "empty",
    observed_at: "2026-08-04T00:00:00Z",
    consumptions: [],
    external: [],
    target_incidents: [],
  },
  plan: null,
} as unknown as ConsumptionState;

it("opens a projection-only Skills Agent without a legacy MCP-shaped row", async () => {
  const projectionOnlyState = {
    ...state,
    agents: [],
    refreshAgents: vi.fn().mockResolvedValue([]),
  } as unknown as InstallState;
  const consumptionState = {
    agents: [skillsOnlyProjection],
    inventory: { consumptions: [], external: [] },
  } as unknown as ConsumptionState;

  render(
    <AgentView
      state={projectionOnlyState}
      skillsState={skillsState}
      consumptionState={consumptionState}
      agentId={skillsOnlyAgent.id}
    />,
  );

  const locations = screen.getByRole("region", { name: "配置位置" });
  expect(within(locations).getByText("配置位置")).toBeVisible();
  expect(within(locations).getByText(/读取或写入的实际位置/)).toBeVisible();
  expect(within(locations).getByText("MCPs")).toBeVisible();
  expect(within(locations).getByText("Models")).toBeVisible();
  expect(within(locations).getByText("Skills")).toBeVisible();
  expect(await within(locations).findByText("/Users/test/.snowflake/cortex/skills")).toBeVisible();
  await userEvent.click(within(locations).getByRole("button", {
    name: "打开文件夹：/Users/test/.snowflake/cortex/skills",
  }));
  expect(openerMocks.openPath).toHaveBeenCalledWith("/Users/test/.snowflake/cortex/skills");
  expect(within(locations).getByRole("button", { name: "编辑配置" })).toBeVisible();
  expect(screen.queryByText(/UNKNOWN/)).not.toBeInTheDocument();
  expect(screen.queryByText(/coding-agent/)).not.toBeInTheDocument();
  expect(screen.queryByText(/公开来源/)).not.toBeInTheDocument();
  expect(screen.queryByText(skillsOnlyAgent.verified_at!)).not.toBeInTheDocument();
  expect(screen.queryByText("已核验")).not.toBeInTheDocument();

  await waitFor(() => {
    expect(screen.getByRole("tab", { name: /Skills/ })).toHaveAttribute("aria-selected", "true");
  });

  await userEvent.click(screen.getByRole("tab", { name: /MCPs/ }));
  expect(screen.getByText("此 Agent 未接入 MCP。")).toBeVisible();
  expect(screen.queryByRole("button", { name: "添加 MCP" })).not.toBeInTheDocument();
});

it("keeps Agent resources visible without a target recovery banner", async () => {
  const consumptionState = {
    ...emptyConsumptionState,
    agents: [skillsOnlyProjection],
    inventory: {
      ...emptyConsumptionState.inventory,
      target_incidents: [{
        id: "target-cortex-user",
        operation_id: "operation-1",
        capability: "skill",
        target_id: "cortex-user",
        target_path: "~/.snowflake/cortex/skills",
        affected_agent_ids: [skillsOnlyAgent.id],
        code: "target_recovery_required",
        retryable: true,
      }],
    },
  } as unknown as ConsumptionState;

  render(
    <AgentView
      state={state}
      skillsState={skillsState}
      consumptionState={consumptionState}
      agentId={skillsOnlyAgent.id}
    />,
  );

  await waitFor(() => {
    expect(screen.getByRole("tab", { name: /Skills/ })).toHaveAttribute("aria-selected", "true");
  });
  expect(screen.queryByText("此配置位置待收敛")).not.toBeInTheDocument();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();

  await userEvent.click(screen.getByRole("tab", { name: /MCPs/ }));
  expect(screen.getByText("此 Agent 未接入 MCP。")).toBeVisible();
  expect(screen.queryByText("此配置位置待收敛")).not.toBeInTheDocument();
});

it("toggles a managed Skill without routing through removal", async () => {
  const setSkillEnabled = vi.fn().mockResolvedValue({
    consumptions: [],
    external: [],
  });
  const consumptionState = {
    agents: [skillsOnlyProjection],
    inventory: {
      consumptions: [{
        agent_id: skillsOnlyAgent.id,
        asset: { domain: "skill", name: "review-changes" },
        desired: true,
        observed: true,
        enabled: true,
        status: "synced",
        reason: null,
        available_actions: [],
        affected_agent_ids: [skillsOnlyAgent.id],
        target: {
          target_id: "cortex-user",
          global_dir: skillsOnlyAgent.skills_global_dir!,
        },
      }],
      external: [],
    },
    plan: null,
    setSkillEnabled,
  } as unknown as ConsumptionState;

  render(
    <AgentView
      state={state}
      skillsState={skillsState}
      consumptionState={consumptionState}
      agentId={skillsOnlyAgent.id}
    />,
  );

  const toggle = await screen.findByRole("switch", { name: "停用 review-changes" });
  await userEvent.click(toggle);
  await waitFor(() => {
    expect(setSkillEnabled).toHaveBeenCalledWith(
      skillsOnlyAgent.id,
      "review-changes",
      false,
    );
  });
  expect(screen.queryByRole("heading", { name: "确认停用 Skill" })).not.toBeInTheDocument();
});

it("toggles one or every managed MCP without opening the review panel", async () => {
  const mcpAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "codex",
    name: "Codex",
    format: "json",
    key: "mcpServers",
    has_global: true,
    supported_transports: ["stdio", "http"],
    global: "~/.codex/config.json",
    skills_global_dir: null,
  };
  const mcpState = {
    ...state,
    agents: [mcpAgent],
  } as unknown as InstallState;
  const setMcpEnabled = vi.fn().mockResolvedValue({ consumptions: [], external: [] });
  const setAllMcpEnabled = vi.fn().mockResolvedValue({ consumptions: [], external: [] });
  const consumptionState = {
    agents: [],
    inventory: {
      consumptions: [{
        agent_id: mcpAgent.id,
        asset: { domain: "mcp", key: "github::stdio" },
        desired: true,
        observed: true,
        enabled: true,
        status: "synced",
        reason: null,
        available_actions: [],
        affected_agent_ids: [mcpAgent.id],
        target: null,
      }],
      external: [],
    },
    plan: null,
    setMcpEnabled,
    setAllMcpEnabled,
  } as unknown as ConsumptionState;

  render(
    <AgentView
      state={mcpState}
      skillsState={skillsState}
      consumptionState={consumptionState}
      agentId={mcpAgent.id}
    />,
  );

  await waitFor(() => expect(document.querySelector('[data-mcp-icon="database"]')).not.toBeNull());

  await userEvent.click(await screen.findByRole("switch", { name: "停用 github" }));
  await waitFor(() => expect(setMcpEnabled).toHaveBeenCalledWith(
    mcpAgent.id,
    "github::stdio",
    false,
  ));
  expect(screen.queryByRole("heading", { name: "确认停用 MCP" })).not.toBeInTheDocument();

  await userEvent.click(screen.getByRole("switch", { name: "停用全部 MCP" }));
  await waitFor(() => expect(setAllMcpEnabled).toHaveBeenCalledWith(mcpAgent.id, false));
  expect(screen.queryByRole("heading", { name: "确认停用 MCP" })).not.toBeInTheDocument();
});

it("keeps meaningful Agent badges while leaving builtin headers clean", () => {
  const communityAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "community-agent",
    name: "Community Agent",
    evidence: "community-extension",
  };
  const customAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "custom-agent",
    name: "Custom Agent",
    builtin: false,
  };
  const badgeState = {
    ...state,
    agents: [skillsOnlyAgent, communityAgent, customAgent],
  } as unknown as InstallState;
  const badgeSkillsState = {
    ...skillsState,
    inventory: { ...skillsState.inventory, agents: [] },
  } as unknown as SkillsState;

  const view = render(
    <AgentView
      state={badgeState}
      skillsState={badgeSkillsState}
      consumptionState={emptyConsumptionState}
      agentId={skillsOnlyAgent.id}
    />,
  );

  expect(screen.queryByText("已核验")).not.toBeInTheDocument();
  expect(screen.queryByText("自定义")).not.toBeInTheDocument();

  view.rerender(
    <AgentView
      state={badgeState}
      skillsState={badgeSkillsState}
      consumptionState={emptyConsumptionState}
      agentId={communityAgent.id}
    />,
  );
  expect(screen.getByText("社区扩展")).toBeVisible();

  view.rerender(
    <AgentView
      state={badgeState}
      skillsState={badgeSkillsState}
      consumptionState={emptyConsumptionState}
      agentId={customAgent.id}
    />,
  );
  expect(screen.getByText("自定义")).toBeVisible();
});

it("keeps a Model-only Agent in the full resource workspace", async () => {
  const modelOnlyAgent: AgentCapabilityView = {
    identity: {
      id: "model-only",
      name: "Model Only",
      enabled: true,
      builtin: true,
      category: "coding-agent",
      evidence: "official",
      docs: "https://example.invalid",
      note: "只管理 Model。",
      verified_at: "2026-07-23",
    },
    installed: true,
    capabilities: {
      model: {
        mode: "managed",
        storage_authority: "arca-mapping",
        installed: true,
        config_paths: ["~/.model-only/config.json"],
        assigned_profiles: [],
        active_profile: null,
        supports_multiple: false,
        supports_global_selection: true,
        credential_mode: "guided",
        supported_protocols: ["openai-responses"],
      },
    },
  };
  const modelOnlyState = {
    ...state,
    agents: [],
    refreshAgents: vi.fn().mockResolvedValue([]),
  } as unknown as InstallState;
  apiMocks.listModelAgents.mockResolvedValueOnce([{
    id: modelOnlyAgent.identity.id,
    name: modelOnlyAgent.identity.name,
    mode: "managed",
    storage_authority: "arca-mapping",
    installed: true,
    config_path: "~/.model-only/config.json",
    config_paths: ["~/.model-only/config.json"],
    docs: "https://example.invalid",
    assigned_profile: null,
    assigned_profiles: [],
    active_profile: null,
    supports_multiple: false,
    credential_mode: "guided",
    supported_protocols: ["openai-responses"],
    note: "",
  } satisfies ModelAgentView]);

  render(
    <AgentView
      state={modelOnlyState}
      skillsState={skillsState}
      consumptionState={{
        agents: [modelOnlyAgent],
        inventory: { consumptions: [], external: [] },
      } as unknown as ConsumptionState}
      agentId={modelOnlyAgent.identity.id}
    />,
  );

  await waitFor(() => {
    expect(screen.getByRole("button", { name: "编辑配置" })).toBeVisible();
  });
  expect(await screen.findByText("/Users/test/.model-only/config.json")).toBeVisible();
  expect(screen.queryByText(/未提供可写的用户级全局配置/)).not.toBeInTheDocument();
});

it("keeps an external MCP card read-only", async () => {
  const mcpAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "codex",
    name: "Codex",
    format: "toml",
    key: "mcp_servers",
    has_global: true,
    global: "~/.codex/config.toml",
    supported_transports: ["stdio", "http"],
    skills_global_dir: "~/.agents/skills",
    skills_global_dirs: ["~/.agents/skills", "~/.claude/skills"],
  };
  const mcpState = {
    ...state,
    entries: [{
      name: "computer-use",
      description: "",
      tags: [],
      config: { stdio: { command: "computer-use" } },
    }],
    agents: [mcpAgent],
    refreshAgents: vi.fn().mockResolvedValue([mcpAgent]),
  } as unknown as InstallState;
  const clearAgentMcp = vi.fn().mockResolvedValue(undefined);
  const consumptionState = {
    agents: [],
    inventory: {
      consumptions: [],
      external: [{
        agent_id: "codex",
        asset: { domain: "mcp", key: "computer-use::stdio" },
        ownership: "external",
        desired: false,
        observed: true,
        enabled: true,
        status: "external-added",
        reason: "mcp_adoptable",
        affected_agent_ids: ["codex"],
        available_actions: ["adopt-observed"],
      }],
    },
    clearAgentMcp,
  } as unknown as ConsumptionState;
  render(
    <AgentView
      state={mcpState}
      skillsState={skillsState}
      consumptionState={consumptionState}
      agentId="codex"
    />,
  );

  const locations = screen.getByRole("region", { name: "配置位置" });
  expect(await within(locations).findByText("/Users/test/.codex/config.toml")).toBeVisible();
  expect(within(locations).getByText("/Users/test/.agents/skills")).toBeVisible();
  expect(within(locations).getByText("/Users/test/.claude/skills")).toBeVisible();
  await userEvent.click(within(locations).getByRole("button", {
    name: "使用默认应用打开文件：/Users/test/.codex/config.toml",
  }));
  expect(openerMocks.openPath).toHaveBeenCalledWith("/Users/test/.codex/config.toml");
  expect(screen.queryByText(`${mcpAgent.id} · ${mcpAgent.category}`)).not.toBeInTheDocument();
  expect(within(locations).getByRole("button", { name: "编辑配置" })).toBeVisible();
  await userEvent.click(within(locations).getByRole("button", { name: "编辑配置" }));
  expect(screen.getByRole("heading", { name: "编辑配置" })).toBeVisible();

  const card = screen.getByText("computer-use").closest<HTMLElement>("li");
  expect(card).not.toBeNull();
  expect(within(card!).queryByText("computer-use::stdio")).not.toBeInTheDocument();
  expect(card!.querySelector(".arca-consumption-copy small")).toBeNull();
  expect(card).toHaveAttribute("data-enabled", "false");
  expect(within(card!).queryByRole("switch")).not.toBeInTheDocument();
  expect(within(card!).queryByRole("button", { name: /查看|移除/ })).not.toBeInTheDocument();
  expect(within(card!).queryByText("外部新增")).not.toBeInTheDocument();
  expect(within(card!).getByRole("button", { name: /^收录 Arca：/ })).toBeVisible();
  const clearAll = screen.getByRole("button", { name: "移除全部 MCP" });
  expect(clearAll).toHaveAttribute("title", expect.stringContaining("包括外部配置"));
  await userEvent.click(clearAll);
  await waitFor(() => expect(clearAgentMcp).toHaveBeenCalledWith("codex"));
});

it("adds an Agent asset as a delta with inline progress and no routine review dialog", async () => {
  const mcpAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "codex",
    name: "Codex",
    format: "toml",
    key: "mcp_servers",
    has_global: true,
    global: "~/.codex/config.toml",
    supported_transports: ["stdio", "http"],
    skills_global_dir: "~/.agents/skills",
  };
  const mcpState = {
    ...state,
    entries: [{
      name: "github",
      description: "GitHub tools",
      tags: [],
      config: { stdio: { command: "github-mcp" } },
    }],
    agents: [mcpAgent],
  } as unknown as InstallState;
  const operation = assetOperationPlanFixture();
  operation.warnings = [];
  operation.can_commit = true;
  operation.affected_agent_ids = ["claude-code", "codex"];
  operation.relationship_changes = [
    {
      agent_id: "codex",
      asset: { domain: "mcp", key: "github::stdio" },
      action: "add",
    },
    {
      agent_id: "claude-code",
      asset: { domain: "mcp", key: "github::stdio" },
      action: "add",
    },
  ];
  const planAdditionsForAgent = vi.fn().mockResolvedValue(operation);
  let finishCommit!: () => void;
  const commit = vi.fn().mockImplementation(() => new Promise((resolve) => {
    finishCommit = () => resolve({ consumptions: [], external: [] });
  }));
  const consumptionState = {
    agents: [],
    inventory: { consumptions: [], external: [] },
    plan: null,
    committing: false,
    planAdditionsForAgent,
    commit,
  } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <AgentView
        state={mcpState}
        skillsState={skillsState}
        consumptionState={consumptionState}
        agentId="codex"
      />
    </ToastProvider>,
  );

  await userEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  const picker = screen.getByRole("dialog", { name: "添加 MCP" });
  await userEvent.click(within(picker).getByRole("button", { name: /github/ }));
  await userEvent.click(within(picker).getByRole("button", { name: "添加 MCP" }));

  await waitFor(() => {
    expect(planAdditionsForAgent).toHaveBeenCalledWith("codex", {
      domain: "mcp",
      asset_keys: ["github::stdio"],
    });
    expect(commit).toHaveBeenCalledOnce();
    expect(commit).toHaveBeenCalledWith({ background: true });
  });
  expect(screen.queryByRole("dialog", { name: /确认添加/ })).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("正在检查并同步 Codex 的资产");

  finishCommit();
  expect(await screen.findByText("MCP 已添加到 Codex。")).toBeVisible();
});

it("renders every external Model as its own read-only card", async () => {
  const modelAgentInfo: AgentInfo = {
    ...skillsOnlyAgent,
    id: "opencode",
    name: "OpenCode",
    format: "json",
    key: "mcp",
    has_global: true,
    global: "~/.config/opencode/opencode.json",
    supported_transports: ["stdio", "http"],
    skills_global_dir: null,
  };
  const modelAgent: ModelAgentView = {
    id: "opencode",
    name: "OpenCode",
    mode: "managed",
    storage_authority: "native-registry",
    installed: true,
    config_path: "~/.config/opencode/opencode.json",
    config_paths: ["~/.config/opencode/opencode.json"],
    docs: "https://opencode.ai/docs/models/",
    assigned_profile: null,
    assigned_profiles: [],
    active_profile: null,
    supports_multiple: true,
    credential_mode: "environment-reference",
    supported_protocols: ["openai-completions"],
    note: "",
  };
  const candidate: ModelAdoptionCandidate = {
    candidate_id: "opencode:openrouter",
    agent_id: "opencode",
    native_id: "openrouter",
    name: "HY3",
    provider: "openrouter",
    model_vendor: "tencent",
    protocol: "openai-completions",
    base_url: "https://openrouter.ai/api/v1",
    model: "tencent/hy3:free",
    env_key: "OPENROUTER_API_KEY",
    active: true,
    credential_kind: "environment-reference",
    status: "adoptable",
    reason: null,
    fingerprint: "same-model",
    settings_hash: "settings",
    target_hash: "target",
    candidate_hash: "candidate",
  };
  apiMocks.listModelAgents.mockResolvedValueOnce([modelAgent]);
  render(
    <AgentView
      state={{ ...state, agents: [modelAgentInfo] } as unknown as InstallState}
      skillsState={skillsState}
      consumptionState={{
        agents: [],
        inventory: {
          consumptions: [],
          external: [{
            agent_id: "opencode",
            asset: { domain: "model", profile_id: `external-${candidate.candidate_id}` },
            ownership: "external",
            desired: false,
            observed: true,
            observed_enabled: true,
            active: true,
            desired_active: false,
            status: "external-added",
            reason: "model_external_current",
            observation_id: `model:opencode:${candidate.candidate_id}:${candidate.fingerprint}`,
            available_actions: ["adopt-observed"],
            affected_agent_ids: ["opencode"],
          }],
        },
      } as unknown as ConsumptionState}
      agentId="opencode"
      externalModelCandidates={[candidate]}
    />,
  );

  await userEvent.click(screen.getByRole("tab", { name: /Models/ }));
  const card = await screen.findByText("HY3").then((node) => node.closest<HTMLElement>("li"));
  expect(card).not.toBeNull();
  expect(card).toHaveAttribute("data-enabled", "false");
  expect(within(card!).getByText("Agent 实际当前")).toBeVisible();
  expect(within(card!).queryByRole("switch")).not.toBeInTheDocument();
  expect(within(card!).queryByRole("button", { name: /查看|移除/ })).not.toBeInTheDocument();
  expect(within(card!).queryByText("外部新增")).not.toBeInTheDocument();
  expect(within(card!).getByRole("button", { name: /^收录 Arca：/ })).toBeVisible();
});

it("renders external Skills as read-only cards", async () => {
  const externalSkill = {
    identity: "target:cortex-user:review",
    name: "review",
    description: "Review changes",
    content_kind: "instructions",
    states: ["external"],
    location: { kind: "agent_target", target_id: "cortex-user", global_dir: "~/.snowflake/cortex/skills" },
    source: null,
    resolved_revision: null,
    content_hash: "hash",
    risk: { level: "low", findings: [], finding_count: 0, findings_truncated: false },
    update: { available: false, checked_at: null, resolved_revision: null, etag: null, error: null, retry_at: null },
    assigned_target_ids: [],
    affected_agent_ids: [skillsOnlyAgent.id],
    installed_at: null,
    updated_at: null,
  };
  const externalSkillsState = {
    ...skillsState,
    inventory: { ...skillsState.inventory!, items: [externalSkill] },
  } as unknown as SkillsState;
  const consumptionState = {
    agents: [],
    inventory: {
      consumptions: [],
      external: [{
        agent_id: skillsOnlyAgent.id,
        asset: { domain: "skill", name: "review" },
        ownership: "external",
        desired: false,
        observed: true,
        status: "external-added",
        reason: "skill_external",
        affected_agent_ids: [skillsOnlyAgent.id],
        available_actions: ["adopt-observed"],
      }],
    },
  } as unknown as ConsumptionState;
  render(
    <AgentView
      state={state}
      skillsState={externalSkillsState}
      consumptionState={consumptionState}
      agentId={skillsOnlyAgent.id}
    />,
  );

  const card = await screen.findByText("review").then((node) => node.closest<HTMLElement>("li"));
  expect(card).not.toBeNull();
  expect(card).toHaveAttribute("data-enabled", "false");
  expect(within(card!).getByText("Review changes")).toBeVisible();
  expect(within(card!).queryByRole("button", { name: /查看|移除/ })).not.toBeInTheDocument();
  expect(within(card!).queryByText("外部新增")).not.toBeInTheDocument();
  expect(within(card!).getByRole("button", { name: /^收录 Arca：/ })).toBeVisible();
});

it("shows each Model's Provider icon in the picker and assigned cards", async () => {
  const piAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "pi",
    name: "Pi Coding Agent",
    format: "json",
    key: "mcpServers",
    has_global: true,
    global: "~/.pi/agent/mcp.json",
    supported_transports: ["stdio", "http"],
    skills_global_dir: "~/.pi/agent/skills",
  };
  const piModelAgent: ModelAgentView = {
    id: "pi",
    name: piAgent.name,
    mode: "managed",
    storage_authority: "native-registry",
    installed: true,
    config_path: "~/.pi/agent/models.json + ~/.pi/agent/settings.json",
    config_paths: ["~/.pi/agent/models.json", "~/.pi/agent/settings.json"],
    docs: "https://github.com/earendil-works/pi",
    assigned_profile: "qwen",
    assigned_profiles: ["qwen"],
    active_profile: "qwen",
    supports_multiple: true,
    credential_mode: "keychain-command",
    supported_protocols: ["openai-responses"],
    note: "",
  };
  apiMocks.listModelAgents.mockResolvedValue([piModelAgent]);
  apiMocks.listModelProfiles.mockResolvedValue([
    {
      id: "qwen",
      name: "Qwen3 7 Plus",
      provider: "alibaba",
      protocol: "openai-responses",
      base_url: "https://dashscope.example.test/v1",
      model: "qwen3.7-plus",
      reasoning: true,
      catalog_key: "alibaba/qwen3.7-plus",
      credential_saved: true,
    },
    {
      id: "gpt",
      name: "GPT OSS",
      provider: "openai",
      protocol: "openai-responses",
      base_url: "https://api.openai.com/v1",
      model: "gpt-oss",
      reasoning: true,
      catalog_key: "openai/gpt-oss",
      credential_saved: true,
    },
  ]);
  const consumptionState = {
    agents: [],
    inventory: {
      consumptions: [{
        agent_id: "pi",
        asset: { domain: "model" as const, profile_id: "qwen" },
        desired: true,
        observed: true,
        enabled: true,
        active: true,
        desired_active: true,
        status: "synced" as const,
        reason: null,
        available_actions: [],
        affected_agent_ids: ["pi"],
      }],
      external: [],
    },
    plan: null,
  } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <AgentView
        state={{ ...state, agents: [piAgent] } as unknown as InstallState}
        skillsState={skillsState}
        consumptionState={consumptionState}
        agentId="pi"
      />
    </ToastProvider>,
  );

  await userEvent.click(await screen.findByRole("tab", { name: /Models/ }));
  const assigned = await screen.findByText("Qwen3 7 Plus").then((node) => node.closest<HTMLElement>("li"));
  expect(assigned?.querySelector('[data-provider-icon="alibaba"]')).toBeInTheDocument();

  await userEvent.click(screen.getByRole("button", { name: "添加 Model" }));
  const picker = screen.getByRole("dialog", { name: "添加 Model" });
  expect(within(picker).getByRole("button", { name: /GPT OSS/ })
    .querySelector('[data-provider-icon="openai"]')).toBeInTheDocument();
  expect(within(picker).queryByRole("button", { name: /Qwen3 7 Plus/ })).not.toBeInTheDocument();
});

it("uses one current-Model switch and activates a disabled backup atomically", async () => {
  const piAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "pi",
    name: "Pi Coding Agent",
    format: "json",
    key: "mcpServers",
    has_global: true,
    global: "~/.pi/agent/mcp.json",
    supported_transports: ["stdio", "http"],
    skills_global_dir: "~/.pi/agent/skills",
  };
  const piModelAgent: ModelAgentView = {
    id: "pi",
    name: piAgent.name,
    mode: "managed",
    storage_authority: "native-registry",
    installed: true,
    config_path: "~/.pi/agent/models.json + ~/.pi/agent/settings.json",
    config_paths: ["~/.pi/agent/models.json", "~/.pi/agent/settings.json"],
    docs: "https://github.com/earendil-works/pi",
    assigned_profile: "idealab",
    assigned_profiles: ["idealab", "qwen"],
    active_profile: "idealab",
    supports_multiple: true,
    credential_mode: "keychain-command",
    supported_protocols: ["openai-responses"],
    note: "",
  };
  const profiles: ModelProfileView[] = [
    {
      id: "idealab",
      name: "idealab",
      provider: "idealab",
      protocol: "openai-responses",
      base_url: "https://idealab.example.test/v1",
      model: "Peach-07-17-DogFooding",
      reasoning: true,
      catalog_key: "idealab/Peach-07-17-DogFooding",
      credential_saved: true,
    },
    {
      id: "qwen",
      name: "Qwen3 7 Plus",
      provider: "max-ai",
      protocol: "openai-responses",
      base_url: "https://max-ai.example.test/v1",
      model: "qwen3.7-plus",
      reasoning: true,
      catalog_key: "max-ai/qwen3.7-plus",
      credential_saved: true,
    },
  ];
  apiMocks.listModelAgents.mockResolvedValue([piModelAgent]);
  apiMocks.listModelProfiles.mockResolvedValue(profiles);
  const inventory = {
    consumptions: [
      {
        agent_id: "pi",
        asset: { domain: "model" as const, profile_id: "idealab" },
        desired: true,
        observed: true,
        enabled: true,
        active: true,
        desired_active: true,
        status: "synced" as const,
        reason: null,
        available_actions: [],
        affected_agent_ids: ["pi"],
      },
      {
        agent_id: "pi",
        asset: { domain: "model" as const, profile_id: "qwen" },
        desired: true,
        observed: true,
        enabled: false,
        active: false,
        desired_active: false,
        status: "synced" as const,
        reason: null,
        available_actions: [],
        affected_agent_ids: ["pi"],
      },
    ],
    external: [],
  };
  const setActiveModel = vi.fn().mockResolvedValue(inventory);
  const taskSkillsState = {
    ...skillsState,
    refresh: vi.fn().mockResolvedValue(skillsState.inventory),
  } as unknown as SkillsState;
  const consumptionState = {
    agents: [],
    inventory,
    plan: null,
    setActiveModel,
  } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <AgentView
        state={{ ...state, agents: [piAgent] } as unknown as InstallState}
        skillsState={taskSkillsState}
        consumptionState={consumptionState}
        agentId="pi"
      />
    </ToastProvider>,
  );

  await userEvent.click(await screen.findByRole("tab", { name: /Models/ }));
  expect(await screen.findByText("配置中 2 个 · 同一时间使用其中一个")).toBeVisible();
  expect(screen.queryByRole("button", { name: "设为当前" })).not.toBeInTheDocument();
  const current = screen.getByRole("switch", {
    name: "idealab 当前正在使用；请选择其他 Model 切换",
  });
  const backup = screen.getByRole("switch", { name: "使用 Qwen3 7 Plus" });
  expect(current).toBeChecked();
  expect(backup).not.toBeChecked();
  expect(
    screen
      .getAllByRole("switch")
      .filter((item) => item.getAttribute("aria-checked") === "true"),
  ).toHaveLength(1);

  await userEvent.click(current);
  expect(await screen.findByText("请先选择其他当前 Model。")).toBeVisible();
  expect(setActiveModel).not.toHaveBeenCalled();

  await userEvent.click(backup);
  await waitFor(() => {
    expect(setActiveModel).toHaveBeenCalledWith("pi", "qwen");
  });
  expect(await screen.findByText("Pi Coding Agent 已切换到 Qwen3 7 Plus。")).toBeVisible();
  expect(screen.queryByText("确认切换当前 Model")).not.toBeInTheDocument();
  expect(screen.queryByText("检查并应用")).not.toBeInTheDocument();
});

it("plans one reviewed empty Model selection when clearing an Agent", async () => {
  const piAgent: AgentInfo = {
    ...skillsOnlyAgent,
    id: "pi",
    name: "Pi Coding Agent",
    format: "json",
    key: "mcpServers",
    has_global: true,
    global: "~/.pi/agent/mcp.json",
    supported_transports: ["stdio", "http"],
    skills_global_dir: "~/.pi/agent/skills",
  };
  const piModelAgent: ModelAgentView = {
    id: "pi",
    name: piAgent.name,
    mode: "managed",
    storage_authority: "native-registry",
    installed: true,
    config_path: "~/.pi/agent/models.json + ~/.pi/agent/settings.json",
    config_paths: ["~/.pi/agent/models.json", "~/.pi/agent/settings.json"],
    docs: "https://github.com/earendil-works/pi",
    assigned_profile: "idealab",
    assigned_profiles: ["idealab", "qwen"],
    active_profile: "idealab",
    supports_multiple: true,
    credential_mode: "keychain-command",
    supported_protocols: ["openai-responses"],
    note: "",
  };
  const profiles: ModelProfileView[] = [
    {
      id: "idealab",
      name: "idealab",
      provider: "idealab",
      protocol: "openai-responses",
      base_url: "https://idealab.example.test/v1",
      model: "Peach-07-17-DogFooding",
      reasoning: true,
      catalog_key: "idealab/Peach-07-17-DogFooding",
      credential_saved: true,
    },
    {
      id: "qwen",
      name: "Qwen3 7 Plus",
      provider: "max-ai",
      protocol: "openai-responses",
      base_url: "https://max-ai.example.test/v1",
      model: "qwen3.7-plus",
      reasoning: true,
      catalog_key: "max-ai/qwen3.7-plus",
      credential_saved: true,
    },
  ];
  apiMocks.listModelAgents.mockResolvedValue([piModelAgent]);
  apiMocks.listModelProfiles.mockResolvedValue(profiles);
  const external = [{
    agent_id: "pi",
    asset: { domain: "model" as const, profile_id: "external-manual" },
    ownership: "external" as const,
    desired: false,
    observed: true,
    observed_enabled: true,
    active: false,
    desired_active: false,
    status: "external-added" as const,
    reason: "model_external",
    observation_id: "model:pi:manual",
    available_actions: [],
    affected_agent_ids: ["pi"],
  }];
  const consumptions = [
    {
      agent_id: "pi",
      asset: { domain: "model" as const, profile_id: "idealab" },
      desired: true,
      observed: true,
      enabled: true,
      active: true,
      desired_active: true,
      status: "synced" as const,
      reason: null,
      available_actions: [],
      affected_agent_ids: ["pi"],
    },
    {
      agent_id: "pi",
      asset: { domain: "model" as const, profile_id: "qwen" },
      desired: true,
      observed: true,
      enabled: false,
      active: false,
      desired_active: false,
      status: "synced" as const,
      reason: null,
      available_actions: [],
      affected_agent_ids: ["pi"],
    },
  ];
  const plan = assetOperationPlanFixture();
  plan.kind = "clear-models";
  plan.domain_plan = {
    domain: "model",
    before: {
      pi: {
        profiles: {
          idealab: { profile_id: "idealab", enabled: true },
          qwen: { profile_id: "qwen", enabled: false },
        },
        active_profile_id: "idealab",
      },
    },
    after: {
      pi: { profiles: {}, active_profile_id: null },
    },
  };
  plan.relationship_changes = consumptions.map((item) => ({
    agent_id: "pi",
    asset: item.asset,
    action: "remove" as const,
  }));
  plan.model_state_changes = [
    {
      agent_id: "pi",
      profile_id: "idealab",
      before: { added: true, enabled: true, active: true },
      after: { added: false, enabled: false, active: false },
      reason: "model_removed",
    },
    {
      agent_id: "pi",
      profile_id: "qwen",
      before: { added: true, enabled: false, active: false },
      after: { added: false, enabled: false, active: false },
      reason: "model_removed",
    },
  ];
  const planClearAgentModels = vi.fn().mockResolvedValue(plan);
  const commit = vi.fn();
  const cancel = vi.fn();
  const inventory = {
    revision: "models",
    observed_at: "2026-08-27T00:00:00Z",
    consumptions,
    external,
    target_incidents: [],
  };
  const consumptionState = {
    agents: [],
    inventory,
    plan: null,
    committing: false,
    planClearAgentModels,
    commit,
    cancel,
  } as unknown as ConsumptionState;
  const taskSkillsState = {
    ...skillsState,
    refresh: vi.fn().mockResolvedValue(skillsState.inventory),
  } as unknown as SkillsState;
  const view = render(
    <ToastProvider>
      <AgentView
        state={{ ...state, agents: [piAgent] } as unknown as InstallState}
        skillsState={taskSkillsState}
        consumptionState={consumptionState}
        agentId="pi"
      />
    </ToastProvider>,
  );

  await userEvent.click(await screen.findByRole("tab", { name: /Models/ }));
  expect(screen.getByText("配置中 3 个 · 同一时间使用其中一个")).toBeVisible();
  const externalCard = screen.getByText("external-manual").closest<HTMLElement>("li");
  expect(externalCard).not.toBeNull();
  expect(within(externalCard!).queryByRole("button", { name: /移除/ })).not.toBeInTheDocument();
  const clear = screen.getByRole("button", { name: "清空全部 Models" });
  expect(clear).toBeEnabled();
  expect(clear).toHaveAttribute("title", expect.stringContaining("包括外部和手工配置"));

  await userEvent.click(clear);
  await waitFor(() => {
    expect(planClearAgentModels).toHaveBeenCalledWith("pi");
  });
  expect(commit).not.toHaveBeenCalled();

  view.rerender(
    <ToastProvider>
      <AgentView
        state={{ ...state, agents: [piAgent] } as unknown as InstallState}
        skillsState={taskSkillsState}
        consumptionState={{ ...consumptionState, plan }}
        agentId="pi"
      />
    </ToastProvider>,
  );
  expect(screen.getByText("配置中 3 个 · 同一时间使用其中一个")).toBeVisible();
  const dialog = screen.getByRole("dialog", { name: "清空全部 Models" });
  expect(dialog.closest('[data-modal-overlay="true"]')).not.toBeNull();
  expect(within(dialog).getByText("Pi Coding Agent · 3 个 Models")).toBeVisible();
  expect(within(dialog).getByText("将移除此 Agent 配置中的全部 Model")).toBeVisible();
  expect(within(dialog).getByText("包括外部和手工配置，操作无法撤销")).toBeVisible();
  expect(within(dialog).getByText("中央 Models、Providers 与凭据保持不变")).toBeVisible();
  expect(within(dialog).queryByText(/models\.json|settings\.json|external|检查并应用/)).not.toBeInTheDocument();

  await userEvent.click(within(dialog).getByRole("button", { name: "清空 3 个 Models" }));
  await waitFor(() => expect(commit).toHaveBeenCalledTimes(1));

  view.rerender(
    <ToastProvider>
      <AgentView
        state={{ ...state, agents: [piAgent] } as unknown as InstallState}
        skillsState={taskSkillsState}
        consumptionState={{ ...consumptionState, plan }}
        agentId="pi"
      />
    </ToastProvider>,
  );
  const cancelDialog = screen.getByRole("dialog", { name: "清空全部 Models" });
  await userEvent.click(within(cancelDialog).getByRole("button", { name: "取消" }));
  expect(cancel).toHaveBeenCalledTimes(1);

  view.rerender(
    <ToastProvider>
      <AgentView
        state={{ ...state, agents: [piAgent] } as unknown as InstallState}
        skillsState={taskSkillsState}
        consumptionState={{
          ...consumptionState,
          inventory: { ...inventory, native_plugin_agents: [], host_mcps: [], native_plugins: [], consumptions: [] },
        }}
        agentId="pi"
      />
    </ToastProvider>,
  );
  expect(screen.getByRole("button", { name: "清空全部 Models" })).toBeEnabled();
});
