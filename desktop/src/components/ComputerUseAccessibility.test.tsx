import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DialogShell } from "./DialogShell";
import { SearchBar } from "./ui";
import { ResourcePickerDialog } from "./ResourcePickerDialog";
import { ConsumptionPickerDialog } from "./ConsumptionPickerDialog";
import { AgentNavigation } from "./AgentNavigation";
import { ToastProvider, useToast } from "./Toast";
import type { AgentInfo } from "../lib/types";

vi.mock("../hooks/usePinnedAgents", () => ({ usePinnedAgents: () => ({ agentIds: [], ready: true, saving: false, commit: vi.fn() }) }));
vi.mock("./AgentView", () => ({ AgentView: () => null }));

const originalGetAnimations = Object.getOwnPropertyDescriptor(Element.prototype, "getAnimations");

beforeEach(() => {
  vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  Object.defineProperty(Element.prototype, "getAnimations", { configurable: true, value: () => [] });
});
afterEach(() => {
  cleanup(); vi.useRealTimers(); vi.unstubAllGlobals();
  if (originalGetAnimations) Object.defineProperty(Element.prototype, "getAnimations", originalGetAnimations);
  else Reflect.deleteProperty(Element.prototype, "getAnimations");
});

function Dialogs() {
  const [child, setChild] = useState(false);
  return <DialogShell kind="picker" title="第一层" onClose={() => undefined}>
    <SearchBar value="" onChange={() => undefined} placeholder="搜索第一层" autoFocus />
    <button onClick={() => setChild(true)}>打开第二层</button>
    {child && <DialogShell kind="editor" title="第二层" onClose={() => setChild(false)}>
      <SearchBar value="" onChange={() => undefined} placeholder="搜索第二层" autoFocus />
    </DialogShell>}
  </DialogShell>;
}

describe("Computer Use interaction contract", () => {
  it("distinguishes equal candidate names by asset ID and disables search correction", () => {
    render(<ConsumptionPickerDialog title="添加 MCPs" subtitle="测试 Agent" mode="multiple"
      actionLabel="添加" options={[{ id: "firecrawl::stdio", name: "firecrawl" }, { id: "firecrawl::http", name: "firecrawl" }]}
      onSelect={vi.fn()} onClose={() => undefined} />);
    expect(screen.getByRole("button", { name: "firecrawl（firecrawl::stdio）" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "firecrawl（firecrawl::http）" })).toBeEnabled();
    expect(screen.getByRole("searchbox")).toHaveAttribute("spellcheck", "false");
    expect(screen.getByRole("searchbox")).toHaveAttribute("autocorrect", "off");
  });

  it("keeps search focus, makes the lower dialog inert, and restores its opener", async () => {
    render(<Dialogs />);
    await waitFor(() => expect(screen.getByRole("searchbox", { name: "搜索第一层" })).toHaveFocus());
    const opener = screen.getByRole("button", { name: "打开第二层" });
    opener.focus(); fireEvent.click(opener);
    await waitFor(() => expect(screen.getByRole("searchbox", { name: "搜索第二层" })).toHaveFocus());
    expect(document.querySelector('[role="dialog"][aria-label="第一层"]')?.parentElement).toHaveAttribute("inert");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "第二层" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "第一层" }).parentElement).not.toHaveAttribute("inert");
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("navigates candidates without committing or selecting a disabled option", async () => {
    const onAdd = vi.fn();
    render(<ResourcePickerDialog title="选择资源" options={[
      { id: "disabled", name: "不可用", disabled: true }, { id: "a", name: "A" }, { id: "b", name: "B" },
    ]} onAdd={onAdd} onClose={() => undefined} />);
    const search = screen.getByRole("searchbox", { name: "搜索资源" });
    await waitFor(() => expect(search).toHaveFocus());
    fireEvent.keyDown(search, { key: "ArrowDown" });
    expect(screen.getByRole("option", { name: "A" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    expect(screen.getByRole("option", { name: "B" })).toHaveFocus();
    expect(onAdd).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "添加" })).toBeDisabled();
    fireEvent.click(screen.getByRole("option", { name: "B" }));
    expect(screen.getByRole("button", { name: "添加" })).toBeEnabled();
  });

  it("opens by shortcut and accepts an exact Agent ID, but rejects ambiguous names and IME Enter", async () => {
    const onSelect = vi.fn();
    const base: AgentInfo = { id: "codex", name: "Demo", format: "json", key: "mcpServers", has_global: true,
      has_project: false, enabled: true, supported_transports: ["stdio"], global: "~/.demo/config.json", project: null,
      skills_global_dir: null, docs: null, note: null, category: "cli", evidence: "official", verified_at: null, builtin: true };
    render(<AgentNavigation agents={[base, { ...base, id: "cursor", category: "desktop" }]} selectedAgentId={null} onSelectAgent={onSelect} />);
    fireEvent.keyDown(document, { key: "k", metaKey: true });
    const search = screen.getByRole("searchbox", { name: "搜索 Agent" });
    await waitFor(() => expect(search).toHaveFocus());
    fireEvent.change(search, { target: { value: "Demo" } });
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.change(search, { target: { value: "cursor" } });
    fireEvent.keyDown(search, { key: "Enter", isComposing: true });
    fireEvent.compositionStart(search);
    fireEvent.keyDown(search, { key: "Enter", isComposing: false });
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.compositionEnd(search, { data: "cursor" });
    fireEvent.keyDown(search, { key: "Enter", keyCode: 229, isComposing: false });
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith("cursor");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("does not open Agent navigation over an unsaved editor", () => {
    render(<><AgentNavigation agents={[]} selectedAgentId={null} onSelectAgent={vi.fn()} />
      <DialogShell kind="editor" title="未保存的编辑" onClose={() => undefined}>表单</DialogShell></>);
    fireEvent.keyDown(document, { key: "k", metaKey: true });
    expect(screen.queryByRole("dialog", { name: "选择和置顶 Agent" })).not.toBeInTheDocument();
  });

  it("keeps error evidence until dismissed and expires success feedback", () => {
    vi.useFakeTimers();
    function Feedback() {
      const toast = useToast();
      return <button onClick={() => { toast.show({ kind: "success", msg: "已保存" }); toast.show({ kind: "error", msg: "写入失败" }); }}>操作</button>;
    }
    render(<ToastProvider><Feedback /></ToastProvider>);
    fireEvent.click(screen.getByRole("button", { name: "操作" }));
    expect(screen.getByRole("status")).toHaveTextContent("已保存");
    act(() => vi.advanceTimersByTime(7000));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("写入失败");
    fireEvent.click(screen.getByRole("button", { name: "关闭错误通知 2" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
