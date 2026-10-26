import { screen } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import type { StartupSyncState } from "../hooks/useStartupSync";
import { StartupSyncBar } from "./StartupSyncBar";

it("names the failed capability and makes successful startup work explicit", async () => {
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
  expect(alert).toHaveTextContent("部分配置暂不可用");
  expect(alert).toHaveTextContent("资产关系");
  expect(alert).toHaveTextContent("其余 2 项已可用");
  expect(alert).toHaveTextContent("2/3");

  await userEvent.click(screen.getByRole("button", { name: "仅重试失败项" }));
  expect(retryFailed).toHaveBeenCalledOnce();
});

it("labels the one-shot settled state as complete instead of still syncing", async () => {
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

  render(<StartupSyncBar state={state} />);

  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("最新配置已同步");
  expect(status).not.toHaveTextContent("正在同步最新配置");
  expect(status).toHaveTextContent("1/1");
});
