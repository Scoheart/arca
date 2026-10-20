import type { ReactNode } from "react";
import "./AgentSurfaceBadge.css";

export const AGENT_SURFACE_TYPES = ["cli", "desktop", "ide", "plugin", "web"] as const;
export type AgentSurface = typeof AGENT_SURFACE_TYPES[number];

// Simple, distinct silhouettes remain readable at pinned-bar sizes.
const SYMBOLS: Record<AgentSurface, ReactNode> = {
  cli: <path d="m3.5 3.5 4 4-4 4M8.5 12h4" />,
  desktop: <><rect x="2" y="2.5" width="12" height="8" rx="1.5" /><path d="M8 10.5v3M5 13.5h6" /></>,
  ide: <path d="m5 4-3 4 3 4m6-8 3 4-3 4M9 3 7 13" />,
  plugin: <path d="M5.5 2v3m5-3v3M3.5 5h9v3a4.5 4.5 0 0 1-9 0ZM8 12.5V14" />,
  web: <><circle cx="8" cy="8" r="6" /><path d="M2 8h12M8 2c3 3.5 3 8.5 0 12-3-3.5-3-8.5 0-12Z" /></>,
};

function badgeSize(size: number): number {
  if (size < 20) return 10;
  if (size <= 24) return 12;
  if (size <= 36) return 15;
  if (size <= 52) return 19;
  return 24;
}

/** Decorative: AgentGlyph supplies the localized surface name and tooltip. */
export function AgentSurfaceBadge({ surface, size }: { surface: AgentSurface; size: number }) {
  const pixels = badgeSize(size);
  const offset = size <= 36 ? -2 : -3;
  return (
    <span className="arca-agent-surface-badge" data-agent-surface={surface} aria-hidden="true"
      style={{ width: pixels, height: pixels, right: offset, bottom: offset, borderRadius: Math.round(pixels * 0.28) }}>
      <svg viewBox="0 0 16 16" fill="none" focusable="false">{SYMBOLS[surface]}</svg>
    </span>
  );
}
