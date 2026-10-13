import { invoke } from "@tauri-apps/api/core";
import type {
  AgentDefinitionInput,
  BackendStatus,
  AgentConfigurationPatch,
  AgentConsumptionSelection,
  AgentCapabilityView,
  AgentInfo,
  ApiKeyDelivery,
  AssetCommandError,
  AssetOperationPlan,
  AssetRef,
  CentralAssetDraft,
  CatalogItem,
  ConsumptionInventory,
  CommitOperationRequest,
  CredentialValidationView,
  CancelOperationRequest,
  InstalledMcp,
  ModelAdoptionCandidate,
  ModelAgentView,
  ModelApplyResult,
  ModelProviderView,
  ProviderModelSummary,
  ModelProfileView,
  McpIconPreferences,
  ModelProviderInstanceView,
  OperationPlan,
  OperationCommitResult,
  PlanOperationRequest,
  PlanRemoveRequest,
  PlanImportRequest,
  PlanRepairRequest,
  PlanSkillAssetImportRequest,
  PlanSkillAssetInstallRequest,
  PlanUpdateRequest,
  ProxySettings,
  RegistryEntry,
  SkillAgentView,
  SkillCommitRequest,
  SkillDetail,
  SkillSourceResolution,
  SkillsInventory,
  SourceView,
  UpdateCheckOutcome,
  UnifiedOperationPlan,
  WorkspaceSnapshot,
} from "./types";

export const getWorkspaceSnapshot = () =>
  invoke<WorkspaceSnapshot>("get_workspace_snapshot");
export const getBackendStatus = () => invoke<BackendStatus>("get_backend_status");
export const listAgentCapabilities = () =>
  invoke<AgentCapabilityView[]>("list_agent_capabilities");
export const planOperation = (request: PlanOperationRequest) =>
  invoke<UnifiedOperationPlan>("plan_operation", { request });
export const commitOperation = (request: CommitOperationRequest) =>
  invoke<OperationCommitResult>("commit_operation", { request });
export const cancelOperation = (request: CancelOperationRequest) =>
  invoke<void>("cancel_operation", { request });

export const listConsumptionInventory = () =>
  invoke<ConsumptionInventory>("list_consumption_inventory");

type ObservationResult<T> = { Ok: T } | { Err: AssetCommandError };
export const getResourceObservation = () => invoke<{
  skills: ObservationResult<SkillsInventory>;
  relationships: ObservationResult<ConsumptionInventory>;
}>("get_resource_observation");
export function observationValue<T>(result: ObservationResult<T>): T {
  if ("Err" in result) throw result.Err;
  return result.Ok;
}
export const listModelAdoptionCandidates = () =>
  invoke<ModelAdoptionCandidate[]>("list_model_adoption_candidates");
export const planSetAgentConsumption = (
  agentId: string,
  selection: AgentConsumptionSelection,
) =>
  invoke<AssetOperationPlan>("plan_set_agent_consumption", {
    request: { agent_id: agentId, selection },
  });
export const planSetMcpEnabled = (
  agentId: string,
  assetKey: string,
  enabled: boolean,
) => invoke<AssetOperationPlan>("plan_set_mcp_enabled", {
  request: { agent_id: agentId, asset_key: assetKey, enabled },
});
export const planSetSkillEnabled = (
  agentId: string,
  name: string,
  enabled: boolean,
  targetId?: string,
) => invoke<AssetOperationPlan>("plan_set_skill_enabled", {
  request: { agent_id: agentId, name, enabled, ...(targetId ? { target_id: targetId } : {}) },
});
export const planSetModelEnabled = (
  agentId: string,
  profileId: string,
  enabled: boolean,
) => invoke<AssetOperationPlan>("plan_set_model_enabled", {
  request: { agent_id: agentId, profile_id: profileId, enabled },
});
export const planSetActiveModel = (
  agentId: string,
  profileId: string,
) => invoke<AssetOperationPlan>("plan_set_active_model", {
  request: { agent_id: agentId, profile_id: profileId },
});
export const planSetAssetConsumers = (asset: AssetRef, agentIds: string[]) =>
  invoke<AssetOperationPlan>("plan_set_asset_consumers", {
    request: { asset, agent_ids: agentIds },
  });
export const planUpdateCentralAsset = (draft: CentralAssetDraft) =>
  invoke<AssetOperationPlan>("plan_update_central_asset", {
    request: { draft },
  });
export const planDeleteCentralAsset = (asset: AssetRef, sourceId?: string) =>
  invoke<AssetOperationPlan>("plan_delete_central_asset", {
    request: { asset, source_id: sourceId ?? null },
  });
export const commitAssetOperation = (
  plan: Pick<AssetOperationPlan, "operation_id" | "candidate_hash">,
) =>
  invoke<ConsumptionInventory>("commit_asset_operation", {
    request: {
      operation_id: plan.operation_id,
      candidate_hash: plan.candidate_hash,
    },
  });
