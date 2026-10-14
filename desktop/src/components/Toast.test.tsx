import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { DialogShell } from "./DialogShell";
import { ToastNotice, ToastProvider, useToast } from "./Toast";

afterEach(() => { cleanup(); vi.useRealTimers(); });

it("keeps an error outside the inert app without stealing dialog focus", async () => {
  function Editor() {
    const toast = useToast();
    return <DialogShell kind="editor" title="Edit provider" onClose={vi.fn()}>
      <button onClick={() => toast.show({ kind: "error", msg: "Save failed" })}>Save</button>
    </DialogShell>;
  }
  const root = document.createElement("div");
  root.id = "root";
  document.body.append(root);
  try {
    render(<ToastProvider><Editor /></ToastProvider>, { container: root });
    await waitFor(() => expect(screen.getByRole("heading")).toHaveFocus());
    const save = screen.getByRole("button", { name: "Save" });
    save.focus();
    fireEvent.click(save);
    const alert = screen.getByRole("alert");
    expect(root).toHaveAttribute("inert");
    expect(root.contains(alert)).toBe(false);
    expect(alert.closest("[inert]")).toBeNull();
    expect(save).toHaveFocus();
    const dismiss = screen.getByRole("button", { name: "关闭错误通知" });
    dismiss.focus();
    fireEvent.keyDown(dismiss, { key: "Tab" });
    expect(screen.getByRole("button", { name: "关闭" })).toHaveFocus();
  } finally { cleanup(); root.remove(); }
});

it("updates one background notice and lets operation feedback appear first", () => {
  vi.useFakeTimers();
  function Save() {
    const toast = useToast();
    return <button onClick={() => toast.show({ kind: "success", msg: "Saved" })}>Save</button>;
  }
  const view = (percent: number) => <ToastProvider><Save />
    <ToastNotice kind="info" noticeKey="download" persistent message={`Downloading ${percent}%`} />
  </ToastProvider>;
  const { rerender } = render(view(1));
  const first = document.querySelector(".arca-toast");
  rerender(view(37));
  expect(document.querySelector(".arca-toast")).toBe(first);
  expect(screen.getByRole("status")).toHaveTextContent("Downloading 37%");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(screen.getByRole("status")).toHaveTextContent("Saved");
  fireEvent.click(screen.getByRole("button", { name: "关闭成功通知" }));
  act(() => vi.advanceTimersByTime(180));
  expect(screen.getByRole("status")).toHaveTextContent("Downloading 37%");
  expect(document.querySelectorAll(".arca-toast")).toHaveLength(1);
});

it("withdraws state-driven errors once their source is resolved", () => {
  const view = (message: string | null) => <ToastProvider><ToastNotice message={message} /></ToastProvider>;
  const { rerender } = render(view("Source failed"));
  expect(screen.getByRole("alert")).toHaveTextContent("Source failed");
  rerender(view(null));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
