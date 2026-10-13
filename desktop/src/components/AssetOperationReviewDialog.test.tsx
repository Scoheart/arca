import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { assetOperationPlanFixture } from "../test/consumptionFixtures";
import { AssetOperationReviewDialog } from "./AssetOperationReviewDialog";

afterEach(cleanup);

it("commits an explicit convergence plan without a second conflict protocol", async () => {
  const plan = assetOperationPlanFixture();
  plan.kind = "update-asset";
  plan.warnings = ["codex: model_owned_fields_drift"];
  const onCommit = vi.fn();
  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      onCommit={onCommit}
      onCancel={vi.fn()}
    />,
  );
  const commit = screen.getByRole("button", { name: "应用更改" });
  expect(commit).toBeEnabled();
  await userEvent.click(commit);
  expect(onCommit).toHaveBeenCalledWith();
});

it("shows central lifecycle impact independently from relationship changes", () => {
  const plan = assetOperationPlanFixture();
  plan.kind = "delete-asset";
  plan.central_changes = [{
    asset: { domain: "model", profile_id: "work" },
    action: "delete",
    summary: ["删除中央模型配置", "将从 2 个已关联 Agent 移除"],
  }];
  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );
  expect(screen.getByText("资源变化")).toBeVisible();
  expect(screen.getByText("删除中央模型配置")).toBeVisible();
  expect(screen.getByText("将从 2 个已关联 Agent 移除")).toBeVisible();
  expect(screen.queryByText(/Profile metadata|consumer/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "删除" })).toBeEnabled();
});

it("explains a central-only Model update without presenting zero Agents as an error", () => {
  const plan = assetOperationPlanFixture();
  plan.kind = "update-asset";
  plan.central_changes = [{
    asset: { domain: "model", profile_id: "work" },
    action: "update",
    summary: ["仅更新中央模型配置", "当前没有已关联的 Agent，无需同步"],
  }];
  plan.affected_agent_ids = [];
  plan.target_files = [];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText("0 个 Agent · 0 个目标")).toBeVisible();
  expect(screen.getByText("仅更新中央模型配置")).toBeVisible();
  expect(screen.getByText("当前没有已关联的 Agent，无需同步")).toBeVisible();
  expect(screen.queryByText(/desired consumer|credential presence/)).not.toBeInTheDocument();
});

it("presents Agent assignment as a direct add action", () => {
  const plan = assetOperationPlanFixture();
  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="claude-code"
      agentName="Claude Code"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "确认添加 MCP" })).toBeVisible();
  expect(screen.getAllByText("Claude Code")).toHaveLength(2);
  expect(screen.getByRole("button", { name: "添加 MCP" })).toBeEnabled();
  expect(screen.getByText("Agent 变更")).toBeVisible();
  expect(screen.queryByText(/desired relationship/)).not.toBeInTheDocument();
});

it.each(["clear-mcp", "clear-models"] as const)(
  "never renders the generic confirmation view for %s",
  (kind) => {
    const plan = assetOperationPlanFixture();
    plan.kind = kind;
    plan.domain_plan = kind === "clear-mcp"
      ? { domain: "mcp", before: {}, after: {} }
      : { domain: "model", before: {}, after: {} };
    const onCommit = vi.fn();

    const { container } = render(
      <AssetOperationReviewDialog
        plan={plan}
        busy={false}
        agentId="pi"
        agentName="Pi Coding Agent"
        onCommit={onCommit}
        onCancel={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
    expect(onCommit).not.toHaveBeenCalled();
  },
);

it("shows an MCP enabled-state change separately from relationship changes", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "mcp",
    before: { "claude-code": ["github::stdio"] },
    after: { "claude-code": ["github::stdio"] },
  };
  plan.relationship_changes = [];
  plan.consumption_state_changes = [{
    agent_id: "claude-code",
    asset: { domain: "mcp", key: "github::stdio" },
    before_enabled: true,
    after_enabled: false,
    affected_agent_ids: ["claude-code"],
    target: null,
  }];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="claude-code"
      agentName="Claude Code"
      assetDisplayNames={{ "mcp:github::stdio": "GitHub" }}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "启用状态变化" })).toBeVisible();
  expect(screen.getByText("停用")).toHaveAttribute("data-action", "disable");
  expect(screen.getByText("MCP · GitHub")).toBeVisible();
  expect(screen.getByText("Claude Code：已启用 → 已停用")).toBeVisible();
  expect(screen.queryByRole("heading", { name: "Agent 变更" })).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "确认停用 MCP" })).toBeVisible();
  expect(screen.getByRole("button", { name: "停用 MCP" })).toBeEnabled();
});

