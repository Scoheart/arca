export interface StdioConfig { command: string; args?: string[]; env?: Record<string, string>; cwd?: string; }
export interface HttpConfig { type: string; url: string; headers?: Record<string, string>; }
/** Provenance of a catalog entry:
 *  - "discovered" — scanned from a local app config (`agent`/`scope` set),
 *  - "manual"     — created by the user by hand,
 *  - "remote"     — from a subscribed remote source (`source` = its id),
 *  - "local"      — from a local file source (`source` = its id). */
export interface RegistryOrigin {
  kind: "discovered" | "manual" | "remote" | "local";
  agent?: string;
  scope?: string;
  source?: string;
}
export interface RegistryEntry {
  name: string; description: string; tags: string[];
  config: { stdio?: StdioConfig; http?: HttpConfig };
  origin?: RegistryOrigin;
  /** Optional homepage / source repo URL (e.g. a GitHub repo), shown as a link. */
  repo?: string;
}
export interface McpIconPreference {
  kind: "builtin" | "custom";
  value: string;
  path?: string;
}
export type McpIconPreferences = Record<string, McpIconPreference>;
/** One entry copy from a source, plus whether it's the in-effect (winning) copy
 *  for its composite key. Copies with `in_effect === false` are shadowed by a
 *  higher-precedence source but still shown so nothing is hidden. */
export interface CatalogItem {
  entry: RegistryEntry;
  in_effect: boolean;
}
export interface AgentInfo {
  id: string; name: string; format: string; key: string;
  has_global: boolean; has_project: boolean; enabled: boolean;
  /** Derived from WorkspaceSnapshot; absent on the legacy MCP-shaped wire. */
  has_model?: boolean;
  has_instructions?: boolean;
  supported_transports: Array<"stdio" | "http">;
  /** Raw stored config paths (e.g. `~/Library/Application Support/…/mcp.json`). */
  global: string | null; project: string | null;
  skills_global_dir: string | null;
  skills_global_dirs?: string[];
  docs: string | null;
  note: string | null;
  category: string;
  evidence: "official" | "official-source" | "catalog" | "custom" | string;
  verified_at: string | null;
  builtin: boolean;
}

export interface AgentIdentityView {
  id: string;
  name: string;
  enabled: boolean;
  builtin: boolean;
  category: string;
  evidence: string;
  docs?: string | null;
  note?: string | null;
  verified_at?: string | null;
}

export interface AgentCapabilityView {
  identity: AgentIdentityView;
  installed: boolean;
  runtime_detected?: boolean | null;
  config_detected?: boolean;
  capabilities: {
    instructions?: boolean;
    mcp?: {
      writable: boolean;
      config_path?: string | null;
      format: string;
      key: string;
      supported_transports: string[];
    } | null;
    model?: {
      mode: string;
      storage_authority: ModelStorageAuthority;
      installed: boolean;
      config_paths: string[];
      assigned_profiles: string[];
      active_profile?: string | null;
      supports_multiple: boolean;
      supports_global_selection: boolean;
      credential_mode: string;
      supported_protocols: ModelProtocol[];
    } | null;
    skill?: {
      installed: boolean;
      target_id: string;
      global_dir: string;
      alias_dirs: string[];
      affected_agent_ids: string[];
    } | null;
  };
}

export type ModelProtocol =
  | "anthropic-messages"
  | "openai-responses"
  | "openai-completions"
  | "gemini-generate-content"
  | "gemini-interactions"
  | "aws-bedrock-converse";

export type AuthRequirement = "required" | "optional" | "none";

export type ApiKeySource =
  | { kind: "arca-store" }
  | { kind: "env"; name: string }
  | { kind: "file"; path: string }
  | { kind: "helper"; command: string; args: string[]; ttl_ms?: number };

export type ApiKeyDelivery = "auto" | "env" | "command" | "agent-store" | "plaintext";

export interface CredentialValidationView {
  source_kind: ApiKeySource["kind"];
  source_identity: string;
  message: string;
}

