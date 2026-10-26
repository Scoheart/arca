import { isImeKey } from "./ImeInput";
import { ToastNotice } from "./Toast";
import { useEffect, useMemo, useRef, useState } from "react";
import type { AgentInfo } from "../lib/types";
import { buildAgentPickerSections, MAX_PINNED_AGENTS } from "../lib/pinnedAgents";
import { usePinnedAgents } from "../hooks/usePinnedAgents";
import { AgentGlyph, isAgentVisible, isAgentEntryVisible } from "./brandIcons";
import { ChevronDownIcon, PackageIcon } from "./icons";
import { PinnedAgentDock } from "./PinnedAgentDock";
import { MODAL_DIALOG_SELECTOR } from "./ui";
import { AgentHandPicker } from "./AgentHandPicker";

export function AgentNavigation({ agents, selectedAgentId, onSelectAgent, onAddAgent }: {
  agents: AgentInfo[];
  selectedAgentId: string | null;
  onSelectAgent(id: string): void;
  onAddAgent?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const anchorRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const pinned = usePinnedAgents();
  const sections = useMemo(() => buildAgentPickerSections(agents, pinned.agentIds, ""), [agents, pinned.agentIds]);
  const pinnedIds = useMemo(() => sections.pinned.map(({ id }) => id), [sections]);
  const available = useMemo(() => [...sections.pinned, ...sections.available], [sections]);
  const selected = available.find(({ id }) => id === selectedAgentId);
  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if (isImeKey(event) || event.repeat || event.altKey || !(event.metaKey || event.ctrlKey)
        || event.key.toLowerCase() !== "k") return;
      const modal = Array.from(document.querySelectorAll(MODAL_DIALOG_SELECTOR)).at(-1);
      if (modal && (!open || modal.getAttribute("data-modal-layer") !== "agent-hand")) return;
      event.preventDefault();
      if (open) {
        const search = modal?.querySelector<HTMLInputElement>('input[type="search"]');
        search?.focus(); search?.select();
      } else setOpen(true);
    };
    document.addEventListener("keydown", handleShortcut);
    return () => document.removeEventListener("keydown", handleShortcut);
  }, [open]);

  const saveVisiblePins = async (ids: string[]) => {
    const hidden = pinned.agentIds.filter((id) => {
      const agent = agents.find((entry) => entry.id === id);
      return !(agent ? isAgentEntryVisible(agent) : isAgentVisible(id));
    });
    const next = [...ids, ...hidden];
    if (next.length > MAX_PINNED_AGENTS) {
      setAnnouncement("置顶位置已满，隐藏 Agent 的置顶记录仍然保留。");
      return false;
    }
    return pinned.commit(next);
  };
  const saveOrder = async (ids: string[], movedId: string) => {
    if (!pinned.ready || pinned.saving || ids.join("\0") === pinnedIds.join("\0")) return;
    if (await saveVisiblePins(ids)) {
      setAnnouncement(`${agents.find(({ id }) => id === movedId)?.name ?? movedId} 已移动到第 ${ids.indexOf(movedId) + 1} 位`);
    }
  };

  return <div className="arca-agent-navigation">
    <ToastNotice kind={announcement.includes("已满") ? "warning" : "success"} message={announcement} />
    <div className="arca-agent-picker-cluster" ref={anchorRef}>
      {pinnedIds.length > 0 && <PinnedAgentDock agents={agents} ids={pinnedIds} selectedId={selectedAgentId}
        disabled={!pinned.ready || pinned.saving} onSelect={onSelectAgent} onReorder={(ids, id) => void saveOrder(ids, id)} />}
      <div className="arca-agent-picker-anchor">
        <button ref={triggerRef} type="button" className="arca-agent-picker-trigger" data-active={selected ? "true" : undefined}
          data-open={open ? "true" : undefined} aria-haspopup="dialog" aria-expanded={open}
          aria-label={selected ? `选择 Agent，当前：${selected.name}` : "选择 Agent"} aria-keyshortcuts="Meta+K Control+K" title="选择 Agent · ⌘K / Ctrl+K" onClick={() => setOpen(true)}>
          {selected ? <AgentGlyph id={selected.id} name={selected.name} size={24} /> : <PackageIcon className="w-5 h-5 flex-shrink-0" />}
          <span className="arca-agent-picker-trigger-name">{selected?.name ?? "选择 Agent"}</span>
          <ChevronDownIcon className="arca-agent-picker-chevron" />
        </button>
      </div>
    </div>
    {open && <AgentHandPicker agents={available} pinnedIds={pinnedIds} selectedAgentId={selectedAgentId} ready={pinned.ready} saving={pinned.saving}
      anchorRef={anchorRef} triggerRef={triggerRef} onSavePins={saveVisiblePins}
      onClose={() => setOpen(false)} onSelect={onSelectAgent}
      onAdd={onAddAgent ? () => { setOpen(false); onAddAgent(); } : undefined} />}
  </div>;
}