it("shows the physical target and all readers for a shared Skill state change", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "skill",
    before: { codex: ["review-changes"], cursor: ["review-changes"] },
    after: { codex: ["review-changes"], cursor: ["review-changes"] },
  };
  plan.relationship_changes = [];
  plan.consumption_state_changes = [{
    agent_id: "codex",
    asset: { domain: "skill", name: "review-changes" },
    before_enabled: false,
    after_enabled: true,
    affected_agent_ids: ["codex", "cursor"],
    target: {
      target_id: "agents-shared",
      global_dir: "~/.agents/skills",
    },
  }];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="codex"
      agentName="Codex"
      agentDisplayNames={{ cursor: "Cursor" }}
      assetDisplayNames={{ "skill:review-changes": "Review Changes" }}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText("启用")).toHaveAttribute("data-action", "enable");
  expect(screen.getByText("Skill · Review Changes")).toBeVisible();
  expect(screen.getByText("Codex：已停用 → 已启用")).toBeVisible();
  expect(screen.getByText("同步影响 Codex、Cursor")).toBeVisible();
  expect(screen.getByText("共享目标 ~/.agents/skills")).toBeVisible();
  expect(screen.getByRole("heading", { name: "确认启用 Skill" })).toBeVisible();
  expect(screen.getByRole("button", { name: "启用 Skill" })).toBeEnabled();
});

it("distinguishes adding a Model from switching the current Model", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "model",
    before: { codex: { profiles: {}, active_profile_id: null } },
    after: {
      codex: {
        profiles: { work: { profile_id: "work", enabled: true } },
        active_profile_id: "work",
      },
    },
  };
  plan.relationship_changes = [{
    agent_id: "codex",
    asset: { domain: "model", profile_id: "work" },
    action: "add",
  }];
  plan.model_state_changes = [{
    agent_id: "codex",
    profile_id: "work",
    before: { added: false, enabled: false, active: false },
    after: { added: true, enabled: true, active: true },
    fallback_profile_id: null,
    reason: "model_added",
  }];
  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="codex"
      agentName="Codex"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );
  expect(screen.getByRole("heading", { name: "确认添加当前 Model" })).toBeVisible();
  expect(screen.queryByRole("heading", { name: "确认切换 Model" })).not.toBeInTheDocument();
  expect(screen.getByText("未添加 → 已启用 · 当前")).toBeVisible();
});

it("keeps a Model enabled-state delta in the richer Model state review without duplication", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "model",
    before: {
      codex: {
        profiles: { work: { profile_id: "work", enabled: false } },
        active_profile_id: null,
      },
    },
    after: {
      codex: {
        profiles: { work: { profile_id: "work", enabled: true } },
        active_profile_id: null,
      },
    },
  };
  plan.relationship_changes = [];
  plan.model_state_changes = [{
    agent_id: "codex",
    profile_id: "work",
    before: { added: true, enabled: false, active: false },
    after: { added: true, enabled: true, active: false },
    fallback_profile_id: null,
    reason: "model_enabled",
  }];
  plan.consumption_state_changes = [{
    agent_id: "codex",
    asset: { domain: "model", profile_id: "work" },
    before_enabled: false,
    after_enabled: true,
    affected_agent_ids: ["codex"],
    target: null,
  }];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="codex"
      agentName="Codex"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "Model 状态变化" })).toBeVisible();
  expect(screen.queryByRole("heading", { name: "启用状态变化" })).not.toBeInTheDocument();
  expect(screen.getByText("已添加 · 已停用 → 已启用 · 非当前")).toBeVisible();
  expect(screen.getByRole("heading", { name: "确认启用 Model" })).toBeVisible();
});

