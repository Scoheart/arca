import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  highRiskPlan,
  skillsInventoryFixture,
  sharedTargetPlanFixture,
} from "../test/skillsFixtures";
import { AgentNavigation } from "./AgentNavigation";
import { ResourceInspector } from "./ResourceWorkspace";
import { SkillReviewDialog } from "./SkillReviewDialog";
import { Modal } from "./ui";

afterEach(cleanup);

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

function ModalStackHarness() {
  const [reviewOpen, setReviewOpen] = useState(false);
  const [riskOpen, setRiskOpen] = useState(false);

  return (
    <>
      <button type="button" onClick={() => setReviewOpen(true)}>
        打开更改确认
      </button>
      {reviewOpen && (
        <Modal
          ariaLabel="Skill 更改确认"
          layer="review"
          onClose={() => setReviewOpen(false)}
        >
          <h2 tabIndex={-1} data-modal-initial-focus>
            Skill 更改确认
          </h2>
          <button type="button" onClick={() => setRiskOpen(true)}>
            确认风险
          </button>
          <button type="button">返回清单</button>
          {riskOpen && (
            <Modal
              ariaLabel="确认风险证据"
              layer="risk"
              onClose={() => setRiskOpen(false)}
            >
              <h2 tabIndex={-1} data-modal-initial-focus>
                确认风险证据
              </h2>
              <button type="button" tabIndex={-1}>
                程序控制焦点
              </button>
              <button type="button">返回</button>
              <button type="button">接受风险</button>
            </Modal>
          )}
        </Modal>
      )}
    </>
  );
}

