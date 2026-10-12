import { RESOURCE_CATEGORIES, RESOURCE_PRESENTATION } from "./resourcePresentation";
import capabilityGuides from "../../../data/agent-capability-guides.json";
import { AgentLaunchAction } from "./AgentLaunchAction";
import { useAgentLauncher } from "../lib/agentLauncherContext";
import "./AgentOverview.css";
import "./CaptureView.css";
import agentDocsHome from "../../../data/agent-docs-home.json";
import { useModelObservationRevision } from "../lib/modelObservation";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { InstallState } from "../hooks/useInstallState";
import type { SkillsState } from "../hooks/useSkillsState";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import { useMcpIconPreferences } from "../hooks/useMcpIconPreferences";
import type {
  AgentConsumptionSelection,
  AssetOperationPlan,
  AssetRef,
  ConsumptionView,
  ConvergenceAction,
  ModelAdoptionCandidate,
  ModelAgentView,
  ModelProfileView,
  OperationPlan,
  ResourceNavigationRequest,
} from "../lib/types";
import { formatError } from "../lib/format";
import { keyOf, transportOf } from "../lib/mcp";
import { consumptionsForAgent, externalForAgent } from "../lib/consumption";
import { requiresAgentReview } from "../lib/agentOperation";
import { listModelAgents, listModelProfiles } from "../lib/api";
import {
  EditIcon,
  ChevronDownIcon,
  DocumentIcon,
  ExternalLinkIcon,
  FolderIcon,
  LinkIcon,
  PlusIcon,
  RefreshIcon,
  NetworkIcon,
} from "./icons";
import { Avatar, Badge } from "./ui";
import { AgentGlyph } from "./brandIcons";
import { ProviderGlyph } from "./providerIcons";
import { AgentConfigurationDialog } from "./AgentConfigurationDialog";
import { useToast } from "./Toast";
import { AgentResourcePanel, type AgentResourceTab } from "./AgentResourcePanel";
import { AgentConsumptionPanel } from "./AgentConsumptionPanel";
import {
  ConsumptionPickerDialog,
  type ConsumptionPickerOption,
} from "./ConsumptionPickerDialog";
import { AssetOperationReviewDialog } from "./AssetOperationReviewDialog";
import { ReviewDialog } from "./ReviewDialog";
import { mergeAgentInfos } from "../lib/agentCapabilities";
import { SkillReviewDialog } from "./SkillReviewDialog";
import { useTranslation } from "react-i18next";
import { McpAvatar } from "./McpIcon";

type PickerDomain = "mcp" | "model" | "skill";
type ConfigLocationKind = "file" | "folder";

function absoluteConfigLocation(path: string, home: string) {
  const value = path.trim();
  const normalizedHome = home.replace(/\/$/, "");
  if (value === "~" && normalizedHome) return normalizedHome;
  if (value.startsWith("~/") && normalizedHome) return `${normalizedHome}/${value.slice(2)}`;
  return value;
}

function modelProtocolLabel(protocol: ModelProfileView["protocol"]) {
  if (protocol === "anthropic-messages") return "Anthropic Messages";
  if (protocol === "openai-responses") return "OpenAI Responses";
  if (protocol === "gemini-generate-content") return "Gemini GenerateContent";
  return "OpenAI Chat Completions";
}

function modelProviderIcon(provider: string | undefined, name: string, size = 28) {
  return <ProviderGlyph id={provider?.trim() || "custom"} name={name} size={size} />;
}

function modelCompatibilityReason(profile: ModelProfileView, agent: ModelAgentView | null) {
  if (!agent || agent.mode !== "managed") return "此 Agent 不支持 MUX Model 管理";
  if (!agent.supported_protocols.includes(profile.protocol)) return "协议不兼容";
  return null;
}

interface AgentViewProps {
  state: InstallState;
  skillsState: SkillsState;
  consumptionState: ConsumptionState;
  agentId: string;
  initialTab?: AgentResourceTab;
  externalModelCandidates?: ModelAdoptionCandidate[];
  onOpenResource?(request: ResourceNavigationRequest): void;
  onOpenCapture?(agentId: string): void;
  onOpenTrace?(agentId: string): void;
}

function completedMessage(plan: AssetOperationPlan, agentName: string) {
  if (plan.kind === "clear-mcp") return `${agentName} 的全部 MCP 已移除。`;
  if (plan.kind === "clear-models") return `${agentName} 的全部 Model 已从权威配置中移除。`;
  const domain = plan.domain_plan.domain;
  const asset = domain === "mcp" ? "MCP" : domain === "model" ? "Model" : "Skill";
  const hasAdd = plan.relationship_changes.some((change) => change.action === "add");
  const hasRemove = plan.relationship_changes.some((change) => change.action === "remove");
  const removesEveryModel = plan.domain_plan.domain === "model"
    && Object.values(plan.domain_plan.after).every(
      (selection) => Object.keys(selection.profiles).length === 0,
    );
  if (domain === "model" && hasAdd) return `Model 已添加到 ${agentName}。`;
  if (hasAdd && !hasRemove) return `${asset} 已添加到 ${agentName}。`;
  if (hasRemove && !hasAdd && removesEveryModel) return `${agentName} 的全部 Model 已移除。`;
  if (hasRemove && !hasAdd) return `${asset} 已从 ${agentName} 移除。`;
  if (domain === "model") {
    if (plan.model_state_changes.some((change) => change.reason === "model_disabled")) {
      return `${agentName} 的 Model 已停用。`;
    }
    if (plan.model_state_changes.some((change) => change.reason === "model_enabled")) {
      return `${agentName} 的 Model 已启用。`;
    }
    return `${agentName} 的当前 Model 已更新。`;
  }
  return `${agentName} 的 ${asset} 已更新。`;
}

