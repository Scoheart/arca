import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import userEvent from "@testing-library/user-event";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SkillsState } from "../hooks/useSkillsState";
import * as api from "../lib/api";
import type { OperationPlan, SkillInventoryItem, SkillSourceResolution, SkillsInventory } from "../lib/types";
import {
  resolutionFixture,
  sharedTargetPlanFixture,
  highRiskPlan,
  skillsInventoryFixture,
  skillsStateFixture,
} from "../test/skillsFixtures";
import { SkillInstallDialog } from "./SkillInstallDialog";
import { SkillsView } from "./SkillsView";
import { ToastProvider } from "./Toast";

const dialogCss = await readFile(resolve(process.cwd(), "src/index.css"), "utf8");

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    getSkillDetail: vi.fn(),
    resolveGithubSkillSource: vi.fn(),
    resolveLocalSkillSourceDialog: vi.fn(),
    resolveArchiveSkillSourceDialog: vi.fn(),
  };
});

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(reason: unknown): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

const twoCandidateResolution = (): SkillSourceResolution => {
  const resolution = resolutionFixture();
  return {
    ...resolution,
    candidates: [
      ...resolution.candidates,
      {
        ...resolution.candidates[0],
        name: "release-notes",
        description: "Prepare release notes",
        relative_path: "release-notes",
        content_hash: "content-release",
      },
    ],
  };
};

const planAs = (kind: OperationPlan["kind"], operationId = `${kind}-operation`) => ({
  ...sharedTargetPlanFixture(),
  operation_id: operationId,
  kind,
});

const safeInstallPlan = (): OperationPlan => ({
  ...sharedTargetPlanFixture(),
  targets: [],
  warnings: [],
});

function renderInstall(overrides: {
  plan?: SkillsState["plan"];
  commit?: SkillsState["commit"];
  cancel?: SkillsState["cancel"];
  onClose?: () => void;
  onCommitted?: (inventory: SkillsInventory) => void;
  onRecoveryRequired?: (message: string) => void;
} = {}) {
  const state = skillsStateFixture();
  const props = {
    plan: overrides.plan ?? state.plan,
    commit: overrides.commit ?? state.commit,
    cancel: overrides.cancel ?? state.cancel,
    onClose: overrides.onClose ?? vi.fn(),
    onCommitted: overrides.onCommitted ?? vi.fn(),
    onRecoveryRequired: overrides.onRecoveryRequired ?? vi.fn(),
  };
  const rendered = render(
    <ToastProvider><SkillInstallDialog {...props} /></ToastProvider>,
  );
  return { ...rendered, props };
}

async function resolveGithub(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText("仓库地址"), "  acme/skills  ");
  await user.click(screen.getByRole("button", { name: "导入" }));
  await screen.findByRole("checkbox", { name: "review-changes" });
}

beforeEach(() => {
  vi.mocked(api.resolveGithubSkillSource).mockResolvedValue(resolutionFixture());
  vi.mocked(api.resolveLocalSkillSourceDialog).mockResolvedValue(null);
  vi.mocked(api.resolveArchiveSkillSourceDialog).mockResolvedValue(null);
  vi.mocked(api.getSkillDetail).mockImplementation(async (identity) => ({
    item: skillsInventoryFixture().items.find((item) => item.identity === identity) ?? skillsInventoryFixture().items[0],
    files: [],
    skill_md: "---\nname: fixture\n---\n",
    skill_md_truncated: false,
  }));
});

afterEach(cleanup);

