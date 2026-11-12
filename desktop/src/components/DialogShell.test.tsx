import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DialogShell } from "./DialogShell";

afterEach(cleanup);

function pointerClick(start: Element, end: Element, clickTarget: Element) {
  const pointer = { pointerId: 1, button: 0, isPrimary: true };
  fireEvent.pointerDown(start, pointer);
  fireEvent.pointerUp(end, pointer);
  fireEvent.click(clickTarget, { detail: 1 });
}

describe("DialogShell", () => {
  it.each([
    ["editor", "编辑"],
    ["picker", "选择"],
    ["review", "确认"],
  ] as const)("gives %s dialogs a semantic default glyph", (kind, label) => {
    render(
      <DialogShell kind={kind} title={`${label}资源`} onClose={() => undefined}>
        内容
      </DialogShell>,
    );

    const shell = screen.getByRole("dialog", { name: `${label}资源` }).firstElementChild!;
    expect(shell.querySelector(".arca-dialog-shell-glyph")).toBeInTheDocument();
    expect(shell).toHaveAttribute("data-dialog-kind", kind);
  });

  it.each([
    ["editor", "wide"],
    ["picker", "md"],
    ["review", "sm"],
  ] as const)("maps %s to the %s preset", async (kind, size) => {
    render(
      <DialogShell kind={kind} title="资源对话框" onClose={() => undefined}>
        内容
      </DialogShell>,
    );

    const shell = screen.getByRole("dialog", { name: "资源对话框" }).firstElementChild;
    expect(shell).toHaveAttribute("data-dialog-kind", kind);
    expect(shell).toHaveAttribute("data-dialog-size", size);
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "资源对话框" })).toHaveFocus();
    });
  });

  it("provides fixed regions and named footer slots", () => {
    render(
      <DialogShell
        kind="editor"
        title="编辑资源"
        subtitle="说明"
        status={<p>状态</p>}
        onClose={() => undefined}
        footerStart={<button type="button">删除</button>}
        footerEnd={<button type="button">保存</button>}
      >
        <label>名称<input /></label>
      </DialogShell>,
    );

    expect(screen.getByText("说明")).toBeVisible();
    expect(screen.getByText("状态")).toBeVisible();
    expect(screen.getByRole("button", { name: "删除" })).toBeVisible();
    expect(screen.getByRole("button", { name: "保存" })).toBeVisible();
  });

  it("blocks close controls, overlay, and Escape while busy", () => {
    const onClose = vi.fn();
    render(
      <DialogShell kind="review" title="确认变更" busy onClose={onClose}>
        内容
      </DialogShell>,
    );

    expect(screen.getByRole("button", { name: "关闭" })).toBeDisabled();
    fireEvent.keyDown(document, { key: "Escape" });
    const backdrop = screen.getByRole("dialog", { name: "确认变更" }).parentElement!;
    pointerClick(backdrop, backdrop, backdrop);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("dismisses only when the entire pointer press is on the backdrop", () => {
    const onClose = vi.fn();
    render(
      <DialogShell kind="editor" title="编辑资源" onClose={onClose}>
        <label>名称<input /></label>
      </DialogShell>,
    );
    const input = screen.getByRole("textbox", { name: "名称" });
    const backdrop = screen.getByRole("dialog", { name: "编辑资源" }).parentElement!;
    pointerClick(input, backdrop, backdrop);
    pointerClick(backdrop, input, backdrop);
    expect(onClose).not.toHaveBeenCalled();

    pointerClick(backdrop, backdrop, backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("restores the original opener after a confirmation replaces its editor", async () => {
    function Review({ onClose }: { onClose(): void }) {
      return <DialogShell kind="review" title="确认变更" onClose={onClose}>内容</DialogShell>;
    }
    function Flow() {
      const [stage, setStage] = useState<"closed" | "edit" | "review">("closed");
      const close = () => setStage("closed");
      return (
        <>
          <button type="button" onClick={() => setStage("edit")}>编辑</button>
          {stage === "edit" && (
            <DialogShell kind="editor" title="编辑资源" onClose={close}>
              <button type="button" onClick={() => setStage("review")}>继续</button>
            </DialogShell>
          )}
          {stage === "review" && <Review onClose={close} />}
        </>
      );
    }
    render(<Flow />);
    const opener = screen.getByRole("button", { name: "编辑" });
    opener.focus();
    fireEvent.click(opener);
    const next = screen.getByRole("button", { name: "继续" });
    next.focus();
    fireEvent.click(next);
    expect(screen.getByRole("dialog", { name: "确认变更" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(opener).toHaveFocus());
  });
});