it("labels a non-current Model addition as a backup without raw conflict codes", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "model",
    before: {
      pi: {
        profiles: { current: { profile_id: "current", enabled: true } },
        active_profile_id: "current",
      },
    },
    after: {
      pi: {
        profiles: {
          current: { profile_id: "current", enabled: true },
          backup: { profile_id: "backup", enabled: true },
        },
        active_profile_id: "current",
      },
    },
  };
  plan.relationship_changes = [{
    agent_id: "pi",
    asset: { domain: "model", profile_id: "backup" },
    action: "add",
  }];
  plan.model_state_changes = [{
    agent_id: "pi",
    profile_id: "backup",
    before: { added: false, enabled: false, active: false },
    after: { added: true, enabled: true, active: false },
    fallback_profile_id: null,
    reason: "model_added",
  }];
  plan.target_files = ["~/.pi/agent/models.json"];
  plan.warnings = [];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="pi"
      agentName="Pi"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "确认添加备选 Model" })).toBeVisible();
  expect(screen.getByRole("button", { name: "添加备选 Model" })).toBeEnabled();
  expect(screen.getByText("未添加 → 已启用 · 非当前")).toBeVisible();
  expect(screen.getByText("~/.pi/agent/models.json")).toBeVisible();
  expect(screen.queryByText("~/.pi/agent/settings.json")).not.toBeInTheDocument();
  expect(screen.queryByText(/model_active_state_drift|model_external_current/)).not.toBeInTheDocument();
});

it("translates Model conflict warnings into user-facing copy", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = { domain: "model", before: {}, after: {} };
  plan.warnings = [
    "pi: model_active_state_drift",
    "pi: model_external_current",
  ];
  plan.can_commit = false;

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText("Pi：当前 Model 与 Arca 记录不一致，请先刷新或重新选择当前 Model")).toBeVisible();
  expect(screen.getByText("Pi：当前 Model 由 Agent 外部配置管理，切换前需要先让 Arca 接管")).toBeVisible();
  expect(screen.queryByText(/pi: model_/)).not.toBeInTheDocument();
});

it("explains Claude Desktop plaintext credential export in the shared Model review", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "model",
    before: { "claude-desktop": { profiles: {}, active_profile_id: null } },
    after: {
      "claude-desktop": {
        profiles: {
          "claude-desktop-planner-test": {
            profile_id: "claude-desktop-planner-test",
            enabled: true,
          },
        },
        active_profile_id: "claude-desktop-planner-test",
      },
    },
  };
  plan.relationship_changes = [{
    agent_id: "claude-desktop",
    asset: { domain: "model", profile_id: "claude-desktop-planner-test" },
    action: "add",
  }];
  plan.model_state_changes = [{
    agent_id: "claude-desktop",
    profile_id: "claude-desktop-planner-test",
    before: { added: false, enabled: false, active: false },
    after: { added: true, enabled: true, active: true },
    fallback_profile_id: null,
    reason: "model_added",
  }];
  plan.target_files = [
    "~/Library/Application Support/Claude/claude_desktop_config.json",
    "~/Library/Application Support/Claude-3p/claude_desktop_config.json",
    "~/Library/Application Support/Claude-3p/configLibrary/_meta.json",
    "~/Library/Application Support/Claude-3p/configLibrary/6d757800-0000-4000-8000-000000000001.json",
  ];
  plan.warnings = ["claude-desktop: model_credential_export_plaintext"];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="claude-desktop"
      agentName="Claude Desktop"
      assetDisplayNames={{ "model:claude-desktop-planner-test": "Claude Desktop Test Model" }}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "确认添加当前 Model" })).toBeVisible();
  expect(screen.getByRole("button", { name: "添加 Model" })).toBeEnabled();
  expect(screen.getByText("Agent 变更")).toBeVisible();
  expect(screen.getByText("~/Library/Application Support/Claude/claude_desktop_config.json")).toBeVisible();
  expect(screen.getByText(
    "Claude：将把所选 Provider 的 API Key 写入 Claude 的私有配置文件（权限 0600）",
  )).toBeVisible();
  expect(screen.queryByText("claude-desktop: model_credential_export_plaintext")).not.toBeInTheDocument();
});

