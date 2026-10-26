import { cleanup, screen, waitFor } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import type { RegistryEntry } from "../lib/types";
import { McpIconPickerDialog } from "./McpIconPickerDialog";

afterEach(cleanup);

const entry: RegistryEntry = {
  name: "brave-search",
  description: "Web search",
  tags: ["search"],
  config: { stdio: { command: "npx", args: ["brave-search"] } },
};

it("selects a built-in icon and closes after the preference is saved", async () => {
  const user = userEvent.setup();
  const onSelectBuiltin = vi.fn().mockResolvedValue(undefined);
  const onClose = vi.fn();
  render(
    <McpIconPickerDialog
      assetKey="brave-search::stdio"
      entry={entry}
      preference={undefined}
      onSelectBuiltin={onSelectBuiltin}
      onUpload={vi.fn().mockResolvedValue(false)}
      onReset={vi.fn().mockResolvedValue(undefined)}
      onClose={onClose}
    />,
  );

  expect(screen.getByText("推荐：搜索")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "选择内置图标：数据库" }));
  await waitFor(() => expect(onSelectBuiltin).toHaveBeenCalledWith("database"));
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("uploads, restores automatic behavior, and keeps failures visible", async () => {
  const user = userEvent.setup();
  const onUpload = vi.fn().mockRejectedValue(new Error("bad image"));
  const onReset = vi.fn().mockResolvedValue(undefined);
  const onClose = vi.fn();
  const view = render(
    <McpIconPickerDialog
      assetKey="brave-search::stdio"
      entry={entry}
      preference={{ kind: "builtin", value: "search" }}
      onSelectBuiltin={vi.fn().mockResolvedValue(undefined)}
      onUpload={onUpload}
      onReset={onReset}
      onClose={onClose}
    />,
  );

  await user.click(screen.getByRole("button", { name: "上传图片" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("bad image");
  expect(onClose).not.toHaveBeenCalled();

  await user.click(screen.getByRole("button", { name: "恢复自动" }));
  await waitFor(() => expect(onReset).toHaveBeenCalledTimes(1));
  expect(onClose).toHaveBeenCalledTimes(1);
  view.unmount();
});