export interface ModelProfile {
  id: string;
  name: string;
  /** Stable reference to one shared Provider instance. */
  provider_id?: string;
  /** Actual API/billing channel, such as openrouter or anthropic. */
  provider: string;
  /** Model creator; independent from the API provider. */
  model_vendor?: string;
  /** Agent-native identities retained by an adopted historical config. */
  native_ids?: Record<string, string>;
  protocol: ModelProtocol;
  base_url: string;
  model: string;
  /** Non-secret environment variable name for Agents such as Grok Build. */
  env_key?: string;
  context_window?: number;
  max_output_tokens?: number;
  /** Missing means the Agent/provider decides; booleans are explicit overrides. */
  reasoning?: boolean;
}

export interface ModelProviderConfig {
  id: string;
  name: string;
  provider: string;
  base_url: string;
  /** Optional full endpoint used to discover this Provider's available models. */
  model_catalog_url?: string;
  /** Optional account/key page override for this connection. */
  portal_url?: string;
  protocols: Partial<Record<ModelProtocol, {
    endpoint_path: string;
  }>>;
  auth_requirement?: AuthRequirement;
  api_key_source?: ApiKeySource;
  /** Hydrated legacy compatibility field; new writes use api_key_source. */
  env_key?: string;
}

export interface ModelProviderInstanceView extends ModelProviderConfig {
  /** Effective portal resolved by core from the override or template. */
  portal?: ModelProviderView["portal"];
  credential_saved: boolean;
  model_count: number;
  model_discovery_supported: boolean;
}

export interface ProviderModelSummary {
  id: string;
  name?: string | null;
  context_length?: number | null;
}

export interface ModelProfileView extends ModelProfile {
  catalog_key: string;
  credential_saved: boolean;
}

export interface ModelProviderView {
  id: string;
  name: string;
  /** Official, provider-specific documentation; absent for custom connections. */
  docs_url?: string | null;
  /** Official key page, account console, or local setup guide supplied by core. */
  portal?: {
    url: string;
    kind: "api-key" | "console" | "setup";
  } | null;
  default_base_url: string | null;
  default_protocol: ModelProtocol;
  additional_endpoints: Array<{
    protocol: ModelProtocol;
    base_url: string;
  }>;
  /** Shared connection root and protocol paths prepared by core. */
  base_url: string | null;
  protocols: Partial<Record<ModelProtocol, {
    endpoint_path: string;
  }>>;
  category: "official" | "gateway" | "local" | "custom";
  model_discovery_supported: boolean;
  /** Display-only instructions for account/region-specific connections. */
  setup?: {
    base_url_placeholder: string;
    hint: "azure" | "bedrock" | "bedrock-converse" | "cloudflare" | "tinfoil";
  } | null;
}

export type ModelStorageAuthority = "native-registry" | "arca-mapping" | "guided";

export interface ModelAgentView {
  id: "claude-code" | "codex" | "pi" | "qoder" | string;
  name: string;
  mode: "managed" | "guided";
  storage_authority: ModelStorageAuthority;
  installed: boolean;
  config_path: string;
  config_paths: string[];
  docs: string;
  assigned_profile: string | null;
  assigned_profiles: string[];
  active_profile: string | null;
  supports_multiple: boolean;
  supports_global_selection?: boolean;
  credential_mode: "keychain-command" | "environment-reference" | "guided" | string;
  credential_capabilities?: {
    native_sources: Array<ApiKeySource["kind"]>;
    arca_keychain_helper: boolean;
    agent_store: boolean;
    plaintext: boolean;
    note?: string;
  };
  credential_policies?: Record<string, {
    source_override?: ApiKeySource | null;
    delivery: ApiKeyDelivery;
  }>;
  default_delivery?: ApiKeyDelivery;
  available_deliveries?: ApiKeyDelivery[];
  supported_protocols: ModelProtocol[];
  note: string;
}

export interface ModelApplyResult {
  agent: string;
  profile: string;
  files: string[];
  restart_required: boolean;
  message: string;
}
export type AgentInstallProbeInput =
  | { kind: "path"; path: string }
  | { kind: "command"; name: string }
  | { kind: "mac-bundle"; bundle_id: string };

export interface AgentSkillsCapabilityInput {
  target_id: string;
  global_dir: string;
  aliases: Array<{
    target_id: string;
    global_dir: string;
  }>;
  docs: string;
  evidence: "official" | "official-source";
  verified_at: string;
  probes: AgentInstallProbeInput[];
}

/** Payload for creating a custom Agent identity and its supported writers
 * (mirrors the public fields of Rust AgentDefinition). */