it("presents a Model removal as a readable danger summary", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "model",
    before: {
      "claude-code": {
        profiles: { openrouter: { profile_id: "openrouter", enabled: true } },
        active_profile_id: "openrouter",
      },
    },
    after: {
      "claude-code": {
        profiles: {},
        active_profile_id: null,
      },
    },
  };
  plan.relationship_changes = [{
    agent_id: "claude-code",
    asset: { domain: "model", profile_id: "openrouter" },
    action: "remove",
  }];
  plan.model_state_changes = [];
  plan.target_files = [];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="claude-code"
      agentName="Claude Code"
      assetDisplayNames={{ "model:openrouter": "OpenRouter 自用" }}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "确认移除 Model" })).toBeVisible();
  expect(screen.getByText(/将从/)).toHaveTextContent(
    "将从Claude Code移除 Model · OpenRouter 自用。",
  );
  expect(screen.getByText("移除")).toHaveAttribute("data-action", "remove");
  expect(screen.getByText("Model · OpenRouter 自用")).toBeVisible();
  expect(screen.getByRole("button", { name: "移除 Model" })).toHaveClass("btn-danger");
  expect(screen.getByRole("dialog", { name: "确认移除 Model" })).toHaveAttribute("aria-modal", "true");
});

it("maps a stale Model removal conflict to a concise retry state", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "model",
    before: {
      "claude-code": {
        profiles: { openrouter: { profile_id: "openrouter", enabled: true } },
        active_profile_id: "openrouter",
      },
    },
    after: {
      "claude-code": {
        profiles: {},
        active_profile_id: null,
      },
    },
  };
  plan.relationship_changes = [{
    agent_id: "claude-code",
    asset: { domain: "model", profile_id: "openrouter" },
    action: "remove",
  }];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      error={{
        code: "model_consumption_missing",
        message: "the requested model is not assigned to this Agent",
      }}
      agentId="claude-code"
      agentName="Claude Code"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("alert")).toHaveTextContent(
    "该 Model 未添加到此 Agent，无法移除。",
  );
  expect(screen.queryByText(/requested model/i)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "重试移除 Model" })).toBeEnabled();
  expect(screen.getByRole("dialog", { name: "确认移除 Model" })).toHaveAttribute("aria-modal", "true");
});

it("separates direct Skill assignment from compatible visibility", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "skill",
    before: { "claude-code": [], opencode: [] },
    after: { "claude-code": ["frontend-design"], opencode: ["frontend-design"] },
  };
  plan.relationship_changes = [
    {
      agent_id: "claude-code",
      asset: { domain: "skill", name: "frontend-design" },
      action: "add",
    },
    {
      agent_id: "opencode",
      asset: { domain: "skill", name: "frontend-design" },
      action: "add",
    },
  ];
  plan.target_files = ["~/.claude/skills/frontend-design"];
  plan.affected_agent_ids = ["claude-code", "opencode"];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="claude-code"
      agentName="Claude Code"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText("Claude Code · 同一目录也被 1 个 Agent 读取")).toBeVisible();
  expect(screen.getByText("只写入一个目录；兼容 Agent 会读取同一份 Skill，不会重复安装。")).toBeVisible();
  expect(screen.getByRole("heading", { name: "生效范围" })).toBeVisible();
  expect(screen.getByText("直接添加")).toBeVisible();
  expect(screen.getByText("兼容可见")).toBeVisible();
  expect(screen.getByRole("heading", { name: "实际写入位置" })).toBeVisible();
  expect(screen.getByText("~/.claude/skills/frontend-design")).toBeVisible();
  expect(screen.queryByText("~/.config/opencode/skills/frontend-design")).not.toBeInTheDocument();
});

