import { screen } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import type { StartupSyncState } from "../hooks/useStartupSync";
import { StartupSyncBar } from "./StartupSyncBar";

it("reports the failed startup task with expandable details and retry", async () => {
  const retryFailed = vi.fn(async () => undefined);
  const state: StartupSyncState = {
    tasks: [
      {
        id: "relationships",
        label: "relationships",
        status: "error",
        error: "snapshot failed",
      },
      {
        id: "agents",
        label: "agents",
        status: "complete",
        error: null,
      },
      {
        id: "skills",
        label: "skills",
        status: "complete",
        error: null,
      },
    ],
    completed: 2,
    total: 3,
    activeLabel: null,
    failed: 1,
    syncing: false,
    slow: false,
    settled: true,
    retryFailed,
  };

  render(<StartupSyncBar state={state} />);

  const alert = screen.getByRole("alert");
  expect(alert).toHaveAttribute("data-kind", "warning");
  expect(alert).toHaveTextContent("启动时读取失败");
  expect(alert).toHaveTextContent("资产关系");
  expect(alert).not.toHaveTextContent("snapshot failed");
  await userEvent.click(screen.getByRole("button", { name: "错误详情" }));
  expect(alert).toHaveTextContent("snapshot failed");

  await userEvent.click(screen.getByRole("button", { name: "仅重试失败项" }));
  expect(retryFailed).toHaveBeenCalledOnce();
});

it("keeps normal startup progress and completion quiet", () => {
  const state: StartupSyncState = {
    tasks: [{
      id: "agents",
      label: "agents",
      status: "complete",
      error: null,
    }],
    completed: 1,
    total: 1,
    activeLabel: null,
    failed: 0,
    syncing: false,
    slow: false,
    settled: true,
    retryFailed: vi.fn(async () => undefined),
  };

  const { rerender } = render(<StartupSyncBar state={{ ...state, settled: false, syncing: true }} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();

  rerender(<StartupSyncBar state={state} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