export interface AgentDefinitionInput {
  global: string | null;
  /** Legacy metadata retained when editing an existing definition. */
  project: string | null;
  format: "" | "json" | "toml" | "yaml";
  key: string;
  enabled: boolean;
  builtin?: boolean;
  name?: string | null;
  docs?: string | null;
  note?: string | null;
  category?: string | null;
  evidence?: string | null;
  verified_at?: string | null;
  skills?: AgentSkillsCapabilityInput | null;
}
export interface McpConfigurationPatch {
  path: string;
  key?: string | null;
}
export interface ModelConfigurationPatch {
  paths: string[];
}
export interface SkillConfigurationPatch {
  global_dir: string;
  alias_dirs: string[];
}
export interface AgentConfigurationPatch {
  mcp?: McpConfigurationPatch | null;
  model?: ModelConfigurationPatch | null;
  skill?: SkillConfigurationPatch | null;
}
export interface InstalledMcp {
  name: string; agent: string; scope: string; file_path: string; transport: string;
  customized?: boolean;
  /** Whether the server is active in the agent's config (true) or paused natively /
   *  remembered in Arca's disabled store (false). */
  enabled: boolean;
  observation_fingerprint: string;
}
export type McpAdoptionStatus = "external-added" | "external-changed";
export interface McpAdoptionCandidate {
  agent_id: string;
  asset_key: string;
  enabled: boolean;
  status: McpAdoptionStatus;
  config_hash: string;
  fingerprint: string;
  settings_hash: string;
  target_hash: string;
  candidate_hash: string;
}
export type ModelAdoptionStatus = "adoptable" | "needs-credential" | "unsupported" | "conflicted";
export type ModelCredentialKind = "none" | "environment-reference" | "literal" | "external-command";
export interface ModelAdoptionCandidate {
  candidate_id: string;
  agent_id: string;
  native_id: string;
  managed_profile_id?: string | null;
  name: string;
  provider: string;
  model_vendor?: string | null;
  protocol: ModelProtocol;
  base_url: string;
  model: string;
  env_key?: string | null;
  active: boolean;
  credential_kind: ModelCredentialKind;
  status: ModelAdoptionStatus;
  reason?: string | null;
  fingerprint: string;
  settings_hash: string;
  target_hash: string;
  candidate_hash: string;
}
export interface PatchInput {
  args?: string[]; env?: Record<string, string>; url?: string; headers?: Record<string, string>;
}

export type ResourceNavigationRequest =
  | { domain: "mcp"; kind: "detail"; name: string; transport: string }
  | { domain: "mcp"; kind: "create" }
  | { domain: "model"; kind: "detail"; profileId: string }
  | { domain: "model"; kind: "create" }
  | { domain: "skill"; kind: "detail"; skillName: string };

export type ResourceNavigationIntent = ResourceNavigationRequest & { id: number };
export type SkillNavigationRequest = Extract<ResourceNavigationRequest, { domain: "skill" }>;
export type SkillNavigationIntent = Extract<ResourceNavigationIntent, { domain: "skill" }>;

/** Top-level GUI view. Resource editors are overlays and intentionally remain
 *  outside navigation state so the app chrome never disappears. */
export type View =
  | { kind: "registry"; intent?: Extract<ResourceNavigationIntent, { domain: "mcp" }> }
  | { kind: "models"; intent?: Extract<ResourceNavigationIntent, { domain: "model" }> }
  | { kind: "skills"; intent?: SkillNavigationIntent }
  | { kind: "agent"; id: string; tab?: "skills" }
  | { kind: "capture"; agentId: string }
  | { kind: "traces"; agentId?: string };

/** A catalog source (mirrors Rust SourceView): a subscribed remote URL or a
 *  local file. Its servers are parsed under ~/.arca/assets/mcps/sources/. */
export type SourceKind = "remote" | "local";
export interface SourceView {
  id: string;
  kind: SourceKind;
  name: string;
  url: string | null;
  path: string | null;
  format: string;
  enabled: boolean;
  added_at: string | null;
  synced_at: string | null;
  server_count: number;
  error: string | null;
  /** True for the auto-managed sources (手动添加 / 自动探索); the UI hides
   *  refresh/remove for these. */
  managed: boolean;
}

export interface ProxySettings {
  proxy_url: string | null;
}

