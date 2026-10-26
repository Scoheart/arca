import { ImeInput, isImeKey } from "./ImeInput";
import { CSSProperties, ReactNode, type KeyboardEvent as ReactKeyboardEvent, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { SearchIcon, XIcon } from "./icons";
import { transportLabel, transportOf } from "../lib/mcp";
import type { RegistryEntry } from "../lib/types";
import { assetAvatarColor } from "../lib/assetAvatar";
import { resourceInitial } from "../lib/resourceInitial";

/* ── Avatar ──────────────────────────────────────────────────────────── */

export function Avatar({
  seed,
  label,
  mono,
  kind,
  icon,
  size = 40,
}: {
  seed: string;
  label?: string;
  mono?: boolean;
  kind?: "mcp" | "model" | "skill";
  icon?: ReactNode;
  size?: number;
}) {
  return (
    <div
      className="arca-asset-avatar flex-shrink-0 flex items-center justify-center text-white font-semibold select-none"
      data-kind={kind}
      data-has-icon={icon ? "true" : undefined}
      aria-hidden="true"
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.3),
        fontSize: Math.round(size * 0.4),
        background: assetAvatarColor(seed),
        color: "#fff",
        fontFamily: mono ? "var(--font-mono)" : undefined,
      }}
    >
      {icon ?? resourceInitial(label ?? seed)}
    </div>
  );
}

/* ── Badge ───────────────────────────────────────────────────────────── */

type Tone = "neutral" | "success" | "warning" | "info";

const TONE: Record<Tone, { bg: string; fg: string }> = {
  neutral: { bg: "var(--surface-quiet)", fg: "var(--text-secondary)" },
  success: { bg: "var(--surface-success)", fg: "var(--text-success)" },
  warning: { bg: "var(--surface-attention)", fg: "var(--text-warning)" },
  info: { bg: "var(--surface-selected)", fg: "var(--text-accent)" },
};

export function Badge({
  tone = "neutral",
  children,
  icon,
}: {
  tone?: Tone;
  children: ReactNode;
  icon?: ReactNode;
}) {
  const c = TONE[tone];
  return (
    <span
      className="arca-badge"
      style={{ background: c.bg, color: c.fg }}
    >
      {icon}
      {children}
    </span>
  );
}

/* ── IconButton ──────────────────────────────────────────────────────── */

export function IconButton({
  title,
  onClick,
  children,
  active,
  disabled,
}: {
  title: string;
  onClick?: (e: React.MouseEvent) => void;
  children: ReactNode;
  active?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      disabled={disabled}
      data-active={active ? "true" : undefined}
      className="arca-icon-btn"
      onClick={(e) => {
        e.stopPropagation();
        onClick?.(e);
      }}
    >
      {children}
    </button>
  );
}

/* ── Switch ──────────────────────────────────────────────────────────── */

/** A small macOS-style toggle. `checked` on → green; off → gray. */
export function Switch({
  checked,
  onChange,
  disabled,
  title,
  ariaLabel,
  compact = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  title?: string;
  ariaLabel?: string;
  compact?: boolean;
}) {
  const width = compact ? 30 : 36;
  const height = compact ? 18 : 22;
  const knob = compact ? 14 : 18;
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      title={title}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation();
        if (!disabled) onChange(!checked);
      }}
      className="arca-switch relative flex-shrink-0 rounded-full border-0 transition-colors"
      style={{
        width,
        height,
        padding: 0,
        background: checked ? "#34C759" : "var(--color-gray-200)",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <span
        className="absolute rounded-full"
        style={{
          top: 2,
          left: checked ? width - knob - 2 : 2,
          width: knob,
          height: knob,
          background: "#fff",
          boxShadow: "0 1px 2px rgba(0,0,0,0.25)",
          transition: "left 0.15s ease",
        }}
      />
    </button>
  );
}

/* ── SearchBar ───────────────────────────────────────────────────────── */

