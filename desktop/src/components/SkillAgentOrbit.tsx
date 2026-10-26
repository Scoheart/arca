import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { AgentGlyph, agentName } from "./brandIcons";
import { acquireRootInert, claimLayerKeyboardEvent, wasHandledByLayer } from "./ui";
import { agentHandMetrics } from "../lib/agentHandLayout";

type Point = { x: number; y: number };
type Pose = Point & { scale: number; opacity: number; rotation: number };
type Size = { width: number; height: number; cardHeight: number };

function handLayout(size: Size, cardWidth: number, count: number) {
  const middle = (count - 1) / 2;
  // One compact hand: overlap the cards, with a shallow arc and outward tilt.
  const { step, spread, rise } = agentHandMetrics(count, cardWidth);
  const handWidth = Math.max(cardWidth, (count - 1) * step + cardWidth + size.cardHeight * .5);
  const scale = Math.max(.1, Math.min(1, (size.width - 48) / handWidth,
    (size.height - 80) / (size.cardHeight + rise + 100)));
  return {
    scale, contentHeight: size.height,
    points: Array.from({ length: count }, (_, index) => {
      const offset = middle === 0 ? 0 : (index - middle) / middle;
      return {
        x: size.width / 2 + (index - middle) * step * scale - cardWidth / 2,
        y: size.height / 2 + (offset * offset * rise - rise / 2) * scale - size.cardHeight / 2,
        rotation: offset * spread,
      };
    }),
  };
}

function transform(pose: Pose) {
  return `translate3d(${pose.x}px, ${pose.y}px, 0) rotate(${pose.rotation}deg) scale(${pose.scale})`;
}

function readPose(card: HTMLElement): Pose {
  const style = getComputedStyle(card);
  const matrix = new DOMMatrixReadOnly(style.transform === "none" ? undefined : style.transform);
  return { x: matrix.m41, y: matrix.m42, scale: Math.hypot(matrix.a, matrix.b),
    rotation: Math.atan2(matrix.b, matrix.a) * 180 / Math.PI, opacity: Number(style.opacity) };
}