export interface InstallRequest {
  server_name: string; transport: "stdio" | "http"; agents: string[];
  overrides: Record<string, PatchInput>;
}

/** Result of re-syncing an edited entry to its installed agents. */
export interface ResyncOutcome {
  /** Agent ids the current config was re-stamped into. */
  synced: string[];
  /** Agent ids skipped because their on-disk config was hand-customized
   *  (only populated when force = false). */
  skipped_customized: string[];
}

export type RiskLevel = "low" | "medium" | "high";
export type SkillContentKind =
  | "automation"
  | "assets"
  | "reference"
  | "instructions";
export type InventoryState =
  | "managed"
  | "assigned"
  | "external"
  | "locally_modified"
  | "broken_link"
  | "conflicting_link"
  | "missing"
  | "update_available";
export type SkillFileKind = "file" | "symlink";
export type FileChangeKind =
  | "added"
  | "modified"
  | "removed"
  | "mode_changed"
  | "link_changed";
export type PlannedLinkState =
  | "missing"
  | "managed"
  | "broken"
  | "directory"
  | "unknown_symlink";
export type SkillOperationKind =
  | "install"
  | "import"
  | "update"
  | "remove"
  | "assignment"
  | "repair";

export interface SkillRiskFinding {
  rule_id: string;
  rule_version: number;
  level: RiskLevel;
  path: string;
  line: number | null;
  reason: string;
}

export interface SkillRiskSummary {
  level: RiskLevel;
  findings: SkillRiskFinding[];
  finding_count: number;
  findings_truncated: boolean;
}

export type SkillSource =
  | {
      kind: "github";
      owner: string;
      repo: string;
      subpath: string;
      requested_ref: string;
      pinned: boolean;
    }
  | { kind: "local"; path: string; subpath: string }
  | { kind: "archive"; path: string; subpath: string }
  | { kind: "imported"; original_path: string; backup_path: string };

export interface SkillUpdateState {
  available: boolean;
  checked_at: string | null;
  resolved_revision: string | null;
  etag: string | null;
  error: string | null;
  retry_at: string | null;
}

export interface ManagedSkillRecord {
  name: string;
  description: string;
  content_kind: SkillContentKind;
  source: SkillSource;
  resolved_revision: string | null;
  content_hash: string;
  installed_at: string;
  updated_at: string;
  risk: SkillRiskSummary;
  update: SkillUpdateState;
}

export interface SkillFile {
  path: string;
  kind: SkillFileKind;
  size: number;
  executable: boolean;
  link_target: string | null;
  sha256: string;
}

export interface SkillFileChange {
  path: string;
  kind: FileChangeKind;
  before_hash: string | null;
  after_hash: string | null;
  unified_diff: string | null;
  diff_truncated: boolean;
}

export interface SkillAgentView {
  id: string;
  name: string;
  target_id: string;
  global_dir: string;
  affected_agent_ids: string[];
  docs: string;
  evidence: string;
  verified_at: string;
}

export interface SkillTargetView {
  target_id: string;
  global_dir: string;
  primary_agent_ids: string[];
  affected_agent_ids: string[];
  assignable: boolean;
}

export type SkillLocation =
  | { kind: "central" }
  | { kind: "agent_target"; target_id: string; global_dir: string };

export interface SkillInventoryItem {
  identity: string;
  name: string;
  description: string;
  content_kind: SkillContentKind;
  states: InventoryState[];
  location: SkillLocation;
  source: SkillSource | null;
  resolved_revision: string | null;
  content_hash: string | null;
  risk: SkillRiskSummary | null;
  update: SkillUpdateState;
  assigned_target_ids: string[];
  affected_agent_ids: string[];
  consumer_agent_ids: string[];
  installed_at: string | null;
  updated_at: string | null;
}

export interface SkillsInventory {
  items: SkillInventoryItem[];
  agents: SkillAgentView[];
  capabilities?: Array<{
    id: string;
    installed: boolean;
    target_id: string;
    global_dir: string;
    affected_agent_ids: string[];
  }>;
  targets: SkillTargetView[];
  recovery_error: string | null;
}

export type AssetRef =
  | { domain: "mcp"; key: string }
  | { domain: "model"; profile_id: string }
  | { domain: "model-provider"; provider_id: string }
  | { domain: "skill"; name: string };

