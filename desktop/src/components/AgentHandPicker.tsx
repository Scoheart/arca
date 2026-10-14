import { ToastNotice } from "./Toast";
import { startTransition, useEffect, useLayoutEffect, useMemo, useId, useRef, useState, type CSSProperties, type RefObject } from "react";
import type { AgentInfo } from "../lib/types";
import { MAX_PINNED_AGENTS } from "../lib/pinnedAgents";
import { agentHandMetrics } from "../lib/agentHandLayout";
import { useHandTrackpadPaging } from "../hooks/useHandTrackpadPaging";
import { AgentGlyph, isAgentEntryVisible } from "./brandIcons";
import { ArrowLeftIcon, ChevronDownIcon, PackageIcon, PinIcon, PinOffIcon, PlusIcon, SearchIcon } from "./icons";
import { Modal } from "./ui";
import { PinnedAgentDock } from "./PinnedAgentDock";
import { AgentEntryTransition, type AgentEntryOrigin } from "./AgentEntryTransition";
import "./AgentHandPicker.css";

type Group = "pinned" | "builtin" | "custom";
type HandItem = { id: string; agent?: AgentInfo };
type CardPose = { transform: string; opacity: string; tileTransform: string };
type PageTurn = { direction: -1 | 1; origins: Map<string, CardPose> };
const snapshotCard = (card: HTMLElement): CardPose => {
  const style = getComputedStyle(card);
  const tile = card.querySelector<HTMLElement>(".arca-agent-hand-tile");
  return { transform: style.transform, opacity: style.opacity,
    tileTransform: tile ? getComputedStyle(tile).transform : "none" };
};
const removeGhost = (ghost: HTMLElement) => {
  ghost.getAnimations({ subtree: true }).forEach((motion) => motion.cancel());
  ghost.remove();
};
const ADD_ID = "__arca_add_agent__";
const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const pose = (x: number, y: number, angle = 0, scale = 1) => `translate3d(${x}px,${y}px,0) rotate(${angle}deg) scale(${scale})`;

