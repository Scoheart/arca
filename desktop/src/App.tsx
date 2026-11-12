import { InstructionDraftGuard } from './components/InstructionDraftGuard';
import { ToastNotice } from "./components/Toast";
import { CredentialSession } from "./components/CredentialSession";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invalidateModelObservation } from "./lib/modelObservation";
import { invalidatePreferences } from "./lib/preferenceObservation";
import { Layout } from "./components/Layout";
import { RegistryView } from "./components/RegistryView";
import { RegistryEditPage } from "./components/RegistryEditPage";
import { AddAgentDialog } from "./components/AddAgentDialog";
import { useInstallState } from "./hooks/useInstallState";
import { useSkillsState } from "./hooks/useSkillsState";
import { useConsumptionState } from "./hooks/useConsumptionState";
import { useUpdater } from "./hooks/useUpdater";
import { useCliTool } from "./hooks/useCliTool";
import { useNetworkSettings } from "./hooks/useNetworkSettings";
import { UpdateBanner } from "./components/UpdateBanner";
import {
  getBackendStatus,
  getResourceObservation,
  observationValue,
  listModelAdoptionCandidates,
} from "./lib/api";
import type {
  BackendStatus,
  ModelAdoptionCandidate,
  ResourceNavigationRequest,
  View,
} from "./lib/types";
import {
  clearResourceIntent,
  createResourceNavigationIntent,
  viewForResourceIntent,
} from "./lib/resourceNavigation";
import { mergeAgentInfos } from "./lib/agentCapabilities";
import { useStartupSync, type StartupTask } from "./hooks/useStartupSync";
import { RefreshIcon } from "./components/icons";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  ALL_OBSERVATION_TASK_IDS,
  focusRefreshDue,
  taskIdsForObservation,
  type ObservationChange,
  type ObservationTaskId,
} from "./lib/observationRefresh";

const AgentView = lazy(() =>
  import("./components/AgentView").then((module) => ({
    default: module.AgentView,
  })),
);
const ModelsView = lazy(() =>
  import("./components/ModelsView").then((module) => ({
    default: module.ModelsView,
  })),
);
const SkillsView = lazy(() =>
  import("./components/SkillsView").then((module) => ({
    default: module.SkillsView,
  })),
);
const SubagentsView = lazy(() =>
  import("./components/SubagentsView").then((module) => ({
    default: module.SubagentsView,
  })),
);
const StartupAgentsView = lazy(() =>
  import("./components/StartupAgentsView").then((module) => ({
    default: module.StartupAgentsView,
  })),
);
const HooksView = lazy(() =>
  import("./components/HooksView").then((module) => ({
    default: module.HooksView,
  })),
);

const CaptureView = lazy(() => import("./components/CaptureView").then(module => ({ default: module.CaptureView })));
const TraceView = lazy(() => import("./components/TraceView").then(module => ({ default: module.TraceView })));

function ViewLoading() {
  return (
    <div className="arca-view-loading" role="status">
      <RefreshIcon data-spinning="true" />
      正在打开视图…
    </div>
  );
}

function capabilityLabel(capability: "mcp" | "model" | "skill") {
  if (capability === "mcp") return "MCP";
  if (capability === "model") return "Model";
  return "Skill";
}

async function runRefreshes(
  refreshes: Array<Pick<StartupTask, "id" | "run">>,
  concurrency = 2,
) {
  let cursor = 0;
  const failures: Array<{ task: string; error: unknown }> = [];
  const worker = async () => {
    while (cursor < refreshes.length) {
      const refresh = refreshes[cursor++];
      try {
        await refresh.run();
      } catch (error) {
        failures.push({ task: refresh.id, error });
      }
    }
  };
  await Promise.all(
    Array.from({ length: Math.min(concurrency, refreshes.length) }, worker),
  );
  return failures;
}