export function SearchBar({
  value,
  onChange,
  placeholder,
  style,
  autoFocus,
  ariaLabel,
  onKeyDown,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  style?: CSSProperties;
  autoFocus?: boolean;
  ariaLabel?: string;
  onKeyDown?: (event: ReactKeyboardEvent<HTMLInputElement>) => void;
}) {
  return (
    <div className="arca-search" style={style}>
      <SearchIcon
        className="arca-search-icon"
      />
      <ImeInput
        type="search"
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="none"
        spellCheck={false}
        placeholder={placeholder}
        aria-label={ariaLabel ?? placeholder ?? "搜索"}
        data-modal-initial-focus={autoFocus || undefined}
        onKeyDown={onKeyDown}
        value={value}
        autoFocus={autoFocus}
        onChange={(e) => onChange(e.target.value)}
      />
      <button
        type="button"
        className="arca-search-clear"
        data-visible={value ? "true" : undefined}
        disabled={!value}
        tabIndex={value ? 0 : -1}
        onClick={() => onChange("")}
        title="清除搜索"
        aria-label="清除搜索"
      >
        <XIcon className="w-3.5 h-3.5" />
      </button>
    </div>
  );
}

/* ── Modal ───────────────────────────────────────────────────────────── */

export const MODAL_DIALOG_SELECTOR = '[role="dialog"][aria-modal="true"]';

const handledLayerKeyboardEvents = new WeakSet<KeyboardEvent>();
const inertRootStates = new WeakMap<
  HTMLElement,
  {
    count: number;
    hadAttribute: boolean;
    propertyValue?: boolean;
  }
>();

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  '[contenteditable="true"]',
  '[tabindex]:not([tabindex="-1"])',
].join(",");

const mountedDialogs = new Set<HTMLElement>();
const hiddenModalLayers = new Map<HTMLElement, () => void>();

function topmostModal(): HTMLElement | null {
  return Array.from(document.querySelectorAll<HTMLElement>(MODAL_DIALOG_SELECTOR))
    .filter((dialog) => mountedDialogs.has(dialog)).at(-1) ?? null;
}

function syncModalLayers(): void {
  const top = topmostModal();
  const hidden = new Set<HTMLElement>();
  mountedDialogs.forEach((dialog) => {
    const overlay = dialog.closest<HTMLElement>("[data-modal-overlay]");
    if (dialog !== top && overlay) hidden.add(overlay);
  });
  hiddenModalLayers.forEach((release, overlay) => {
    if (!hidden.has(overlay)) { release(); hiddenModalLayers.delete(overlay); }
  });
  hidden.forEach((overlay) => {
    if (!hiddenModalLayers.has(overlay)) hiddenModalLayers.set(overlay, acquireElementInert(overlay));
  });
}

function modalElementVisible(element: HTMLElement, dialog: HTMLElement): boolean {
  if (element.closest('[hidden], [inert], [aria-hidden="true"]') || element.matches(":disabled")) return false;
  for (let current: HTMLElement | null = element; current && current !== dialog; current = current.parentElement) {
    if (current.tagName === "DETAILS" && !current.hasAttribute("open")
      && !current.querySelector(":scope > summary")?.contains(element)) return false;
    const style = window.getComputedStyle(current);
    if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse") return false;
  }
  return true;
}