it("explains direct and shared-directory impact for a Skill removal", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "skill",
    before: { "claude-code": ["dws"], opencode: ["dws"] },
    after: { "claude-code": [], opencode: [] },
  };
  plan.relationship_changes = [
    {
      agent_id: "claude-code",
      asset: { domain: "skill", name: "dws" },
      action: "remove",
    },
    {
      agent_id: "opencode",
      asset: { domain: "skill", name: "dws" },
      action: "remove",
    },
  ];
  plan.target_files = ["~/.agents/skills/dws"];
  plan.affected_agent_ids = ["claude-code", "opencode"];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentId="claude-code"
      agentName="Claude Code"
      agentDisplayNames={{ "claude-code": "Claude Code", opencode: "OpenCode" }}
      assetDisplayNames={{ "skill:dws": "dws" }}
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  const summary = screen.getByRole("heading", { name: "影响摘要" }).closest("section");
  expect(summary).toHaveTextContent("将从Claude Code移除 Skill · dws。");
  expect(summary).toHaveTextContent("共用目录将同步影响OpenCode的 Skill 可见性。");
  expect(screen.getByText("直接移除")).toHaveAttribute("data-action", "remove");
  expect(screen.getByText("同步不可见")).toHaveAttribute("data-action", "remove");
  expect(screen.getByText("~/.agents/skills/dws")).toBeVisible();
  expect(screen.queryByText("opencode")).not.toBeInTheDocument();
});

it("maps an unsafe managed Skill link error and restores the known target path", () => {
  const plan = assetOperationPlanFixture();
  plan.domain_plan = {
    domain: "skill",
    before: { "claude-code": ["dws"] },
    after: { "claude-code": [] },
  };
  plan.relationship_changes = [{
    agent_id: "claude-code",
    asset: { domain: "skill", name: "dws" },
    action: "remove",
  }];
  plan.target_files = ["~/.agents/skills/dws"];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      error={'Conflict { message: "only an exact managed Skill link can be disabled", path: "" }'}
      agentId="claude-code"
      agentName="Claude Code"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("alert")).toHaveTextContent(
    "无法移除：这不是可安全移除的托管 Skill 链接（~/.agents/skills/dws）。",
  );
  expect(screen.queryByText(/only an exact managed/i)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "重试移除 Skill" })).toBeEnabled();
});

it("shows configuration paths and shared Skill migration compactly", () => {
  const plan = assetOperationPlanFixture();
  plan.kind = "update-configuration";
  plan.domain_plan = {
    domain: "agent-capabilities",
    agent_id: "codex",
    before: {
      mcp: { path: "~/.codex/config.toml", key: "mcp_servers" },
      model: { paths: ["~/.codex/config.toml"] },
      skill: { global_dir: "~/.agents/skills", alias_dirs: [] },
    },
    after: {
      mcp: { path: "~/.codex/config.toml", key: "mcp_servers" },
      model: { paths: ["~/.codex/config.toml"] },
      skill: { global_dir: "~/.private/skills", alias_dirs: [] },
    },
    skills_before: {},
    skills_after: {},
    affected_agent_ids: ["codex", "cursor"],
    migrated_skill_names: ["review-changes", "frontend-design"],
  };
  plan.affected_agent_ids = ["codex", "cursor"];
  plan.relationship_changes = [];
  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentName="Codex"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByRole("heading", { name: "确认修改配置" })).toBeVisible();
  expect(screen.getByText("~/.agents/skills")).toBeVisible();
  expect(screen.getByText("review-changes、frontend-design")).toBeVisible();
  expect(screen.getByText("Codex · Skills 目录变更涉及 1 个其他 Agent")).toBeVisible();
  expect(screen.queryByText(/另影响/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "保存配置" })).toBeEnabled();
});

it("describes shared Skills readers even when the changed directory has no Skills to migrate", () => {
  const plan = assetOperationPlanFixture();
  plan.kind = "update-configuration";
  plan.domain_plan = {
    domain: "agent-capabilities",
    agent_id: "codex",
    before: {
      mcp: { path: "~/.codex/config.toml", key: "mcp_servers" },
      model: { paths: ["~/.codex/config.toml"] },
      skill: { global_dir: "~/.agents/skills", alias_dirs: [] },
    },
    after: {
      mcp: { path: "~/.codex/config.toml", key: "mcp_servers" },
      model: { paths: ["~/.codex/config.toml"] },
      skill: { global_dir: "~/.private/skills", alias_dirs: [] },
    },
    skills_before: {},
    skills_after: {},
    affected_agent_ids: ["codex", "cursor"],
    migrated_skill_names: [],
  };
  plan.affected_agent_ids = ["codex", "cursor"];
  plan.relationship_changes = [];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentName="Codex"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText("Codex · Skills 目录变更涉及 1 个其他 Agent")).toBeVisible();
  expect(screen.queryByText(/另影响/)).not.toBeInTheDocument();
});