export const cancelAssetOperation = (operationId: string) =>
  invoke<void>("cancel_asset_operation", { operationId });

export type { AssetCommandError };

export const listRegistry = () => invoke<RegistryEntry[]>("list_registry");
export const getRegistrySnapshot = () => invoke<{
  entries: RegistryEntry[];
  catalog: CatalogItem[];
  custom_keys: string[];
  sources: SourceView[];
}>("get_registry_snapshot");
export const listModelProfiles = () =>
  invoke<Array<ModelProfileView & { provider?: string; base_url?: string }>>(
    "list_model_profiles",
  ).then((profiles) =>
    profiles.map((profile) => ({
      ...profile,
      // Rust omits empty strings from the flattened wire payload. Restore the
      // required desktop invariant for legacy Profiles without a Provider.
      provider: profile.provider ?? "",
      base_url: profile.base_url ?? "",
    }))
  );

export const exportModelCurl = (profileId: string, includeApiKey = true) =>
  invoke<string>("export_model_curl", { profileId, includeApiKey });
export const listModelProviders = () =>
  invoke<ModelProviderView[]>("list_model_providers");
export const listModelProviderInstances = () =>
  invoke<ModelProviderInstanceView[]>("list_model_provider_instances");
export const copyToClipboard = (text: string) =>
  invoke<void>("copy_to_clipboard", { text });
export const discoverProviderModels = (providerId: string) =>
  invoke<ProviderModelSummary[]>("discover_provider_models", { providerId });
export const revealModelProviderCredential = (providerId: string) =>
  invoke<string>("reveal_model_provider_credential", { providerId });
export const inferModelProvider = (baseUrl: string) =>
  invoke<string>("infer_model_provider", { baseUrl });
export const validateModelCredentialSource = (source: import("./types").ApiKeySource) =>
  invoke<CredentialValidationView>("validate_model_credential_source", { source });
export const listModelAgents = () =>
  invoke<ModelAgentView[]>("list_model_agents");
export const setModelCredentialDelivery = (
  agentId: string,
  profileId: string,
  delivery: ApiKeyDelivery,
  confirmPlaintext = false,
) => invoke<ModelApplyResult>("set_model_credential_delivery", {
  agentId,
  profileId,
  delivery,
  confirmPlaintext,
});
export const setAgentCredentialDelivery = (
  agentId: string,
  delivery: ApiKeyDelivery,
  confirmPlaintext = false,
) => invoke<ModelApplyResult>("set_agent_credential_delivery", {
  agentId,
  delivery,
  confirmPlaintext,
});
/** All entry copies across sources (not deduped), each flagged in_effect. */
export const listRegistryAll = () => invoke<CatalogItem[]>("list_registry_all");
/** 桌面包内 arca CLI 的安装状态（sidecar → ~/.local/bin 软链）。 */
export type CliStatus = { bundled: boolean; installed: boolean; link_path: string; in_path: boolean };
export const cliStatus = () => invoke<CliStatus>("cli_status");
export const installCli = () => invoke<CliStatus>("install_cli");
export type UpdateEnvironment = {
  canSelfUpdate: boolean;
  reason: "disk-image" | "app-translocation" | "read-only-volume" | null;
};
export const updateEnvironment = () =>
  invoke<UpdateEnvironment>("update_environment");
export const listCustomRegistryKeys = () =>
  invoke<string[]>("list_custom_registry_keys");
export const listAgents = () => invoke<AgentInfo[]>("list_agents");
export const getPinnedAgents = () =>
  invoke<string[]>("get_pinned_agents");
export const setPinnedAgents = (agentIds: string[]) =>
  invoke<string[]>("set_pinned_agents", { agentIds });
export const getUiLocale = () =>
  invoke<"zh-CN" | "en-US" | null>("get_ui_locale");
export const setUiLocale = (locale: "zh-CN" | "en-US" | null) =>
  invoke<"zh-CN" | "en-US" | null>("set_ui_locale", { locale });
export const listMcpIconPreferences = () =>
  invoke<McpIconPreferences>("list_mcp_icon_preferences");
export const setMcpBuiltinIcon = (assetKey: string, iconId: string) =>
  invoke<McpIconPreferences>("set_mcp_builtin_icon", { assetKey, iconId });
export const importMcpIconDialog = (assetKey: string) =>
  invoke<McpIconPreferences | null>("import_mcp_icon_dialog", { assetKey });
export const resetMcpIcon = (assetKey: string) =>
  invoke<McpIconPreferences>("reset_mcp_icon", { assetKey });
export const getProxySettings = () =>
  invoke<ProxySettings>("get_proxy_settings");
export const setProxySettings = (proxyUrl: string | null) =>
  invoke<ProxySettings>("set_proxy_settings", { proxyUrl });
export const addAgent = (id: string, def: AgentDefinitionInput) =>
  invoke<void>("add_agent", { id, def });