describe("SkillInstallDialog central asset intake", () => {
  it("keeps the source step compact without an empty single-action footer", async () => {
    const { props } = renderInstall();
    const dialog = screen.getByRole("dialog", { name: "添加 Skills" });
    expect(dialog.querySelector(".arca-dialog-shell-footer")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消" })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(props.onClose).toHaveBeenCalledOnce();
  });

  it("presents GitHub, folder, and archive as three peer source entries", () => {
    renderInstall();
    expect(screen.getByRole("heading", { name: "GitHub" })).toBeVisible();
    expect(screen.getByRole("button", { name: "选择本地文件夹" })).toBeVisible();
    expect(screen.getByRole("button", { name: "选择 Skill 压缩包" })).toBeVisible();
    expect(screen.queryByText("或")).not.toBeInTheDocument();
    expect(document.querySelector(".arca-skill-source-divider")).not.toBeInTheDocument();
    expect(document.querySelector(".arca-skill-local-sources")).not.toBeInTheDocument();
  });

  it("imports every resolved central candidate immediately without assigning Agents", async () => {
    const plan = vi.fn().mockResolvedValue(safeInstallPlan());
    const commit = vi.fn().mockResolvedValue(skillsInventoryFixture());
    vi.mocked(api.resolveGithubSkillSource).mockResolvedValueOnce(twoCandidateResolution());
    const { props } = renderInstall({ plan, commit });
    await resolveGithub(userEvent.setup());
    expect(api.resolveGithubSkillSource).toHaveBeenCalledWith("acme/skills");
    expect(plan).toHaveBeenCalledWith({
      operation: "install_skill",
      request: { resolution_id: "resolve-fixture", skill_names: ["review-changes", "release-notes"], replace_conflicts: true },
    });
    await waitFor(() => expect(props.onCommitted).toHaveBeenCalledOnce());
    expect(commit).toHaveBeenCalledWith(safeInstallPlan(), null);
    expect(JSON.stringify(plan.mock.calls[0][0])).not.toContain("agent");
    expect(screen.queryByRole("dialog", { name: "确认 Skill 更改" })).not.toBeInTheDocument();
  });

  it("uses the native local picker and keeps the source step on cancel", async () => {
    renderInstall();
    await userEvent.click(screen.getByRole("button", { name: "选择本地文件夹" }));
    expect(api.resolveLocalSkillSourceDialog).toHaveBeenCalledOnce();
    expect(screen.getByRole("heading", { name: "添加 Skills" })).toBeVisible();
  });

  it("uses the native archive picker and keeps the source step on cancel", async () => {
    renderInstall();
    await userEvent.click(screen.getByRole("button", { name: "选择 Skill 压缩包" }));
    expect(api.resolveArchiveSkillSourceDialog).toHaveBeenCalledOnce();
    expect(screen.getByRole("heading", { name: "添加 Skills" })).toBeVisible();
  });

  it("imports an archive directly without opening an audit dialog", async () => {
    const resolution: SkillSourceResolution = {
      ...resolutionFixture(),
      source: {
        kind: "archive",
        path: "~/Downloads/skills.zip",
        subpath: "review-changes",
      },
      resolved_revision: null,
    };
    const planned = safeInstallPlan();
    const plan = vi.fn().mockResolvedValue(planned);
    const commit = vi.fn().mockResolvedValue(skillsInventoryFixture());
    vi.mocked(api.resolveArchiveSkillSourceDialog).mockResolvedValueOnce(resolution);
    renderInstall({ plan, commit });

    await userEvent.click(screen.getByRole("button", { name: "选择 Skill 压缩包" }));
    await screen.findByRole("checkbox", { name: "review-changes" });

    await waitFor(() => expect(commit).toHaveBeenCalledWith(planned, null));
    expect(screen.queryByRole("dialog", { name: "确认 Skill 更改" })).not.toBeInTheDocument();
  });

  it("shows a failed import and allows retrying the staged source", async () => {
    const plan = vi.fn().mockRejectedValueOnce({ code: "conflict", message: "central Skill content already exists" })
      .mockResolvedValueOnce(safeInstallPlan());
    const commit = vi.fn().mockResolvedValue(skillsInventoryFixture());
    renderInstall({ plan, commit });
    await resolveGithub(userEvent.setup());
    expect(await screen.findByText("发现冲突")).toBeVisible();
    expect(commit).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "备份并下载" }));
    await waitFor(() => expect(commit).toHaveBeenCalledOnce());
    expect(plan).toHaveBeenCalledTimes(2);
    for (const [request] of plan.mock.calls) expect(request.request.replace_conflicts).toBe(true);
  });

  it("coalesces close cleanup and cancels the staged resolution once", async () => {
    const user = userEvent.setup();
    const cancellation = deferred<void>();
    const cancel = vi.fn(() => cancellation.promise);
    const onClose = vi.fn();
    renderInstall({ cancel, onClose, plan: vi.fn().mockRejectedValue({ code: "network_error", message: "retry later" }) });
    await resolveGithub(user);
    await screen.findByText("retry later");
    const close = screen.getByRole("button", { name: "关闭" });
    fireEvent.click(close);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(cancel).toHaveBeenCalledOnce();
    cancellation.resolve();
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  });

  it("does not cancel content after a successful commit", async () => {
    const user = userEvent.setup();
    const committing = deferred<SkillsInventory>();
    const commit = vi.fn(() => committing.promise);
    const cancel = vi.fn().mockResolvedValue(undefined);
    const onCommitted = vi.fn();
    renderInstall({ commit, cancel, onCommitted });
    await resolveGithub(user);
    await waitFor(() => expect(commit).toHaveBeenCalledOnce());
    expect(cancel).not.toHaveBeenCalled();
    const inventory = skillsInventoryFixture();
    committing.resolve(inventory);
    await waitFor(() => expect(onCommitted).toHaveBeenCalledWith(inventory));
    expect(cancel).not.toHaveBeenCalled();
  });

  it("leaves recovery-owned staging untouched after unmount", async () => {
    const user = userEvent.setup();
    const committing = deferred<SkillsInventory>();
    const cancel = vi.fn().mockResolvedValue(undefined);
    const { unmount } = renderInstall({ commit: vi.fn(() => committing.promise), cancel });
    await resolveGithub(user);
    await waitFor(() => expect(screen.getByRole("button", { name: /下载中/ })).toBeDisabled());
    unmount();
    committing.reject({ code: "recovery_required", message: "recovery required" });
    await act(async () => { await committing.promise.catch(() => undefined); });
    expect(cancel).not.toHaveBeenCalled();
  });

  it("binds direct import to the exact plan findings without a second confirmation", async () => {
    const planned = highRiskPlan("high-risk");
    const plan = vi.fn().mockResolvedValue(planned);
    const commit = vi.fn().mockResolvedValue(skillsInventoryFixture());
    const { props } = renderInstall({ plan, commit });
    await resolveGithub(userEvent.setup());
    await waitFor(() => expect(props.onCommitted).toHaveBeenCalledOnce());
    expect(commit).toHaveBeenCalledOnce();
    expect(commit).toHaveBeenCalledWith(planned, "high-risk");
    expect(plan).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog", { name: "确认 Skill 更改" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "我已了解高风险内容及其影响" })).not.toBeInTheDocument();
  });

  it("rejects a plan belonging to another resolution and cleans up both operations", async () => {
    const planned = { ...safeInstallPlan(), operation_id: "other-resolution" };
    const commit = vi.fn();
    const cancel = vi.fn().mockResolvedValue(undefined);
    const { props } = renderInstall({ plan: vi.fn().mockResolvedValue(planned), commit, cancel });
    await resolveGithub(userEvent.setup());
    await screen.findByText("安装计划未绑定当前来源，请重新读取来源。");
    expect(commit).not.toHaveBeenCalled();
    expect(cancel).toHaveBeenCalledWith("other-resolution");
    await userEvent.click(screen.getByRole("button", { name: "关闭" }));
    await waitFor(() => expect(props.onClose).toHaveBeenCalledOnce());
    expect(cancel).toHaveBeenCalledWith("resolve-fixture");
  });

  it("accepts a source resolution after StrictMode replays mount effects", async () => {
    const state = skillsStateFixture();
    const cancel = vi.fn().mockResolvedValue(undefined);
    render(<StrictMode><ToastProvider><SkillInstallDialog
      plan={state.plan} commit={state.commit} cancel={cancel}
      onClose={vi.fn()} onCommitted={vi.fn()} onRecoveryRequired={vi.fn()}
    /></ToastProvider></StrictMode>);
    await resolveGithub(userEvent.setup());
    expect(screen.getByRole("checkbox", { name: "review-changes" })).toBeChecked();
    expect(cancel).not.toHaveBeenCalled();
  });
});