function App() {
  const { t } = useTranslation();
  const [view, setView] = useState<View>({ kind: "models" });
  const [addAgentOpen, setAddAgentOpen] = useState(false);
  const [mcpEditorOpen, setMcpEditorOpen] = useState(false);
  const [externalModelCandidates, setExternalModelCandidates] = useState<ModelAdoptionCandidate[]>([]);
  const [backendStatus, setBackendStatus] = useState<BackendStatus | null>(null);
  const nextResourceNavigationId = useRef(0);
  const externalModelGeneration = useRef(0);
  const [observationSubscription] = useState(() => {
    let release!: () => void;
    const ready = new Promise<void>((resolve) => { release = resolve; });
    return { ready, release };
  });
  const state = useInstallState({ autoLoad: false });
  const skillsState = useSkillsState({ autoLoad: false });
  const consumptionState = useConsumptionState({ autoLoad: false });
  const networkSettings = useNetworkSettings();
  useEffect(() => {
    getBackendStatus()
      .then(setBackendStatus)
      .catch(() => undefined);
  }, []);
  const agents = useMemo(
    () => mergeAgentInfos(state.agents, consumptionState.agents),
    [consumptionState.agents, state.agents],
  );
  const refreshExternalModels = useCallback(async () => {
    const generation = ++externalModelGeneration.current;
    const candidates = await listModelAdoptionCandidates();
    if (generation === externalModelGeneration.current) setExternalModelCandidates(candidates);
  }, []);
  const refreshResourceInventory = useCallback(async () => {
    // Start both hook generations now. A later mutation/read still supersedes
    // either result, while this pair shares exactly one filesystem scan.
    const observation = getResourceObservation();
    const [relationships, skills] = await Promise.allSettled([
      consumptionState.refresh(async () => observationValue((await observation).relationships)),
      skillsState.refreshSilently(async () => observationValue((await observation).skills)),
    ]);
    const failures = [];
    if (relationships.status === "rejected") failures.push({ task: "relationships", error: relationships.reason });
    if (skills.status === "rejected") failures.push({ task: "skills", error: skills.reason });
    if (failures.length > 0) throw failures;
  }, [consumptionState.refresh, skillsState.refreshSilently]);
  const foregroundStartupTasks = useMemo<StartupTask[]>(
    () => [
      { id: "registry", label: "registry", run: async () => { await observationSubscription.ready; return state.refreshRegistry(); } },
      { id: "agents", label: "agents", run: async () => { await observationSubscription.ready; return state.refreshAgents(); } },
      { id: "agent-capabilities", label: "agent-capabilities", run: async () => { await observationSubscription.ready; return consumptionState.refreshAgents(); } },
      { id: "relationships", label: "resource-inventory", run: async () => { await observationSubscription.ready; return refreshResourceInventory(); } },
    ],
    [
      consumptionState.refreshAgents,
      refreshResourceInventory,
      state.refreshAgents,
      state.refreshRegistry,
      observationSubscription,
    ],
  );

  const deferredStartupTasks = useMemo<StartupTask[]>(
    () => [{ id: "external-models", label: "external-models", run: refreshExternalModels }],
    [refreshExternalModels],
  );

  const startupSync = useStartupSync({
    foreground: foregroundStartupTasks,
    deferred: deferredStartupTasks,
    foregroundConcurrency: 2,
  });
  // Update checks are optional network activity, separate from local asset reads.
  const updater = useUpdater(networkSettings.settings.proxy_url, { autoCheck: startupSync.settled });
  // CLI link repair is useful but irrelevant to the first interaction. Run it
  // only after all fresh read-only startup work has settled.
  useCliTool({ start: startupSync.settled });

  const observationTasks = useMemo<Record<ObservationTaskId, () => Promise<unknown>>>(
    () => ({
      preferences: invalidatePreferences,
      agents: state.refreshAgents,
      "agent-capabilities": consumptionState.refreshAgents,
      relationships: consumptionState.refresh,
      skills: skillsState.refreshSilently,
      registry: state.refreshRegistry,
      sources: state.refreshSources,
      "external-models": refreshExternalModels,
      models: invalidateModelObservation,
    }),
    [
      consumptionState.refresh,
      consumptionState.refreshAgents,
      refreshExternalModels,
      skillsState.refreshSilently,
      state.refreshAgents,
      state.refreshRegistry,
      state.refreshSources,
    ],
  );
  const refreshObservedTasks = useCallback(
    async (taskIds: readonly ObservationTaskId[]) => {
      const ids = new Set(taskIds);
      if (ids.has("registry")) ids.delete("sources");
      const selected: Array<Pick<StartupTask, "id" | "run">> = [];
      if (ids.has("skills") && ids.has("relationships")) {
        ids.delete("skills");
        ids.delete("relationships");
        selected.push({ id: "relationships / skills", run: refreshResourceInventory });
      }
      selected.push(...[...ids].map((id) => ({ id, run: observationTasks[id] })));
      return runRefreshes(selected);
    },
    [observationTasks, refreshResourceInventory],
  );

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let refreshing = false;
    let lastFocusRefreshAt = Date.now();
    const pending = new Set<ObservationTaskId>();
    const refreshObservedState = async () => {
      timer = null;
      if (disposed || refreshing || pending.size === 0) return;
      const selected = [...pending];
      pending.clear();
      refreshing = true;
      try {
        await refreshObservedTasks(selected);
      } finally {
        refreshing = false;
        if (pending.size > 0 && !disposed) {
          schedule([]);
        }
      }
    };
    const schedule = (taskIds: readonly ObservationTaskId[]) => {
      for (const taskId of taskIds) pending.add(taskId);
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => void refreshObservedState(), 300);
    };
    const unlisten = listen<ObservationChange>(
      "asset-observation-changed",
      (event) => schedule(taskIdsForObservation(event.payload)),
    ).then((dispose) => {
      // Subscribe before the first local snapshot. Events arriving during
      // startup are already queued; update-server latency cannot disable them.
      if (!disposed) observationSubscription.release();
      return dispose;
    }).catch(() => {
      // A host without events still gets initial reads and the focus fallback.
      if (!disposed) observationSubscription.release();
      return undefined;
    });
    const scheduleFocusFallback = () => {
      if (document.visibilityState !== "visible") return;
      const now = Date.now();
      if (!focusRefreshDue(lastFocusRefreshAt, now)) return;
      lastFocusRefreshAt = now;
      schedule(ALL_OBSERVATION_TASK_IDS);
    };
    window.addEventListener("focus", scheduleFocusFallback);
    document.addEventListener("visibilitychange", scheduleFocusFallback);
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      window.removeEventListener("focus", scheduleFocusFallback);
      document.removeEventListener("visibilitychange", scheduleFocusFallback);
      void unlisten.then((dispose) => dispose?.());
    };
  }, [refreshObservedTasks, observationSubscription]);

  const openResource = useCallback((request: ResourceNavigationRequest) => {
    const id = ++nextResourceNavigationId.current;
    setView(viewForResourceIntent(createResourceNavigationIntent(id, request)));
  }, []);

  const consumeResourceIntent = useCallback((id: number) => {
    setView((current) => clearResourceIntent(current, id));
  }, []);

  const assetDiagnostics = (consumptionState.inventory?.capability_errors ?? [])
    // Host scans are shown on the affected Agent. Skill read failures already
    // have their own resource state and, at launch, the startup notice.
    .filter(({ code }) => code !== "mcp_host_observation_unavailable" && code !== "skill_inventory_unavailable");

  return (
    <Layout
      updater={updater}
      proxyUrl={networkSettings.settings.proxy_url}
      proxySettingsLoading={networkSettings.loading}
      onSaveProxy={networkSettings.save}
      agents={agents}
      view={view}
      onSelectRegistry={() => setView({ kind: "registry" })}
      onSelectModels={() => setView({ kind: "models" })}
      onSelectStartupAgents={() => setView({ kind: "startup_agents" })}
      onSelectSkills={() => setView({ kind: "skills" })}
      onSelectSubagents={() => setView({ kind: "subagents" })}
      onSelectHooks={() => setView({ kind: "hooks" })}
      onSelectAgent={(id) => setView({ kind: "agent", id })}
      onAddAgent={() => setAddAgentOpen(true)}
      onRescan={async () => {
        const failures = await refreshObservedTasks(ALL_OBSERVATION_TASK_IDS);
        if (failures.length > 0) throw failures;
      }}
      startupSync={startupSync}
    >
      <InstructionDraftGuard />
      <Suspense fallback={<ViewLoading />}>
        {view.kind === "traces" ? (<TraceView initialAgentId={view.agentId} />) : view.kind === "capture" ? (<CaptureView key={view.agentId} agents={agents} agentId={view.agentId} arcaProxy={networkSettings.settings.proxy_url} />) : view.kind === "skills" ? (
          <SkillsView
            state={skillsState}
            onOpenAgent={(id) => setView({ kind: "agent", id, tab: "skills" })}
            intent={view.intent}
            onIntentConsumed={consumeResourceIntent}
          />
        ) : view.kind === "models" ? (
          <ModelsView
            consumptionState={consumptionState}
            intent={view.intent}
            onIntentConsumed={consumeResourceIntent}
          />
        ) : view.kind === "startup_agents" ? (
          <StartupAgentsView
            consumptionState={consumptionState}
            intent={view.intent}
            onIntentConsumed={consumeResourceIntent}
            onOpenAgent={(id) => setView({ kind: "agent", id, tab: "agents" })}
          />
        ) : view.kind === "subagents" ? (
          <SubagentsView
            consumptionState={consumptionState}
            intent={view.intent}
            onIntentConsumed={consumeResourceIntent}
            onOpenAgent={(id) => setView({ kind: "agent", id, tab: "subagents" })}
          />
        ) : view.kind === "hooks" ? (
          <HooksView
            consumptionState={consumptionState}
            intent={view.intent}
            onIntentConsumed={consumeResourceIntent}
            onOpenAgent={(id) => setView({ kind: "agent", id, tab: "hooks" })}
          />
        ) : view.kind === "agent" ? (
          <AgentView
            key={view.id}
            state={state}
            skillsState={skillsState}
            consumptionState={consumptionState}
            agentId={view.id}
            initialTab={view.tab}
            externalModelCandidates={externalModelCandidates}
            onOpenResource={openResource}
            onOpenCapture={(agentId) => setView({ kind: "capture", agentId })}
            onOpenTrace={(agentId) => setView({ kind: "traces", agentId })}
          />
        ) : (
          <RegistryView
            state={state}
            consumptionState={consumptionState}
            intent={view.intent}
            onIntentConsumed={consumeResourceIntent}
            onCreate={() => setMcpEditorOpen(true)}
            suppressOperationReview={mcpEditorOpen}
            onRetryLoad={startupSync.retryFailed}
            retryLoadDisabled={startupSync.syncing}
          />
        )}
      </Suspense>

      {addAgentOpen && (
        <AddAgentDialog
          onClose={() => setAddAgentOpen(false)}
          onAdded={async () => {
            await Promise.allSettled([
              state.refreshAgents(),
              consumptionState.refreshAgents(),
              refreshResourceInventory(),
            ]);
          }}
        />
      )}

      {mcpEditorOpen && (
        <RegistryEditPage
          state={state}
          consumptionState={consumptionState}
          name={null}
          onBack={() => setMcpEditorOpen(false)}
        />
      )}

      <ToastNotice kind="warning" noticeKey="backend-recovery" message={backendStatus?.state === "read_only"
        ? t("migration.recoveryTitle") : null} details={backendStatus?.state === "read_only" ? {
          ...backendStatus, description: t("migration.recoveryMessage", { stage: backendStatus.stage }),
        } : null} />
      <ToastNotice kind="warning" noticeKey="backend-capability" message={backendStatus?.state === "capability_unavailable"
        ? t("migration.capabilityTitle") : null} details={backendStatus?.state === "capability_unavailable" ? {
          ...backendStatus, description: t("migration.capabilityMessage", { stage: backendStatus.stage }),
        } : null} />
      <ToastNotice kind="warning" noticeKey="asset-observation"
        message={backendStatus?.state !== "capability_unavailable" && assetDiagnostics.length > 0
          ? t("observations.incompleteTitle", { capabilities: [...new Set(assetDiagnostics.map(({ capability }) => capabilityLabel(capability)))].join(" / ") })
          : null}
        details={assetDiagnostics.map((diagnostic) => ({
          ...diagnostic,
          message: diagnostic.code === "skill_recovery_required"
            ? skillsState.inventory?.recovery_error ?? t("observations.skillRecoveryRequired")
            : t("observations.capabilityFallback"),
        }))} />

      <UpdateBanner updater={updater} />
      <CredentialSession />
    </Layout>
  );
}

export default App;