export function AgentHandPicker({ agents: allAgents, pinnedIds, selectedAgentId, ready, saving, anchorRef, triggerRef, onSavePins, onSelect, onAdd, onClose }: {
  agents: AgentInfo[];
  pinnedIds: string[];
  selectedAgentId: string | null;
  ready: boolean;
  saving: boolean;
  anchorRef: RefObject<HTMLDivElement | null>;
  triggerRef: RefObject<HTMLButtonElement | null>;
  onSavePins(ids: string[]): Promise<boolean>;
  onSelect(id: string): void;
  onAdd?: () => void;
  onClose(): void;
}) {
  const searchHintId = useId();
  const agents = useMemo(() => allAgents.filter(isAgentEntryVisible), [allAgents]);
  const [group, setGroup] = useState<Group>("builtin");
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [width, setWidth] = useState(Math.min(window.innerWidth * .6 + 96, window.innerWidth - 144));
  const [viewportWidth, setViewportWidth] = useState(window.innerWidth);
  const [dockPosition, setDockPosition] = useState({ left: 16, top: 16, width: 460, dockWidth: 0 });
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const [replacement, setReplacement] = useState<AgentInfo | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [busy, setBusy] = useState(false);
  const [closing, setClosing] = useState(false);
  const selectedAgent = agents.find((agent) => agent.id === selectedAgentId);
  const [playing, setPlaying] = useState<string | null>(null);
  const [entryTransition, setEntryTransition] = useState<{ agent: AgentInfo; origin: AgentEntryOrigin; overlay: HTMLElement | null } | null>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const boardRef = useRef<HTMLDivElement>(null);
  const dockRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const cardRefs = useRef(new Map<string, HTMLElement>());
  const motions = useRef(new Set<Animation>());
  const lock = useRef(false);
  const mounted = useRef(true);
  const entered = useRef(false);
  const pageGhostsRef = useRef<HTMLDivElement>(null);
  const pageTurn = useRef<PageTurn | null>(null);
  const pageMotions = useRef(new Set<Animation>());
  const keyboard = useRef(false);
  const focusFrame = useRef<number | null>(null);

  const rows = useMemo(() => {
    const text = query.trim().toLocaleLowerCase();
    if (text) return agents.filter((a) => [a.name, a.id, a.category].join(" ").toLocaleLowerCase().includes(text));
    if (group === "pinned") return pinnedIds.flatMap((id) => agents.find((a) => a.id === id) ?? []);
    return agents.filter((a) => group === "builtin" ? a.builtin : !a.builtin);
  }, [agents, pinnedIds, group, query]);
  const items = useMemo<HandItem[]>(() => [
    ...rows.map((agent) => ({ id: agent.id, agent })),
    ...(!query.trim() && group === "custom" && onAdd ? [{ id: ADD_ID }] : []),
  ], [rows, group, query, onAdd]);
  const pageSize = width < 430 ? 3 : width < 620 ? 6 : width < 780 ? 9 : 10;
  const pageCount = Math.max(1, Math.ceil(items.length / pageSize));
  const actualPage = Math.min(page, pageCount - 1);
  useHandTrackpadPaging(stageRef, pageCount > 1 && !busy && !saving && !replacement, (pages) => {
    turnPage(actualPage + pages);
  });
  const visible = items.slice(actualPage * pageSize, (actualPage + 1) * pageSize);
  const handSpan = Math.min(viewportWidth * .6, width - 32);
  const cardWidth = width < 430 ? 76 : Math.max(84, Math.min(112, handSpan / 8.5));
  const cardHeight = cardWidth * 4 / 3;
  const metrics = agentHandMetrics(visible.length, cardWidth);
  // Match the visible fan's outer edges, including the rotated end cards.
  const angle = metrics.spread * Math.PI / 180;
  const edgeWidth = cardWidth * Math.cos(angle) + cardHeight * Math.sin(angle);
  const desiredStep = (handSpan - edgeWidth) / Math.max(1, visible.length - 1);
  const step = visible.length >= 5 ? Math.max(0, desiredStep) : Math.min(metrics.step * 1.2, Math.max(0, desiredStep));
  const positions = visible.map((_, index) => {
    const unit = visible.length <= 1 ? 0 : (index - (visible.length - 1) / 2) / ((visible.length - 1) / 2);
    return {
      x: width / 2 - cardWidth / 2 + (index - (visible.length - 1) / 2) * step,
      y: 128 - cardHeight / 2 + unit * unit * metrics.rise - metrics.rise / 2,
      angle: unit * metrics.spread,
    };
  });
  const handKey = `${visible.map(({ id }) => id).join("\0")}:${width}:${viewportWidth}`;

  const clearPageGhosts = (layer = pageGhostsRef.current) => {
    if (!layer) return;
    layer.getAnimations({ subtree: true }).forEach((motion) => motion.cancel());
    layer.replaceChildren();
  };

  const turnPage = (requested: number) => {
    if (lock.current || saving || replacement) return;
    const next = Math.max(0, Math.min(pageCount - 1, requested));
    if (next === actualPage) return;
    const direction = next > actualPage ? 1 : -1;
    const origins = new Map<string, CardPose>();
    const layer = pageGhostsRef.current;
    if (layer && !reducedMotion()) {
      const nextIds = new Set(items.slice(next * pageSize, (next + 1) * pageSize).map(({ id }) => id));
      // Read every pose before inserting or cancelling anything. Interleaved
      // style reads and DOM writes can force repeated layout work on WebKit.
      const departing = visible.flatMap(({ id }, index) => {
        const card = cardRefs.current.get(id);
        return card ? [{ id, index, card, ...snapshotCard(card) }] : [];
      });
      const returning = Array.from(layer.children).flatMap((node) => {
        const ghost = node as HTMLElement;
        const id = ghost.dataset.ghostId;
        return id && nextIds.has(id) ? [{ id, ghost, ...snapshotCard(ghost) }] : [];
      });
      // A quick reversal picks the previous hand up where it is still flying,
      // instead of erasing it and dealing a second copy from a fixed origin.
      returning.forEach(({ id, ghost, ...origin }) => {
        if (Number(origin.opacity) >= Number(origins.get(id)?.opacity ?? -1)) origins.set(id, origin);
        removeGhost(ghost);
      });
      const fragment = document.createDocumentFragment();
      const exits = departing.flatMap(({ id, card, index, ...origin }) => {
        if (Number(origin.opacity) < .01) return [];
        const ghost = card.cloneNode(true) as HTMLElement;
        ghost.removeAttribute("data-hand-id");
        ghost.removeAttribute("data-dealing");
        ghost.dataset.ghostId = id;
        ghost.setAttribute("inert", "");
        ghost.style.willChange = "transform, opacity";
        ghost.querySelectorAll("[id]").forEach((node) => node.removeAttribute("id"));
        ghost.querySelectorAll<HTMLElement>("button").forEach((node) => { node.tabIndex = -1; });
        const tile = ghost.querySelector<HTMLElement>(".arca-agent-hand-tile");
        if (tile) tile.style.transform = origin.tileTransform;
        fragment.append(ghost);
        return [{ ghost, index, ...origin }];
      });
      layer.append(fragment);
      exits.forEach(({ ghost, index, transform, opacity }) => {
        const matrix = new DOMMatrixReadOnly(transform === "none" ? undefined : transform);
        const rotation = Math.atan2(matrix.b, matrix.a) * 180 / Math.PI;
        const scale = Math.hypot(matrix.a, matrix.b);
        const order = direction > 0 ? index : visible.length - index - 1;
        const motion = ghost.animate([
          { transform, opacity },
          { transform: pose(matrix.e - direction * 80, matrix.f + 8, rotation - direction * 4, scale * .98), opacity: 0 },
        ], { duration: 180, delay: order * 5, easing: "cubic-bezier(.22,.7,.36,1)", fill: "both" });
        motion.onfinish = () => removeGhost(ghost);
      });
      // Unrelated outgoing hands keep their short exit, including under rapid
      // navigation. Each is removed on finish; no timers or navigation queue.
    } else {
      clearPageGhosts();
    }
    pageTurn.current = { direction, origins };
    setPage(next);
    setHighlighted(null);
  };

  useEffect(() => {
    mounted.current = true;
    const ghostLayer = pageGhostsRef.current;
    return () => {
      mounted.current = false; entered.current = false;
      clearPageGhosts(ghostLayer);
      if (focusFrame.current !== null) cancelAnimationFrame(focusFrame.current);
      motions.current.forEach((motion) => motion.cancel()); motions.current.clear();
    };
  }, []);

  useEffect(() => {
    // Warm only the view module while the user chooses; no Agent data or writes.
    void import("./AgentView").catch(() => undefined);
  }, []);

  useLayoutEffect(() => {
    let frame: number | null = null;
    const measure = () => {
      if (boardRef.current) setWidth(boardRef.current.clientWidth);
      setViewportWidth(window.innerWidth);
      const bounds = anchorRef.current?.getBoundingClientRect();
      const sourceDock = anchorRef.current?.querySelector<HTMLElement>(".arca-pinned-agent-bar");
      const dockWidth = sourceDock?.getBoundingClientRect().width ?? 0;
      const left = bounds?.left ?? 24;
      // Borrow the actual toolbar geometry, including responsive pin slots.
      // Only a collapsed icon-only selector needs extra room to type a query.
      const next = { left, top: bounds?.top ?? 16, dockWidth,
        width: Math.min(Math.max(bounds?.width ?? 220, dockWidth + (dockWidth ? 8 : 0) + 160), window.innerWidth - left - 16) };
      setDockPosition((previous) => previous.left === next.left && previous.top === next.top &&
        previous.width === next.width && previous.dockWidth === next.dockWidth ? previous : next);
    };
    const scheduleMeasure = () => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => { frame = null; measure(); });
    };
    measure();
    const observer = new ResizeObserver(scheduleMeasure);
    if (boardRef.current) observer.observe(boardRef.current);
    if (anchorRef.current) observer.observe(anchorRef.current);
    window.addEventListener("resize", scheduleMeasure);
    return () => {
      if (frame !== null) cancelAnimationFrame(frame);
      observer.disconnect(); window.removeEventListener("resize", scheduleMeasure);
    };
  }, [anchorRef]);

  const animate = (element: HTMLElement, frames: Keyframe[], duration: number, delay = 0, easing = "cubic-bezier(.22,.7,.25,1)") => {
    if (reducedMotion()) return Promise.resolve();
    const motion = element.animate(frames, { duration, delay, easing, fill: "both" });
    motions.current.add(motion);
    return motion.finished.catch(() => undefined).then(() => { motions.current.delete(motion); });
  };

  useLayoutEffect(() => {
    if (lock.current) return;
    setHighlighted(null);
    const board = boardRef.current;
    if (!board) return;
    const bounds = board.getBoundingClientRect();
    const source = triggerRef.current?.getBoundingClientRect();
    const first = !entered.current;
    const turn = pageTurn.current;
    const direction = turn?.direction;
    pageTurn.current = null;
    if (!direction) clearPageGhosts();
    const pile = pose(first && source ? source.left + source.width / 2 - bounds.left - cardWidth / 2 : width / 2 - cardWidth / 2,
      first && source ? source.top + source.height / 2 - bounds.top - cardHeight / 2 : 115, -10, .4);
    const deals: Animation[] = [];
    const movingCards: HTMLElement[] = [];
    visible.forEach(({ id }, index) => {
      const card = cardRefs.current.get(id);
      if (!card || reducedMotion()) return;
      if (first) card.dataset.dealing = "true";
      const point = positions[index];
      if (!first && !direction) return;
      const order = direction === -1 ? visible.length - index - 1 : index;
      const travel = direction ?? 1;
      const centerX = width / 2 - cardWidth / 2;
      const origin = turn?.origins.get(id);
      // Keep most of the fan spread throughout the exchange. A short wave and
      // early opacity settle avoid the old pile-up, blank beat and long tail.
      const frames = first
        ? [{ transform: pile, opacity: 0 }, { transform: card.style.transform, opacity: 1 }]
        : [{ transform: origin?.transform ?? pose(centerX + (point.x - centerX) * .88 + travel * 72,
          point.y + 10, point.angle + travel * 4, .97), opacity: origin?.opacity ?? 0 },
          { opacity: 1, offset: .3 }, { transform: card.style.transform, opacity: 1 }];
      card.style.willChange = "transform, opacity";
      movingCards.push(card);
      const motion = card.animate(frames, {
        duration: first ? 250 : 280,
        delay: first ? index * Math.min(28, 168 / Math.max(1, visible.length - 1)) : origin ? 0 : order * 5,
        easing: "cubic-bezier(.22,1,.36,1)", fill: "backwards",
      });
      deals.push(motion); motions.current.add(motion);
      if (direction) pageMotions.current.add(motion);
      const tile = card.querySelector<HTMLElement>(".arca-agent-hand-tile");
      if (tile && origin && origin.tileTransform !== "none") {
        const lift = tile.animate([{ transform: origin.tileTransform }, { transform: "none" }],
          { duration: 280, easing: "cubic-bezier(.22,1,.36,1)" });
        deals.push(lift); motions.current.add(lift);
        lift.onfinish = () => { motions.current.delete(lift); };
      }
      const settled = () => {
        motions.current.delete(motion);
        pageMotions.current.delete(motion);
        // A cancelled old flight may settle after another flight has started
        // on this reused slot. Do not clear the new flight's compositor hint.
        if (!card.getAnimations().some((flight) => flight.playState === "running")) {
          delete card.dataset.dealing;
          card.style.removeProperty("will-change");
        }
      };
      motion.onfinish = settled;
      motion.oncancel = settled;
    });
    entered.current = true;
    return () => {
      movingCards.forEach((card) => card.style.removeProperty("will-change"));
      deals.forEach((motion) => { motion.cancel(); motions.current.delete(motion); pageMotions.current.delete(motion); });
      cardRefs.current.forEach((card) => { delete card.dataset.dealing; });
    };
    // Slot contents/layout can change without replaying the opening deal.
    // Pin/save/highlight renders must not replay the entrance animation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handKey]);

  const begin = () => {
    if (lock.current || saving) return false;
    clearPageGhosts();
    lock.current = true; setBusy(true); setHighlighted(null); return true;
  };
  const finish = () => { lock.current = false; if (mounted.current) { setBusy(false); setPlaying(null); } };
  const freezeCards = () => {
    const current = [...cardRefs.current.values()].map((card) => ({ card, transform: getComputedStyle(card).transform, opacity: getComputedStyle(card).opacity }));
    motions.current.forEach((motion) => motion.cancel()); motions.current.clear();
    current.forEach(({ card, transform, opacity }) => { card.style.transform = transform; card.style.opacity = opacity; });
    return current;
  };
  const close = async () => {
    if (replacement && !lock.current) { setReplacement(null); searchRef.current?.focus(); setAnnouncement(""); return; }
    if (!begin()) return;
    setClosing(true);
    const board = boardRef.current?.getBoundingClientRect();
    const source = triggerRef.current?.getBoundingClientRect();
    const target = pose(source && board ? source.left + source.width / 2 - board.left - cardWidth / 2 : width / 2 - cardWidth / 2,
      source && board ? source.top + source.height / 2 - board.top - cardHeight / 2 : 120, -10, .35);
    const cards = freezeCards();
    // Keep the cards readable as they gather; fade only near the end of the
    // flight, then reveal the page without abruptly removing the backdrop.
    const stagger = Math.min(20, 120 / Math.max(1, cards.length - 1));
    const flights = cards.map(({ card, transform, opacity }, index) => animate(card, [
      { transform, opacity, offset: 0 },
      { opacity, offset: .65 },
      { transform: target, opacity: 0, offset: 1 },
    ], 380, (cards.length - index - 1) * stagger, "cubic-bezier(.4,0,.2,1)"));
    const overlay = boardRef.current?.closest<HTMLElement>("[data-modal-overlay]");
    if (overlay) flights.push(animate(overlay, [{ opacity: 1 }, { opacity: 0 }], 180, 360));
    await Promise.all(flights);
    if (mounted.current) onClose();
  };
  const select = (id: string, immediate = false) => {
    if (replacement || !begin()) return;
    if (immediate) { onSelect(id); onClose(); return; }
    setClosing(true);
    const cards = freezeCards();
    const current = cards.find(({ card }) => card.dataset.handId === id);
    const agent = agents.find((agent) => agent.id === id);
    const tile = current?.card.querySelector<HTMLElement>(".arca-agent-hand-tile");
    if (!current || !tile || !agent || reducedMotion()) {
      onSelect(id); onClose(); return;
    }
    const bounds = tile.getBoundingClientRect();
    const matrix = new DOMMatrixReadOnly(current.transform === "none" ? undefined : current.transform);
    setPlaying(id);
    setEntryTransition({ agent, origin: {
      left: bounds.left + (bounds.width - tile.offsetWidth) / 2,
      top: bounds.top + (bounds.height - tile.offsetHeight) / 2,
      width: tile.offsetWidth, height: tile.offsetHeight,
      rotation: Math.atan2(matrix.b, matrix.a) * 180 / Math.PI,
    }, overlay: current.card.closest<HTMLElement>("[data-modal-overlay]") });
    cards.filter(({ card }) => card !== current.card).forEach(({ card, transform, opacity }, index) => {
      void animate(card, [{ transform, opacity }, { transform: pose(width / 2 - cardWidth / 2, 120, -8, .6), opacity: 0 }], 140, index * 8);
    });
    // Start loading now, but retain the revealed page if the lazy view suspends
    // instead of briefly replacing the whole content with a loading fallback.
    startTransition(() => onSelect(id));
  };
  const add = async () => {
    if (!onAdd || !begin()) return;
    freezeCards(); setPlaying(ADD_ID);
    const card = cardRefs.current.get(ADD_ID);
    if (card) {
      await animate(card, [{ transform: card.style.transform, opacity: 1 },
        { transform: pose(width / 2 - cardWidth / 2, 40, 0, 1.7), opacity: 0 }], 180);
    }
    if (mounted.current) onAdd();
  };
  const savePins = async (next: string[], agent: AgentInfo, slot?: number) => {
    const restoreKeyboardFocus = keyboard.current;
    if (!ready || !begin()) return;
    // Keep pin feedback immediate; persistence runs alongside the short flight.
    const result = onSavePins(next);
    const card = cardRefs.current.get(agent.id);
    const target = slot !== undefined ? dockRef.current?.querySelector<HTMLElement>(`[data-pin-slot="${slot}"]`) : null;
    let flight = Promise.resolve();
    if (card && target && target.getClientRects().length && boardRef.current) {
      const bounds = boardRef.current.getBoundingClientRect(), destination = target.getBoundingClientRect();
      setPlaying(agent.id);
      flight = animate(card, [{ transform: getComputedStyle(card).transform, opacity: 1 }, {
        transform: pose(destination.left + destination.width / 2 - bounds.left - cardWidth / 2,
          destination.top + destination.height / 2 - bounds.top - cardHeight / 2,
          0, Math.min(destination.width / cardWidth, destination.height / cardHeight)), opacity: .3,
      }], 280);
    }
    const [saved] = await Promise.all([result, flight]);
    if (!mounted.current) return;
    // Remove filled pin-flight styles without restarting every other card.
    card?.getAnimations().forEach((motion) => motion.cancel());
    finish(); setReplacement(null);
    setAnnouncement(saved ? `${agent.name} 已${next.includes(agent.id) ? "置顶" : "取消置顶"}` : "未保存，已恢复原来的置顶状态");
    // Pin changes can remove a card from the 常用 group. Restore focus deliberately.
    if (restoreKeyboardFocus || !card?.isConnected) {
      focusFrame.current = requestAnimationFrame(() => {
        focusFrame.current = null;
        if (!mounted.current) return;
        (card?.isConnected ? card.querySelector<HTMLButtonElement>(".arca-agent-hand-pin") : searchRef.current)?.focus();
      });
    }
  };
  const togglePin = (agent: AgentInfo) => {
    if (!ready || saving || lock.current) return;
    if (pinnedIds.includes(agent.id)) { void savePins(pinnedIds.filter((id) => id !== agent.id), agent); return; }
    if (pinnedIds.length >= MAX_PINNED_AGENTS) {
      setReplacement(agent); setHighlighted(null);
      setAnnouncement(`选择上方一个牌槽，替换为 ${agent.name}`);
      dockRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
      return;
    }
    void savePins([...pinnedIds, agent.id], agent, pinnedIds.length);
  };
  const changeGroup = (next: Group) => { if (busy || saving) return; setGroup(next); setQuery(""); setPage(0); setReplacement(null); setAnnouncement(""); setHighlighted(null); };

  // Pin replacement must expose every slot even when the main toolbar is compact.
  const shownDockWidth = replacement ? MAX_PINNED_AGENTS * 34 + (MAX_PINNED_AGENTS - 1) * 3 + 10 : dockPosition.dockWidth;
  const shownTopbarWidth = replacement
    ? Math.min(Math.max(dockPosition.width, shownDockWidth + 168), viewportWidth - 32)
    : dockPosition.width;
  const shownTopbarLeft = replacement ? Math.min(dockPosition.left, viewportWidth - shownTopbarWidth - 16) : dockPosition.left;

  return <Modal width="min(calc(60vw + 96px), calc(100vw - 144px))" maxHeight="calc(100dvh - 120px)" ariaLabel="选择和置顶 Agent" layer="agent-hand" onClose={() => void close()}>
    <div className="arca-agent-hand-picker" data-busy={busy || saving || undefined} data-playing={playing || undefined} data-entering={entryTransition ? "true" : undefined} data-closing={closing || undefined}>
      <div className="arca-agent-hand-topbar" style={{ left: shownTopbarLeft, top: dockPosition.top, width: shownTopbarWidth }}>
      <div ref={dockRef} className="arca-agent-hand-pinned" style={{ width: shownDockWidth, display: shownDockWidth ? undefined : "none" }}>
        <PinnedAgentDock expanded agents={agents} ids={pinnedIds} selectedId={selectedAgentId} disabled={!ready || saving || busy}
          replacementName={replacement?.name} onReplace={(index) => {
            if (!replacement) return;
            const next = [...pinnedIds]; next[index] = replacement.id;
            void savePins(next, replacement, index);
          }} onSelect={(id) => void select(id)} onEmpty={() => searchRef.current?.focus()}
          onReorder={(next, id) => {
            if (next.join("\0") === pinnedIds.join("\0") || !ready || lock.current) return;
            const agent = agents.find((agent) => agent.id === id);
            if (agent) void savePins(next, agent);
          }} />
      </div>
      <div className="arca-agent-hand-toolbar">
        <label className="arca-agent-hand-search" data-active={selectedAgent ? "true" : undefined}>
          <span className="arca-agent-hand-search-content"><SearchIcon className="w-4 h-4" />
          <input ref={searchRef} data-modal-initial-focus type="search" autoComplete="off" spellCheck={false} placeholder="搜索名称或 ID" aria-label="搜索 Agent" aria-describedby={searchHintId}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing || event.altKey || event.metaKey || event.ctrlKey) return;
              if (event.key === "ArrowDown" && visible.length) {
                event.preventDefault();
                cardRefs.current.get(visible[0].id)?.querySelector<HTMLButtonElement>(".arca-agent-hand-select")?.focus();
              }
              if (event.key !== "Enter" || !query.trim()) return;
              event.preventDefault();
              const needle = query.trim().toLocaleLowerCase();
              const idMatch = rows.find((agent) => agent.id.toLocaleLowerCase() === needle);
              const nameMatches = rows.filter((agent) => agent.name.toLocaleLowerCase() === needle);
              const match = idMatch ?? (nameMatches.length === 1 ? nameMatches[0] : rows.length === 1 ? rows[0] : undefined);
              if (match) select(match.id, true);
              else setAnnouncement(rows.length ? "有多个匹配项，请输入完整名称或 ID，或按向下键选择" : "未找到匹配的 Agent");
            }}
            value={query} disabled={busy || saving} onChange={(event) => { setQuery(event.target.value); setPage(0); setReplacement(null); setAnnouncement(""); }} />
          </span>
          <span className="arca-agent-hand-return" aria-hidden="true">
            {selectedAgent ? <AgentGlyph id={selectedAgent.id} name={selectedAgent.name} size={24} /> : <PackageIcon className="w-5 h-5 flex-shrink-0" />}
            <span className="arca-agent-picker-trigger-name">{selectedAgent?.name ?? "选择 Agent"}</span>
            <ChevronDownIcon className="arca-agent-picker-chevron" />
          </span>
        </label>
      </div>
      </div>
      <p id={searchHintId} className="sr-only">输入完整名称或 ID，按 Enter 直接进入；向下键选择卡片。{query.trim() ? `找到 ${rows.length} 个 Agent。` : ""}</p>
      <div ref={stageRef} className="arca-agent-hand-stage">
      {pageCount > 1 && <button type="button" className="arca-agent-hand-page arca-agent-hand-page-prev" aria-label="上一手 Agent" title="上一页"
        disabled={actualPage === 0 || busy || saving || Boolean(replacement)} onClick={() => turnPage(actualPage - 1)}><ArrowLeftIcon className="w-5 h-5" /></button>}
      <div ref={boardRef} className="arca-agent-hand-board" style={{ height: Math.max(234, cardHeight + metrics.rise + 76) }} onPointerLeave={() => { if (!keyboard.current) setHighlighted(null); }}
        onPointerMove={(event) => {
          if (busy || replacement || pageMotions.current.size > 0 || event.buttons !== 0 || (!event.movementX && !event.movementY)) return;
          keyboard.current = false;
          // The raised, rotated card can extend into its neighbour's horizontal
          // lane. Keep its actual hit area (especially the pin) interactive.
          const target = event.target instanceof Element ? event.target : null;
          const hitCard = target?.closest<HTMLElement>(".arca-agent-hand-card");
          const hitId = hitCard?.dataset.handId;
          if (hitCard && hitId && event.currentTarget.contains(hitCard) &&
              (hitId === highlighted || target?.closest(".arca-agent-hand-pin"))) {
            setHighlighted(hitId);
            return;
          }
          const bounds = event.currentTarget.getBoundingClientRect();
          const x = event.clientX - bounds.left, y = event.clientY - bounds.top;
          let nearest = -1, distance = Infinity;
          positions.forEach((point, index) => { const dx = Math.abs(x - point.x - cardWidth / 2); if (dx < distance) { nearest = index; distance = dx; } });
          const point = positions[nearest];
          setHighlighted(point && distance < cardWidth * .65 && y > point.y - 32 && y < point.y + cardHeight + 5 ? visible[nearest].id : null);
        }}>
        <div ref={pageGhostsRef} className="arca-agent-hand-outgoing" aria-hidden="true" />
        {visible.map(({ id, agent }, index) => <article key={`hand-slot-${index}`} ref={(node) => { if (node) cardRefs.current.set(id, node); else cardRefs.current.delete(id); }}
          data-hand-id={id} className="arca-agent-hand-card" data-highlighted={highlighted === id || undefined} data-playing={playing === id || undefined}
          data-new={!agent || undefined} style={{ width: cardWidth, height: cardHeight, transform: pose(positions[index].x, positions[index].y, positions[index].angle), "--hand-order": index } as CSSProperties}>
          <div className="arca-agent-hand-tile">
            <button type="button" className="arca-agent-hand-select" disabled={busy || Boolean(replacement)}
              title={agent?.name ?? "添加自定义 Agent"} aria-label={agent ? `进入 ${agent.name}（${id}）` : "添加自定义 Agent"}
              onFocus={(event) => { if (event.currentTarget.matches(":focus-visible")) { keyboard.current = true; setHighlighted(id); } }}
              onBlur={() => { if (keyboard.current) setHighlighted(null); }}
              onKeyDown={(event) => {
                if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key) || event.altKey) return;
                event.preventDefault(); keyboard.current = true;
                const next = event.key === "Home" ? 0 : event.key === "End" ? visible.length - 1 : (index + (event.key === "ArrowLeft" ? -1 : 1) + visible.length) % visible.length;
                cardRefs.current.get(visible[next].id)?.querySelector<HTMLButtonElement>(".arca-agent-hand-select")?.focus();
              }}
              onClick={(event) => agent ? void select(id, event.detail === 0) : void add()}>
              {agent ? <AgentGlyph id={id} name={agent.name} size={30} /> : <PlusIcon className="w-7 h-7" />}
              <span><strong>{agent?.name ?? "添加 Agent"}</strong></span>
            </button>
            {agent && <button type="button" className="arca-agent-hand-pin" title={pinnedIds.includes(id) ? "取消置顶" : "置顶"}
              aria-label={`${pinnedIds.includes(id) ? "取消置顶" : "置顶"} ${agent.name}`} aria-pressed={pinnedIds.includes(id)} disabled={!ready || busy || saving || Boolean(replacement)}
              onFocus={(event) => { if (event.currentTarget.matches(":focus-visible")) { keyboard.current = true; setHighlighted(id); } }} onBlur={() => { if (keyboard.current) setHighlighted(null); }}
              onClick={() => togglePin(agent)}>{pinnedIds.includes(id) ? <PinOffIcon className="w-3.5 h-3.5" /> : <PinIcon className="w-3.5 h-3.5" />}</button>}
          </div>
        </article>)}
        {!visible.length && <p className="arca-agent-hand-empty">{query ? "未找到匹配的 Agent" : "点击卡片上的图钉，将常用 Agent 放到这里"}</p>}
      </div>
      {pageCount > 1 && <button type="button" className="arca-agent-hand-page arca-agent-hand-page-next" aria-label="下一手 Agent" title="下一页"
        disabled={actualPage + 1 === pageCount || busy || saving || Boolean(replacement)} onClick={() => turnPage(actualPage + 1)}><ArrowLeftIcon className="w-5 h-5 rotate-180" /></button>}
      </div>
      <div className="arca-agent-hand-pagination">
        {pageCount > 1 && <nav aria-label="Agent 分页">
          {Array.from({ length: pageCount }, (_, index) => <button key={index} type="button"
            className="arca-agent-hand-dot" aria-label={`第 ${index + 1} 页，共 ${pageCount} 页${actualPage === index ? "，当前页" : ""}`}
            aria-current={actualPage === index ? "page" : undefined} title={`第 ${index + 1} 页`}
            disabled={busy || saving || Boolean(replacement)}
            onClick={() => turnPage(index)} />)}
        </nav>}
      </div>
      <div className="arca-agent-hand-footer">
      <div className="arca-agent-hand-filters" role="group" aria-label="Agent 分类">
        {([['pinned', '常用'], ['builtin', '内置'], ['custom', '自定义']] as const).map(([id, label]) => <button type="button" key={id}
          aria-pressed={group === id && !query.trim()} disabled={busy || saving} onClick={() => changeGroup(id)}>{label}</button>)}
      </div>
      {replacement && <p className="arca-agent-hand-announcement">{`选择上方牌槽，替换为 ${replacement.name} · Esc 取消`}</p>}
      <ToastNotice kind={announcement.includes("未保存") ? "error" : "success"} message={announcement} />
      </div>
    </div>
    {entryTransition && <AgentEntryTransition {...entryTransition} onDone={onClose} />}
  </Modal>;
}
