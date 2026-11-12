import { useTranslation } from "react-i18next";
import type { StartupSyncState } from "../hooks/useStartupSync";
import { ToastNotice } from "./Toast";

export function StartupSyncBar({ state }: { state: StartupSyncState }) {
  const { t, i18n } = useTranslation();
  const taskLabel = (label: string | null) => {
    switch (label) {
      case "registry": return t("startup.tasks.registry");
      case "agents": return t("startup.tasks.agents");
      case "agent-capabilities": return t("startup.tasks.agentCapabilities");
      case "relationships": return t("startup.tasks.relationships");
      case "resource-inventory": return t("startup.tasks.resourceInventory");
      case "sources": return t("startup.tasks.sources");
      case "workspace": return t("startup.tasks.workspace");
      case "skills": return t("startup.tasks.skills");
      case "installed": return t("startup.tasks.installed");
      case "external-models": return t("startup.tasks.externalModels");
      default: return null;
    }
  };
  if (!state.settled || state.failed === 0) return null;
  const failures = state.tasks.filter((task) => task.status === "error").map((task) => ({
    label: taskLabel(task.label) ?? task.label,
    error: task.error,
  }));
  const tasks = new Intl.ListFormat(i18n.resolvedLanguage, { style: "short", type: "conjunction" })
    .format(failures.map((task) => task.label));
  return <ToastNotice noticeKey="startup-sync" kind="warning"
    message={t("startup.incomplete", { tasks })}
    details={failures.map(({ label, error }) => [label, error].filter(Boolean).join("\n")).join("\n\n")}
    action={<button type="button" className="btn-ghost" onClick={() => void state.retryFailed()}>
      {t("startup.retryFailed")}</button>} />;
}