describe("Skills central lifecycle orchestration", () => {
  const renderWorkspace = (inventory: SkillsInventory, overrides: Partial<SkillsState> = {}) => {
    const state = { ...skillsStateFixture(), inventory, ...overrides };
    return render(<ToastProvider><SkillsView state={state} /></ToastProvider>);
  };

  async function openInspector(item: SkillInventoryItem) {
    await userEvent.click(screen.getByRole("button", { name: new RegExp(item.name) }));
    return screen.findByLabelText(`${item.name} 详情`);
  }

  it("opens central intake only from the top-level toolbar", async () => {
    renderWorkspace(skillsInventoryFixture());
    await userEvent.click(screen.getByRole("button", { name: "添加 Skills" }));
    expect(screen.getByRole("dialog", { name: "添加 Skills" })).toBeVisible();
    expect(screen.queryByText("目标 Agent")).not.toBeInTheDocument();
  });

  it("imports an external copy without implicit Agent ids", async () => {
    const inventory = skillsInventoryFixture();
    const external: SkillInventoryItem = {
      ...inventory.items[1],
      identity: "target:agents-user:external-copy",
      name: "external-copy",
      states: ["external"],
      location: { kind: "agent_target", target_id: "agents-user", global_dir: "~/.agents/skills" },
      source: null,
      affected_agent_ids: ["codex", "cursor", "gemini"],
    };
    inventory.items = [external];
    const planned = planAs("import");
    const plan = vi.fn().mockResolvedValue(planned);
    const commit = vi.fn().mockResolvedValue(skillsInventoryFixture());
    renderWorkspace(inventory, { plan, commit });
    const inspector = await openInspector(external);
    await userEvent.click(within(inspector).getByRole("button", { name: "导入" }));
    expect(plan).toHaveBeenCalledWith({
      operation: "import_skill",
      request: {
        identity: external.identity,
        replace_conflicts: false,
      },
    });
    expect(plan.mock.calls[0][0].request).not.toHaveProperty("agent_ids");
    await waitFor(() => expect(commit).toHaveBeenCalledWith(planned, null));
    expect(screen.queryByRole("dialog", { name: "确认 Skill 更改" })).not.toBeInTheDocument();
  });

  it("has no direct assignment switches in the Skill inspector", async () => {
    const inventory = skillsInventoryFixture();
    renderWorkspace(inventory);
    const inspector = await openInspector(inventory.items[0]);
    expect(within(inspector).queryByRole("switch")).not.toBeInTheDocument();
  });

  it("routes a high-risk external import into review without auto-confirming findings", async () => {
    const inventory = skillsInventoryFixture();
    const external: SkillInventoryItem = { ...inventory.items[1], name: "risk-copy", identity: "target:agents-user:risk-copy",
      states: ["external"], source: null,
      location: { kind: "agent_target", target_id: "agents-user", global_dir: "~/.agents/skills" } };
    inventory.items = [external];
    const planned = { ...highRiskPlan("import-risk"), kind: "import" as const };
    const plan = vi.fn().mockResolvedValue(planned);
    const commit = vi.fn();
    renderWorkspace(inventory, { plan, commit });
    const inspector = await openInspector(external);
    await userEvent.click(within(inspector).getByRole("button", { name: "导入" }));
    expect(await screen.findByRole("dialog", { name: "确认 Skill 更改" })).toBeVisible();
    expect(commit).not.toHaveBeenCalled();
    expect(plan).toHaveBeenCalledWith({ operation: "import_skill", request: { identity: external.identity, replace_conflicts: false } });
  });
});

describe("Skill dialog layout contract", () => {
  it("keeps dialog bodies bounded and scrollable", () => {
    expect(dialogCss).toContain(".arca-skill-dialog-body");
    expect(dialogCss).toContain(".arca-skill-review-body");
    expect(dialogCss).toContain("overflow-y: auto");
    expect(dialogCss).toContain("@media (max-width: 920px)");
    expect(dialogCss).toContain("@media (max-height: 640px)");
  });
});