function modalFocusableElements(dialog: HTMLElement, includeNotifications = true): HTMLElement[] {
  const elements = Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
  if (includeNotifications && topmostModal() === dialog) {
    document.querySelectorAll<HTMLElement>("[data-toast-viewport]").forEach((viewport) => {
      elements.push(...viewport.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
    });
  }
  return elements.filter(
    (element) => element.tabIndex >= 0 && modalElementVisible(element, dialog),
  );
}

function acquireElementInert(root: HTMLElement): () => void {
  let state = inertRootStates.get(root);
  if (!state) {
    state = {
      count: 0,
      hadAttribute: root.hasAttribute("inert"),
      propertyValue: "inert" in root ? root.inert : undefined,
    };
    inertRootStates.set(root, state);
  }
  state.count += 1;
  if ("inert" in root) root.inert = true;
  root.setAttribute("inert", "");

  return () => {
    const current = inertRootStates.get(root);
    if (!current) return;
    current.count -= 1;
    if (current.count > 0) return;

    if (typeof current.propertyValue === "boolean" && "inert" in root) {
      root.inert = current.propertyValue;
    }
    if (current.hadAttribute) root.setAttribute("inert", "");
    else root.removeAttribute("inert");
    inertRootStates.delete(root);
  };
}

export function acquireRootInert(): () => void {
  const root = document.getElementById("root");
  return root ? acquireElementInert(root) : () => undefined;
}

/** Keep names concise while distinguishing resources that share a display name. */
export function pickerOptionLabels(options: ReadonlyArray<{ id: string; name: string }>): Map<string, string> {
  const counts = new Map<string, number>();
  for (const option of options) counts.set(option.name, (counts.get(option.name) ?? 0) + 1);
  return new Map(options.map(({ id, name }) => [id, (counts.get(name) ?? 0) > 1 ? `${name}（${id}）` : name]));
}

/** Arrow keys move between candidates; Enter/Space still select explicitly. */
export function navigatePickerOptions(event: ReactKeyboardEvent<HTMLElement>): void {
  if (isImeKey(event.nativeEvent) || event.altKey || event.metaKey || event.ctrlKey) return;
  const search = event.target instanceof HTMLInputElement;
  if (search && !["ArrowDown", "ArrowUp"].includes(event.key)) return;
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  const dialog = event.currentTarget.closest(MODAL_DIALOG_SELECTOR);
  const options = Array.from(dialog?.querySelectorAll<HTMLButtonElement>("[data-picker-option]:not(:disabled)") ?? []);
  if (!options.length) return;
  event.preventDefault();
  const index = options.indexOf(document.activeElement as HTMLButtonElement);
  const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1
    : index < 0 ? event.key === "ArrowUp" ? options.length - 1 : 0
      : Math.max(0, Math.min(options.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
  options[next].focus();
}

/** Coordinate one Escape/Tab across independently mounted UI layers. */
export function claimLayerKeyboardEvent(event: KeyboardEvent): void {
  handledLayerKeyboardEvents.add(event);
}

export function wasHandledByLayer(event: KeyboardEvent): boolean {
  return handledLayerKeyboardEvents.has(event);
}

/** Portal-backed modal shell with explicit stack, focus, and inert semantics. */
export function Modal({
  width = 520,
  maxHeight = "82vh",
  borderRadius = "20px",
  ariaLabel = "对话框",
  busy = false,
  layer,
  onClose,
  children,
}: {
  width?: CSSProperties["width"];
  maxHeight?: CSSProperties["maxHeight"];
  borderRadius?: CSSProperties["borderRadius"];
  ariaLabel?: string;
  busy?: boolean;
  layer?: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const openerRef = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    const opener = openerRef.current;
    mountedDialogs.add(dialog);
    syncModalLayers();
    const releaseRootInert = acquireRootInert();
    const focusFrame = requestAnimationFrame(() => {
      if (topmostModal() !== dialog) return;
      const initialTarget =
        Array.from(dialog.querySelectorAll<HTMLElement>("[data-modal-initial-focus]"))
          .find((element) => modalElementVisible(element, dialog)) ??
        dialog.querySelector<HTMLElement>("[data-modal-title]") ??
        modalFocusableElements(dialog, false)[0] ??
        dialog;
      initialTarget.focus({ preventScroll: true });
    });

    const handleKeyDown = (event: KeyboardEvent) => {
      if (isImeKey(event) || handledLayerKeyboardEvents.has(event) || topmostModal() !== dialog) return;

      if (event.key === "Escape") {
        claimLayerKeyboardEvent(event);
        event.preventDefault();
        onCloseRef.current();
        return;
      }

      if (event.key !== "Tab") return;
      const focusable = modalFocusableElements(dialog);
      const active = document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
      const activeIndex = active ? focusable.indexOf(active) : -1;
      const shouldWrap = event.shiftKey
        ? activeIndex <= 0
        : activeIndex === -1 || activeIndex === focusable.length - 1;
      const nextIndex = event.shiftKey ? activeIndex - 1 : activeIndex + 1;
      const next = shouldWrap
        ? event.shiftKey ? focusable.at(-1) ?? dialog : focusable[0] ?? dialog
        : focusable[nextIndex];
      // Toast portals may precede or follow this modal in document order.
      // Explicitly bridge that boundary while leaving ordinary Tab native.
      const crossesPortal = Boolean(active && !dialog.contains(active)) || Boolean(next && !dialog.contains(next));
      if (!shouldWrap && !crossesPortal) return;

      claimLayerKeyboardEvent(event);
      event.preventDefault();
      next.focus();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      cancelAnimationFrame(focusFrame);
      document.removeEventListener("keydown", handleKeyDown);
      mountedDialogs.delete(dialog);
      syncModalLayers();
      releaseRootInert();
      requestAnimationFrame(() => {
        if (!opener?.isConnected) return;
        const remainingModal = topmostModal();
        if (!remainingModal || remainingModal.contains(opener)) opener.focus({ preventScroll: true });
      });
    };
  }, []);

  return createPortal(
    <div
      className="fixed inset-0 flex items-center justify-center z-40"
      data-modal-overlay="true"
      data-modal-layer={layer}
      style={{ background: "var(--surface-modal-scrim)", zIndex: 700 }}
      onClick={(event) => {
        if (event.target === event.currentTarget) event.stopPropagation();
        if (
          event.target !== event.currentTarget ||
          topmostModal() !== dialogRef.current
        ) return;
        onCloseRef.current();
      }}
    >
      <div
        ref={dialogRef}
        className="flex flex-col rounded-mac-lg overflow-hidden"
        role="dialog"
        aria-modal="true"
        aria-busy={busy || undefined}
        aria-label={ariaLabel}
        data-modal-layer={layer}
        tabIndex={-1}
        style={{
          width,
          maxHeight,
          background: "var(--surface-overlay)",
          border: 0,
          borderRadius,
          boxShadow: "var(--shadow-sheet)",
        }}
        onClick={(event) => event.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

/** Standard form-dialog header: brand-gradient glyph tile + title + subtitle + ✕. */
export function ModalHeader({
  glyph,
  title,
  subtitle,
  onClose,
}: {
  glyph: ReactNode;
  title: string;
  subtitle: ReactNode;
  onClose: () => void;
}) {
  return (
    <div className="flex items-start gap-4 px-6 py-5">
      <div
        className="w-11 h-11 rounded-mac flex-shrink-0 flex items-center justify-center text-2xl font-semibold leading-none"
        style={{ background: "var(--surface-selected)", color: "var(--text-accent)" }}
      >
        {glyph}
      </div>
      <div className="flex-1 min-w-0">
        <h2
          className="text-base font-semibold m-0 mb-1"
          data-modal-title
          tabIndex={-1}
          style={{ color: "var(--text-primary)" }}
        >
          {title}
        </h2>
        <p className="text-xs m-0 leading-relaxed" style={{ color: "var(--text-secondary)" }}>
          {subtitle}
        </p>
      </div>
      <button
        type="button"
        onClick={onClose}
        title="关闭"
        aria-label="关闭"
        className="flex-shrink-0 w-7 h-7 rounded-full flex items-center justify-center border-0 cursor-pointer mt-0.5"
        style={{ background: "var(--border-hairline)", color: "var(--text-secondary)" }}
      >
        <XIcon className="w-3.5 h-3.5" />
      </button>
    </div>
  );
}

/* ── TransportPill ───────────────────────────────────────────────────── */

/** Gray mono pill showing an entry's transport (stdio / http / sse / custom).
 *  `compact` is the tighter list-row variant. */
export function TransportPill({ entry, compact }: { entry: RegistryEntry; compact?: boolean }) {
  const full = transportLabel(entry);
  // Long/custom http types (e.g. "streamable-http") collapse to the transport
  // bucket so the pill stays short and card meta rows don't wrap; the exact type
  // is kept in the tooltip.
  const label = full.length <= 5 ? full : transportOf(entry);
  return (
    <span
      title={full}
      className="arca-transport-pill"
      data-compact={compact ? "true" : undefined}
    >
      {label}
    </span>
  );
}

/* ── Sticky glass header ─────────────────────────────────────────────── */

/** Shared style for the views' sticky glass page headers. */
export const stickyHeaderStyle: CSSProperties = {
  background: "var(--header-bg)",
  backdropFilter: "blur(var(--glass-blur)) saturate(var(--glass-saturate))",
  WebkitBackdropFilter: "blur(var(--glass-blur)) saturate(var(--glass-saturate))",
  borderBottom: "1px solid color-mix(in srgb, var(--glass-border) 55%, transparent)",
};
