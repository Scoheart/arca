import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import links from "../../../data/agent-install-links.json";
import { DownloadIcon } from "./icons";
import { useToast } from "./Toast";
import { formatError } from "../lib/format";

export function AgentInstallAction({ agentId }: { agentId: string }) {
  const link = (links as Record<string, { url: string; label: string }>)[agentId];
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(true);
  const [revision, setRevision] = useState(0);
  const { show } = useToast();
  useEffect(() => {
    let active = true;
    setBusy(true);
    setInstalled(null);
    void invoke<boolean | null>("detect_agent_installation", { agentId })
      .then((value) => { if (active) setInstalled(value); })
      .catch(() => { if (active) setInstalled(null); })
      .finally(() => { if (active) setBusy(false); });
    return () => { active = false; };
  }, [agentId, revision]);
  useEffect(() => {
    const refresh = () => setRevision((value) => value + 1);
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);
  if (!link) return null;
  const status = busy ? "检测中…" : installed === true ? "已检测到" : installed === false ? "未检测到" : "安装状态未知";
  const state = busy ? "checking" : installed === true ? "installed" : installed === false ? "missing" : "unknown";
  return <div className="arca-agent-install-actions" data-state={state} role="group" aria-label="下载与安装检测">
    <button type="button" className="arca-agent-install-link" title={`${link.label} · ${status}`} aria-label={`${link.label}，${status}`} onClick={() => {
      void openUrl(link.url).catch((error) => show({ kind: "error", msg: `无法打开安装页面：${formatError(error)}` }));
    }}><DownloadIcon className="w-3.5 h-3.5" />{link.label}<span className="arca-agent-install-dot" aria-hidden="true" /></button>
    <span className="sr-only" aria-live="polite">{status}</span>
  </div>;
}