export type ConsumptionStatus =
  | "synced"
  | "external-added"
  | "external-changed"
  | "external-removed"
  | "unparseable"
  | "ambiguous"
  | "unsupported";

export type OwnershipState = "managed" | "external";
export type ConvergenceAction = "adopt-observed" | "restore-desired" | "detach";

export interface ConsumptionTarget {
  target_id: string;
  global_dir: string;
}

export interface ConsumptionView {
  agent_id: string;
  asset: AssetRef;
  ownership: OwnershipState;
  desired: boolean;
  observed: boolean;
  enabled?: boolean | null;
  observed_enabled?: boolean | null;
  active?: boolean | null;
  desired_active?: boolean | null;
  status: ConsumptionStatus;
  reason: string | null;
  observation_id?: string | null;
  available_actions: ConvergenceAction[];
  affected_agent_ids: string[];
  target?: ConsumptionTarget | null;
}

export interface ConsumptionInventory {
  native_plugin_agents: string[];
  host_mcps: HostMcpObservation[];
  native_plugins: NativePluginObservation[];
  revision: string;
  observed_at: string;
  consumptions: ConsumptionView[];
  external: ConsumptionView[];
  capability_errors?: Array<{
    capability: "mcp" | "model" | "skill";
    code: string;
  }>;
  target_incidents: TargetIncident[];
}

export interface TargetIncident {
  id: string;
  operation_id: string;
  capability: "mcp" | "model" | "skill";
  target_id: string;
  target_path: string;
  affected_agent_ids: string[];
  code: string;
  retryable: boolean;
}

export type AgentConsumptionSelection =
  | { domain: "mcp"; asset_keys: string[] }
  | { domain: "model"; profile_ids: string[] }
  | { domain: "skill"; names: string[] };

export type RelationshipAction = "add" | "remove";

export type CentralAssetAction = "create" | "update" | "delete";

export interface CentralAssetChange {
  asset: AssetRef;
  action: CentralAssetAction;
  summary: string[];
}

export type CentralAssetDraft =
  | {
      domain: "mcp";
      existing_key?: string;
      entry: RegistryEntry;
    }
  | {
      domain: "model";
      existing_id?: string;
      profile: ModelProfile;
      /** undefined keeps, empty string clears, non-empty replaces. */
      credential?: string;
    }
  | {
      domain: "model-provider";
      existing_id?: string;
      provider: ModelProviderConfig;
      /** undefined keeps, empty string clears, non-empty replaces. */
      credential?: string;
    };

export interface RelationshipChange {
  agent_id: string;
  asset: AssetRef;
  action: RelationshipAction;
}

export interface ConsumptionStateChange {
  agent_id: string;
  asset: AssetRef;
  before_enabled: boolean;
  after_enabled: boolean;
  affected_agent_ids: string[];
  target?: ConsumptionTarget | null;
}

export interface ModelStateSnapshot {
  added: boolean;
  enabled: boolean;
  active: boolean;
}

export interface ModelStateChange {
  agent_id: string;
  profile_id: string;
  before: ModelStateSnapshot;
  after: ModelStateSnapshot;
  fallback_profile_id?: string | null;
  reason: string;
}

export interface ModelConsumptionRecord {
  profile_id: string;
  enabled: boolean;
  credential?: {
    source_override?: ApiKeySource | null;
    delivery: ApiKeyDelivery;
  };
  last_selected_at?: string | null;
}

export interface ModelAgentSelection {
  profiles: Record<string, ModelConsumptionRecord>;
  active_profile_id?: string | null;
  default_delivery?: ApiKeyDelivery;
}

export type DomainPlan =
  | {
      domain: "mcp";
      before: Record<string, string[]>;
      after: Record<string, string[]>;
    }
  | {
      domain: "model";
      before: Record<string, ModelAgentSelection>;
      after: Record<string, ModelAgentSelection>;
    }
  | {
      domain: "skill";
      before: Record<string, string[]>;
      after: Record<string, string[]>;
    }
  | {
      domain: "agent-capabilities";
      agent_id: string;
      before: AgentConfigurationPatch;
      after: AgentConfigurationPatch;
      skills_before: Record<string, string[]>;
      skills_after: Record<string, string[]>;
      affected_agent_ids: string[];
      migrated_skill_names: string[];
    };

