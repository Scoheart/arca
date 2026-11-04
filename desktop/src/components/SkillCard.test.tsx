import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { skillsInventoryFixture } from "../test/skillsFixtures";
import { SkillCard } from "./SkillCard";

afterEach(cleanup);

describe("SkillCard", () => {
  it("keeps detail and Agent navigation as separate native keyboard actions", async () => {
    const item = skillsInventoryFixture().items[0];
    const onOpen = vi.fn();
    const onOpenAgent = vi.fn();
    const user = userEvent.setup();
    render(<SkillCard item={item} selected={false} onOpen={onOpen}
      agentIds={["claude-code", "cursor"]} onOpenAgent={onOpenAgent} />);
    const detail = screen.getByRole("button", { name: /打开 Skill review-changes 详情/ });
    expect(detail.querySelector("button")).toBeNull();
    expect(screen.getByText(item.description)).toBeVisible();
    expect(screen.queryByText("使用中")).not.toBeInTheDocument();
    expect(screen.queryByText(/rev 0123/)).not.toBeInTheDocument();
    detail.focus();
    await user.keyboard("{Enter}");
    expect(onOpen).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "打开 Cursor 的 Skills" }));
    expect(onOpenAgent).toHaveBeenCalledWith("cursor");
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("lets every overflow Agent be reached without opening Skill detail", async () => {
    const user = userEvent.setup();
    const onOpenAgent = vi.fn();
    const onOpen = vi.fn();
    render(<SkillCard item={skillsInventoryFixture().items[0]} selected={false} onOpen={onOpen}
      agentIds={["claude-code", "cursor", "codex", "gemini", "opencode", "copilot-cli"]}
      onOpenAgent={onOpenAgent} />);
    await user.click(screen.getByRole("button", { name: "查看另外 2 个 Agent" }));
    const dialog = screen.getByRole("dialog", { name: "review-changes · 全部 Agent" });
    await user.click(within(dialog).getByRole("button", { name: /OpenCode/ }));
    expect(onOpenAgent).toHaveBeenCalledWith("opencode");
    expect(onOpen).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows an empty assignment and keeps error details in the Inspector", () => {
    const item = skillsInventoryFixture().items[1];
    render(<SkillCard item={{ ...item, update: { ...item.update, error: "private diagnostic" } }} selected onOpen={() => {}} />);
    expect(screen.getByText("尚未分配")).toBeVisible();
    expect(screen.getByText("检查失败")).toBeVisible();
    expect(screen.queryByText("private diagnostic")).not.toBeInTheDocument();
  });
});
