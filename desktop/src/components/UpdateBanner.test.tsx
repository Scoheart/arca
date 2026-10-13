import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { UpdaterState } from "../hooks/useUpdater";
import { UpdateBanner } from "./UpdateBanner";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

afterEach(cleanup);

function updater(percent: number | null): UpdaterState {
  return {
    phase: { kind: "downloading", percent },
    checkNow: vi.fn(),
    download: vi.fn(),
    restart: vi.fn(),
    dismiss: vi.fn(),
    later: vi.fn(),
  };
}

function availableUpdater(notes: string | null): UpdaterState {
  return {
    phase: { kind: "available", version: "1.9.0", notes },
    checkNow: vi.fn(),
    download: vi.fn(),
    restart: vi.fn(),
    dismiss: vi.fn(),
    later: vi.fn(),
  };
}

it("keeps release body and GitHub release URLs out of the available update card", () => {
  render(
    <UpdateBanner
      updater={availableUpdater(
        "修复更新体验\n[查看发布页](https://github.com/Scoheart/arca/releases/tag/v1.9.0)\nhttps://github.com/Scoheart/arca/releases/latest",
      )}
    />,
  );

  expect(screen.getByText("发现新版本 v1.9.0")).toBeVisible();
  expect(screen.getByRole("button", { name: "立即更新" })).toBeVisible();
  expect(screen.getByRole("button", { name: "稍后" })).toBeVisible();
  expect(screen.queryByText(/github\.com\/Scoheart\/arca\/releases/i)).not.toBeInTheDocument();
  expect(screen.queryByText("修复更新体验")).not.toBeInTheDocument();
});

it("renders no empty notes region when the release body is link-only", () => {
  const { container } = render(
    <UpdateBanner
      updater={availableUpdater("https://github.com/Scoheart/arca/releases/latest")}
    />,
  );

  expect(screen.getByText("发现新版本 v1.9.0")).toBeVisible();
  expect(container).not.toHaveTextContent("github.com");
  expect(container.querySelector(".whitespace-pre-wrap")).toBeNull();
});

it("keeps the determinate fill synchronized with the displayed percentage", () => {
  render(<UpdateBanner updater={updater(37)} />);

  expect(screen.getByText("正在下载更新 37%")).toBeVisible();
  const progress = screen.getByRole("progressbar", { name: "更新下载进度" });
  expect(progress).toHaveAttribute("aria-valuenow", "37");
  expect(progress.firstElementChild).toHaveStyle({ width: "37%" });
  expect((progress.firstElementChild as HTMLElement).style.transition).toBe("");
});

it("keeps unknown-length downloads indeterminate", () => {
  render(<UpdateBanner updater={updater(null)} />);

  expect(screen.getByText("正在下载更新…")).toBeVisible();
  const progress = screen.getByRole("progressbar", { name: "更新下载进度" });
  expect(progress).not.toHaveAttribute("aria-valuenow");
});
