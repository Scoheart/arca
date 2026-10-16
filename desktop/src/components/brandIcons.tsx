import iconAliases from "../assets/agents/aliases.json";
import agentSurfaces from "../assets/agents/surfaces.json";
import builtinAgents from "../../../data/agents.json";
import catalogAgents from "../../../data/agent-catalog.json";
import type { ReactNode } from "react";

type AgentSurface = "cli" | "desktop" | "ide" | "plugin" | "web";

const iconModules = import.meta.glob("../assets/agents/*.{png,svg,webp,ico}", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

const LOGOS = Object.fromEntries(
  Object.entries(iconModules).map(([path, url]) => [path.split("/").pop()!.replace(/\.[^.]+$/, ""), url])
) as Record<string, string>;
const ICON_ALIASES: Record<string, string> = iconAliases;
const SURFACE_VALUES = new Set<AgentSurface>(["cli", "desktop", "ide", "plugin", "web"]);
const AGENT_SURFACES: Record<string, string> = agentSurfaces;

function resolvedLogoKey(id: string): string {
  return ICON_ALIASES[id] ?? id;
}

/** Availability follows bundled assets, so adding a logo restores visibility. */
export function hasAgentIcon(id: string): boolean {
  return Boolean(LOGOS[resolvedLogoKey(id)]);
}

/** Presentation only: never remove a hidden Agent's stored configuration. */
export function isAgentVisible(id: string): boolean {
  return (!Object.hasOwn(builtinAgents, id) && !Object.hasOwn(catalogAgents, id)) || hasAgentIcon(id);
}

/** Runtime provenance also covers older persisted built-ins and custom catalog-ID overrides. */
export function isAgentEntryVisible(agent: { id: string; builtin: boolean }): boolean {
  return !agent.builtin || hasAgentIcon(agent.id);
}

function declaredSurface(id: string): AgentSurface | null {
  const value = AGENT_SURFACES[id];
  return SURFACE_VALUES.has(value as AgentSurface) ? value as AgentSurface : null;
}

const COLLIDING_LOGO_KEYS = (() => {
  const surfacesByLogo = new Map<string, Set<AgentSurface>>();
  for (const id of Object.keys(AGENT_SURFACES)) {
    const surface = declaredSurface(id);
    const logoKey = resolvedLogoKey(id);
    if (!surface || !LOGOS[logoKey]) continue;
    const surfaces = surfacesByLogo.get(logoKey) ?? new Set<AgentSurface>();
    surfaces.add(surface);
    surfacesByLogo.set(logoKey, surfaces);
  }
  return new Set(
    [...surfacesByLogo.entries()]
      .filter(([, surfaces]) => surfaces.size > 1)
      .map(([logoKey]) => logoKey),
  );
})();

function visibleSurface(id: string): AgentSurface | null {
  const logoKey = resolvedLogoKey(id);
  if (!COLLIDING_LOGO_KEYS.has(logoKey)) return null;
  return declaredSurface(id);
}

/** Logos that are complete app icons (own background + rounded corners), so they
 *  render edge-to-edge instead of as a mark centered on a white tile. */
const FULL_BLEED = new Set<string>([
  "boltai",
  "capy",
  "cline",
  "cline-desktop",
  "codebuddy-code",
  "workbuddy",
  "workbuddy-cn",
  "factory-desktop",
  "factory-droid",
  "firebender",
  "freebuff",
  "hermes",
  "kilo-code",
  "kilo-desktop",
  "kimi-code",
  "kiro",
  "lmstudio",
  "minimax-code",
  "openhands",
  "pi",
  "qoder",
  "qoder-cli",
  "qoder-desktop",
  "qoderwork",
  "raycast",
  "cortex-code",
  "roo-code",
  "rovo-dev",
  "warp",
]);

const THEMED_MARKS = new Set<string>(["augment"]);
const WIDE_TILES: Record<string, string> = { crush: "#654cff" };

/** Brand colours only; Agent names come from the canonical data definitions. */
const AGENT_COLORS: Record<string, string> = {
  "claude-code": "#D97757",
  "claude-desktop": "#C15F3C",
  cursor: "#111827",
  "cursor-cli": "#111827",
  "codebuddy-code": "#7257FF",
  "codebuddy-ide": "#7257FF",
  workbuddy: "#7257FF",
  "workbuddy-cn": "#7257FF",
  vscode: "#0A7ACA",
  codex: "#10A37F",
  "codex-desktop": "#10A37F",
  zed: "#084CCF",
  zcode: "#356DFF",
  windsurf: "#09B6A2",
  "roo-code": "#6C47FF",
  gemini: "#4285F4",
  "grok-build": "#111111",
  "minimax-code": "#75B9EA",
  qoder: "#6E56CF",
  "qoder-desktop": "#11100E",
  "qoder-cli": "#6E56CF",
  qoderwork: "#25D959",
  devin: "#1F2937",
  kiro: "#7C3AED",
  junie: "#E5484D",
  "amazon-q": "#FF9900",
  "cline-desktop": "#27313B",
  opencode: "#1F2937",
  "opencode-desktop": "#1F2937",
  "copilot-cli": "#24292E",
  cline: "#2563EB",
  freebuff: "#111111",
  continue: "#111827",
  warp: "#00B4C6",
  pi: "#8B5CF6",
};

const AGENT_DEFINITIONS: Record<string, { name?: string }> = {
  ...catalogAgents,
  ...builtinAgents,
};

export function agentName(id: string, explicitName?: string): string {
  return explicitName || AGENT_DEFINITIONS[id]?.name || id;
}

const FALLBACK_COLORS = ["#3568D4", "#16856B", "#B84A62", "#9A6618", "#5E55B8", "#277B91"];

function fallbackColor(id: string): string {
  let hash = 0;
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return FALLBACK_COLORS[hash % FALLBACK_COLORS.length];
}

function surfaceBadgeSize(size: number): number {
  if (size <= 24) return 10;
  if (size <= 36) return 12;
  return 14;
}

function AgentSurfaceBadge({ surface, size }: { surface: AgentSurface; size: number }) {
  const badgeSize = surfaceBadgeSize(size);
  return (
    <span
      className="arca-agent-surface-badge"
      data-agent-surface={surface}
      aria-hidden="true"
      style={{ width: badgeSize, height: badgeSize }}
    >
      <svg viewBox="0 0 12 12" fill="none" focusable="false">
        {surface === "cli" && (
          <>
            <path d="M2.25 3.25 4.5 5.5 2.25 7.75" />
            <path d="M5.5 8h4" />
          </>
        )}
        {surface === "desktop" && (
          <>
            <rect x="1.5" y="2" width="9" height="6.75" rx="1.25" />
            <path d="M4 10h4" />
          </>
        )}
        {surface === "ide" && (
          <>
            <rect x="1.5" y="1.75" width="9" height="8.5" rx="1.25" />
            <path d="M5 2v8M6.75 4h2M6.75 6h2" />
          </>
        )}
        {surface === "plugin" && (
          <>
            <path d="M4.25 1.75v2.1M6.75 1.75v2.1" />
            <rect x="2.25" y="3.75" width="6.5" height="4.25" rx="1" />
            <path d="M8.75 5.15H10.5M8.75 6.65H10.5" />
          </>
        )}
        {surface === "web" && (
          <>
            <circle cx="6" cy="6" r="4.5" />
            <path d="M1.5 6h9M6 1.5c2.25 2.5 2.25 6.5 0 9-2.25-2.5-2.25-6.5 0-9Z" />
          </>
        )}
      </svg>
    </span>
  );
}

/**
 * Square brand badge for an agent: the real logo on a white tile when available,
 * otherwise a brand-coloured monogram.
 */
export function AgentGlyph({ id, name, size = 26 }: { id: string; name?: string; size?: number }) {
  const logo = LOGOS[resolvedLogoKey(id)];
  const brandColor = AGENT_COLORS[id];
  const displayName = agentName(id, name);
  const radius = Math.round(size * 0.3);
  let baseGlyph: ReactNode;

  if (logo) {
    if (WIDE_TILES[id]) {
      baseGlyph = (
        <div
          className="flex items-center justify-center"
          style={{
            width: size,
            height: size,
            borderRadius: radius,
            background: WIDE_TILES[id],
            overflow: "hidden",
          }}
        >
          <img
            src={logo}
            alt={displayName}
            draggable={false}
            style={{ width: "100%", height: "100%", objectFit: "contain", display: "block" }}
          />
        </div>
      );
    } else if (FULL_BLEED.has(id) || FULL_BLEED.has(resolvedLogoKey(id))) {
      // App-icon logos (own background) fill the badge; mark-only logos sit on a white tile.
      baseGlyph = (
        <img
          src={logo}
          alt={displayName}
          draggable={false}
          style={{ width: size, height: size, borderRadius: radius, objectFit: "cover", display: "block" }}
        />
      );
    } else {
      baseGlyph = (
        <div
          className="flex items-center justify-center"
          style={{
            width: size,
            height: size,
            borderRadius: radius,
            background: THEMED_MARKS.has(id) ? "var(--surface-app)" : "#fff",
            border: "1px solid var(--border-hairline)",
          }}
        >
          <img
            src={logo}
            alt={displayName}
            draggable={false}
            style={{ width: Math.round(size * 0.64), height: Math.round(size * 0.64), objectFit: "contain" }}
          />
        </div>
      );
    }
  } else {
    const label = displayName[0]?.toUpperCase() ?? "?";
    baseGlyph = (
      <div
        className="flex items-center justify-center text-white font-semibold select-none"
        style={{
          width: size,
          height: size,
          borderRadius: radius,
          background: brandColor ?? fallbackColor(id),
          fontSize: Math.round(size * 0.5),
        }}
      >
        {label}
      </div>
    );
  }

  const surface = logo ? visibleSurface(id) : null;
  return (
    <span className="arca-agent-glyph" data-agent-id={id} style={{ width: size, height: size }}>
      <span className="arca-agent-glyph-base" style={{ width: size, height: size }}>
        {baseGlyph}
      </span>
      {surface && <AgentSurfaceBadge surface={surface} size={size} />}
    </span>
  );
}
