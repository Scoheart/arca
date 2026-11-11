import i18n from "../i18n";
import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getAgentLaunchInfo, getAgentRuntimeVersion, type AgentLaunchInfo } from "../lib/agentLaunch";
import { formatError } from "../lib/format";
import { ChevronDownIcon, DownloadIcon, ExternalLinkIcon, FolderIcon, EditIcon, PlayIcon, RefreshIcon } from "./icons";
import { useAgentLauncher } from "../lib/agentLauncherContext";
import { useToast } from "./Toast";
import installLinks from "../../../data/agent-install-links.json";
import agentDocsHome from "../../../data/agent-docs-home.json";
import { useTranslation } from "react-i18next";

export function AgentLaunchAction({ agentId, contextMenu = false, showLabel = false, onChosen }: { agentId: string; contextMenu?: boolean; showLabel?: boolean; onChosen?(): void }) {
  const launcher = useAgentLauncher();
  const { t } = useTranslation();
  const [info, setInfo] = useState<AgentLaunchInfo | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const versionTarget = useRef<string | null>(null);
  const { show } = useToast();
  useEffect(() => {
    let active = true;
    let request = 0;
    const refresh = () => {
      const current = ++request;
      void getAgentLaunchInfo(agentId).then((value) => {
        if (!active || current !== request) return;
        const resolved = value.resolved_target;
        const target = JSON.stringify([agentId, resolved?.kind,
          resolved?.kind === "app" ? resolved.path : resolved?.kind === "cli" ? resolved.command : null]);
        if (versionTarget.current !== target || !value.available) setVersion(null);
        versionTarget.current = target;
        setInfo(value); setError(false);
        if (value.available && !contextMenu) void getAgentRuntimeVersion(agentId)
          .then((found) => { if (active && current === request) setVersion(found); })
          .catch(() => {});
      }).catch(() => { if (active && current === request) setError(true); });
    };
    refresh(); window.addEventListener("focus", refresh);
    return () => { active = false; window.removeEventListener("focus", refresh); };
  }, [agentId, contextMenu, launcher.revision]);
  useEffect(() => {
    if (!expanded || contextMenu) return;
    const outside = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setExpanded(false); };
    document.addEventListener("pointerdown", outside);
    root.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
    return () => document.removeEventListener("pointerdown", outside);
  }, [expanded, contextMenu]);
  const busy = launcher.busyId !== null;
  const missing = info && !info.available && info.kind && info.install_url && !info.configured_target;
  const label = !info ? error ? i18n.t("desktop.agentLaunchAction.retry") : i18n.t("desktop.agentLaunchAction.detecting") : !info.supported ? i18n.t("desktop.agentLaunchAction.notSupportedYet") : missing ? i18n.t("desktop.agentLaunchAction.downloadAndInstall")
    : !info.available ? i18n.t("desktop.agentLaunchAction.configureLaunchMethod") : info.host_name ? i18n.t("desktop.agentLaunchAction.open", { value0: info.host_name }) : i18n.t("desktop.agentLaunchAction.openAgent");
  const pending = launcher.busyId === agentId || (!info && !error);
  const actionLabel = launcher.busyId === agentId ? i18n.t("desktop.agentLaunchAction.opening") : label;
  const ready = Boolean(info?.available);
  const runtimeName = info?.host_name ?? (info?.configured_target ? t("agentRuntime.customEntry") : info?.name);
  const runtimeStatus = info?.host_name ? t("agentRuntime.hostAvailable")
    : info?.installed === true && !info.configured_target ? t("agentRuntime.installed") : t("agentRuntime.entryAvailable");
  const installUrl = info?.install_url ?? (installLinks as Record<string, { url: string }>)[agentId]?.url;
  const installationStatus = !info ? error ? i18n.t("desktop.agentLaunchAction.statusUnknown") : i18n.t("desktop.agentLaunchAction.detecting") : ready ? i18n.t("desktop.agentLaunchAction.launchEntryAvailable") : i18n.t("desktop.agentLaunchAction.launcherNotDetected");
  const canChooseDirectory = info?.kind === "cli" && info.available;
  const installedPath = info?.resolved_target?.kind === "app" ? info.resolved_target.path
    : info?.resolved_target?.kind === "cli" ? info.resolved_target.command : null;
  const hasMenu = Boolean(info?.kind);
  const shortLabel = pending ? i18n.t("desktop.agentLaunchAction.wait") : !info && error ? i18n.t("desktop.agentLaunchAction.retry") : info?.supported === false ? i18n.t("desktop.agentLaunchAction.unsupported") : ready ? info?.host_name ?? i18n.t("desktop.agentLaunchAction.run") : missing ? i18n.t("desktop.agentLaunchAction.install") : i18n.t("desktop.agentLaunchAction.settings");
  const choose = (action: () => void) => { setExpanded(false); onChosen?.(); action(); };
  const configure = () => { void launcher.configure(agentId); };
  const launch = () => choose(() => { void launcher.launch(agentId); });
  const openInstallation = () => {
    if (installUrl) void openUrl(installUrl).catch((failure) => show({ kind: "error", msg: formatError(failure) }));
  };
  // Web clients and frameworks have no native runtime target. Their guide is
  // useful; asking users to choose an App or CLI entry would create a false one.
  if (info && !info.kind) {
    const docsUrl = (agentDocsHome as Record<string, string>)[agentId];
    if (!docsUrl) return null;
    return <button type="button" className={contextMenu ? undefined : "btn-secondary"}
      role={contextMenu ? "menuitem" : undefined} onClick={() => choose(() => {
        void openUrl(docsUrl).catch((failure) => show({ kind: "error", msg: formatError(failure) }));
      })}><ExternalLinkIcon className="w-4 h-4" />{i18n.t("desktop.agentLaunchAction.setupGuide")}</button>;
  }
  const menu = <>
    {contextMenu && <button type="button" role="menuitem" disabled={busy || (!info && !error) || info?.supported === false} onClick={launch}>
      {missing ? <DownloadIcon className="w-4 h-4" /> : <ExternalLinkIcon className="w-4 h-4" />}{label}</button>}
    {canChooseDirectory && <button type="button" role="menuitem" disabled={busy} title={info?.directory ?? undefined}
      onClick={() => choose(() => { void launcher.launch(agentId, true); })}><FolderIcon className="w-4 h-4" />{i18n.t("desktop.agentLaunchAction.openInAnotherFolder")}</button>}
    {contextMenu && installUrl && !missing && <button type="button" role="menuitem" disabled={busy} onClick={() => choose(openInstallation)}>
      <DownloadIcon className="w-4 h-4" />{i18n.t("desktop.agentLaunchAction.installationDocumentation")}</button>}
    <button type="button" role="menuitem" disabled={busy} onClick={() => choose(configure)}>
      <EditIcon className="w-4 h-4" />{i18n.t("desktop.agentLaunchAction.launchSettings")}</button>
    {info?.startup_agents && info.startup_agents.length > 0 && (
      <>
        <div className="arca-launch-menu-divider" />
        <div className="arca-launch-menu-header text-[11px] font-semibold text-muted px-2 py-1 select-none">
          启动 Agent
        </div>
        {info.startup_agents.map((option) => (
          <button
            key={option.id}
            type="button"
            role="menuitem"
            disabled={busy}
            className="flex items-center justify-between text-xs w-full text-left"
            onClick={() => choose(() => { void launcher.launchWithStartupAgent(agentId, option.id); })}
          >
            <span className="flex items-center gap-1.5 truncate">
              <span className={`w-1.5 h-1.5 rounded-full ${option.is_active ? "bg-emerald-500" : "bg-transparent border border-muted"}`} />
              <span className={option.is_active ? "font-semibold text-foreground" : "text-muted"}>
                {option.name}
              </span>
            </span>
            {option.is_default && <span className="text-[10px] text-muted ml-2 shrink-0">(默认)</span>}
          </button>
        ))}
      </>
    )}
  </>;
  return <>
    {!contextMenu && version && info?.available && info.supported && <span className="arca-launch-version" role="status"
      aria-label={t("agentRuntime.status", { name: runtimeName ?? agentId, status: runtimeStatus,
        version: version ? t("agentRuntime.version", { version }) : t("agentRuntime.unknownVersion") })}
      title={`${runtimeName}${version ? ` · v${version}` : ` · ${runtimeStatus}`}${installedPath ? `\n${installedPath}` : ""}`}>
      v{version}
    </span>}
    {!contextMenu && installUrl && <button type="button" className="arca-launch-install btn-secondary" onClick={openInstallation}
      title={i18n.t("desktop.agentLaunchAction.installationDocumentation2", { value0: installationStatus })} aria-label={i18n.t("desktop.agentLaunchAction.installationDocumentation3", { value0: info?.name ?? agentId, value1: installationStatus })}>
      <DownloadIcon className="w-3.5 h-3.5" /><span>{i18n.t("desktop.agentLaunchAction.installationDocumentation")}</span><span className="arca-launch-install-status" data-ready={ready || undefined} aria-hidden="true" />
    </button>}
    <div ref={root} className={contextMenu ? "arca-launch-menu-content" : "arca-launch-action"}
    data-ready={ready || undefined} data-labeled={showLabel || undefined} data-split={hasMenu} aria-busy={pending} onKeyDown={(event) => {
    if (contextMenu) return; // The portal menu owns focus, Escape and Tab.
    if (event.key === "Escape") { event.stopPropagation(); setExpanded(false); trigger.current?.focus(); }
    if ((expanded || contextMenu) && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const items = Array.from(root.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? []);
      const current = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
      items[next]?.focus();
    }
  }}>
    {contextMenu ? menu : <>
      <button type="button" className="arca-launch-primary" disabled={busy || (!info && !error) || info?.supported === false} onClick={launch}
        aria-label={actionLabel}
        title={`${actionLabel}${info?.active_startup_agent ? ` · Agent: ${info.active_startup_agent}` : ""}${info?.available ? ` · ${info.host_name ?? info.name}` : ""}${info?.directory && info.kind === "cli" ? `\n${info.directory}` : ""}`}>
        <span className="arca-launch-symbol" data-spinning={pending || undefined} aria-hidden="true">
          {pending || (!info && error) ? <RefreshIcon /> : missing ? <DownloadIcon /> : !ready ? <EditIcon /> : <PlayIcon />}
        </span>
        {showLabel && <span className="arca-launch-caption" aria-hidden="true">{shortLabel}</span>}
        {ready && !pending && !showLabel && <i className="arca-launch-ready-dot" aria-hidden="true" />}
      </button>
      {hasMenu && <button ref={trigger} type="button" className="arca-launch-more" aria-label={i18n.t("desktop.agentLaunchAction.agentLaunchOptions")} aria-haspopup="menu" aria-expanded={expanded}
        disabled={busy} onClick={() => setExpanded((value) => !value)}><ChevronDownIcon className="w-3.5 h-3.5" /></button>}
      {hasMenu && expanded && <div className="arca-launch-menu" role="menu" aria-label={i18n.t("desktop.agentLaunchAction.agentLaunchOptions")}>{menu}</div>}
    </>}
    </div>
  </>;
}
