import iconCatalog from "../../../data/mcp-icons.json";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { McpIconPreference, RegistryEntry } from "../lib/types";

export type McpIconId =
  | "mcp" | "search" | "browser" | "document" | "knowledge" | "files"
  | "database" | "terminal" | "code" | "api" | "cloud" | "automation"
  | "observability" | "map" | "communication" | "media" | "security" | "ai";

export type McpIconOption = {
  id: McpIconId;
  tone: "blue" | "teal" | "green" | "amber" | "orange" | "violet";
};

export const MCP_ICON_OPTIONS = iconCatalog as McpIconOption[];

const OPTION_BY_ID = new Map(MCP_ICON_OPTIONS.map((option) => [option.id, option]));

function LineIcon({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  );
}

export function McpIconGlyph({ id }: { id: McpIconId }) {
  const paths: Record<McpIconId, ReactNode> = {
    mcp: (
      <>
        <rect x="5" y="5" width="14" height="14" rx="3" />
        <path d="M9 9h6v6H9z" />
        <path d="M12 2v3M12 19v3M2 12h3M19 12h3" />
      </>
    ),
    search: (
      <>
        <circle cx="11" cy="11" r="7" />
        <path d="m20 20-4.35-4.35" />
      </>
    ),
    browser: (
      <>
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path d="M3 9h18" />
        <circle cx="6.5" cy="6.5" r=".75" fill="currentColor" stroke="none" />
        <circle cx="9.5" cy="6.5" r=".75" fill="currentColor" stroke="none" />
        <circle cx="12.5" cy="6.5" r=".75" fill="currentColor" stroke="none" />
      </>
    ),
    document: (
      <>
        <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
        <path d="M14 2v4a2 2 0 0 0 2 2h4" />
        <path d="M10 9H8M16 13H8M16 17H8" />
      </>
    ),
    knowledge: (
      <>
        <path d="M2 4a2 2 0 0 1 2-2h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
        <path d="M22 4a2 2 0 0 0-2-2h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
      </>
    ),
    files: (
      <>
        <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
      </>
    ),
    database: (
      <>
        <ellipse cx="12" cy="5" rx="9" ry="3" />
        <path d="M3 5v14a9 3 0 0 0 18 0V5" />
        <path d="M3 12a9 3 0 0 0 18 0" />
      </>
    ),
    terminal: (
      <>
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path d="m7 9 3 3-3 3M13 15h4" />
      </>
    ),
    code: (
      <>
        <path d="m16 18 6-6-6-6M8 6l-6 6 6 6" />
        <path d="m14 4-4 16" />
      </>
    ),
    api: (
      <>
        <path d="M18 16.98h-5.99c-1.1 0-1.95.94-2.48 1.9A4 4 0 0 1 2 17c.01-.7.2-1.4.57-2" />
        <path d="m6 17 3.13-5.78c.53-.97.1-2.18-.5-3.1a4 4 0 1 1 6.89-4.06l3.19 5.74c.48.95 1.47 1.2 2.29 1.2" />
        <circle cx="12" cy="12" r="2" />
      </>
    ),
    cloud: (
      <>
        <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />
      </>
    ),
    automation: (
      <>
        <rect width="8" height="8" x="3" y="3" rx="2" />
        <path d="M7 11v4a2 2 0 0 0 2 2h4" />
        <rect width="8" height="8" x="13" y="13" rx="2" />
        <path d="M17 9V5a2 2 0 0 0-2-2h-4" />
      </>
    ),
    observability: (
      <>
        <path d="M3 12h3.5l2.5-7 4 14 3-9 2 4H21" />
      </>
    ),
    map: (
      <>
        <path d="M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0Z" />
        <circle cx="12" cy="10" r="3" />
      </>
    ),
    communication: (
      <>
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      </>
    ),
    media: (
      <>
        <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
        <circle cx="9" cy="9" r="2" />
        <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
      </>
    ),
    security: (
      <>
        <path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" />
        <path d="m9 12 2 2 4-4" />
      </>
    ),
    ai: (
      <>
        <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z" />
        <path d="M20 3v4" />
        <path d="M22 5h-4" />
      </>
    ),
  };
  return <span className="arca-mcp-icon-glyph" data-mcp-icon={id}><LineIcon>{paths[id]}</LineIcon></span>;
}

