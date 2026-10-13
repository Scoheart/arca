import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ResourceState } from "./ResourceState";

afterEach(cleanup);

describe("ResourceState", () => {
  it("keeps loading geometry stable and accessible", () => {
    const { container } = render(<ResourceState kind="loading" title="正在读取资源" />);
    expect(screen.getByRole("status", { name: "正在读取资源" })).toBeVisible();
    expect(container.querySelectorAll(".arca-resource-skeleton > span")).toHaveLength(6);
  });

  it.each(["read-error", "recovery"] as const)("announces %s as an alert", (kind) => {
    render(<ResourceState kind={kind} title="资源不可用" detail="请稍后重试" />);
    expect(screen.getByRole("alert")).toHaveTextContent("资源不可用请稍后重试");
  });

  it("renders one explicit empty-state action", () => {
    render(
      <ResourceState
        kind="empty"
        title="暂无资源"
        action={<button type="button">新建资源</button>}
      />,
    );
    expect(screen.getByRole("button", { name: "新建资源" })).toBeVisible();
  });
});
