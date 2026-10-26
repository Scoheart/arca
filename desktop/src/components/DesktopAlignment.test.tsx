import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { DialogShell } from "./DialogShell";
import { DialogDisclosure } from "./DialogDisclosure";
import { ConsumptionPickerDialog } from "./ConsumptionPickerDialog";
import { ResourcePickerDialog } from "./ResourcePickerDialog";
import { RegistryEditPage } from "./RegistryEditPage";
import { EnvEditor } from "./EnvEditor";
import { SkillReviewDialog } from "./SkillReviewDialog";
import { highRiskPlan, skillsInventoryFixture } from "../test/skillsFixtures";
import { ToastProvider } from "./Toast";
import type { InstallState } from "../hooks/useInstallState";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import type { RegistryEntry } from "../lib/types";

afterEach(cleanup);

it("keeps initial focus and Tab cycling out of hidden, collapsed and disabled fields", async () => {
  render(<DialogShell kind="editor" title="编辑" onClose={() => undefined} footerEnd={<button>保存</button>}>
    <div hidden><input aria-label="隐藏页签" data-modal-initial-focus /></div>
    <fieldset disabled><input aria-label="禁用字段" /></fieldset>
    <DialogDisclosure title="高级设置"><input aria-label="高级字段" /></DialogDisclosure>
  </DialogShell>);
  await waitFor(() => expect(screen.getByRole("heading", { name: "编辑" })).toHaveFocus());
  screen.getByRole("button", { name: "保存" }).focus();
  fireEvent.keyDown(document, { key: "Tab" });
  expect(screen.getByRole("button", { name: "关闭" })).toHaveFocus();
  fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
  expect(screen.getByRole("button", { name: "保存" })).toHaveFocus();
});

it("preserves edited secondary fields when collapsed and expanded", async () => {
  const user = userEvent.setup();
  render(<DialogDisclosure title="高级设置"><label>参数<input defaultValue="" /></label></DialogDisclosure>);
  await user.click(screen.getByText("高级设置"));
  await user.type(screen.getByLabelText("参数"), "--resume");
  await user.click(screen.getByText("高级设置"));
  await user.click(screen.getByText("高级设置"));
  expect(screen.getByLabelText("参数")).toHaveValue("--resume");
});

it("makes a pending form inert without disabling the error/review shell", () => {
  render(<DialogShell kind="editor" busy title="保存中" onClose={() => undefined}>
    <input aria-label="名称" />
  </DialogShell>);
  expect(screen.getByLabelText("名称").closest(".arca-dialog-shell-body")).toHaveAttribute("inert");
  expect(screen.getByRole("button", { name: "关闭" })).toBeDisabled();
});

it("blocks repeat submissions and presents rejected picker actions without losing selection", async () => {
  let reject!: (reason: Error) => void;
  const onAdd = vi.fn(() => new Promise<void>((_, fail) => { reject = fail; }));
  render(<ResourcePickerDialog title="选择模型" options={[{ id: "one", name: "One" }]} onAdd={onAdd} onClose={() => undefined} />);
  fireEvent.click(screen.getByRole("option", { name: "One" }));
  const add = screen.getByRole("button", { name: "添加" });
  fireEvent.click(add); fireEvent.click(add);
  expect(onAdd).toHaveBeenCalledOnce();
  await act(async () => { reject(new Error("try again")); });
  expect(screen.getByRole("alert")).toHaveTextContent("try again");
  expect(screen.getByRole("button", { name: "添加" })).toBeEnabled();
});

it("does not submit a selected resource that becomes unavailable", () => {
  const props = { title: "添加资源", subtitle: "Fixture", mode: "multiple" as const, actionLabel: "添加", onSelect: vi.fn(), onClose: vi.fn() };
  const { rerender } = render(<ConsumptionPickerDialog {...props} options={[{ id: "one", name: "One" }]} />);
  fireEvent.click(screen.getByRole("button", { name: "One" }));
  rerender(<ConsumptionPickerDialog {...props} options={[{ id: "one", name: "One", disabled: true }]} />);
  expect(screen.getByRole("button", { name: "添加" })).toBeDisabled();
  expect(props.onSelect).not.toHaveBeenCalled();
});

it("shows a single risk-review step and requires acknowledgment before retrying the bound plan", async () => {
  const plan = highRiskPlan("bound-fixture");
  plan.kind = "install";
  const onCommit = vi.fn().mockRejectedValueOnce({ code: "confirmation_required", message: "review required", findings_hash: plan.findings_hash })
    .mockResolvedValueOnce(skillsInventoryFixture());
  render(<SkillReviewDialog plan={plan} onCommit={onCommit} onClose={() => undefined}
    onCommitted={() => undefined} onRecoveryRequired={() => undefined} />);
  fireEvent.click(screen.getByRole("button", { name: "确认安装" }));
  await screen.findByRole("dialog", { name: "确认高风险覆盖" });
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(screen.getByRole("button", { name: "仍然安装" })).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox", { name: "我已了解高风险内容及其影响" }));
  fireEvent.click(screen.getByRole("button", { name: "仍然安装" }));
  await waitFor(() => expect(onCommit).toHaveBeenLastCalledWith(plan, "bound-fixture"));
});

it("preserves the CLI work directory, empty arguments and whitespace on a Desktop MCP edit", async () => {
  const entry: RegistryEntry = { name: "fixture", description: "", tags: [],
    config: { stdio: { command: "fixture-agent", args: [" spaced ", "", "--flag"], cwd: "/fixture/project" } },
    origin: { kind: "manual", source: "manual" } };
  const planUpdate = vi.fn().mockResolvedValue({ can_commit: true, warnings: [], relationship_changes: [], affected_agent_ids: [], model_state_changes: [] });
  const state = { entries: [entry], customKeys: new Set(["fixture::stdio"]), refreshRegistry: vi.fn().mockResolvedValue([]) } as unknown as InstallState;
  const consumptionState = { plan: null, planUpdate, commit: vi.fn().mockResolvedValue({}) } as unknown as ConsumptionState;
  render(<ToastProvider><RegistryEditPage state={state} consumptionState={consumptionState}
    name="fixture" entry={entry} onBack={() => undefined} /></ToastProvider>);
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledOnce());
  expect(planUpdate.mock.calls[0][0].entry.config.stdio).toMatchObject(entry.config.stdio!);
  await waitFor(() => expect(screen.getByRole("button", { name: "保存" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("启动参数"), { target: { value: " first \n last " } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledTimes(2));
  expect(planUpdate.mock.calls[1][0].entry.config.stdio.args).toEqual([" first ", " last "]);
});

it("keeps special environment names as data instead of mutating object prototypes", () => {
  const onChange = vi.fn();
  const { container } = render(<EnvEditor value={{}} onChange={onChange} />);
  const inputs = container.querySelectorAll("input");
  fireEvent.change(inputs[1], { target: { value: "fixture-value" } });
  fireEvent.change(inputs[0], { target: { value: "__proto__" } });
  const value = onChange.mock.calls.at(-1)![0];
  expect(Object.hasOwn(value, "__proto__")).toBe(true);
  expect(value.__proto__).toBe("fixture-value");
});