const RULES: Array<{ id: McpIconId; words: string[] }> = [
  { id: "observability", words: ["observability", "monitor", "metrics", "logging", "logs", "trace", "sentry", "grafana"] },
  { id: "browser", words: ["browser", "chrome", "playwright", "puppeteer", "selenium"] },
  { id: "search", words: ["search", "brave", "exa", "tavily", "perplexity"] },
  { id: "database", words: ["database", "postgres", "mysql", "sqlite", "redis", "mongodb", "supabase"] },
  { id: "knowledge", words: ["knowledge", "wiki", "notion", "confluence", "rag", "memory", "-km"] },
  { id: "document", words: ["document", "docs", "alidocs", "pdf", "word", "markdown"] },
  { id: "files", words: ["filesystem", "files", "drive", "dropbox", "storage"] },
  { id: "terminal", words: ["terminal", "shell", "ssh", "exec", "command"] },
  { id: "code", words: ["github", "gitlab", "devops", "android", "code", "repository", "repo"] },
  { id: "map", words: ["map", "gaode", "amap", "geo", "location"] },
  { id: "communication", words: ["slack", "discord", "mail", "message", "chat", "dingtalk"] },
  { id: "media", words: ["canvas", "image", "video", "media", "audio"] },
  { id: "security", words: ["security", "audit", "secret", "auth", "vulnerability"] },
  { id: "automation", words: ["automation", "workflow", "flow", "scheduler", "cron"] },
  { id: "cloud", words: ["cloud", "aws", "azure", "gcp", "aliyun", "alibaba"] },
  { id: "api", words: ["api", "http", "fetch", "gateway", "webhook"] },
  { id: "ai", words: ["ai", "llm", "model", "assistant", "agent"] },
];

function searchableEntry(entry: RegistryEntry) {
  const stdio = entry.config.stdio;
  const http = entry.config.http;
  return [
    entry.name,
    entry.description,
    ...entry.tags,
    stdio?.command,
    ...(stdio?.args ?? []),
    http?.url,
  ].filter(Boolean).join(" ").toLocaleLowerCase();
}

export function inferMcpIcon(entry: RegistryEntry): McpIconId | null {
  const text = searchableEntry(entry);
  return RULES.find((rule) => rule.words.some((word) => text.includes(word)))?.id ?? null;
}

export function mcpMonogram(name: string) {
  const leaf = name.split("/").filter(Boolean).at(-1) ?? name;
  const parts = leaf.split(/[^a-z0-9]+/i).filter((part) => part && !["mcp", "server"].includes(part.toLocaleLowerCase()));
  if (parts.length >= 2) return `${parts[0][0]}${parts[1][0]}`.toUpperCase();
  const compact = (parts[0] ?? leaf).replace(/[^a-z0-9]/gi, "");
  return (compact.slice(0, 2) || "M").toUpperCase();
}

export function McpAvatar({
  assetKey,
  entry,
  preference,
  size = 34,
}: {
  assetKey: string;
  entry: RegistryEntry;
  preference?: McpIconPreference;
  size?: number;
}) {
  const [customFailed, setCustomFailed] = useState(false);
  useEffect(() => setCustomFailed(false), [preference?.kind, preference?.path]);
  const inferred = useMemo(() => inferMcpIcon(entry), [entry]);
  const requestedBuiltin = preference?.kind === "builtin" ? preference.value as McpIconId : null;
  const selected = requestedBuiltin && OPTION_BY_ID.has(requestedBuiltin) ? requestedBuiltin : null;
  const customPath = preference?.kind === "custom" && preference.path && !customFailed
    ? preference.path
    : null;
  const custom = customPath !== null;
  const iconId = selected ?? inferred;
  const option = iconId ? OPTION_BY_ID.get(iconId) : undefined;
  const source = custom ? "custom" : selected ? "builtin" : inferred ? "auto" : "fallback";

  return (
    <div
      className="arca-asset-avatar arca-mcp-avatar flex-shrink-0"
      data-kind="mcp"
      data-icon-source={source}
      data-icon-tone={custom ? "custom" : option?.tone ?? "neutral"}
      data-asset-key={assetKey}
      aria-hidden="true"
      style={{ width: size, height: size, borderRadius: Math.round(size * 0.3) }}
    >
      {custom ? (
        <img
          src={convertFileSrc(customPath)}
          alt=""
          draggable={false}
          onError={() => setCustomFailed(true)}
        />
      ) : iconId ? (
        <McpIconGlyph id={iconId} />
      ) : (
        <span className="arca-mcp-monogram">{mcpMonogram(entry.name)}</span>
      )}
    </div>
  );
}
