import { cleanup, screen } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import { afterEach, describe, expect, it, vi } from "vitest";
import { skillDetailFixture, skillsInventoryFixture } from "../test/skillsFixtures";
import { SkillInspector } from "./SkillInspector";

afterEach(cleanup);

describe("SkillInspector reading view", () => {
  it("shows metadata and the expanded body without technical panels or actions", () => {
    const item = skillsInventoryFixture().items[0];
    const detail = { ...skillDetailFixture(), item,
      skill_md: '---\r\nname: review-changes\r\ndescription: metadata\r\n---\r\n# Read this\n<script>alert("no")</script>',
    };
    render(<SkillInspector item={item} detail={detail} loading={false} error={null} onClose={vi.fn()} onPlan={vi.fn()} />);
    expect(screen.getByText(item.description)).toBeVisible();
    expect(screen.getByText("acme/skills")).toBeVisible();
    const body = screen.getByRole("region", { name: "Skill 正文" });
    expect(body).toHaveTextContent("# Read this");
    expect(body).not.toHaveTextContent("description: metadata");
    expect(body.querySelector("script")).toBeNull();
    for (const label of ["风险详情", "技术信息", "查看正文", "移除", "修复", "导入"]) {
      expect(screen.queryByText(label)).not.toBeInTheDocument();
    }
    expect(body.querySelector("details")).toBeNull();
  });

  it("shows loading and failures without displaying stale content", () => {
    const item = skillsInventoryFixture().items[0];
    const props = { item, detail: skillDetailFixture(), onClose: vi.fn() };
    const { rerender } = render(<SkillInspector {...props} loading error={null} />);
    expect(screen.getByRole("status")).toHaveTextContent("正在读取正文");
    rerender(<SkillInspector {...props} loading={false} error={{ code: "detail_unavailable", message: "unavailable" }} />);
    expect(screen.getByRole("alert")).toHaveTextContent("unavailable");
    expect(screen.getByRole("region", { name: "Skill 正文" }).querySelector("pre")).toBeNull();
  });
});
