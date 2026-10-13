import { useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";
import type { AgentInfo } from "../lib/types";
import { AgentGlyph } from "./brandIcons";

export interface AgentEntryOrigin {
  left: number;
  top: number;
  width: number;
  height: number;
  rotation: number;
}

/** A visual bridge only: navigation and Agent loading have already started. */
export function AgentEntryTransition({ agent, origin, overlay, onDone }: {
  agent: AgentInfo;
  origin: AgentEntryOrigin;
  overlay: HTMLElement | null;
  onDone(): void;
}) {
  const frameRef = useRef<HTMLDivElement>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const glyphRef = useRef<HTMLSpanElement>(null);
  const nameRef = useRef<HTMLElement>(null);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;

  useLayoutEffect(() => {
    const frame = frameRef.current, surface = surfaceRef.current;
    const glyph = glyphRef.current, name = nameRef.current;
    if (!frame || !surface || !glyph || !name) { doneRef.current(); return; }
    let active = true, ended = false;
    let header: HTMLElement | undefined;
    let originalVisibility = "";
    let hiddenHeader = false;
    const originalOverlayOpacity = overlay?.style.opacity ?? "";
    const motions = new Set<Animation>();
    const play = (element: HTMLElement, frames: Keyframe[], duration: number, delay = 0) => {
      const motion = element.animate(frames, { duration, delay, easing: "cubic-bezier(.2,.8,.2,1)", fill: "both" });
      motions.add(motion);
      return motion.finished.catch(() => undefined);
    };
    const restore = () => {
      if (header && hiddenHeader) header.style.visibility = originalVisibility;
      motions.forEach((motion) => motion.cancel()); motions.clear();
    };
    const finish = () => {
      if (!active || ended) return;
      ended = true;
      // Cancelling a filled opacity animation restores the opaque scrim.
      // Hold its terminal state until React removes the modal from the DOM.
      if (overlay) overlay.style.opacity = "0";
      frame.style.visibility = "hidden";
      restore();
      doneRef.current();
    };
    const onVisibility = () => { if (document.hidden) finish(); };
    // WebKit can leave Animation.finished pending when its compositor pauses.
    // Navigation has already completed; decorative motion must not hold the UI.
    const completionDeadline = window.setTimeout(finish, 1000);
    window.addEventListener("resize", finish);
    document.addEventListener("visibilitychange", onVisibility);

    const run = async () => {
      frame.style.visibility = "";
      if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) { finish(); return; }
      const zoom = Math.min(1.9, (window.innerWidth - 64) / origin.width, (window.innerHeight - 96) / origin.height);
      const centerX = (window.innerWidth - origin.width) / 2;
      const centerY = (window.innerHeight - origin.height) / 2 - 10;
      const center = `translate3d(${centerX}px,${centerY}px,0) rotate(0deg) scale(${zoom})`;
      // The card's center stays under the same point while it turns face-on.
      await play(frame, [
        { transform: `translate3d(${origin.left}px,${origin.top}px,0) rotate(${origin.rotation}deg) scale(1)` },
        { transform: center },
      ], 190);
      if (!active || ended) return;

      header = Array.from(document.querySelectorAll<HTMLElement>("[data-agent-entry-id]"))
        .find((element) => element.dataset.agentEntryId === agent.id);
      const destinationGlyph = header?.querySelector<HTMLElement>("[data-agent-entry-glyph]");
      const destinationName = header?.querySelector<HTMLElement>("[data-agent-entry-name]");
      if (!header || !destinationGlyph || !destinationName || !header.getClientRects().length) {
        // Never extend the transition to wait for a lazy module or backend I/O.
        await Promise.all([
          play(frame, [{ opacity: 1, transform: center }, { opacity: 0, transform: `translate3d(${centerX}px,${centerY - 12}px,0) scale(${zoom * 1.04})` }], 180),
          overlay ? play(overlay, [{ opacity: 1 }, { opacity: 0 }], 180) : Promise.resolve(),
        ]);
        finish(); return;
      }

      // Read all endpoints before hiding the real title or changing the proxy.
      const glyphFrom = glyph.getBoundingClientRect(), glyphTo = destinationGlyph.getBoundingClientRect();
      const nameFrom = name.getBoundingClientRect(), nameTo = destinationName.getBoundingClientRect();
      const frameFrom = frame.getBoundingClientRect();
      const targetNameStyle = getComputedStyle(destinationName);
      const nameScale = parseFloat(targetNameStyle.fontSize) / (11 * zoom);
      originalVisibility = header.style.visibility;
      header.style.visibility = "hidden";
      hiddenHeader = true;
      name.style.textAlign = "left";
      name.style.width = `${nameTo.width / (zoom * nameScale)}px`;
      name.style.fontWeight = targetNameStyle.fontWeight;
      name.style.lineHeight = `${parseFloat(targetNameStyle.lineHeight) / (zoom * nameScale) || 14}px`;
      const glyphTarget = `translate(${(glyphTo.left - glyphFrom.left) / zoom}px,${(glyphTo.top - glyphFrom.top) / zoom}px) scale(${glyphTo.width / glyphFrom.width})`;
      const nameTarget = `translate(${(nameTo.left - nameFrom.left) / zoom}px,${(nameTo.top - nameFrom.top) / zoom}px) scale(${nameScale})`;
      const surfaceTarget = `translate(${(glyphTo.left - frameFrom.left) / zoom}px,${(glyphTo.top - frameFrom.top) / zoom}px) scale(${glyphTo.width / frameFrom.width})`;
      // The real content is already mounted behind the scrim. Reveal it by
      // fading the scrim only; resetting those sections to opacity 0 flashes.
      await Promise.all([
        play(glyph, [{ transform: "none" }, { transform: glyphTarget }], 240),
        play(name, [{ transform: "none" }, { transform: nameTarget }], 240),
        play(surface, [{ transform: "none", opacity: 1 }, { transform: surfaceTarget, opacity: 0 }], 240),
        overlay ? play(overlay, [{ opacity: 1 }, { opacity: 0 }], 240) : Promise.resolve(),
      ]);
      finish();
    };
    // Cancellation and unavailable animation APIs must never strand the modal.
    void run().catch(finish);
    return () => {
      window.clearTimeout(completionDeadline);
      active = false; restore();
      // Layout cleanup runs inside the same commit that removes the modal,
      // so restoring borrowed styles here cannot paint an opaque extra frame.
      if (overlay) overlay.style.opacity = originalOverlayOpacity;
      window.removeEventListener("resize", finish);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [agent.id, origin, overlay]);

  return createPortal(<div className="arca-agent-entry-layer" aria-hidden="true">
    <div ref={frameRef} className="arca-agent-entry-card" style={{ width: origin.width, height: origin.height }}>
      <div ref={surfaceRef} className="arca-agent-entry-surface" />
      <span ref={glyphRef} className="arca-agent-entry-glyph"><AgentGlyph id={agent.id} name={agent.name} size={30} /></span>
      <strong ref={nameRef} className="arca-agent-entry-name">{agent.name}</strong>
    </div>
  </div>, document.body);
}
