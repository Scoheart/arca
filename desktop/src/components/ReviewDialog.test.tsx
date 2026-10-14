import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReviewDialog } from "./ReviewDialog";

afterEach(cleanup);

describe("ReviewDialog", () => {
  it("shows exact effects and uses the explicit action verb", () => {
    render(
      <ReviewDialog title="删除 MCP" confirmLabel="删除 MCP" onConfirm={() => undefined} onClose={() => undefined}>
        <p>从目录移除，并从 2 个 Agent 卸载；写入前创建备份。</p>
      </ReviewDialog>,
    );
    expect(screen.getByText(/2 个 Agent/)).toBeVisible();
    const dialog = screen.getByRole("dialog", { name: "删除 MCP" });
    expect(dialog.style.width).toContain("408px");
    expect(dialog.querySelector(".arca-review-dialog-danger")).not.toBeNull();
    expect(dialog.querySelector(".arca-dialog-shell-leading svg")).not.toBeNull();
    expect(screen.getByRole("button", { name: "删除 MCP" })).toHaveClass("btn-danger-solid");
  });

  it("keeps context open and reports a failed mutation", async () => {
    render(
      <ReviewDialog
        title="应用模型"
        confirmLabel="应用模型"
        onConfirm={() => Promise.reject(new Error("write conflict"))}
        onClose={() => undefined}
      >
        目标 ~/.agent/config
      </ReviewDialog>,
    );
    fireEvent.click(screen.getByRole("button", { name: "应用模型" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("write conflict");
    expect(screen.getByRole("dialog", { name: "应用模型" })).toBeVisible();
  });

  it("blocks every dismiss path while pending", async () => {
    let resolve!: () => void;
    const pending = new Promise<void>((done) => { resolve = done; });
    const onClose = vi.fn();
    render(
      <ReviewDialog title="删除来源" confirmLabel="删除来源" onConfirm={() => pending} onClose={onClose}>
        删除缓存
      </ReviewDialog>,
    );
    fireEvent.click(screen.getByRole("button", { name: "删除来源" }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    resolve();
    await waitFor(() => expect(screen.getByRole("button", { name: "取消" })).toBeEnabled());
  });
});
