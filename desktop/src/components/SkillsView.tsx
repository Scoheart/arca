import { ResourceIcon } from "./resourcePresentation";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  type ReactNode,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  normalizeSkillCommandError,
  type SkillsState,
} from "../hooks/useSkillsState";
import * as api from "../lib/api";
import {
  aggregateSkillsByName,
} from "../lib/skills";
import { groupSkillSources, skillSourceGroup, skillConsumerAgents, type SkillSourceCategory } from "../lib/skillSources";
import type {
  OperationPlan,
  SkillCommandError,
  SkillDetail,
  SkillNavigationIntent,
  SkillsInventory,
} from "../lib/types";
import {
  FolderIcon,
  LinkIcon,
  PackageIcon,
  PlusIcon,
  RefreshIcon,
} from "./icons";
import { SkillCard } from "./SkillCard";
import { DialogShell } from "./DialogShell";
import { ResourceState } from "./ResourceState";
import { SkillInstallDialog } from "./SkillInstallDialog";
import {
  SkillInspector,
  type SkillLifecycleIntent,
} from "./SkillInspector";
import { SkillReviewDialog } from "./SkillReviewDialog";
import { useToast } from "./Toast";
import {
  ResourceWorkspace,
  ResourceOverview,
  SidebarItem,
  SidebarSection,
  WorkspaceSidebar,
} from "./ResourceWorkspace";

const sourceCategories: { category: SkillSourceCategory; icon: ReactNode }[] = [
  { category: "github", icon: <LinkIcon className="w-3.5 h-3.5" /> },
  { category: "local", icon: <FolderIcon className="w-3.5 h-3.5" /> },
  { category: "archive", icon: <PackageIcon className="w-3.5 h-3.5" /> },
  { category: "imported", icon: <FolderIcon className="w-3.5 h-3.5" /> },
  { category: "unknown", icon: <PackageIcon className="w-3.5 h-3.5" /> },
];

interface SkillsViewProps {
  state: SkillsState;
  intent?: SkillNavigationIntent;
  onIntentConsumed?(id: number): void;
  onOpenAgent?(id: string): void;
}

interface LifecycleReview {
  plan: OperationPlan;
}