/** Floating cards with background input blocked until the return flight completes. */
export function SkillAgentOrbit({ ids, names, skillName, sourceRef, onExit }: {
  ids: string[];
  names: ReadonlyMap<string, string>;
  skillName?: string;
  sourceRef: RefObject<HTMLDivElement | null>;
  onExit: (id?: string) => void;
}) {
  const { t } = useTranslation();
  const stageRef = useRef<HTMLDivElement>(null);
  const cardsLayerRef = useRef<HTMLDivElement>(null);
  const cardRefs = useRef(new Map<string, HTMLButtonElement>());
  const positioned = useRef(false);
  const motionsRef = useRef<Animation[]>([]);
  const navigating = useRef(false);
  const keyboardSelection = useRef(false);
  const exitRef = useRef(onExit);
  exitRef.current = onExit;
  const [closing, setClosing] = useState(false);
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const playedMotion = useRef<Animation | null>(null);
  const [size, setSize] = useState<Size>({ width: window.innerWidth, height: window.innerHeight, cardHeight: 104 });
  const cardWidth = 76;
  const layout = useMemo(() => handLayout(size, cardWidth, ids.length), [size, cardWidth, ids.length]);

  useLayoutEffect(() => () => {
    motionsRef.current.forEach((animation) => animation.cancel());
    motionsRef.current = [];
    playedMotion.current?.cancel();
    // StrictMode replays mount effects. The replay must deal the cards again,
    // not treat their cancelled destination styles as an already-open hand.
    positioned.current = false;
  }, []);

  useLayoutEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const measure = () => {
      const cardHeight = Math.max(104, ...Array.from(cardRefs.current.values(), (card) => card.offsetHeight));
      const width = stage.clientWidth;
      const height = stage.clientHeight;
      setSize((old) => old.width === width && old.height === height && old.cardHeight === cardHeight
        ? old : { width, height, cardHeight });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(stage);
    cardRefs.current.forEach((card) => observer.observe(card));
    return () => observer.disconnect();
  }, [ids, names, cardWidth]);

  useLayoutEffect(() => {
    const layer = cardsLayerRef.current;
    if (!layer) return;
    const bounds = layer.getBoundingClientRect();
    const sources = Array.from(sourceRef.current?.querySelectorAll<HTMLElement>(".arca-skill-agent-card") ?? []);
    const sourceRects = sources.map((source) => source.getBoundingClientRect());
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const first = !positioned.current;
    let active = true;
    const animations: Animation[] = [];
    // Read all current transforms before writing any destination styles.
    const current = ids.map((id) => {
      const card = cardRefs.current.get(id);
      return card && !first && card.style.transform ? readPose(card) : null;
    });
    // Keep an interrupted flight at its current position before starting the return trip.
    motionsRef.current.forEach((animation) => animation.cancel());
    ids.forEach((id, i) => {
      const card = cardRefs.current.get(id);
      if (!card) return;
      const source = sourceRects[sourceRects.length - 1];
      const origin: Pose = {
        x: (source ? source.left + source.width / 2 : size.width / 2) - bounds.left - cardWidth / 2,
        y: (source ? source.top + source.height / 2 : size.height / 2) - bounds.top - card.offsetHeight / 2,
        scale: source ? source.width / cardWidth : .25,
        rotation: -14,
        opacity: 0,
      };
      const target: Pose = { ...layout.points[i],
        y: layout.points[i].y + (size.cardHeight - card.offsetHeight) / 2,
        scale: layout.scale, rotation: layout.points[i].rotation, opacity: 1 };
      // Every new card is dealt from the same pile, and stays hidden until its turn.
      const from = current[i] ?? { ...origin, rotation: -14, opacity: 0 };
      const to = closing ? origin : target;
      card.style.transform = transform(to);
      card.style.opacity = String(to.opacity);
      card.disabled = closing || navigating.current || (first && !reduced);
      if (reduced) return;
      // Deal and collect follow the same curve in opposite directions.
      const pileControl = { x: origin.x + (target.x - origin.x) * .22, y: origin.y - 48 };
      const handControl = { x: target.x - 24, y: target.y + 36 };
      const control1 = closing ? handControl : pileControl;
      const control2 = closing ? pileControl : handControl;
      const frames = Array.from({ length: 13 }, (_, step) => {
        const progress = step / 12;
        const q = 1 - progress;
        const pose: Pose = {
          x: q ** 3 * from.x + 3 * q * q * progress * control1.x
            + 3 * q * progress * progress * control2.x + progress ** 3 * to.x,
          y: q ** 3 * from.y + 3 * q * q * progress * control1.y
            + 3 * q * progress * progress * control2.y + progress ** 3 * to.y,
          scale: from.scale + (to.scale - from.scale) * progress,
          rotation: from.rotation * q + to.rotation * progress - Math.sin(progress * Math.PI) * 8,
          opacity: closing ? from.opacity * Math.min(1, (1 - progress) * 12)
            : first ? (step === 0 ? 0 : 1) : from.opacity + (to.opacity - from.opacity) * progress,
        };
        return { transform: transform(pose), opacity: pose.opacity, offset: progress };
      });
      const animation = card.animate(frames, {
        duration: closing ? 180 : first ? 260 : 240,
        delay: first ? i * 35 : closing ? (ids.length - i - 1) * 24 : 0,
        easing: "cubic-bezier(.22,.62,.3,1)", fill: "backwards",
      });
      animation.onfinish = () => {
        if (!active || closing || navigating.current) return;
        card.disabled = false;
      };
      animations.push(animation);
    });
    motionsRef.current = animations;
    positioned.current = true;
    if (closing) void Promise.allSettled(animations.map((animation) => animation.finished)).then(() => {
      if (active) exitRef.current();
    });
    return () => { active = false; };
  }, [layout, closing, cardWidth, ids, sourceRef, size.width, size.height]);

  const close = () => {
    if (closing || navigating.current) return;
    setClosing(true);
  };

  const openAgent = (id: string) => {
    if (closing || navigating.current) return;
    navigating.current = true;
    setPlaying(id);
    const card = cardRefs.current.get(id);
    const tile = card?.querySelector<HTMLElement>(".arca-skill-agent-orbit-tile");
    if (!tile || window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      exitRef.current(id);
      return;
    }
    const rotation = layout.points[ids.indexOf(id)]?.rotation ?? 0;
    const animation = tile.animate([
      { transform: getComputedStyle(tile).transform, opacity: 1 },
      { transform: `translateY(-105px) rotate(${-rotation}deg) scale(1.12)`, opacity: 0 },
    ], { duration: 160, easing: "cubic-bezier(.3,.05,.7,.5)", fill: "forwards" });
    playedMotion.current = animation;
    animation.onfinish = () => exitRef.current(id);
  };

  const closeRef = useRef(close);
  closeRef.current = close;
  useLayoutEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const opener = sourceRef.current?.querySelector<HTMLElement>(".arca-skill-agent-more");
    const releaseRootInert = acquireRootInert();
    const frame = requestAnimationFrame(() => {
      // Focus the modal, not a card: opening the hand has no selected Agent.
      stage.focus({ preventScroll: true });
    });
    const keydown = (event: KeyboardEvent) => {
      if (wasHandledByLayer(event) || !["Escape", "Tab", "ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
      claimLayerKeyboardEvent(event);
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") { closeRef.current(); return; }
      keyboardSelection.current = true;
      const available = Array.from(cardRefs.current.values()).filter((card) => !card.disabled);
      const index = available.findIndex((card) => card === document.activeElement);
      const backwards = event.key === "ArrowLeft" || (event.key === "Tab" && event.shiftKey);
      const next = event.key === "Home" ? 0 : event.key === "End" ? available.length - 1 : backwards
        ? (index <= 0 ? available.length - 1 : index - 1)
        : (index + 1) % available.length;
      const nextCard = available[next];
      setHighlighted(nextCard ? ids.find((id) => cardRefs.current.get(id) === nextCard) ?? null : null);
      (nextCard ?? stage).focus({ preventScroll: true });
    };
    document.addEventListener("keydown", keydown, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", keydown, true);
      releaseRootInert();
      requestAnimationFrame(() => {
        if (!navigating.current && opener?.isConnected && !opener.closest("[inert]")) {
          opener.focus({ preventScroll: true });
        }
      });
    };
  }, [sourceRef, ids]);

  return createPortal(
    <div ref={stageRef} className="arca-skill-agent-orbit" role="dialog" aria-modal="true" tabIndex={-1}
      aria-label={[skillName, t("skillLibrary.allAgents")].filter(Boolean).join(" · ")}
      data-closing={closing || undefined}
      data-playing={playing ? "true" : undefined}
      onPointerMove={(event) => {
        if (closing || navigating.current || (!event.movementX && !event.movementY)) return;
        keyboardSelection.current = false;
        // Select using stable fan slots, not the raised card's changing hit box.
        // This lets the cursor sweep across every overlapping card without flicker.
        let nearest = -1;
        let distance = Infinity;
        layout.points.forEach((point, index) => {
          const dx = Math.abs(event.clientX - point.x - cardWidth / 2);
          if (dx < distance) { nearest = index; distance = dx; }
        });
        const point = layout.points[nearest];
        const within = point && distance < cardWidth * layout.scale * .7
          && Math.abs(event.clientY - point.y - size.cardHeight / 2) < (size.cardHeight / 2 + 32) * layout.scale;
        const next = within && !cardRefs.current.get(ids[nearest])?.disabled ? ids[nearest] : null;
        setHighlighted((current) => current === next ? current : next);
      }}
      onPointerLeave={() => { if (!keyboardSelection.current) setHighlighted(null); }}
      onPointerDown={(event) => { event.stopPropagation(); }}
      onKeyDown={(event) => { event.stopPropagation(); }}
      onClick={(event) => {
        event.stopPropagation();
        if (!(event.target instanceof Element) || !event.target.closest("button")) close();
      }}>
      <div className="arca-skill-agent-orbit-scroll">
        <div ref={cardsLayerRef} className="arca-skill-agent-orbit-cards" style={{ height: layout.contentHeight }}>
          {ids.map((id, index) => {
            const name = agentName(id, names.get(id));
            return <button key={id} type="button" className="arca-skill-agent-orbit-card"
              ref={(node) => { if (node) cardRefs.current.set(id, node); else cardRefs.current.delete(id); }}
              style={{ width: cardWidth, "--fan-angle": `${layout.points[index].rotation}deg`, "--hand-index": index } as CSSProperties}
              data-playing={playing === id ? "true" : undefined}
              data-highlighted={highlighted === id ? "true" : undefined}
              onFocus={() => { if (keyboardSelection.current) setHighlighted(id); }}
              onBlur={() => setHighlighted((current) => current === id ? null : current)}
              disabled={closing || playing !== null} title={name} aria-label={t("skillLibrary.openAgent", { name })}
              onClick={(event) => openAgent(event.detail > 0 ? highlighted ?? id : id)}>
              <span className="arca-skill-agent-orbit-tile">
                <AgentGlyph id={id} name={name} size={28} />
                <span className="arca-skill-agent-orbit-name">{name}</span>
              </span>
            </button>;
          })}
        </div>
      </div>
    </div>,
    document.body,
  );
}