export const updateAgent = (id: string, def: AgentDefinitionInput) =>
  invoke<void>("update_agent", { id, def });
export const planUpdateAgentCapabilities = (
  id: string,
  patch: AgentConfigurationPatch,
) => invoke<AssetOperationPlan>("plan_update_agent_capabilities", {
  request: { agent_id: id, patch },
});
export const scanInstalled = () => invoke<InstalledMcp[]>("scan_installed");
/** Read-only, identity-only preview; parsing remains owned by core. */
export interface PastedMcpSummary { name: string; transport: "stdio" | "http"; }
export const previewPastedConfig = (text: string) =>
  invoke<PastedMcpSummary[]>("preview_pasted_config", { text });
/** Parse a pasted config blob (JSON/TOML/YAML) and add its servers to the manual
 *  source. Returns the added server names. */
export const importPastedConfig = (text: string) =>
  invoke<string[]>("import_pasted_config", { text });

// ── Catalog sources (subscribe remote / add local) ────────────────────────
export const listSources = () => invoke<SourceView[]>("list_sources");
/** Subscribe to a remote config URL: fetch + cache + register as a source. */
export const subscribeSource = (url: string, name?: string) =>
  invoke<SourceView>("subscribe_source", { url, name: name ?? null });
/** Open a native file picker and add the chosen file as a local source.
 *  Resolves to null if the user cancels. */
export const addLocalSourceDialog = () =>
  invoke<SourceView | null>("add_local_source_dialog");
/** Export the complete effective catalog to JSON via a native save dialog.
 *  Resolves to the written path, or null if the user cancels. */
export const exportEffectiveDialog = () =>
  invoke<string | null>("export_effective_dialog");
/** Re-fetch (remote) / re-copy (local) a source's file. */
export const refreshSource = (id: string) =>
  invoke<SourceView>("refresh_source", { id });
export const setSourceEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_source_enabled", { id, enabled });
export const removeSource = (id: string) =>
  invoke<void>("remove_source", { id });

/** Stable key for an (server, agent) cell in the matrix. `serverKey` is the
 *  composite registry key (`name::transport`), so stdio and http variants of the
 *  same server get distinct cells. */
export const cellKey = (serverKey: string, agentId: string) =>
  `${serverKey}|${agentId}`;

export const listSkillsInventory = () =>
  invoke<SkillsInventory>("list_skills_inventory");
export const listSkillAgents = () =>
  invoke<SkillAgentView[]>("list_skill_agents");
export const getSkillDetail = (identity: string) =>
  invoke<SkillDetail>("get_skill_detail", { identity });
export const resolveGithubSkillSource = (value: string) =>
  invoke<SkillSourceResolution>("resolve_skill_source", { value });
export const resolveLocalSkillSourceDialog = () =>
  invoke<SkillSourceResolution | null>("resolve_local_skill_source_dialog");
export const resolveArchiveSkillSourceDialog = () =>
  invoke<SkillSourceResolution | null>("resolve_archive_skill_source_dialog");
export const planSkillAssetInstall = (request: PlanSkillAssetInstallRequest) =>
  invoke<OperationPlan>("plan_skill_asset_install", { request });
export const commitSkillInstall = (request: SkillCommitRequest) =>
  invoke<SkillsInventory>("commit_skill_install", { request });
export const planSkillAssetImport = (request: PlanSkillAssetImportRequest) =>
  invoke<OperationPlan>("plan_skill_asset_import", { request });
export const planSkillImport = (request: PlanImportRequest) =>
  invoke<OperationPlan>("plan_skill_import", { request });
export const commitSkillImport = (request: SkillCommitRequest) =>
  invoke<SkillsInventory>("commit_skill_import", { request });
export const planSkillUpdate = (request: PlanUpdateRequest) =>
  invoke<OperationPlan>("plan_skill_update", { request });
export const commitSkillUpdate = (request: SkillCommitRequest) =>
  invoke<SkillsInventory>("commit_skill_update", { request });
export const planSkillRemove = (request: PlanRemoveRequest) =>
  invoke<OperationPlan>("plan_skill_remove", { request });
export const commitSkillRemove = (request: SkillCommitRequest) =>
  invoke<SkillsInventory>("commit_skill_remove", { request });
export const commitSkillAssignment = (request: SkillCommitRequest) =>
  invoke<SkillsInventory>("commit_skill_assignment", { request });
export const planSkillRepair = (request: PlanRepairRequest) =>
  invoke<OperationPlan>("plan_skill_repair", { request });
export const commitSkillRepair = (request: SkillCommitRequest) =>
  invoke<SkillsInventory>("commit_skill_repair", { request });
export const checkSkillUpdates = (manual: boolean) =>
  invoke<UpdateCheckOutcome>("check_skill_updates", { manual });
export const cancelSkillOperation = (operationId: string) =>
  invoke<void>("cancel_skill_operation", { operationId });
