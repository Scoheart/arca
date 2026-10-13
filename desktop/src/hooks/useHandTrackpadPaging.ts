import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface NativeScroll {
  gesture: number;
  phase: number;
  momentum: boolean;
  deltaX: number;
  deltaY: number;
  x: number;
  y: number;
}

/** macOS phase boundaries, not wheel timing, define one physical swipe. */
export function useHandTrackpadPaging(
  surface: RefObject<HTMLDivElement | null>,
  enabled: boolean,
  onPage: (direction: -1 | 1) => void,
) {
  const latest = useRef({ enabled, onPage });
  useLayoutEffect(() => { latest.current = { enabled, onPage }; });

  useEffect(() => {
    const overlay = surface.current?.closest<HTMLElement>("[data-modal-overlay]");
    if (!overlay) return;
    let active = true;
    let mode: "pending" | "native" | "wheel" = isTauri() ? "pending" : "wheel";
    let unlisten: UnlistenFn | undefined;
    let gesture: number | undefined;
    let admitted = false;
    let consumed = false;
    let x = 0, y = 0;
    let lastFallbackAt = 0;
    const accepts = (target: Element | null) => Boolean(target && overlay.contains(target) &&
      target.closest("[data-modal-overlay]") === overlay &&
      !target.closest(".arca-agent-hand-topbar, .arca-agent-hand-footer, input, textarea, select"));
    const reset = () => { consumed = false; x = 0; y = 0; };
    const move = (dx: number, dy: number) => {
      if (consumed || !latest.current.enabled) return;
      x += dx; y += dy;
      if (Math.abs(y) >= 32 && Math.abs(y) > Math.abs(x)) { consumed = true; return; }
      if (Math.abs(x) < 32 || Math.abs(x) <= Math.abs(y)) return;
      // Stay consumed through finger-up AND the entire momentum tail. Only
      // the next native Began/gesture ID unlocks; there is no time cooldown.
      consumed = true;
      latest.current.onPage(x > 0 ? 1 : -1);
    };
    const fallback = (dx: number, dy: number) => {
      const now = performance.now();
      if (now - lastFallbackAt > 160) reset();
      lastFallbackAt = now;
      move(dx, dy);
    };
    const onNative = (event: NativeScroll) => {
      if (!active || mode !== "native" || event.momentum || !latest.current.enabled) return;
      if (event.phase & 32) return; // MayBegin is not a new physical swipe yet.
      const target = document.elementFromPoint(event.x * window.innerWidth, event.y * window.innerHeight);
      if (event.phase === 0) {
        // Conventional mouse wheels have no AppKit gesture phases.
        if (accepts(target)) fallback(event.deltaX, event.deltaY);
        return;
      }
      if (gesture !== event.gesture || (event.phase & 1)) {
        gesture = event.gesture;
        reset();
        admitted = accepts(target);
      }
      if (event.phase & 16) { consumed = true; return; } // Cancelled.
      if (admitted) move(event.deltaX, event.deltaY);
      if (event.phase & 8) consumed = true; // Ended: wait for next Began.
    };
    if (isTauri()) {
      void (async () => {
        try {
          const stop = await listen<NativeScroll>("arca-trackpad-scroll", ({ payload }) => onNative(payload));
          if (!active) { stop(); return; }
          unlisten = stop;
          const available = await invoke<boolean>("trackpad_gestures_available");
          if (active) mode = available ? "native" : "wheel";
        } catch {
          if (active) mode = "wheel";
        }
      })();
    }
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey || !latest.current.enabled ||
          !accepts(event.target instanceof Element ? event.target : null)) return;
      if (Math.abs(event.deltaX) <= Math.abs(event.deltaY)) return;
      if (event.cancelable) event.preventDefault();
      // Native events own paging on macOS. Never count the DOM copy twice.
      if (mode !== "wheel") return;
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? overlay.clientWidth : 1;
      fallback(event.deltaX * unit, event.deltaY * unit);
    };
    overlay.addEventListener("wheel", onWheel, { passive: false, capture: true });
    return () => {
      active = false;
      unlisten?.();
      overlay.removeEventListener("wheel", onWheel, true);
    };
  }, [surface]);
}