it("reviews an MCP key-only location change without implying migration", () => {
  const plan = assetOperationPlanFixture();
  plan.kind = "update-configuration";
  plan.domain_plan = {
    domain: "agent-capabilities",
    agent_id: "amp",
    before: {
      mcp: { path: "~/.config/amp/settings.json", key: "mcpServers" },
      model: { paths: [] },
      skill: null,
    },
    after: {
      mcp: { path: "~/.config/amp/settings.json", key: "amp.mcpServers" },
      model: { paths: [] },
      skill: null,
    },
    skills_before: {},
    skills_after: {},
    affected_agent_ids: ["amp"],
    migrated_skill_names: [],
  };
  plan.target_files = [];
  plan.affected_agent_ids = ["amp"];
  plan.relationship_changes = [];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentName="Amp"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText("MCP 配置键")).toBeVisible();
  expect(screen.queryByText("MCP 文件路径")).not.toBeInTheDocument();
  expect(screen.getByText("mcpServers")).toBeVisible();
  expect(screen.getByText("amp.mcpServers")).toBeVisible();
  expect(screen.getByText(
    "只更新 Arca 后续使用的 MCP 配置位置；旧文件不会删除，现有 MCP 配置不会复制到新位置。",
  )).toBeVisible();
});

it("explains that changing Model paths does not move existing configuration", () => {
  const plan = assetOperationPlanFixture();
  plan.kind = "update-configuration";
  plan.domain_plan = {
    domain: "agent-capabilities",
    agent_id: "codex",
    before: {
      mcp: { path: "~/.codex/config.toml", key: "mcp_servers" },
      model: { paths: ["~/.codex/config.toml"] },
      skill: { global_dir: "~/.agents/skills", alias_dirs: [] },
    },
    after: {
      mcp: { path: "~/.codex/config.toml", key: "mcp_servers" },
      model: { paths: ["~/.codex/models.toml"] },
      skill: { global_dir: "~/.agents/skills", alias_dirs: [] },
    },
    skills_before: {},
    skills_after: {},
    affected_agent_ids: ["codex"],
    migrated_skill_names: [],
  };
  plan.target_files = [];
  plan.affected_agent_ids = ["codex"];
  plan.relationship_changes = [];

  render(
    <AssetOperationReviewDialog
      plan={plan}
      busy={false}
      agentName="Codex"
      onCommit={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText(
    "只更新 Arca 后续使用的 Model 配置位置；旧文件不会删除，现有 Model 配置不会复制到新位置。",
  )).toBeVisible();
  expect(screen.queryByText(/后续使用的 MCP/)).not.toBeInTheDocument();
});

it("opens review above the whole application and leaves page content in place", async () => {
  const onCancel = vi.fn();
  const { container } = render(
    <main style={{ transform: "translateZ(0)", overflow: "hidden" }}>
      <p>Agent asset list</p>
      <AssetOperationReviewDialog
        plan={assetOperationPlanFixture()}
        busy={false}
        agentName="Claude Code"
        onCommit={vi.fn()}
        onCancel={onCancel}
      />
    </main>,
  );
  const dialog = screen.getByRole("dialog", { name: "确认添加 MCP" });
  expect(dialog).toHaveAttribute("aria-modal", "true");
  expect(dialog.parentElement?.parentElement).toBe(document.body);
  expect(container).not.toContainElement(dialog);
  expect(screen.getByText("Agent asset list")).toBeVisible();
  expect(screen.queryByText("检查并应用")).not.toBeInTheDocument();
  await userEvent.keyboard("{Escape}");
  expect(onCancel).toHaveBeenCalledOnce();
});
