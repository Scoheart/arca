import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { StartupSyncState } from "../hooks/useStartupSync";
import { RefreshIcon } from "./icons";

export function StartupSyncBar({ state }: { state: StartupSyncState }) {
  const { t, i18n } = useTranslation();
  const [showSettled, setShowSettled] = useState(false);

  useEffect(() => {
    if (!state.settled || state.failed > 0) {
      setShowSettled(false);
      return;
    }
    setShowSettled(true);
    const timer = setTimeout(() => setShowSettled(false), 1_400);
    return () => clearTimeout(timer);
  }, [state.failed, state.settled]);

  if (state.settled && state.failed === 0 && !showSettled) return null;

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
  const activeLabel = taskLabel(state.activeLabel);
  const failedLabels = state.tasks
    .filter((task) => task.status === "error")
    .map((task) => taskLabel(task.label) ?? task.label);
  const failedList = new Intl.ListFormat(i18n.resolvedLanguage, {
    style: "short",
    type: "conjunction",
  }).format(failedLabels);
  const detail = state.failed > 0
    ? t("startup.failedNamed", {
        tasks: failedList,
        completed: state.completed,
      })
    : state.settled
      ? null
      : state.slow
        ? t("startup.slow")
        : activeLabel ?? t("startup.waiting");
  const title = state.failed > 0
    ? t("startup.partialTitle")
    : state.settled
      ? t("startup.complete")
      : t("startup.title");

  return (
    <div
      className="arca-startup-sync"
      data-slow={state.slow ? "true" : undefined}
      data-settled={state.settled ? "true" : undefined}
      data-failed={state.failed > 0 ? "true" : undefined}
      role={state.failed > 0 ? "alert" : "status"}
      aria-live="polite"
    >
      <RefreshIcon
        className="arca-startup-sync-icon"
        data-spinning={state.syncing ? "true" : undefined}
      />
      <strong>{title}</strong>
      {detail && <span className="arca-startup-sync-detail">{detail}</span>}
      <span className="arca-startup-sync-count">
        {state.completed}/{state.total}
      </span>
      {state.failed > 0 && (
        <button
          type="button"
          className="arca-startup-sync-retry"
          disabled={state.syncing}
          onClick={() => void state.retryFailed()}
        >
          {t("startup.retryFailed")}
        </button>
      )}
    </div>
  );
}