export function SkillsView({
  state,
  intent,
  onIntentConsumed,
  onOpenAgent,
}: SkillsViewProps) {
  const { t } = useTranslation();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [source, setSource] = useState("all");
  const [checking, setChecking] = useState(false);
  const [removeGroup, setRemoveGroup] = useState<{ label: string; names: string[] } | null>(null);
  const [removingGroup, setRemovingGroup] = useState(false);
  const removingGroupRef = useRef(false);
  const [selectedIdentity, setSelectedIdentity] = useState<string | null>(null);
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<SkillCommandError | null>(null);
  const [installOpen, setInstallOpen] = useState(false);
  const [navigationNotice, setNavigationNotice] = useState<string | null>(null);
  const [lifecycleReview, setLifecycleReview] =
    useState<LifecycleReview | null>(null);
  const [lifecyclePlanning, setLifecyclePlanning] = useState(false);
  const [recoveryRequired, setRecoveryRequired] = useState<string | null>(null);
  const detailGeneration = useRef(0);
  const lifecycleGeneration = useRef(0);
  const lifecyclePlanRef = useRef<OperationPlan | null>(null);
  const lifecyclePendingRef = useRef(false);
  const lifecycleCommittedRef = useRef(new Set<string>());
  const lifecycleRecoveryRef = useRef(new Set<string>());
  const lifecycleCancellationsRef = useRef(new Map<string, Promise<void>>());
  const lifecycleCommitRef = useRef<{
    operationId: string;
    promise: Promise<SkillsInventory>;
  } | null>(null);
  const cancelRef = useRef(state.cancel);
  const toastRef = useRef(toast);
  const mounted = useRef(true);
  const lastConsumedIntentId = useRef<number | null>(null);
  cancelRef.current = state.cancel;
  toastRef.current = toast;
  const items = useMemo(
    () => aggregateSkillsByName(state.inventory?.items ?? []),
    [state.inventory?.items],
  );
  const sources = useMemo(() => groupSkillSources(items), [items]);
  const selectedSource = sources.find((group) => group.id === source);
  const sourceLocation = selectedSource?.category === "github"
    ? `https://github.com/${selectedSource.path}`
    : selectedSource?.path;
  const activeSource = selectedSource?.id ?? "all";
  const consumers = useMemo(() => skillConsumerAgents(state.inventory), [state.inventory]);
  const agentNames = useMemo(() => new Map(
    state.inventory?.agents.map((agent) => [agent.id, agent.name]) ?? [],
  ), [state.inventory?.agents]);
  const deferredQuery = useDeferredValue(query);
  const searchIndex = useMemo(() => items.map((item) => ({
    item, source: skillSourceGroup(item).id,
    text: `${item.name} ${item.description}`.toLowerCase(),
  })), [items]);
  const filtered = useMemo(() => {
    const needle = deferredQuery.trim().toLowerCase();
    return searchIndex.filter((row) =>
      (activeSource === "all" || row.source === activeSource) && (!needle || row.text.includes(needle)),
    ).map((row) => row.item);
  }, [searchIndex, activeSource, deferredQuery]);
  const selected = selectedIdentity
    ? state.inventory?.items.find((item) => item.identity === selectedIdentity) ?? null
    : null;
  // External copies deliberately have no managed content fingerprint. A new
  // inventory snapshot must invalidate their open detail even at the same path.
  const unversionedDetailSnapshot = selected && selected.content_hash === null ? state.inventory : null;
  const selectedCopies = useMemo(() => selected
    ? (state.inventory?.items ?? []).filter((item) => item.name === selected.name)
    : [], [selected?.name, state.inventory?.items]);
  useEffect(() => {
    if (source !== "all" && !selectedSource) setSource("all");
  }, [source, selectedSource]);
  const recoveryError =
    recoveryRequired ??
    state.inventory?.recovery_error ??
    (state.error?.code === "recovery_required" ? state.error.message : null);
  const checkDisabled =
    checking ||
    removingGroup ||
    lifecycleReview !== null ||
    lifecyclePlanning ||
    state.loading ||
    state.pendingOperation !== null ||
    recoveryError !== null;

  const cancelLifecycleOnce = useCallback(
    (operationId: string, reportError: boolean) => {
      const existing = lifecycleCancellationsRef.current.get(operationId);
      if (existing) return existing;
      const pending = cancelRef.current(operationId).catch((reason: unknown) => {
        if (reportError && mounted.current) {
          toastRef.current.show({
            kind: "error",
            msg: normalizeSkillCommandError(reason).message,
          });
        }
      });
      lifecycleCancellationsRef.current.set(operationId, pending);
      return pending;
    },
    [],
  );

  const cleanupLifecyclePlan = useCallback(
    async (plan: OperationPlan, reportError: boolean) => {
      const committing = lifecycleCommitRef.current;
      if (committing?.operationId === plan.operation_id) {
        try {
          await committing.promise;
        } catch {
          // A failed commit leaves this plan staged unless recovery owns it.
        }
      }
      if (
        !lifecycleCommittedRef.current.has(plan.operation_id) &&
        !lifecycleRecoveryRef.current.has(plan.operation_id)
      ) {
        await cancelLifecycleOnce(plan.operation_id, reportError);
      }
    },
    [cancelLifecycleOnce],
  );

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      detailGeneration.current += 1;
      lifecycleGeneration.current += 1;
      const plan = lifecyclePlanRef.current;
      if (plan) void cleanupLifecyclePlan(plan, false);
    };
  }, [cleanupLifecyclePlan]);

  const closeInspector = useCallback(() => {
    detailGeneration.current += 1;
    lifecycleGeneration.current += 1;
    setSelectedIdentity(null);
    setDetail(null);
    setDetailLoading(false);
    setDetailError(null);
  }, []);

  const planLifecycle = async (intent: SkillLifecycleIntent) => {
    if (
      lifecyclePendingRef.current ||
      lifecyclePlanRef.current ||
      state.pendingOperation !== null ||
      recoveryError
    ) {
      return;
    }

    const generation = ++lifecycleGeneration.current;
    lifecyclePendingRef.current = true;
    setLifecyclePlanning(true);
    try {
      const plan = await (() => {
        switch (intent.kind) {
          case "import":
            return state.plan({
              operation: "import_skill",
              request: {
                identity: intent.identity,
                replace_conflicts: intent.replaceConflicts,
              },
            });
          case "update":
            return state.plan({
              operation: "update_skill",
              request: {
                skill_name: intent.skillName,
                replace_local_changes: intent.replaceLocalChanges,
              },
            });
          case "remove":
            return state.plan({
              operation: "remove_skill",
              request: { skill_name: intent.skillName },
            });
          case "repair":
            return state.plan({
              operation: "repair_skill",
              request: {
                skill_name: intent.skillName,
                repair: intent.repair,
              },
            });
        }
      })();

      if (!mounted.current || lifecycleGeneration.current !== generation) {
        await cancelLifecycleOnce(plan.operation_id, false);
        return;
      }
      lifecyclePlanRef.current = plan;
      const routineUpdate = intent.kind === "update"
        && !intent.replaceLocalChanges
        && !plan.requires_risk_override
        && plan.skills.every((skill) => !skill.existing_states.some((value) =>
          value === "locally_modified" || value === "conflicting_link" || value === "broken_link"))
        && plan.targets.every((target) => target.expected === "managed" || target.expected === "missing");
      const routineRepair = intent.kind === "repair"
        && !plan.requires_risk_override
        && plan.skills.every((skill) => !skill.replace_existing)
        && plan.targets.every((target) => target.expected === "managed" || target.expected === "missing" || target.expected === "broken");
      if ((intent.kind === "import" && !plan.requires_risk_override) || routineUpdate || routineRepair) {
        const inventory = await commitLifecycle(plan, null);
        if (lifecyclePlanRef.current?.operation_id === plan.operation_id) {
          lifecyclePlanRef.current = null;
        }
        if (mounted.current && lifecycleGeneration.current === generation) {
          toast.show({ kind: "success", msg: routineUpdate ? "Skill 已更新。" : routineRepair ? "Skill 已修复。" : "Skill 已导入。" });
          const selectedName = selected?.name;
          if (
            selectedName &&
            (routineUpdate || !inventory.items.some((item) => item.name === selectedName))
          ) {
            closeInspector();
          }
        }
        return;
      }
      // Lifecycle confirmation replaces the detail Inspector instead of stacking
      // a second full-size dialog over the selected Skill.
      detailGeneration.current += 1;
      setSelectedIdentity(null);
      setDetail(null);
      setDetailLoading(false);
      setDetailError(null);
      setLifecycleReview({ plan });
    } catch (reason) {
      const error = normalizeSkillCommandError(reason);
      const stillCurrent =
        mounted.current && lifecycleGeneration.current === generation;
      if (error.code === "recovery_required") {
        if (stillCurrent) {
          enterRecovery(error.message);
        } else if (lifecyclePlanRef.current) {
          lifecyclePlanRef.current = null;
        }
      } else {
        const failedPlan = lifecyclePlanRef.current;
        lifecyclePlanRef.current = null;
        if (failedPlan) await cleanupLifecyclePlan(failedPlan, false);
        if (stillCurrent) {
          toast.show({ kind: "error", msg: error.message });
        }
      }
    } finally {
      lifecyclePendingRef.current = false;
      if (mounted.current) setLifecyclePlanning(false);
    }
  };

  const removeCollection = async () => {
    if (!removeGroup || removingGroupRef.current || checkDisabled) return;
    const { names } = removeGroup;
    removingGroupRef.current = true;
    setRemovingGroup(true);
    lifecyclePendingRef.current = true;
    let removed = 0;
    let pending: OperationPlan | null = null;
    try {
      for (const name of names) {
        pending = await state.plan({ operation: "remove_skill", request: { skill_name: name } });
        await commitLifecycle(pending, pending.requires_risk_override ? pending.findings_hash : null);
        pending = null;
        removed += 1;
      }
      if (mounted.current) {
        setRemoveGroup(null);
        closeInspector();
        toast.show({ kind: "success", msg: `已删除 ${removed} 个 Skill。` });
      }
    } catch (reason) {
      const error = normalizeSkillCommandError(reason);
      if (error.code === "recovery_required") {
        if (mounted.current) enterRecovery(error.message);
      } else if (pending) {
        await cleanupLifecyclePlan(pending, false);
      }
      if (mounted.current) {
        setRemoveGroup(null);
        toast.show({ kind: "error", msg: `已删除 ${removed}/${names.length} 个 Skill，剩余项已停止处理：${error.message}` });
      }
    } finally {
      lifecyclePendingRef.current = false;
      removingGroupRef.current = false;
      if (mounted.current) setRemovingGroup(false);
    }
  };

  const closeLifecycleReview = () => {
    const plan = lifecyclePlanRef.current;
    lifecyclePlanRef.current = null;
    setLifecycleReview(null);
    if (plan) return cleanupLifecyclePlan(plan, true);
  };

  const commitLifecycle: SkillsState["commit"] = (plan, confirmation) => {
    const pending = state.commit(plan, confirmation);
    lifecycleCommitRef.current = {
      operationId: plan.operation_id,
      promise: pending,
    };
    void pending
      .then(
        () => {
          lifecycleCommittedRef.current.add(plan.operation_id);
        },
        (reason: unknown) => {
          if (normalizeSkillCommandError(reason).code === "recovery_required") {
            lifecycleRecoveryRef.current.add(plan.operation_id);
          }
          throw reason;
        },
      )
      .finally(() => {
        if (lifecycleCommitRef.current?.promise === pending) {
          lifecycleCommitRef.current = null;
        }
      })
      .catch(() => undefined);
    return pending;
  };

  const lifecycleCommitted = (
    inventory: SkillsInventory,
    message = "Skill 操作已完成。",
  ) => {
    const plan = lifecyclePlanRef.current;
    if (plan) lifecycleCommittedRef.current.add(plan.operation_id);
    lifecyclePlanRef.current = null;
    setLifecycleReview(null);
    toast.show({ kind: "success", msg: message });
    const selectedName = selected?.name;
    if (selectedName && !inventory.items.some((item) => item.name === selectedName)) {
      closeInspector();
    }
  };

  const enterRecovery = (message: string) => {
    const plan = lifecyclePlanRef.current;
    if (plan) lifecycleRecoveryRef.current.add(plan.operation_id);
    lifecyclePlanRef.current = null;
    setLifecycleReview(null);
    setInstallOpen(false);
    setRecoveryRequired(message);
  };

  const changeQuery = (value: string) => {
    closeInspector();
    setQuery(value);
  };

  const changeSource = (value: string) => {
    closeInspector();
    setSource(value);
  };

  const openSkill = useCallback((identity: string) => {
    setNavigationNotice(null);
    setSelectedIdentity(identity);
  }, []);
  const cards = useMemo(() => filtered.map((item) => (
    <div role="listitem" key={item.identity}>
      <SkillCard item={item} selected={item.name === selected?.name}
        onOpen={() => openSkill(item.identity)} agentIds={consumers.get(item.name) ?? []}
        agentNames={agentNames} onOpenAgent={onOpenAgent} />
    </div>
  )), [filtered, selected?.name, openSkill, consumers, agentNames, onOpenAgent]);

  useEffect(() => {
    if (!recoveryError) return;
    setInstallOpen(false);
  }, [recoveryError]);

  useEffect(() => {
    const inventory = state.inventory;
    if (
      !intent ||
      lastConsumedIntentId.current === intent.id ||
      (!inventory && !recoveryError)
    ) {
      return;
    }

    lastConsumedIntentId.current = intent.id;
    setNavigationNotice(null);
    if (!inventory) {
      closeInspector();
      setInstallOpen(false);
    } else {
      const item = inventory.items.find(
        (candidate) =>
          candidate.name === intent.skillName &&
          candidate.location.kind === "central",
      );
      if (item) {
        setQuery("");
        setSource("all");
        setSelectedIdentity(item.identity);
      } else {
        closeInspector();
        setNavigationNotice(
          `未找到 Skill 资产“${intent.skillName}”。`,
        );
      }
    }
    onIntentConsumed?.(intent.id);
  }, [closeInspector, intent, onIntentConsumed, recoveryError, state.inventory]);

  useEffect(() => {
    if (
      selectedIdentity &&
      (!selected || !filtered.some((item) => item.name === selected.name))
    ) {
      closeInspector();
    }
  }, [closeInspector, filtered, selectedIdentity, selected?.name]);

  useEffect(() => {
    if (!selected) return;

    const generation = ++detailGeneration.current;
    let active = true;
    setDetail(null);
    setDetailError(null);
    setDetailLoading(true);

    void api
      .getSkillDetail(selected.identity)
      .then((next) => {
        if (active && detailGeneration.current === generation) setDetail(next);
      })
      .catch((reason: unknown) => {
        if (active && detailGeneration.current === generation) {
          setDetailError(normalizeSkillCommandError(reason));
        }
      })
      .finally(() => {
        if (active && detailGeneration.current === generation) {
          setDetailLoading(false);
        }
      });

    return () => {
      active = false;
      if (detailGeneration.current === generation) detailGeneration.current += 1;
    };
  }, [selected?.identity, selected?.content_hash, selected?.updated_at,
    selected?.location.kind === "agent_target" ? selected.location.global_dir : null,
    unversionedDetailSnapshot]);

  const checkUpdates = async () => {
    if (checkDisabled) return;
    setChecking(true);
    try {
      await state.checkUpdates(true);
    } catch {
      // The app-owned hook retains and presents the structured error.
    } finally {
      if (mounted.current) setChecking(false);
    }
  };

  const retry = () => {
    void state.refresh().catch(() => undefined);
  };

  const stateNotice = recoveryError ? (
    <div className="arca-skill-notice" data-tone="recovery" role="status">
      <strong>Skills 已进入只读恢复状态</strong>
      <span>{recoveryError}</span>
    </div>
  ) : state.error && state.inventory ? (
    <div className="arca-skill-notice" data-tone="error" role="status">
      <strong>最近一次操作未完成</strong>
      <span>{state.error.message}</span>
      {state.error.retry_at && <code>可重试时间：{state.error.retry_at}</code>}
    </div>
  ) : null;
  const inventoryNotice = navigationNotice || stateNotice ? (
    <>
      {navigationNotice && (
        <div className="arca-skill-notice" data-tone="error" role="status">
          <strong>{navigationNotice}</strong>
        </div>
      )}
      {stateNotice}
    </>
  ) : null;

  return (
    <div className="arca-skill-workspace">
      <ResourceWorkspace
        overview={
          <ResourceOverview
            eyebrow={selectedSource
              ? `${t(`skillLibrary.${selectedSource.category}`)} · ${selectedSource.count} Skills`
              : `${items.length} Skills · ${sources.length} ${t("skillLibrary.sources")}`}
            title={selectedSource ? selectedSource.label || t("skillLibrary.unknown") : t("skillLibrary.all")}
            description={selectedSource ? undefined : t("skillLibrary.description")}
            icon={<ResourceIcon domain="skill" className="w-6 h-6" />}
            actions={selectedSource && items.some((item) => skillSourceGroup(item).id === selectedSource.id && item.location.kind === "central") ? (
              <button type="button" className="btn-ghost" disabled={checkDisabled || removingGroup}
                onClick={() => setRemoveGroup({ label: selectedSource.label || t("skillLibrary.unknown"), names: items.filter((item) => skillSourceGroup(item).id === selectedSource.id && item.location.kind === "central").map((item) => item.name) })}>
                删除集合
              </button>
            ) : undefined}
          >
            {sourceLocation && <dl className="arca-resource-overview-facts">
              <div>
                <dt>{selectedSource?.category === "github" ? "GitHub" : t("skillLibrary.sources")}</dt>
                <dd>{selectedSource?.category === "github" ? (
                  <a href={sourceLocation} className="arca-inline-link" title="在浏览器中打开 GitHub"
                    onClick={(event) => {
                      event.preventDefault();
                      void openUrl(sourceLocation).catch(() => toast.show({ kind: "error", msg: "无法打开 GitHub，请稍后重试。" }));
                    }}>
                    <code>{sourceLocation}</code>
                  </a>
                ) : <code title={sourceLocation}>{sourceLocation}</code>}</dd>
              </div>
            </dl>}
          </ResourceOverview>
        }
        sidebar={
          <WorkspaceSidebar title="Skills" count={items.length}
            actions={
              <button
                className="btn-primary"
                type="button"
                disabled={checkDisabled}
                onClick={() => setInstallOpen(true)}
              >
                <PlusIcon className="w-4 h-4" />
                添加 Skills
              </button>
            }
            footer={
              <button
                className="btn-ghost"
                type="button"
                disabled={checkDisabled}
                onClick={() => void checkUpdates()}
              >
                <span
                  className="arca-skill-check-icon"
                  data-busy={checking ? "true" : undefined}
                  aria-hidden="true"
                >
                  <RefreshIcon className="w-4 h-4" />
                </span>
                {checking ? "检查中…" : "检查更新"}
              </button>
            }
          >
            <SidebarSection title={t("skillLibrary.sources")}>
              <SidebarItem active={activeSource === "all"}
                icon={<ResourceIcon domain="skill" className="w-3.5 h-3.5" />}
                label={t("skillLibrary.all")} count={items.length}
                onClick={() => changeSource("all")} />
            </SidebarSection>
            {sourceCategories.map(({ category, icon }) => {
              const groups = sources.filter((group) => group.category === category);
              if (!groups.length) return null;
              return <SidebarSection key={category} title={t(`skillLibrary.${category}`)}>
                {groups.map((group) => <SidebarItem key={group.id}
                  active={activeSource === group.id} icon={icon}
                  label={group.label || t("skillLibrary.unknown")} title={group.path || undefined}
                  count={group.count} onClick={() => changeSource(group.id)} />)}
              </SidebarSection>;
            })}
          </WorkspaceSidebar>
        }
        query={query}
        onQueryChange={changeQuery}
        searchPlaceholder={activeSource === "all" ? "搜索 Skills" : t("skillLibrary.searchSource")}
        listLabel="Skills"
        resultCount={filtered.length}
        inspector={
          selected ? (
            <SkillInspector
              item={selected}
              copies={selectedCopies}
              onSelectCopy={setSelectedIdentity}
              detail={detail?.item.identity === selected.identity ? detail : null}
              loading={detailLoading}
              error={detailError}
              onClose={closeInspector}
              onPlan={(intent) => void planLifecycle(intent)}
              planning={lifecyclePlanning}
              readOnly={removingGroup || recoveryError !== null || state.pendingOperation !== null}
            />
          ) : undefined
        }
        onInspectorClose={closeInspector}
      >
        {!state.inventory && state.loading ? (
          <ResourceState
            kind="loading"
            title="正在读取 Skills…"
          />
        ) : !state.inventory && recoveryError ? (
          <ResourceState
            kind="recovery"
            icon={<ResourceIcon domain="skill" className="w-6 h-6" />}
            title="Skills 已进入只读恢复状态"
            detail={recoveryError}
          />
        ) : !state.inventory && state.error ? (
          <ResourceState
            kind="read-error"
            icon={<ResourceIcon domain="skill" className="w-6 h-6" />}
            title="读取 Skills 失败"
            detail={state.error.message}
            action={
              <button className="btn-primary" type="button" onClick={retry}>
                重试
              </button>
            }
          />
        ) : filtered.length === 0 ? (
          <>
            {inventoryNotice}
            <ResourceState
              kind={items.length === 0 ? "empty" : "no-match"}
              icon={<ResourceIcon domain="skill" className="w-6 h-6" />}
              title={items.length === 0 ? "暂无 Skills" : "没有匹配项"}
              detail={items.length === 0 ? "从 GitHub、本地文件夹或压缩包添加。" : "调整搜索或来源筛选后重试。"}
              action={items.length === 0 ? undefined : (
                <button className="btn-secondary" type="button" onClick={() => {
                  setQuery("");
                  setSource("all");
                }}>清除筛选</button>
              )}
            />
          </>
        ) : (
          <>
            {inventoryNotice}
            <div
              className="arca-skill-card-grid"
              role="list"
              aria-label={t("centralAssets.skillList")}
            >
              {cards}
            </div>
          </>
        )}
      </ResourceWorkspace>
      {installOpen && state.inventory && !recoveryError && (
        <SkillInstallDialog
          plan={state.plan}
          commit={state.commit}
          cancel={state.cancel}
          onClose={() => {
            setInstallOpen(false);
          }}
          onCommitted={() => {
            setInstallOpen(false);
            toast.show({ kind: "success", msg: "Skill 已添加。" });
          }}
          onRecoveryRequired={enterRecovery}
        />
      )}
      {removeGroup && (
        <DialogShell kind="review" title="删除 Skill 集合" subtitle={removeGroup.label}
          busy={removingGroup} onClose={() => setRemoveGroup(null)}
          footerEnd={<><button type="button" className="btn-ghost" disabled={removingGroup} onClick={() => setRemoveGroup(null)}>取消</button>
            <button type="button" className="btn-primary" disabled={removingGroup || checkDisabled} onClick={() => void removeCollection()}>{removingGroup ? "删除中…" : `删除 ${removeGroup.names.length} 个 Skill`}</button></>}>
          <p>删除此集合中的 {removeGroup.names.length} 个中央 Skill，并解除对应的 Agent 分配。Core 会保留恢复备份。</p>
          <p>外部扫描到的文件保持只读。各项独立提交，如遇失败将停止，已完成的删除会保留。</p>
        </DialogShell>
      )}
      {lifecycleReview && (
        <SkillReviewDialog
          plan={lifecycleReview.plan}
          onCommit={commitLifecycle}
          onClose={closeLifecycleReview}
          onCommitted={lifecycleCommitted}
          onRecoveryRequired={enterRecovery}
        />
      )}
    </div>
  );
}
