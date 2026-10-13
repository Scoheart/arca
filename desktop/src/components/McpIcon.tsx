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
    mcp: <><path d="M8 8h8v8H8z" /><path d="M12 3v5M12 16v5M3 12h5M16 12h5" /><circle cx="12" cy="3" r="1" /><circle cx="12" cy="21" r="1" /><circle cx="3" cy="12" r="1" /><circle cx="21" cy="12" r="1" /></>,
    search: <><circle cx="10.5" cy="10.5" r="6.5" /><path d="m15.5 15.5 5 5" /></>,
    browser: <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 8h18" /><circle cx="6" cy="6" r=".5" /><circle cx="9" cy="6" r=".5" /></>,
    document: <><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v5h5M9 12h6M9 16h6" /></>,
    knowledge: <><path d="M4 5.5A3.5 3.5 0 0 1 7.5 2H11v18H7.5A3.5 3.5 0 0 0 4 23z" /><path d="M20 5.5A3.5 3.5 0 0 0 16.5 2H13v18h3.5A3.5 3.5 0 0 1 20 23z" /></>,
    files: <><path d="M3 7h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /><path d="M3 7V5a2 2 0 0 1 2-2h5l2 2h4" /></>,
    database: <><ellipse cx="12" cy="5" rx="7" ry="3" /><path d="M5 5v7c0 1.7 3.1 3 7 3s7-1.3 7-3V5M5 12v7c0 1.7 3.1 3 7 3s7-1.3 7-3v-7" /></>,
    terminal: <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="m7 9 3 3-3 3M13 15h4" /></>,
    code: <><path d="m9 6-6 6 6 6M15 6l6 6-6 6M13 4l-2 16" /></>,
    api: <><circle cx="5" cy="12" r="2" /><circle cx="19" cy="6" r="2" /><circle cx="19" cy="18" r="2" /><path d="m7 11 10-4M7 13l10 4" /></>,
    cloud: <><path d="M6 19h12a4 4 0 0 0 .7-7.9A7 7 0 0 0 5.2 9 5 5 0 0 0 6 19Z" /></>,
    automation: <><path d="M7 7h10v10H7z" /><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9 7 7M17 17l2.1 2.1M19.1 4.9 17 7M7 17l-2.1 2.1" /></>,
    observability: <><path d="M3 13h4l2-7 4 13 2-6h6" /></>,
    map: <><path d="M12 22s7-6.1 7-13A7 7 0 1 0 5 9c0 6.9 7 13 7 13Z" /><circle cx="12" cy="9" r="2.5" /></>,
    communication: <><path d="M4 4h16v12H9l-5 4z" /><path d="M8 9h8M8 12h5" /></>,
    media: <><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="8" cy="9" r="1.5" /><path d="m4 17 5-5 3 3 2-2 6 5" /></>,
    security: <><path d="M12 3 4.5 6v5.5c0 4.7 3 8.1 7.5 9.5 4.5-1.4 7.5-4.8 7.5-9.5V6z" /><path d="m9 12 2 2 4-5" /></>,
    ai: <><path d="m12 3-1.7 5.3A3 3 0 0 1 8.3 10L3 12l5.3 1.7a3 3 0 0 1 2 2L12 21l1.7-5.3a3 3 0 0 1 2-2L21 12l-5.3-1.7a3 3 0 0 1-2-2Z" /></>,
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