export interface AssetOperationPlan {
  operation_id: string;
  kind: "set-consumption" | "clear-mcp" | "clear-models" | "update-asset" | "delete-asset" | "adopt" | "update-configuration";
  domain_plan: DomainPlan;
  central_changes: CentralAssetChange[];
  relationship_changes: RelationshipChange[];
  consumption_state_changes?: ConsumptionStateChange[];
  model_state_changes: ModelStateChange[];
  target_files: string[];
  affected_agent_ids: string[];
  warnings: string[];
  can_commit: boolean;
  candidate_hash: string;
}

export interface CoreError {
  code: string;
  message: string;
  details?: Record<string, unknown>;
  retry_at?: string | null;
  confirmation?: { kind: string; token: string } | null;
}

export interface WorkspaceSnapshot {
  revision: string;
  agents: AgentCapabilityView[];
  assets: {
    mcp: RegistryEntry[];
    models: ModelProfileView[];
    skills: SkillsInventory;
  };
  relationships: ConsumptionInventory;
}

export type UnifiedOperationPlan =
  | { domain: "asset"; plan: AssetOperationPlan }
  | { domain: "skill"; plan: OperationPlan };

export type PlanOperationRequest =
  | {
      operation: "converge_consumption";
      request: {
        agent_id: string;
        asset: AssetRef;
        action: ConvergenceAction;
        observed_revision: string;
      };
    }
  | { operation: "update_central_asset"; request: { draft: CentralAssetDraft } }
  | { operation: "delete_central_asset"; request: { asset: AssetRef; source_id?: string | null } }
  | {
      operation: "set_agent_consumption";
      request: { agent_id: string; selection: AgentConsumptionSelection };
    }
  | {
      operation: "ensure_agent_consumption";
      request: { agent_id: string; selection: AgentConsumptionSelection };
    }
  | {
      operation: "remove_agent_consumption";
      request: { agent_id: string; selection: AgentConsumptionSelection };
    }
  | {
      operation: "clear_agent_mcp";
      request: { agent_id: string };
    }
  | {
      operation: "clear_agent_models";
      request: { agent_id: string };
    }
  | {
      operation: "set_asset_consumers";
      request: { asset: AssetRef; agent_ids: string[] };
    }
  | {
      operation: "update_asset_consumers";
      request: {
        asset: AssetRef;
        add_agent_ids: string[];
        remove_agent_ids: string[];
      };
    }
  | {
      operation: "set_mcp_enabled";
      request: { agent_id: string; asset_key: string; enabled: boolean };
    }
  | {
      operation: "set_all_mcp_enabled";
      request: { agent_id: string; enabled: boolean };
    }
  | {
      operation: "set_skill_enabled";
      request: { agent_id: string; name: string; enabled: boolean; target_id?: string | null };
    }
  | {
      operation: "set_model_enabled";
      request: { agent_id: string; profile_id: string; enabled: boolean };
    }
  | {
      operation: "set_active_model";
      request: { agent_id: string; profile_id: string };
    }
  | {
      operation: "update_agent_capabilities";
      request: { agent_id: string; patch: AgentConfigurationPatch };
    }
  | { operation: "install_skill"; request: PlanSkillAssetInstallRequest }
  | { operation: "import_skill"; request: PlanSkillAssetImportRequest }
  | { operation: "assign_skill"; request: PlanAssignmentRequest }
  | { operation: "update_skill"; request: PlanUpdateRequest }
  | { operation: "remove_skill"; request: PlanRemoveRequest }
  | { operation: "repair_skill"; request: PlanRepairRequest };

export type CommitOperationRequest =
  | {
      domain: "asset";
      request: {
        operation_id: string;
        candidate_hash: string;
      };
    }
  | {
      domain: "skill";
      kind: SkillOperationKind;
      request: SkillCommitRequest;
    };

export type CancelOperationRequest =
  | { domain: "asset"; operation_id: string }
  | { domain: "skill"; operation_id: string };

export type OperationCommitResult =
  | { domain: "asset"; inventory: ConsumptionInventory; converged: boolean }
  | { domain: "skill"; inventory: SkillsInventory };

export interface AssetCommandError {
  code: string;
  message: string;
  details?: Record<string, unknown>;
}

