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
      case "sources": return t("startup.tasks.sources");
      case "workspace": return t("startup.tasks.workspace");
      case "skills": return t("startup.tasks.skills");
      case "installed": return t("startup.tasks.installed");
      case "external-models": return t("startup.tasks.externalModels");
      case "updates": return t("startup.tasks.updates");
      default: return null;
    }
  };
  const failedList = new Intl.ListFormat(i18n.resolvedLanguage, { style: "short", type: "conjunction" })
    .format(state.tasks.filter((task) => task.status === "error").map((task) => taskLabel(task.label) ?? task.label));
  const failure = state.failed > 0;
  return <ToastNotice noticeKey="startup-sync" kind={failure ? "warning" : state.settled ? "success" : "info"}
    persistent={!state.settled && !failure}
    message={failure ? `${t("startup.partialTitle")} · ${t("startup.failedNamed", { tasks: failedList, completed: state.completed })} · ${state.completed}/${state.total}`
      : state.settled ? `${t("startup.complete")} · ${state.completed}/${state.total}`
      : `${t("startup.title")} · ${state.slow ? t("startup.slow") : taskLabel(state.activeLabel) ?? t("startup.waiting")} · ${state.completed}/${state.total}`}
    action={failure ? <button type="button" className="btn-ghost" onClick={() => void state.retryFailed()}>
      {t("startup.retryFailed")}</button> : undefined} />;
}