export function AgentView({
  state,
  skillsState,
  consumptionState,
  agentId,
  initialTab = "mcps",
  externalModelCandidates = [],
  onOpenResource,
  onOpenCapture,
  onOpenTrace,
}: AgentViewProps) {
  const { t } = useTranslation();
  const { entries, refreshAgents } = state;
  const { show: showToast } = useToast();
  const agentLauncher = useAgentLauncher();
  const [editingAgent, setEditingAgent] = useState(false);
  const [editLaunchFirst, setEditLaunchFirst] = useState(false);
  const [pickerDomain, setPickerDomain] = useState<PickerDomain | null>(null);
  const [modelProfiles, setModelProfiles] = useState<ModelProfileView[]>([]);
  const [modelAgents, setModelAgents] = useState<ModelAgentView[]>([]);
  const [modelsLoading, setModelsLoading] = useState(true);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [resourceTab, setResourceTab] = useState<AgentResourceTab>(initialTab);
  const mcpIcons = useMcpIconPreferences(resourceTab === "mcps");
  const [preparingChange, setPreparingChange] = useState(false);
  const [togglingMcp, setTogglingMcp] = useState<{
    key: string;
    enabled: boolean;
  } | null>(null);
  const [togglingAllMcp, setTogglingAllMcp] = useState<{ enabled: boolean } | null>(null);
  const [togglingSkill, setTogglingSkill] = useState<{
    name: string;
    enabled: boolean;
  } | null>(null);
  const [changingModel, setChangingModel] = useState<{ profileId: string } | null>(null);
  const [assetConvergence, setAssetConvergence] = useState<{ operationId: string; action: ConvergenceAction } | null>(null);
  const [skillConvergencePlan, setSkillConvergencePlan] = useState<OperationPlan | null>(null);
  const [userHome, setUserHome] = useState("");

  const navigateResource = useCallback((request: ResourceNavigationRequest) => {
    onOpenResource?.(request);
  }, [onOpenResource]);

  useEffect(() => {
    let active = true;
    homeDir().then((path) => {
      if (active) setUserHome(path);
    }).catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  const agents = useMemo(
    () => mergeAgentInfos(state.agents, consumptionState.agents),
    [consumptionState.agents, state.agents],
  );
  const agent = useMemo(
    () => agents.find((item) => item.id === agentId) ?? null,
    [agentId, agents],
  );

  useEffect(() => {
    if (agent && !agent.has_global && agent.skills_global_dir) {
      setResourceTab("skills");
    }
  }, [agent?.has_global, agent?.id, agent?.skills_global_dir]);

  const modelRevision = useModelObservationRevision();
  const modelQueryGeneration = useRef(0);
  const needsModelProfiles = resourceTab === "models" || pickerDomain === "model";
  const pendingModelQuery = useRef<{
    includeProfiles: boolean;
    revision: number;
    promise: Promise<[ModelProfileView[] | null, ModelAgentView[]]>;
  } | null>(null);
  const refreshModels = useCallback(async (reusePending = false) => {
    const generation = ++modelQueryGeneration.current;
    let request = pendingModelQuery.current;
    if (!reusePending || !request || request.includeProfiles !== needsModelProfiles
      || request.revision !== modelRevision) {
      request = {
        includeProfiles: needsModelProfiles,
        revision: modelRevision,
        promise: Promise.all([
          needsModelProfiles ? listModelProfiles() : Promise.resolve(null),
          listModelAgents(),
        ]),
      };
      pendingModelQuery.current = request;
    }
    try {
      const [profiles, nextAgents] = await request.promise;
      if (generation !== modelQueryGeneration.current) return;
      if (profiles !== null) setModelProfiles(profiles);
      setModelAgents(nextAgents);
      setModelsError(null);
    } catch (error) {
      if (generation !== modelQueryGeneration.current) return;
      setModelsError(formatError(error));
      throw error;
    } finally {
      if (pendingModelQuery.current === request) pendingModelQuery.current = null;
    }
  }, [needsModelProfiles, modelRevision]);

  useEffect(() => {
    let active = true;
    setModelsLoading(true);
    setModelsError(null);
    refreshModels(true)
      .catch((error) => { if (active) showToast({ kind: "error", msg: "读取模型配置失败：" + formatError(error) }); })
      .finally(() => { if (active) setModelsLoading(false); });
    return () => { active = false; modelQueryGeneration.current += 1; };
  }, [refreshModels, showToast, modelRevision]);

  const modelAgent = useMemo(
    () => modelAgents.find((item) => item.id === agentId) ?? null,
    [modelAgents, agentId],
  );
  const canEditConfiguration = Boolean(
    agent?.has_global || modelAgent !== null || agent?.skills_global_dir,
  );
  const compatibleProfiles = useMemo(
    () => modelAgent
      ? modelProfiles.filter((profile) => modelCompatibilityReason(profile, modelAgent) === null)
      : [],
    [modelAgent, modelProfiles],
  );
  const inventory = consumptionState.inventory;
  const { mcpRows, modelRows, skillRows, mcpExternal, skillExternal, modelExternal } = useMemo(() => ({
    mcpRows: consumptionsForAgent(inventory, agentId, "mcp"),
    modelRows: consumptionsForAgent(inventory, agentId, "model"),
    skillRows: consumptionsForAgent(inventory, agentId, "skill"),
    mcpExternal: externalForAgent(inventory, agentId, "mcp"),
    skillExternal: externalForAgent(inventory, agentId, "skill"),
    modelExternal: externalForAgent(inventory, agentId, "model"),
  }), [inventory, agentId]);
  const displayedMcpRows = useMemo(
    () => mcpRows.map((item) => (
      togglingAllMcp
        ? { ...item, enabled: togglingAllMcp.enabled }
        : togglingMcp && item.asset.domain === "mcp" && item.asset.key === togglingMcp.key
          ? { ...item, enabled: togglingMcp.enabled }
          : item
    )),
    [mcpRows, togglingAllMcp, togglingMcp],
  );
  const displayedSkillRows = useMemo(
    () => skillRows.map((item) => (
      togglingSkill && item.asset.domain === "skill" && item.asset.name === togglingSkill.name
        ? { ...item, enabled: togglingSkill.enabled }
        : item
    )),
    [skillRows, togglingSkill],
  );
  const authorityModelRows = modelAgent?.storage_authority === "native-registry"
    ? modelRows.filter((item) => item.observed)
    : modelRows;
  const modelConfiguredCount = authorityModelRows.length + modelExternal.length;
  const modelVisibleCount = modelAgent?.storage_authority === "native-registry"
    ? modelConfiguredCount
    : modelRows.length;
  const agentModelMigrationCandidates = useMemo(
    () => externalModelCandidates
      .filter((candidate) => candidate.agent_id === agentId)
      .sort((left, right) => Number(right.active) - Number(left.active)
        || (left.name || left.model).localeCompare(right.name || right.model)),
    [agentId, externalModelCandidates],
  );
  const displayedModelRows = useMemo(
    () => authorityModelRows.map((item) => {
      const profileId = item.asset.domain === "model" ? item.asset.profile_id : "";
      if (modelAgent?.supports_global_selection === false) return item;
      const current = changingModel
        ? profileId === changingModel.profileId
        : item.desired_active ?? item.active ?? false;
      return { ...item, enabled: current };
    }),
    [authorityModelRows, changingModel, modelAgent?.supports_global_selection],
  );

  const centralSkills = useMemo(() => (skillsState.inventory?.items ?? []).filter(
    (item) => item.location.kind === "central" && item.states.includes("managed"),
  ), [skillsState.inventory]);
  const centralSkillsByName = useMemo(() => new Map(
    centralSkills.map((skill) => [skill.name, skill]),
  ), [centralSkills]);
  const externalSkillsByName = useMemo(() => {
    const byName = new Map<string, NonNullable<typeof skillsState.inventory>["items"][number]>();
    for (const skill of skillsState.inventory?.items ?? []) {
      if (skill.location.kind === "agent_target" && skill.states.includes("external")
        && skill.affected_agent_ids.includes(agentId) && !byName.has(skill.name)) byName.set(skill.name, skill);
    }
    return byName;
  }, [skillsState.inventory, agentId]);
  const sharedSkillCounts = useMemo(() => new Map(skillRows.flatMap((row) =>
    row.asset.domain === "skill" ? [[row.asset.name, row.affected_agent_ids.length] as const] : [],
  )), [skillRows]);
  const mcpEntriesByKey = useMemo(() => new Map(entries.map((entry) => [keyOf(entry), entry])), [entries]);
  const modelProfilesById = useMemo(() => new Map(modelProfiles.map((profile) => [profile.id, profile])), [modelProfiles]);
  const agentDisplayNames = useMemo(() => Object.fromEntries(
    agents.map((item) => [item.id, item.name]),
  ), [agents]);
  const assetDisplayNames = useMemo(() => Object.fromEntries([
    ...entries.map((entry) => [`mcp:${keyOf(entry)}`, entry.name] as const),
    ...modelProfiles.map((profile) => [`model:${profile.id}`, profile.name] as const),
    ...centralSkills.map((skill) => [`skill:${skill.name}`, skill.name] as const),
  ]), [entries, modelProfiles, centralSkills]);

  if (!agent) return <div className="mux-agent-state">未找到该 Agent</div>;

  if (!agent.has_global && !agent.skills_global_dir && !modelsLoading && !modelAgent) {
    return (
      <div className="mux-agent-page">
        <div className="mux-agent-shell">
          <section className="mux-agent-context" aria-label={`${agent.name} 参考信息`}>
            <AgentHeader agent={agent} tone="reference" actions={<><button className="mux-agent-tool" onClick={() => onOpenTrace?.(agent.id)}><DocumentIcon className="w-4 h-4" />{t("trace.open")}</button><button className="mux-agent-tool" onClick={() => onOpenCapture?.(agent.id)}><NetworkIcon className="w-4 h-4" />抓包</button><AgentLaunchAction key={agent.id} agentId={agent.id} /></>} />
            <div className="mux-agent-reference">
              <strong>{agent.note ?? "未提供可写的用户级全局配置。"}</strong>
            </div>
            {(capabilityGuides as Record<string, Record<string, string>>)[agentId] && <div className="mux-agent-file-map mux-agent-guided-cards">
              {RESOURCE_CATEGORIES.filter((resource) => (capabilityGuides as Record<string, Record<string, string>>)[agentId][resource.domain]).map((resource) => <ConfigPath
                key={resource.domain} label={resource.label}
                icon={<resource.Icon className="w-4 h-4" />}
                docsUrl={(capabilityGuides as Record<string, Record<string, string>>)[agentId][resource.domain]} description="应用内配置" paths={[]} kind="file" home={userHome}
                onOpen={() => {}} unavailableLabel="查看设置文档" />)}
            </div>}
          </section>
        </div>
      </div>
    );
  }

  const runtimeSkillAgent = skillsState.inventory?.agents.find((item) => item.id === agentId) ?? null;

  const currentIds = (domain: PickerDomain): string[] => {
    if (domain === "mcp") {
      return mcpRows.flatMap((item) => item.asset.domain === "mcp" ? [item.asset.key] : []);
    }
    if (domain === "model") {
      return modelRows.flatMap((item) => item.asset.domain === "model" ? [item.asset.profile_id] : []);
    }
    return skillRows.flatMap((item) => item.asset.domain === "skill" ? [item.asset.name] : []);
  };
  const picker = pickerDomain ? pickerData(pickerDomain) : null;

  function pickerData(domain: PickerDomain): {
    title: string;
    mode: "single" | "multiple";
    actionLabel: string;
    busyLabel: string;
    emptyMessage: string;
    searchPlaceholder: string;
    options: ConsumptionPickerOption[];
  } {
    const assigned = new Set(currentIds(domain));
    if (domain === "mcp") {
      return {
        title: "添加 MCPs",
        mode: "multiple",
        actionLabel: "添加 MCPs",
        busyLabel: "添加中…",
        emptyMessage: "没有可添加的 MCP",
        searchPlaceholder: "搜索 MCPs",
        options: entries
          .filter((entry) => agent?.supported_transports.includes(transportOf(entry)) && !assigned.has(keyOf(entry)))
          .map((entry) => ({
            id: keyOf(entry),
            name: entry.name,
            description: entry.description,
            icon: <McpAvatar
              assetKey={keyOf(entry)}
              entry={entry}
              preference={mcpIcons.preferences[keyOf(entry)]}
              size={28}
            />,
            meta: <TransportMark transport={transportOf(entry)} />,
          })),
      };
    }
    if (domain === "model") {
      return {
        title: "添加 Models",
        mode: modelAgent?.supports_multiple ? "multiple" : "single",
        actionLabel: "添加 Models",
        busyLabel: "添加中…",
        emptyMessage: "没有可添加的兼容 Model",
        searchPlaceholder: "搜索 Models",
        options: modelProfiles
          .filter((profile) => !assigned.has(profile.id))
          .map((profile) => {
            const reason = modelCompatibilityReason(profile, modelAgent);
            return {
              id: profile.id,
              name: profile.name,
              description: profile.model,
              icon: modelProviderIcon(profile.provider, profile.name),
              meta: <TransportMark transport={modelProtocolLabel(profile.protocol)} />,
              disabled: reason !== null,
              reason: reason ?? undefined,
            };
          }),
      };
    }
    return {
      title: "添加 Skills",
      mode: "multiple",
      actionLabel: "添加 Skills",
      busyLabel: "添加中…",
      emptyMessage: "没有可添加的 Skill",
      searchPlaceholder: "搜索 Skills",
      options: centralSkills.filter((item) => !assigned.has(item.name)).map((item) => ({
        id: item.name,
        name: item.name,
        description: item.description,
      })),
    };
  }

  const createSelection = (domain: PickerDomain, ids: string[]): AgentConsumptionSelection => {
    if (domain === "mcp") return { domain, asset_keys: ids };
    if (domain === "model") return { domain, profile_ids: ids };
    return { domain, names: ids };
  };

  const planSelection = async (
    domain: PickerDomain,
    ids: string[],
    mode: "add" | "replace" | "remove",
  ) => {
    setPreparingChange(true);
    try {
      const plan = mode === "add"
        ? await consumptionState.planAdditionsForAgent(agentId, createSelection(domain, ids))
        : await consumptionState.planForAgent(agentId, createSelection(domain, ids));
      setPickerDomain(null);
      if (!requiresAgentReview(plan)) {
        await commitPlan(plan);
      }
    } catch (error) {
      showToast({ kind: "error", msg: "无法准备变更：" + formatError(error) });
    } finally {
      setPreparingChange(false);
    }
  };

  const planAdditions = (domain: PickerDomain, ids: string[]) => {
    const replacesSingleModel = domain === "model" && modelAgent?.supports_multiple === false;
    return planSelection(domain, ids, replacesSingleModel ? "replace" : "add");
  };

  const planRemoval = (asset: AssetRef) => {
    if (asset.domain === "model-provider") return;
    const id = asset.domain === "mcp" ? asset.key : asset.domain === "model" ? asset.profile_id : asset.name;
    return planSelection(
      asset.domain,
      currentIds(asset.domain).filter((candidate) => candidate !== id),
      "remove",
    );
  };

  const clearModels = async () => {
    setPreparingChange(true);
    try {
      await consumptionState.planClearAgentModels(agentId);
    } catch (error) {
      showToast({ kind: "error", msg: "无法准备清空 Models：" + formatError(error) });
    } finally {
      setPreparingChange(false);
    }
  };

  const commitClearModels = async () => {
    await consumptionState.commit();
    showToast({
      kind: "success",
      msg: `${agent.name} 的全部 Model 已从权威配置中移除。`,
    });
  };

  const clearMcp = async () => {
    setPreparingChange(true);
    try {
      await consumptionState.clearAgentMcp(agentId);
      showToast({ kind: "success", msg: `${agent.name} 的全部 MCP 已移除。` });
    } catch (error) {
      showToast({ kind: "error", msg: "移除全部 MCP 失败：" + formatError(error) });
    } finally {
      setPreparingChange(false);
    }
  };

  const commitPlan = async (
    preparedPlan?: AssetOperationPlan,
    successMessage?: string,
  ) => {
    const activePlan = preparedPlan ?? consumptionState.plan;
    try {
      await consumptionState.commit(preparedPlan ? { background: true } : undefined);
      showToast({
        kind: "success",
        msg: successMessage
          ?? (activePlan ? completedMessage(activePlan, agent.name) : `${agent.name} 的配置已更新。`),
      });
    } catch (error) {
      showToast({ kind: "error", msg: "同步失败：" + formatError(error) });
    }
  };

  const toggleMcpEnabled = async (item: typeof mcpRows[number], enabled: boolean) => {
    if (item.asset.domain !== "mcp") return;
    const key = item.asset.key;
    const name = entries.find((entry) => keyOf(entry) === key)?.name
      ?? key.replace(/::(?:stdio|http)$/, "");
    setTogglingMcp({ key, enabled });
    try {
      await consumptionState.setMcpEnabled(agentId, key, enabled);
      showToast({ kind: "success", msg: `${name} 已${enabled ? "启用" : "停用"}。` });
    } catch (error) {
      showToast({ kind: "error", msg: `${enabled ? "启用" : "停用"}失败：${formatError(error)}` });
    } finally {
      setTogglingMcp((current) => current?.key === key ? null : current);
    }
  };

  const toggleAllMcpEnabled = async (enabled: boolean) => {
    setTogglingAllMcp({ enabled });
    try {
      await consumptionState.setAllMcpEnabled(agentId, enabled);
      showToast({
        kind: "success",
        msg: `${agent.name} 的全部 MCP 已${enabled ? "启用" : "停用"}。`,
      });
    } catch (error) {
      showToast({
        kind: "error",
        msg: `${enabled ? "启用" : "停用"}全部 MCP 失败：${formatError(error)}`,
      });
    } finally {
      setTogglingAllMcp(null);
    }
  };

  const toggleSkillEnabled = async (item: typeof skillRows[number], enabled: boolean) => {
    if (item.asset.domain !== "skill") return;
    const name = item.asset.name;
    setTogglingSkill({ name, enabled });
    try {
      await consumptionState.setSkillEnabled(agentId, name, enabled, item.target?.target_id);
      showToast({ kind: "success", msg: `${name} 已${enabled ? "启用" : "停用"}。` });
    } catch (error) {
      showToast({ kind: "error", msg: `${enabled ? "启用" : "停用"}失败：${formatError(error)}` });
    } finally {
      setTogglingSkill((current) => current?.name === name ? null : current);
    }
  };

  const setActiveModel = async (item: typeof modelRows[number]) => {
    if (item.asset.domain !== "model" || item.desired_active) return;
    const profileId = item.asset.profile_id;
    const name = modelProfiles.find((profile) => profile.id === profileId)?.name ?? profileId;
    setChangingModel({ profileId });
    try {
      await consumptionState.setActiveModel(agentId, profileId);
      showToast({ kind: "success", msg: `${agent.name} 已切换到 ${name}。` });
    } catch (error) {
      showToast({ kind: "error", msg: `切换失败：${formatError(error)}` });
    } finally {
      setChangingModel((current) => current?.profileId === profileId ? null : current);
    }
  };

  const switchActiveModel = (item: typeof modelRows[number], current: boolean) => {
    if (current) return void setActiveModel(item);
    showToast({ kind: "error", msg: "请先选择其他当前 Model。" });
  };

  const converge = async (item: ConsumptionView, action: ConvergenceAction) => {
    if (preparingChange) return;
    setPreparingChange(true);
    try {
      const result = await consumptionState.planConvergence(item, action);
      if (result.domain === "skill") setSkillConvergencePlan(result.plan);
      else setAssetConvergence({ operationId: result.plan.operation_id, action });
    } catch (error) {
      showToast({
        kind: "error",
        msg: t("observations.convergencePrepareFailed", { error: formatError(error) }),
      });
    } finally {
      setPreparingChange(false);
    }
  };

  const openAsset = (asset: AssetRef) => {
    if (asset.domain === "mcp") {
      const split = asset.key.lastIndexOf("::");
      navigateResource({
        domain: "mcp",
        kind: "detail",
        name: split < 0 ? asset.key : asset.key.slice(0, split),
        transport: (split < 0 ? "stdio" : asset.key.slice(split + 2)) as "stdio" | "http",
      });
    } else if (asset.domain === "model") {
      navigateResource({ domain: "model", kind: "detail", profileId: asset.profile_id });
    } else if (asset.domain === "skill") {
      navigateResource({ domain: "skill", kind: "detail", skillName: asset.name });
    }
  };

  return (
    <div className="mux-agent-page">
      <div className="mux-agent-shell">
        <section className="mux-agent-workbench" aria-label={`${agent.name} 工作区`}>
          <AgentHeader agent={agent} actions={<>
            <button className="mux-agent-tool" onClick={() => onOpenTrace?.(agent.id)}><DocumentIcon className="w-4 h-4" />{t("trace.open")}</button>
            <button className="mux-agent-tool" onClick={() => onOpenCapture?.(agent.id)}><NetworkIcon className="w-4 h-4" />抓包</button>
            {canEditConfiguration && (
              <button type="button" className="mux-agent-tool" title="编辑配置" aria-label="编辑配置" onClick={() => { setEditLaunchFirst(false); setEditingAgent(true); }}>
                <EditIcon className="w-3.5 h-3.5" /><span>编辑</span>
              </button>
            )}
            <AgentLaunchAction key={agent.id} agentId={agent.id} showLabel
              onConfigure={canEditConfiguration ? () => { setEditLaunchFirst(true); setEditingAgent(true); } : undefined} />
          </>} />

        <AgentResourcePanel
          key={agent.id}
          value={resourceTab}
          onChange={setResourceTab}
          counts={{
            mcps: mcpRows.length + mcpExternal.length,
            models: modelVisibleCount,
            skills: skillRows.length + skillExternal.length,
          }}
        >
          {consumptionState.plan?.kind === "clear-models" && !preparingChange && (
            <ReviewDialog
              title="清空全部 Models"
              subtitle={`${agent.name} · ${modelVisibleCount} 个 Models`}
              confirmLabel={`清空 ${modelVisibleCount} 个 Models`}
              onConfirm={commitClearModels}
              onClose={() => void consumptionState.cancel()}
            >
              <div className="mux-clear-models-impact">
                <strong>将移除此 Agent 配置中的全部 Model</strong>
                <span>包括外部和手工配置，操作无法撤销</span>
              </div>
              <p className="mux-clear-models-preserved">
                中央 Models、Providers 与凭据保持不变
              </p>
            </ReviewDialog>
          )}
          {consumptionState.plan
            && consumptionState.plan.kind !== "clear-models"
            && !preparingChange ? (
            <AssetOperationReviewDialog
              plan={consumptionState.plan}
              convergenceAction={assetConvergence?.operationId === consumptionState.plan.operation_id ? assetConvergence.action : undefined}
              busy={consumptionState.committing}
              error={consumptionState.error}
              agentId={agent.id}
              agentName={agent.name}
              agentDisplayNames={agentDisplayNames}
              assetDisplayNames={assetDisplayNames}
              onCommit={() => commitPlan()}
              onCancel={consumptionState.cancel}
            />
          ) : null}
          {preparingChange && (
            <div className="mux-agent-operation-progress" role="status" aria-live="polite">
              <RefreshIcon data-spinning="true" />
              <span>正在检查并同步 {agent.name} 的资产…</span>
            </div>
          )}
          {resourceTab === "mcps" ? !agent.has_global ? (
            <div className="mux-agent-inline-state">此 Agent 未接入 MCP。</div>
          ) : (
            <AgentConsumptionPanel
              domain="mcp"
              title={RESOURCE_PRESENTATION.mcp.label}
              description={`${mcpRows.length} 项`}
              manageLabel="添加 MCPs"
              rows={displayedMcpRows}
              columns={3}
              external={mcpExternal}
              externalMode="cards"
              onManage={() => setPickerDomain("mcp")}
              manageDisabled={!agent.has_global || preparingChange}
              bulkRemoveLabel="移除全部 MCP"
              bulkRemoveTitle={`清空 ${agent.name} 的全部 MCP，包括外部配置；不会删除中央 MCP 资产`}
              bulkRemoveDisabled={preparingChange || togglingAllMcp !== null || mcpRows.length + mcpExternal.length === 0}
              onBulkRemove={() => void clearMcp()}
              bulkToggleLabel="全部"
              bulkEnabled={mcpRows.length > 0 && displayedMcpRows.every((item) => item.enabled === true)}
              bulkToggleDisabled={preparingChange || togglingMcp !== null || togglingAllMcp !== null || mcpRows.length === 0}
              onBulkEnabledChange={(enabled) => void toggleAllMcpEnabled(enabled)}
              onEnabledChange={(item, enabled) => void toggleMcpEnabled(item, enabled)}
              enabledChangeDisabled={(item) => togglingAllMcp !== null
                || togglingMcp?.key === (item.asset.domain === "mcp" ? item.asset.key : "")
                || item.status !== "synced"}
              onRemove={(asset) => void planRemoval(asset)}
              onConverge={(item, action) => void converge(item, action)}
              convergenceDisabled={preparingChange}
              removeLabel={(name) => `从 ${agent.name} 移除 ${name}`}
              removeDisabled={preparingChange}
              emptyTitle="暂无 MCPs"
              present={(asset) => {
                const key = asset.domain === "mcp" ? asset.key : "";
                const entry = mcpEntriesByKey.get(key);
                const iconEntry = entry ?? {
                  name: key.replace(/::(?:stdio|http)$/, ""),
                  description: "",
                  tags: [],
                  config: {},
                };
                return {
                  name: iconEntry.name,
                  description: entry?.description?.trim() || undefined,
                  icon: <McpAvatar assetKey={key} entry={iconEntry} preference={mcpIcons.preferences[key]} size={28} />,
                  meta: <TransportMark transport={entry ? transportOf(entry) : key.split("::").at(-1) ?? ""} />,
                };
              }}
            />
          ) : resourceTab === "models" ? (
            modelAgent?.mode === "guided" ? (
              <section className="mux-agent-section mux-agent-resource-content">
                <div className="mux-agent-guided-model">
                  <div><strong>由 Agent 管理</strong><span>{modelAgent.note || `请在 ${agent.name} 内切换。`}</span></div>
                  <button type="button" className="btn-secondary" onClick={() => openUrl(modelAgent.docs)}>
                    <LinkIcon className="w-4 h-4" />打开设置文档
                  </button>
                </div>
              </section>
            ) : (
              modelsLoading ? (
                <div className="mux-agent-inline-state">正在读取 Model…</div>
              ) : modelsError ? (
                <div className="mux-agent-inline-state">Model 读取失败：{modelsError}</div>
              ) : !modelAgent ? (
                <div className="mux-agent-inline-state">此 Agent 尚未接入 Models。</div>
              ) : (
                <AgentConsumptionPanel
                  domain="model"
                  title={RESOURCE_PRESENTATION.model.label}
                  description={modelAgent.storage_authority === "native-registry"
                    ? `配置中 ${modelVisibleCount} 个${modelAgent.supports_global_selection === false ? ` · 重启 ${agent.name} 后在会话中选用` : modelAgent.supports_multiple ? " · 同一时间使用其中一个" : ""}`
                    : `MUX 管理 ${modelVisibleCount} 个`}
                  manageLabel="添加 Models"
                  rows={displayedModelRows}
                  columns={3}
                  external={modelAgent.storage_authority === "native-registry" ? modelExternal : []}
                  externalMode="cards"
                  bulkRemoveLabel="清空全部 Models"
                  bulkRemoveTitle={modelAgent.storage_authority === "native-registry"
                    ? `清空 ${agent.name} 真实配置中的全部 Model，包括外部和手工配置；中央资产与凭据保留`
                    : `清空 ${agent.name} 的全部 MUX Model 映射；中央资产与凭据保留`}
                  bulkRemoveDisabled={modelVisibleCount === 0
                    || preparingChange
                    || consumptionState.committing
                    || changingModel !== null}
                  onBulkRemove={() => void clearModels()}
                  onManage={() => setPickerDomain("model")}
                  manageDisabled={preparingChange || compatibleProfiles.length === 0}
                  onOpenAsset={openAsset}
                  onEnabledChange={modelAgent.supports_global_selection === false ? undefined : switchActiveModel}
                  toggleKind="current"
                  enabledChangeDisabled={(item) => changingModel !== null
                    || item.status === "ambiguous"}
                  renderAction={(item) => {
                    if (item.desired_active) {
                      return (
                        <Badge tone={item.active === false ? "warning" : "success"}>
                          {item.active === false ? "期望当前" : "当前"}
                        </Badge>
                      );
                    }
                    if (item.active) {
                      return <Badge tone="warning">Agent 实际当前</Badge>;
                    }
                    return null;
                  }}
                  onRemove={(asset) => void planRemoval(asset)}
                  onConverge={(item, action) => void converge(item, action)}
                  convergenceDisabled={preparingChange}
                  removeLabel={(name) => `从 ${agent.name} 移除 ${name}`}
                  removeDisabled={preparingChange || changingModel !== null}
                  emptyTitle="暂无 Models"
                  emptyDescription={compatibleProfiles.length === 0
                    ? "模型库中没有兼容资产。"
                    : `从 Models 资产库添加到 ${agent.name}。`}
                  emptyAction={compatibleProfiles.length === 0 ? (
                    <button
                      type="button"
                      className="btn-secondary"
                      onClick={() => navigateResource({ domain: "model", kind: "create" })}
                    >
                      <PlusIcon className="w-4 h-4" />添加 Models
                    </button>
                  ) : undefined}
                  present={(asset) => {
                    const profileId = asset.domain === "model" ? asset.profile_id : "";
                    const externalCandidate = profileId.startsWith("external-")
                      ? agentModelMigrationCandidates.find(
                        (candidate) => profileId === `external-${candidate.candidate_id}`,
                      )
                      : undefined;
                    if (externalCandidate) {
                      const name = externalCandidate.name || externalCandidate.model;
                      return {
                        name,
                        description: `${externalCandidate.model} · ${modelProtocolLabel(externalCandidate.protocol)} · ${externalCandidate.provider}`,
                        icon: modelProviderIcon(externalCandidate.provider, name),
                      };
                    }
                    const profile = modelProfilesById.get(profileId);
                    return {
                      name: profile?.name ?? profileId,
                      description: profile
                        ? `${profile.model} · ${modelProtocolLabel(profile.protocol)} · ${profile.provider}`
                        : "MUX 中央模型资产已缺失",
                      icon: modelProviderIcon(profile?.provider, profile?.name ?? profileId),
                    };
                  }}
                />
              )
            )
          ) : (
            <AgentConsumptionPanel
              domain="skill"
              title={RESOURCE_PRESENTATION.skill.label}
              description={`${skillRows.length} 项`}
              manageLabel="添加 Skills"
              rows={displayedSkillRows}
              columns={3}
              external={skillExternal}
              externalMode="cards"
              onManage={() => setPickerDomain("skill")}
              onOpenAsset={openAsset}
              manageDisabled={!runtimeSkillAgent || preparingChange}
              onEnabledChange={(item, enabled) => void toggleSkillEnabled(item, enabled)}
              enabledChangeDisabled={(item) => togglingSkill?.name === (item.asset.domain === "skill" ? item.asset.name : "")
                || item.status !== "synced"}
              onRemove={(asset) => void planRemoval(asset)}
              onConverge={(item, action) => void converge(item, action)}
              convergenceDisabled={preparingChange}
              removeLabel={(name) => `从 ${agent.name} 移除 ${name}`}
              removeDisabled={preparingChange}
              emptyTitle="暂无 Skills"
              present={(asset) => {
                const name = asset.domain === "skill" ? asset.name : "";
                const skill = centralSkillsByName.get(name);
                const externalSkill = externalSkillsByName.get(name);
                const sharedCount = sharedSkillCounts.get(name) ?? 0;
                return {
                  name,
                  description: skill?.description ?? externalSkill?.description ?? "Skill 资产已缺失",
                  icon: <Avatar seed={name} kind="skill" size={28} />,
                  meta: sharedCount > 1
                    ? <Badge tone="warning">共用 · {sharedCount}</Badge>
                    : null,
                };
              }}
            />
          )}
        </AgentResourcePanel>
        </section>
      </div>

      {pickerDomain && picker && (
        <ConsumptionPickerDialog
          domain={pickerDomain ?? undefined}
          title={picker.title}
          mode={picker.mode}
          subtitle={agent.name}
          options={picker.options}
          actionLabel={picker.actionLabel}
          busyLabel={picker.busyLabel}
          emptyMessage={picker.emptyMessage}
          searchPlaceholder={picker.searchPlaceholder}
          onClose={() => setPickerDomain(null)}
          onSelect={(ids) => planAdditions(pickerDomain, ids)}
        />
      )}

      {skillConvergencePlan && (
        <SkillReviewDialog
          plan={skillConvergencePlan}
          onCommit={skillsState.commit}
          onClose={async () => {
            await skillsState.cancel(skillConvergencePlan.operation_id);
            setSkillConvergencePlan(null);
          }}
          onCommitted={async (next) => {
            skillsState.hydrate(next);
            setSkillConvergencePlan(null);
            await consumptionState.refresh();
          }}
          onRecoveryRequired={(message) => {
            showToast({ kind: "error", msg: message });
          }}
        />
      )}

      {editingAgent && (
        <AgentConfigurationDialog
          agent={agent}
          modelAgent={modelAgent}
          initialSection={editLaunchFirst ? "launch" : "paths"}
          onLaunchSaved={agentLauncher.refresh}
          onClose={() => setEditingAgent(false)}
          onSaved={async () => {
            await Promise.allSettled([
              refreshAgents(),
              refreshModels(),
              consumptionState.refreshAgents(),
              consumptionState.refresh(),
              skillsState.refresh(),
            ]);
          }}
        />
      )}


    </div>
  );
}

function TransportMark({ transport }: { transport: string }) {
  return <span className="mux-transport-mark">{transport}</span>;
}

function AgentHeader({
  agent,
  tone,
  actions,
}: {
  agent: InstallState["agents"][number];
  tone?: "reference";
  actions?: ReactNode;
}) {
  const { show } = useToast();
  const docsHome = (agentDocsHome as Record<string, string>)[agent.id] ?? agent.docs;
  const identity = <>
    <span data-agent-entry-glyph className="inline-flex flex-shrink-0"><AgentGlyph id={agent.id} name={agent.name} size={44} /></span>
    <div className="mux-agent-header-copy">
      <div>
        <h2 data-agent-entry-name>{agent.name}</h2>
        {tone === "reference" ? <Badge>仅供参考</Badge> : agent.evidence === "community-extension" ? (
          <Badge tone="warning">社区扩展</Badge>
        ) : !agent.builtin ? <Badge>自定义</Badge> : null}
      </div>
    </div>
  </>;
  return (
    <header className="mux-agent-header mux-agent-overview-header" data-tone={tone} data-agent-entry-id={agent.id} aria-label={`Agent ${agent.name} (${agent.id})`}>
      {docsHome ? <a className="mux-agent-header-identity mux-agent-docs-link"
        href={docsHome} title={`打开 ${agent.name} 官方文档`} aria-label={`打开 ${agent.name} 官方文档`}
        onClick={(event) => {
          event.preventDefault();
          void openUrl(docsHome).catch((error) => show({ kind: "error", msg: `无法打开官方文档：${formatError(error)}` }));
        }}>{identity}</a> : <div className="mux-agent-header-identity">{identity}</div>}
      {actions && <div className="mux-agent-overview-actions" role="group" aria-label="Agent 操作">{actions}</div>}
    </header>
  );
}

function ConfigPath({
  icon,
  label,
  description,
  paths,
  kind,
  home,
  onOpen,
  docsUrl,
  unavailableLabel = "不可用",
  inline = false,
}: {
  icon: ReactNode;
  label: string;
  description: string;
  paths: string[];
  kind: ConfigLocationKind;
  home: string;
  onOpen(path: string, kind: ConfigLocationKind): Promise<unknown> | unknown;
  docsUrl?: string;
  unavailableLabel?: string;
  inline?: boolean;
}) {
  const { show } = useToast();
  const openDocs = () => {
    if (!docsUrl) return;
    void openUrl(docsUrl).catch((error) => show({ kind: "error", msg: `无法打开 ${label} 文档：${formatError(error)}` }));
  };
  const state = description.includes("失败") ? "error" : description.includes("读取中") ? "loading" : paths.length ? "ready" : "unavailable";
  return (
    <details className="mux-config-disclosure" data-state={state} data-inline={inline || undefined} open={inline || undefined} aria-label={`${label} 配置位置`}>
      <summary aria-label={`${label} 配置位置 · ${state === "error" || state === "loading" ? description : paths.length ? `${paths.length} 个${kind === "folder" ? "文件夹" : "文件"}` : description}`}>
        <span className="mux-config-symbol" aria-hidden="true">{icon}</span>
        <strong>{label}</strong>
        <span className="mux-config-indicator" title={state === "error" || state === "loading" ? description : paths.length ? `${paths.length} 个配置${kind === "folder" ? "目录" : "文件"}` : unavailableLabel} aria-hidden="true">
          {state === "error" ? "!" : state === "loading" ? <RefreshIcon className="mux-config-loading" /> : paths.length ? <>
            {kind === "folder" ? <FolderIcon /> : <DocumentIcon />}<span>{paths.length}</span>
          </> : "—"}
        </span>
        <ChevronDownIcon className="mux-config-chevron" />
      </summary>
      <div className="mux-config-body">
        <div className="mux-config-description">
          <span>{description}</span>
          {docsUrl && <button type="button" className="mux-config-docs" title={`查看 ${label} 文档`} aria-label={`查看 ${label} 文档`} onClick={openDocs}>
            <ExternalLinkIcon className="w-3.5 h-3.5" />
          </button>}
        </div>
        {paths.length > 0 ? (
          <div className="mux-agent-file-paths">
            {paths.map((path) => {
              const absolutePath = absoluteConfigLocation(path, home);
              const root = home.replace(/\/$/, "");
              const displayPath = root && absolutePath.startsWith(`${root}/`) ? `~/${absolutePath.slice(root.length + 1)}` : absolutePath;
              return (
                <button
                  type="button"
                  className="mux-agent-file-path"
                  key={path}
                  title={kind === "folder" ? `在 Finder 中打开 ${absolutePath}` : `使用所选编辑器打开 ${absolutePath}`}
                  aria-label={`${kind === "folder" ? "打开文件夹" : "使用所选编辑器打开文件"}：${absolutePath}`}
                  onClick={() => void onOpen(path, kind)}
                >
                  <code>{displayPath}</code>
                  <ExternalLinkIcon className="w-3 h-3" />
                </button>
              );
            })}
          </div>
        ) : (
          description !== unavailableLabel && <span className="mux-agent-file-unavailable">{unavailableLabel}</span>
        )}
      </div>
    </details>
  );
}
