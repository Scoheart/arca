import { cleanup, screen } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { ConsumptionPickerDialog } from "./ConsumptionPickerDialog";

afterEach(cleanup);

it("selects one asset and names the direct Agent action", async () => {
  const onSelect = vi.fn();
  render(
    <ConsumptionPickerDialog
      title="切换 Model"
      subtitle="选择 Codex 要切换到的 Model。"
      mode="single"
      actionLabel="切换 Model"
      options={[
        { id: "fast", name: "Fast", description: "gpt-fast" },
        { id: "deep", name: "Deep", description: "gpt-deep" },
      ]}
      onSelect={onSelect}
      onClose={vi.fn()}
    />,
  );

  const action = screen.getByRole("button", { name: "切换 Model" });
  expect(action).toBeDisabled();
  await userEvent.click(screen.getByRole("option", { name: /Deep/ }));
  expect(action).toBeEnabled();
  await userEvent.click(action);
  expect(onSelect).toHaveBeenCalledWith(["deep"]);
  expect(screen.queryByText("确认更改")).not.toBeInTheDocument();
});

it("adds multiple MCPs in one action", async () => {
  const onSelect = vi.fn();
  render(
    <ConsumptionPickerDialog
      title="添加 MCP"
      subtitle="Codex"
      mode="multiple"
      actionLabel="添加 MCP"
      options={[
        { id: "github::stdio", name: "GitHub" },
        { id: "context7::http", name: "Context7" },
      ]}
      onSelect={onSelect}
      onClose={vi.fn()}
    />,
  );

  await userEvent.click(screen.getByRole("button", { name: /GitHub/ }));
  await userEvent.click(screen.getByRole("button", { name: /Context7/ }));
  await userEvent.click(screen.getByRole("button", { name: "添加 MCP（2）" }));
  expect(onSelect).toHaveBeenCalledWith(["context7::http", "github::stdio"]);
});
