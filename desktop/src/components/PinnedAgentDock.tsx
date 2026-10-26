import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AgentLaunchAction } from "./AgentLaunchAction";
import type { AgentInfo } from "../lib/types";
import { MAX_PINNED_AGENTS, movePinnedAgentBy, previewPinnedAgentOrder } from "../lib/pinnedAgents";
import { AgentGlyph } from "./brandIcons";
import { PlusIcon } from "./icons";

export function PinnedAgentDock({ agents, ids, selectedId, expanded = false, disabled,
  replacementName, onReplace, onSelect, onEmpty, onReorder,
}: {
  agents: AgentInfo[];
  ids: string[];
  selectedId?: string | null;
  expanded?: boolean;
  disabled: boolean;
  replacementName?: string;
  onReplace?(index: number): void;
  onSelect(id: string): void;
  onEmpty?(): void;
  onReorder(ids: string[], movedId: string): void;
}) {
  const [context, setContext] = useState<{ id: string; x: number; y: number; opener: HTMLButtonElement } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!context) return;
    menuRef.current?.focus();
    const close = (event: PointerEvent) => { if (!menuRef.current?.contains(event.target as Node)) setContext(null); };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [context]);
  const showContext = (id: string, opener: HTMLButtonElement, x: number, y: number) => {
    setContext({ id, opener, x: Math.max(8, Math.min(x, window.innerWidth - 218)), y: Math.max(8, Math.min(y, window.innerHeight - 175)) });
  };
  const [preview, setPreview] = useState<string[] | null>(null);
  const dragged = useRef<string | null>(null);
  const previewRef = useRef<string[] | null>(null);
  const dock = useRef<HTMLElement>(null);
  const positions = useRef(new Map<string, number>());
  const order = preview ?? ids;
  // Fresh inventory arrays do not mean the pin order changed.
  const orderKey = order.join("\0");

  useLayoutEffect(() => {
    // AgentGlyph also exposes data-agent-id. Measure only the direct slot
    // buttons, otherwise each inner glyph overwrites its button's position.
    const buttons = Array.from(dock.current?.querySelectorAll<HTMLElement>(":scope > button[data-pin-slot][data-agent-id]") ?? []);
    const measured = buttons.map((button) => ({ button, id: button.dataset.agentId!, left: button.offsetLeft }));
    const motions: Animation[] = [];
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    for (const { button, id, left } of measured) {
      const before = positions.current.get(id);
      if (!reduced && before !== undefined && before !== left) {
        motions.push(button.animate([{ transform: `translateX(${before - left}px)` }, { transform: "none" }], {
          duration: 170, easing: "cubic-bezier(.2,.8,.2,1)",
        }));
      }
    }
    positions.current = new Map(measured.map(({ id, left }) => [id, left]));
    return () => motions.forEach((motion) => motion.cancel());
  }, [orderKey]);

  const clearDrag = () => { dragged.current = null; previewRef.current = null; setPreview(null); };
  return <><nav ref={dock} className={`arca-pinned-agent-bar${expanded ? " arca-hand-dock" : ""}`}
    aria-label="置顶 Agent" data-replacing={replacementName ? "true" : undefined}
    onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setPreview(null); }}>
    {Array.from({ length: expanded ? MAX_PINNED_AGENTS : order.length }, (_, index) => {
      const id = order[index];
      const agent = agents.find((agent) => agent.id === id);
      const label = replacementName ? `用 ${replacementName} 替换 ${agent?.name ?? "空位"}` : agent?.name ?? "空置顶位";
      return <button key={id ?? `empty-${index}`} type="button" className="arca-pinned-agent"
        data-agent-id={id} data-pin-slot={index} data-empty={!agent || undefined}
        data-active={selectedId === id ? "true" : undefined}
        aria-current={selectedId === id ? "page" : undefined}
        aria-label={label} title={replacementName ? label : agent ? `${agent.name} · 拖动排序，Option + 左右方向键调整` : label}
        aria-keyshortcuts="Alt+ArrowLeft Alt+ArrowRight" disabled={disabled}
        draggable={Boolean(agent) && !disabled && !replacementName}
        onContextMenu={(event) => {
          if (!agent || disabled || replacementName) return;
          event.preventDefault(); event.stopPropagation();
          showContext(id, event.currentTarget, event.clientX, event.clientY);
        }}
        onClick={() => replacementName ? onReplace?.(index) : agent ? onSelect(id) : onEmpty?.()}
        onKeyDown={(event) => {
          if (agent && !disabled && !replacementName && (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10"))) {
            event.preventDefault();
            const bounds = event.currentTarget.getBoundingClientRect();
            showContext(id, event.currentTarget, bounds.left, bounds.bottom + 5);
            return;
          }
          if (!agent || disabled || replacementName || !event.altKey || !["ArrowLeft", "ArrowRight"].includes(event.key)) return;
          event.preventDefault();
          onReorder(movePinnedAgentBy(ids, id, event.key === "ArrowLeft" ? -1 : 1), id);
        }}
        onDragStart={(event) => {
          if (!agent || disabled || replacementName) { event.preventDefault(); return; }
          dragged.current = id; previewRef.current = ids;
          event.dataTransfer.effectAllowed = "move";
          event.dataTransfer.setData("text/plain", id);
        }}
        onDragOver={(event) => {
          if (!dragged.current || disabled || replacementName) return;
          event.preventDefault(); event.dataTransfer.dropEffect = "move";
          const bounds = event.currentTarget.getBoundingClientRect();
          const next = agent ? previewPinnedAgentOrder(ids, dragged.current, id,
            event.clientX >= bounds.left + bounds.width / 2 ? "after" : "before")
            : [...ids.filter((id) => id !== dragged.current), dragged.current];
          previewRef.current = next;
          setPreview(next);
        }}
        onDrop={(event) => {
          if (!dragged.current || disabled || replacementName) return;
          event.preventDefault();
          onReorder(previewRef.current ?? ids, dragged.current);
          clearDrag();
        }} onDragEnd={clearDrag}>
        {agent ? <span className="arca-pinned-agent-glyph"><AgentGlyph id={agent.id} name={agent.name} size={30} /></span>
          : <PlusIcon className="w-3.5 h-3.5" />}
      </button>;
    })}
  </nav>
    {context && createPortal(<div ref={menuRef} className="arca-agent-launch-context" role="menu" aria-label="Agent 操作" tabIndex={-1}
      style={{ left: context.x, top: context.y }} onKeyDown={(event) => {
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); context.opener.focus(); setContext(null); return; }
        if (["ArrowDown", "ArrowUp", "Home", "End", "Tab"].includes(event.key)) {
          event.preventDefault(); event.stopPropagation();
          const items = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? []);
          if (!items.length) return;
          const current = items.indexOf(document.activeElement as HTMLButtonElement);
          const backwards = event.key === "ArrowUp" || (event.key === "Tab" && event.shiftKey);
          const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : current < 0 ? (backwards ? items.length - 1 : 0)
            : (current + (backwards ? -1 : 1) + items.length) % items.length;
          items[next]?.focus();
        }
      }}>
      <AgentLaunchAction key={context.id} agentId={context.id} contextMenu onChosen={() => setContext(null)} />
    </div>, document.body)}
  </>;
}