export type BackendStatus =
  | { state: "starting" }
  | { state: "ready" }
  | {
      state: "capability_unavailable";
      capability: "mcp" | "model" | "skill";
      stage: string;
      code: string;
      message: string;
    }
  | { state: "read_only"; stage: string; message: string };

export interface SkillDetail {
  item: SkillInventoryItem;
  files: SkillFile[];
  skill_md: string;
  skill_md_truncated: boolean;
}

export interface SkillCandidateSummary {
  name: string;
  description: string;
  relative_path: string;
  content_kind: SkillContentKind;
  content_hash: string;
  file_count: number;
  total_bytes: number;
}

export interface SkillSourceResolution {
  operation_id: string;
  source: SkillSource;
  resolved_revision: string | null;
  candidates: SkillCandidateSummary[];
}

export interface PlannedSkill {
  manifest: {
    name: string;
    description: string;
    license: string | null;
    compatibility: string | null;
    metadata: Record<string, string>;
    allowed_tools: string | null;
  };
  existing_source: SkillSource | null;
  source: SkillSource;
  resolved_revision: string | null;
  files: SkillFileChange[];
  risk: SkillRiskSummary;
  existing_states: InventoryState[];
  replace_existing: boolean;
  content_hash: string;
}

export interface PlannedTarget {
  target_id: string;
  global_dir: string;
  expected: PlannedLinkState;
  primary_agent_ids: string[];
  affected_agent_ids: string[];
}

export interface OperationPlan {
  operation_id: string;
  kind: SkillOperationKind;
  skills: PlannedSkill[];
  targets: PlannedTarget[];
  settings_hash: string;
  candidate_hash: string;
  findings_hash: string;
  requires_risk_override: boolean;
  warnings: string[];
}

export interface PlanInstallRequest {
  resolution_id: string;
  skill_names: string[];
  agent_ids: string[];
  replace_conflicts: boolean;
}

export interface PlanSkillAssetInstallRequest {
  resolution_id: string;
  skill_names: string[];
  replace_conflicts: boolean;
}

export interface PlanImportRequest {
  identity: string;
  agent_ids: string[];
  replace_conflicts: boolean;
}

export interface PlanSkillAssetImportRequest {
  identity: string;
  replace_conflicts: boolean;
}

export interface PlanUpdateRequest {
  skill_name: string;
  replace_local_changes: boolean;
}

export interface PlanRemoveRequest {
  skill_name: string;
}

export interface PlanAssignmentRequest {
  skill_name: string;
  agent_ids: string[];
  enabled: boolean;
}

export interface PlanRepairRequest {
  skill_name: string;
  repair: { kind: "central" } | { kind: "target"; target_id: string };
}

export interface SkillCommitRequest {
  operation_id: string;
  candidate_hash: string;
  findings_confirmation: string | null;
}

export interface UpdateCheckOutcome {
  performed: boolean;
  checked: number;
  available: string[];
  skipped_pinned: string[];
  errors: Record<string, string>;
  checked_at: string | null;
}

export interface SkillCommandError {
  code: string;
  message: string;
  retry_at?: string;
  findings_hash?: string;
}

export type PluginPresence = "bundled" | "installed" | "local" | "cache_only" | "declared";
export type PluginComponentKind = "mcp" | "skill" | "rule" | "agent" | "command" | "hook" | "app" | "lsp" | "output_style" | "workflow" | "theme" | "monitor" | "eval" | "executable" | "type_declaration" | "channel" | "mcp_bundle" | "context" | "policy" | "plan" | "code" | "auth_provider";
export interface PluginComponent {
  kind: PluginComponentKind; name: string; source_path: string; native_id: string | null;
}
export interface NativePluginObservation {
  agent_id: string; plugin_id: string; name: string; version: string; revision: string;
  presence: PluginPresence; enabled: boolean | null; source_paths: string[];
  components: PluginComponent[]; docs_url: string; source_fingerprint: string;
}
export interface HostMcpObservation {
  agent_id: string; server_id: string; name: string; provider: string;
  origin: "builtin" | "plugin"; presence: PluginPresence; versions: string[];
  source_paths: string[]; docs_url: string; source_fingerprint: string;
}

export interface NativeResourceInventory {
  native_plugin_agents: string[];
  host_mcps: HostMcpObservation[];
  native_plugins: NativePluginObservation[];
  unavailable: boolean;
}
