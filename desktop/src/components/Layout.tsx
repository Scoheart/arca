import { isImeKey } from "./ImeInput";
import { RESOURCE_CATEGORIES } from "./resourcePresentation";
import { ReactNode, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import type { AgentInfo, ProxySettings, View } from "../lib/types";
import {
  DownloadIcon,
  LanguageIcon,
  MoonIcon,
  NetworkIcon,
  RefreshIcon,
  SunIcon,
  SlidersIcon,
  TerminalIcon,
  EditIcon,
} from "./icons";
import { applyTheme, getInitialTheme, type Theme } from "../lib/theme";
import { formatError } from "../lib/format";
import { useLocale } from "../i18n/LocaleProvider";
import type { LocalePreference } from "../i18n";
import { useToast } from "./Toast";
import type { UpdaterState } from "../hooks/useUpdater";
import { AgentLauncherProvider } from "./AgentLauncherProvider";
import { AgentNavigation } from "./AgentNavigation";
import { DialogShell } from "./DialogShell";
import { FormSelect } from "./FormSelect";
import { TerminalSelect } from "./TerminalSelect";
import "./WorkspaceSettings.css";
import { FileEditorSelect } from "./FileEditorSelect";
import { ProxySettingsDialog } from "./ProxySettingsDialog";
import { MODAL_DIALOG_SELECTOR } from "./ui";
import { StartupSyncBar } from "./StartupSyncBar";
import type { StartupSyncState } from "../hooks/useStartupSync";

interface LayoutProps {
  children: ReactNode;
  agents: AgentInfo[];
  view: View;
  onSelectRegistry: () => void;
  onSelectModels: () => void;
  onSelectSkills: () => void;
  onSelectAgent: (id: string) => void;
  onAddAgent?: () => void;
  onRescan?: () => Promise<unknown> | void;
  updater?: UpdaterState;
  proxyUrl: string | null;
  proxySettingsLoading: boolean;
  onSaveProxy: (proxyUrl: string | null) => Promise<ProxySettings>;
  startupSync?: StartupSyncState;
}

export function Layout({
  children,
  agents,
  view,
  onSelectRegistry,
  onSelectModels,
  onSelectSkills,
  onSelectAgent,
  onAddAgent,
  onRescan,
  updater,
  proxyUrl,
  proxySettingsLoading,
  onSaveProxy,
  startupSync,
}: LayoutProps) {
  const [rescanning, setRescanning] = useState(false);
  const [theme, setTheme] = useState<Theme>(getInitialTheme);
  const [version, setVersion] = useState("");
  const [proxySettingsOpen, setProxySettingsOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const toast = useToast();
  const { t } = useTranslation();
  const localeState = useLocale();

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  useEffect(() => {
    const find = (event: KeyboardEvent) => {
      if (isImeKey(event) || event.altKey || !(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "f") return;
      const modal = Array.from(document.querySelectorAll<HTMLElement>(MODAL_DIALOG_SELECTOR)).at(-1);
      const search = (modal ?? document.querySelector("main"))?.querySelector<HTMLInputElement>('input[type="search"]:not(:disabled)');
      if (!search || search.closest("[inert]")) return;
      event.preventDefault(); search.focus(); search.select();
    };
    document.addEventListener("keydown", find);
    return () => document.removeEventListener("keydown", find);
  }, []);

  const checkingUpdate = updater?.phase.kind === "checking";
  const handleCheckUpdate = async () => {
    if (!updater || checkingUpdate) return;
    const result = await updater.checkNow({ manual: true });
    // "available"/"error" both surface via the UpdateBanner; only the quiet
    // outcome needs feedback here.
    if (result === "latest") toast.show({ kind: "success", msg: t("layout.latest") });
  };

  const toggleTheme = () => {
    const next: Theme = theme === "dark" ? "light" : "dark";
    setTheme(next);
    applyTheme(next);
  };

  const selectLocale = async (preference: LocalePreference) => {
    try {
      await localeState.setPreference(preference);
    } catch (error) {
      toast.show({
        kind: "error",
        msg: t("layout.languageSaveFailed", { error: formatError(error) }),
      });
    }
  };

  const handleRescan = async () => {
    if (!onRescan || rescanning) return;
    setRescanning(true);
    try {
      await onRescan();
    } finally {
      setRescanning(false);
    }
  };

  return (
    <AgentLauncherProvider><div className="flex flex-col h-full">
      {/* Top bar */}
      <header
        className="arca-topbar flex-shrink-0 flex items-center gap-3 px-5"
        style={{
          height: 56,
          background: "var(--surface-workspace)",
          position: "relative",
          zIndex: 100,
        }}
      >
        {/* Arca wordmark */}
        <span
          className="arca-wordmark text-[22px] font-bold bg-gradient-to-r from-brand-gold via-brand-coral to-brand-magenta bg-clip-text select-none flex-shrink-0"
          style={{ WebkitTextFillColor: "transparent", letterSpacing: 0 }}
        >
          Arca
        </span>

        {/* Top-level resources (also the way back from an Agent view) */}
        <div className="arca-seg arca-skill-seg flex-shrink-0" role="navigation" aria-label="中央资源">
          {RESOURCE_CATEGORIES.map((resource) => <button
            key={resource.id}
            className="arca-seg-item"
            data-active={view.kind === resource.view ? "true" : undefined}
            aria-pressed={view.kind === resource.view}
            onClick={{ models: onSelectModels, mcps: onSelectRegistry, skills: onSelectSkills }[resource.id]}
          >
            <span className="flex items-center gap-1.5">
              <resource.Icon className="w-3.5 h-3.5" />
              <span className="arca-resource-label">{resource.label}</span>
            </span>
          </button>)}
        </div>

        {/* The picker settles at one icon, then pinned Agents disappear one
            whole shortcut at a time until their strip has no footprint. */}
        <div className="arca-topbar-navigation-lane">
          <AgentNavigation
            agents={agents}
            selectedAgentId={view.kind === "agent" ? view.id : null}
            onSelectAgent={onSelectAgent}
            onAddAgent={onAddAgent}
          />
        </div>

        <button type="button" className="arca-settings-trigger" aria-label="设置" title="设置"
          aria-haspopup="dialog" aria-expanded={settingsOpen} onClick={() => setSettingsOpen(true)}>
          <SlidersIcon className="w-4 h-4" /><span>设置</span>
          {updater?.phase.kind === "available" && <span className="arca-settings-update-dot" aria-label="有可用更新" />}
        </button>
      </header>

      {startupSync && <StartupSyncBar state={startupSync} />}

      {/* Content — transparent so the body's tinted backdrop shows through the
          glass surfaces. min-h-0 is critical for overflow to work. */}
      <main aria-label={view.kind === "traces" ? t("trace.title") : view.kind === "capture" ? "Agent 抓包工作区" : view.kind === "agent" ? `${agents.find((agent) => agent.id === view.id)?.name ?? view.id} 工作区` : `${view.kind === "registry" ? "MCPs" : view.kind === "models" ? "Models" : "Skills"} 资源库`} className="flex-1 min-h-0 overflow-hidden" style={{ background: "transparent" }}>
        {children}
      </main>

      {settingsOpen && !proxySettingsOpen && (
        <DialogShell kind="editor" size="sm" title="设置" className="arca-workspace-settings" leading={<SlidersIcon className="w-5 h-5" />}
          onClose={() => setSettingsOpen(false)}>
          <div className="arca-settings-section">
            <div className="arca-settings-row"><span className="arca-settings-label"><EditIcon className="w-4 h-4" />{t("fileEditor.label")}</span><FileEditorSelect showLabel /></div>
            <div className="arca-settings-row"><span className="arca-settings-label"><TerminalIcon className="w-4 h-4" />默认终端</span><TerminalSelect /></div>
          </div>
          <div className="arca-settings-section">
            <div className="arca-settings-row"><span className="arca-settings-label"><SunIcon className="w-4 h-4" />外观</span>
              <div className="arca-seg"><button type="button" className="arca-seg-item" data-active={theme === "light" || undefined} aria-label={t("layout.lightTheme")} aria-pressed={theme === "light"} onClick={() => { if (theme !== "light") toggleTheme(); }}><SunIcon className="w-4 h-4" /></button>
                <button type="button" className="arca-seg-item" data-active={theme === "dark" || undefined} aria-label={t("layout.darkTheme")} aria-pressed={theme === "dark"} onClick={() => { if (theme !== "dark") toggleTheme(); }}><MoonIcon className="w-4 h-4" /></button></div>
            </div>
            <div className="arca-settings-row"><span className="arca-settings-label"><LanguageIcon className="w-4 h-4" />{t("layout.language")}</span>
              <FormSelect ariaLabel={t("layout.language")} value={localeState.preference ?? "system"} disabled={localeState.saving}
                options={[{ value: "system", label: t("layout.followSystem") }, { value: "zh-CN", label: t("layout.simplifiedChinese") }, { value: "en-US", label: t("layout.english") }]}
                onChange={(value) => void selectLocale(value === "system" ? null : value as LocalePreference)} />
            </div>
            <div className="arca-settings-row"><span className="arca-settings-label"><NetworkIcon className="w-4 h-4" />{t("layout.networkProxy")}</span>
              <button type="button" className="arca-settings-value" disabled={proxySettingsLoading} onClick={() => setProxySettingsOpen(true)}><span className="arca-settings-status" data-active={Boolean(proxyUrl)} />{proxyUrl ? "已配置" : "未配置"}<span aria-hidden="true">›</span></button>
            </div>
          </div>
          <div className="arca-settings-section">
            {onRescan && <button type="button" className="arca-settings-action" disabled={rescanning} onClick={() => void handleRescan().catch((error) => toast.show({ kind: "error", msg: formatError(error) }))}>
              <RefreshIcon className="w-4 h-4" /><span>{rescanning ? t("layout.scanning") : t("layout.rescan")}</span></button>}
            <button type="button" className="arca-settings-action" disabled={!updater || checkingUpdate} onClick={() => { setSettingsOpen(false); void handleCheckUpdate(); }}>
              <DownloadIcon className="w-4 h-4" /><span>{checkingUpdate ? t("layout.checking") : t("layout.checkUpdate")}</span><small>{version && `v${version}`}</small></button>
          </div>
        </DialogShell>
      )}

      {proxySettingsOpen && (
        <ProxySettingsDialog
          proxyUrl={proxyUrl}
          onClose={() => setProxySettingsOpen(false)}
          onSave={onSaveProxy}
        />
      )}
    </div></AgentLauncherProvider>
  );
}