function BackgroundEscapeHarness() {
  const [modalOpen, setModalOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(true);

  return (
    <>
      <AgentNavigation
        agents={[appAgent(1)]}
        selectedAgentId={null}
        onSelectAgent={() => undefined}
      />
      <button type="button" onClick={() => setModalOpen(true)}>
        显示风险确认
      </button>
      {inspectorOpen && (
        <ResourceInspector
          title="背景 Skill"
          avatar={<span aria-hidden="true">S</span>}
          onClose={() => setInspectorOpen(false)}
        >
          背景详情
        </ResourceInspector>
      )}
      {modalOpen && (
        <Modal ariaLabel="风险确认" layer="risk" onClose={() => setModalOpen(false)}>
          <button type="button">保留并继续</button>
        </Modal>
      )}
    </>
  );
}

describe("SkillReviewDialog", () => {
  it("describes assignment loss only for caller-identified changed targets", () => {
    const plan = sharedTargetPlanFixture();
    plan.kind = "assignment";
    plan.targets.push({
      target_id: "claude-user",
      global_dir: "~/.claude/skills",
      expected: "managed",
      primary_agent_ids: ["claude-code"],
      affected_agent_ids: ["claude-code"],
    });

    render(
      <SkillReviewDialog
        plan={plan}
        assignmentContext={{
          enabled: false,
          agentIds: ["cursor"],
          targetIds: ["agents-user"],
        }}
        onCommit={vi.fn()}
        onClose={vi.fn()}
        onCommitted={vi.fn()}
        onRecoveryRequired={vi.fn()}
      />,
    );

    const impact = screen.getByRole("region", { name: "分配影响" });
    expect(within(impact).getByText("将停止为 Cursor 分配")).toBeVisible();
    expect(within(impact).getByText("~/.agents/skills")).toBeVisible();
    expect(
      within(impact).getByText("Codex、Cursor、Gemini CLI 将失去访问"),
    ).toBeVisible();
    expect(within(impact).queryByText("~/.claude/skills")).not.toBeInTheDocument();
    expect(within(impact).queryByText(/Claude Code.*将失去访问/)).not.toBeInTheDocument();
  });

  it("commits normally after the Strict Mode effect replay", async () => {
    const inventory = skillsInventoryFixture();
    const onCommitted = vi.fn();
    render(
      <StrictMode>
        <SkillReviewDialog
          plan={sharedTargetPlanFixture()}
          onCommit={vi.fn().mockResolvedValue(inventory)}
          onClose={vi.fn()}
          onCommitted={onCommitted}
          onRecoveryRequired={vi.fn()}
        />
      </StrictMode>,
    );

    await userEvent.click(screen.getByRole("button", { name: "确认安装" }));
    expect(onCommitted).toHaveBeenCalledWith(inventory);
  });

  it.each([
    ["install", "确认安装"],
    ["import", "确认导入"],
    ["update", "确认更新"],
    ["remove", "确认移除"],
    ["assignment", "确认更改分配"],
    ["repair", "确认修复"],
  ] as const)("routes %s through the same injected commit boundary", async (kind, label) => {
    const plan = sharedTargetPlanFixture();
    plan.kind = kind;
    const onCommit = vi.fn().mockResolvedValue(skillsInventoryFixture());

    render(
      <SkillReviewDialog
        plan={plan}
        onCommit={onCommit}
        onClose={vi.fn()}
        onCommitted={vi.fn()}
        onRecoveryRequired={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: label }));
    expect(onCommit).toHaveBeenCalledOnce();
    expect(onCommit).toHaveBeenCalledWith(plan, null);
  });

  it("keeps commit single-flight and blocks every close path while pending", async () => {
    const inventory = skillsInventoryFixture();
    let resolveCommit!: (value: typeof inventory) => void;
    const onCommit = vi.fn().mockReturnValue(
      new Promise<typeof inventory>((resolve) => {
        resolveCommit = resolve;
      }),
    );
    const onClose = vi.fn();

    render(
      <SkillReviewDialog
        plan={sharedTargetPlanFixture()}
        onCommit={onCommit}
        onClose={onClose}
        onCommitted={vi.fn()}
        onRecoveryRequired={vi.fn()}
      />,
    );

    const confirm = screen.getByRole("button", { name: "确认安装" });
    act(() => {
      confirm.click();
      confirm.click();
    });

    expect(onCommit).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "关闭" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();

    resolveCommit(inventory);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "关闭" })).toBeEnabled();
    });
  });

  it("routes the first confirmation through the injected commit owner with null", async () => {
    const plan = sharedTargetPlanFixture();
    const inventory = skillsInventoryFixture();
    const onCommit = vi.fn().mockResolvedValue(inventory);
    const onCommitted = vi.fn();

    render(
      <SkillReviewDialog
        plan={plan}
        onCommit={onCommit}
        onClose={vi.fn()}
        onCommitted={onCommitted}
        onRecoveryRequired={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "确认安装" }));

    expect(onCommit).toHaveBeenCalledOnce();
    expect(onCommit).toHaveBeenCalledWith(plan, null);
    expect(onCommitted).toHaveBeenCalledWith(inventory);
  });

  it("confirms known high risk once in the existing dialog using the original findings hash", async () => {
    const plan = highRiskPlan("findings-exact");
    const onCommit = vi.fn().mockResolvedValue(skillsInventoryFixture());
    render(<SkillReviewDialog plan={plan} onCommit={onCommit} onClose={vi.fn()}
      onCommitted={vi.fn()} onRecoveryRequired={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: "确认 Skill 更改" });
    const acknowledgment = within(dialog).getByRole("checkbox", { name: /我已了解/ });
    const confirm = within(dialog).getByRole("button", { name: "仍然安装" });
    expect(confirm).toBeDisabled();
    expect(onCommit).not.toHaveBeenCalled();
    await userEvent.click(acknowledgment);
    await userEvent.click(confirm);
    await waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
    expect(onCommit).toHaveBeenCalledWith(plan, "findings-exact");
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(screen.getByRole("dialog", { name: "确认 Skill 更改" })).toBe(dialog);
  });

  it("cancels the single risk review on one Escape without attempting a commit", () => {
    const onClose = vi.fn();
    const onCommit = vi.fn();
    render(<SkillReviewDialog plan={highRiskPlan("findings-escape")} onCommit={onCommit}
      onClose={onClose} onCommitted={vi.fn()} onRecoveryRequired={vi.fn()} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("keeps a failed risk-confirmed commit in the same dialog", async () => {
    const onCommit = vi.fn().mockRejectedValue({ code: "conflict", message: "目标在确认期间发生变化。" });
    render(<SkillReviewDialog plan={highRiskPlan("findings-error")} onCommit={onCommit}
      onClose={vi.fn()} onCommitted={vi.fn()} onRecoveryRequired={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: "确认 Skill 更改" });
    await userEvent.click(within(dialog).getByRole("checkbox", { name: /我已了解/ }));
    await userEvent.click(within(dialog).getByRole("button", { name: "仍然安装" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("目标在确认期间发生变化。");
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(onCommit).toHaveBeenCalledOnce();
  });

  it.each([
    [{ code: "confirmation_required", findings_hash: undefined }, "风险确认信息不完整，请重新操作。"],
    [{ code: "confirmation_required", findings_hash: "different-hash" }, "风险内容已变化，请重新操作。"],
    [{ code: "plan_stale" }, "确认已过期，请重新操作。"],
  ])("invalidates the same review when its confirmation no longer matches", async (error, expectedMessage) => {
    const onCommit = vi.fn().mockRejectedValue(error);
    render(<SkillReviewDialog plan={highRiskPlan("findings-expected")} onCommit={onCommit}
      onClose={vi.fn()} onCommitted={vi.fn()} onRecoveryRequired={vi.fn()} />);
    await userEvent.click(screen.getByRole("checkbox", { name: /我已了解/ }));
    await userEvent.click(screen.getByRole("button", { name: "仍然安装" }));
    expect(await screen.findByText(expectedMessage)).toBeVisible();
    expect(screen.getByRole("button", { name: "仍然安装" })).toBeDisabled();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(onCommit).toHaveBeenCalledOnce();
  });

  it("expires the review after a stale-plan response", async () => {
    const plan = sharedTargetPlanFixture();
    const onCommit = vi.fn().mockRejectedValue({
      code: "plan_stale",
      message: "target changed",
    });

    render(
      <SkillReviewDialog
        plan={plan}
        onCommit={onCommit}
        onClose={vi.fn()}
        onCommitted={vi.fn()}
        onRecoveryRequired={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "确认安装" }));

    expect(
      await screen.findByText("确认已过期，请重新操作。"),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "确认安装" })).toBeDisabled();
  });

  it("hands recovery-required failures to the global recovery owner", async () => {
    const onRecoveryRequired = vi.fn();
    render(
      <SkillReviewDialog
        plan={sharedTargetPlanFixture()}
        onCommit={vi.fn().mockRejectedValue({
          code: "recovery_required",
          message: "检测到未完成事务。",
        })}
        onClose={vi.fn()}
        onCommitted={vi.fn()}
        onRecoveryRequired={onRecoveryRequired}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "确认安装" }));

    expect(onRecoveryRequired).toHaveBeenCalledOnce();
    expect(onRecoveryRequired).toHaveBeenCalledWith("检测到未完成事务。");
  });

  it("keeps other structured operation errors visible", async () => {
    render(
      <SkillReviewDialog
        plan={sharedTargetPlanFixture()}
        onCommit={vi.fn().mockRejectedValue({
          code: "conflict",
          message: "目标目录已被其他内容占用。",
        })}
        onClose={vi.fn()}
        onCommitted={vi.fn()}
        onRecoveryRequired={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "确认安装" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("目标目录已被其他内容占用。");
  });

  it("does not promise a retained backup when central repair restores a missing copy", () => {
    const plan = sharedTargetPlanFixture();
    plan.kind = "repair";
    plan.skills[0].replace_existing = true;
    plan.skills[0].existing_source = null;

    render(
      <SkillReviewDialog
        plan={plan}
        onCommit={vi.fn()}
        onClose={vi.fn()}
        onCommitted={vi.fn()}
        onRecoveryRequired={vi.fn()}
      />,
    );

    expect(screen.getByText("来源")).toBeVisible();
    expect(screen.queryByText("现有来源")).not.toBeInTheDocument();
    expect(
      screen.queryByText("先在 ~/.arca/backups/skills/ 保留备份，再替换现有中央副本"),
    ).not.toBeInTheDocument();
  });

  it("renders immutable files, bounded risk evidence, targets, and warnings", () => {
    const plan = highRiskPlan("findings-review");
    plan.skills[0].files[0] = {
      ...plan.skills[0].files[0],
      before_hash: "before-file-hash",
      after_hash: "after-file-hash",
      unified_diff: "@@ -1 +1 @@\n-old\n+new",
      diff_truncated: true,
    };
    plan.skills[0].risk.finding_count = 3;
    plan.skills[0].risk.findings_truncated = true;
    plan.skills[0].existing_states = ["conflicting_link"];
    plan.skills[0].replace_existing = true;
    plan.skills[0].existing_source = {
      kind: "local",
      path: "~/existing-skills",
      subpath: "review-changes",
    };

    render(
      <SkillReviewDialog
        plan={plan}
        onCommit={vi.fn()}
        onClose={vi.fn()}
        onCommitted={vi.fn()}
        onRecoveryRequired={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("dialog", { name: "确认 Skill 更改" }),
    ).toBeVisible();
    expect(screen.getByRole("heading", { name: "确认 Skill 更改" })).toBeVisible();
    expect(screen.getByText("review-changes")).toBeVisible();
    expect(screen.getByText("现有来源")).toBeVisible();
    expect(screen.getByText("本地 · ~/existing-skills / review-changes")).toBeVisible();
    expect(screen.getByText("候选来源")).toBeVisible();
    expect(screen.getByText(/GitHub · acme\/skills/)).toBeVisible();
    expect(
      screen.getByText("0123456789abcdef0123456789abcdef01234567"),
    ).toBeVisible();
    expect(screen.getByText("链接冲突")).toBeVisible();
    expect(
      screen.getByText("先在 ~/.arca/backups/skills/ 保留备份，再替换现有中央副本"),
    ).toBeVisible();
    expect(screen.getByText("SKILL.md")).toBeVisible();
    expect(screen.getByText("文本差异已截断")).toBeVisible();
    expect(screen.getByText("before-file-hash")).toBeVisible();
    expect(screen.getByText("after-file-hash")).toBeVisible();
    expect(screen.getByText("scripts/install.sh:2")).toBeVisible();
    expect(screen.getByText(/downloads content/)).toBeVisible();
    expect(screen.getByText("已显示 1 / 3 条证据")).toBeVisible();
    expect(screen.getByText("~/.agents/skills")).toBeVisible();
    expect(screen.getByText("Codex、Cursor、Gemini CLI")).toBeVisible();
    expect(
      screen.getByText("Gemini CLI also observes this shared directory"),
    ).toBeVisible();
  });
});

describe("Modal stack infrastructure", () => {
  it("portals labeled dialogs and closes only the topmost modal per Escape", async () => {
    const user = userEvent.setup();
    const { container } = render(<ModalStackHarness />);

    const opener = screen.getByRole("button", { name: "打开更改确认" });
    await user.click(opener);
    const review = screen.getByRole("dialog", { name: "Skill 更改确认" });
    expect(document.body).toContainElement(review);
    expect(container).not.toContainElement(review);

    const riskTrigger = screen.getByRole("button", { name: "确认风险" });
    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: "Skill 更改确认" }),
      ).toHaveFocus(),
    );
    await user.click(riskTrigger);
    expect(screen.getByRole("dialog", { name: "确认风险证据" })).toBeVisible();

    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("dialog", { name: "确认风险证据" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Skill 更改确认" })).toBeVisible();
    await waitFor(() => expect(riskTrigger).toHaveFocus());

    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("dialog", { name: "Skill 更改确认" }),
    ).not.toBeInTheDocument();
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("lets only the topmost scrim close and ignores clicks inside a dialog", async () => {
    const user = userEvent.setup();
    render(<ModalStackHarness />);

    await user.click(screen.getByRole("button", { name: "打开更改确认" }));
    await user.click(screen.getByRole("button", { name: "确认风险" }));
    const reviewScrim = document.querySelector<HTMLElement>(
      '[data-modal-layer="review"]',
    );
    const riskScrim = document.querySelector<HTMLElement>(
      '[data-modal-layer="risk"]',
    );
    expect(reviewScrim).not.toBeNull();
    expect(riskScrim).not.toBeNull();

    fireEvent.click(reviewScrim!);
    expect(screen.getByRole("dialog", { name: "Skill 更改确认" })).toBeVisible();
    expect(screen.getByRole("dialog", { name: "确认风险证据" })).toBeVisible();

    fireEvent.click(screen.getByRole("dialog", { name: "确认风险证据" }));
    expect(screen.getByRole("dialog", { name: "确认风险证据" })).toBeVisible();

    fireEvent.click(riskScrim!);
    expect(
      screen.queryByRole("dialog", { name: "确认风险证据" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Skill 更改确认" })).toBeVisible();
  });

  it("traps forward and reverse Tab inside the topmost modal", async () => {
    const user = userEvent.setup();
    render(<ModalStackHarness />);

    await user.click(screen.getByRole("button", { name: "打开更改确认" }));
    await user.click(screen.getByRole("button", { name: "确认风险" }));
    const first = screen.getByRole("button", { name: "返回" });
    const last = screen.getByRole("button", { name: "接受风险" });
    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: "确认风险证据" }),
      ).toHaveFocus(),
    );

    last.focus();
    await user.tab();
    expect(first).toHaveFocus();
    await user.tab({ shift: true });
    expect(last).toHaveFocus();
  });

  it("keeps the Agent picker and Inspector open when the same Escape closes a modal", async () => {
    const user = userEvent.setup();
    render(<BackgroundEscapeHarness />);

    await user.click(screen.getByRole("button", { name: "选择 Agent" }));
    expect(
      screen.getByRole("dialog", { name: "选择和置顶 Agent" }),
    ).not.toHaveAttribute("aria-modal");
    expect(screen.getByLabelText("背景 Skill 详情")).toBeVisible();

    // Click without a pointerdown so the already-open picker stays behind the modal.
    fireEvent.click(screen.getByRole("button", { name: "显示风险确认" }));
    expect(screen.getByRole("dialog", { name: "风险确认" })).toBeVisible();
    await user.keyboard("{Escape}");

    expect(
      screen.queryByRole("dialog", { name: "风险确认" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "选择和置顶 Agent" })).toBeVisible();
    expect(screen.getByLabelText("背景 Skill 详情")).toBeVisible();
  });

  it("keeps background layers open when the topmost scrim closes a modal", async () => {
    const user = userEvent.setup();
    render(<BackgroundEscapeHarness />);

    await user.click(screen.getByRole("button", { name: "选择 Agent" }));
    fireEvent.click(screen.getByRole("button", { name: "显示风险确认" }));
    const scrim = document.querySelector<HTMLElement>(
      '[data-modal-overlay="true"][data-modal-layer="risk"]',
    );
    expect(scrim).not.toBeNull();

    fireEvent.pointerDown(scrim!);
    fireEvent.click(scrim!);

    expect(
      screen.queryByRole("dialog", { name: "风险确认" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "选择和置顶 Agent" })).toBeVisible();
    expect(screen.getByLabelText("背景 Skill 详情")).toBeVisible();
  });

  it("keeps #root inert until the last modal closes and restores its prior state", async () => {
    const user = userEvent.setup();
    const appRoot = document.createElement("div");
    appRoot.id = "root";
    document.body.append(appRoot);
    render(<ModalStackHarness />);

    await user.click(screen.getByRole("button", { name: "打开更改确认" }));
    expect(appRoot).toHaveAttribute("inert");
    await user.click(screen.getByRole("button", { name: "确认风险" }));
    expect(appRoot).toHaveAttribute("inert");

    await user.keyboard("{Escape}");
    expect(appRoot).toHaveAttribute("inert");
    await user.keyboard("{Escape}");
    expect(appRoot).not.toHaveAttribute("inert");

    appRoot.setAttribute("inert", "");
    await user.click(screen.getByRole("button", { name: "打开更改确认" }));
    await user.keyboard("{Escape}");
    expect(appRoot).toHaveAttribute("inert");
    appRoot.remove();
  });
});
