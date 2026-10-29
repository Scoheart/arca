import type { AssetOperationPlan, ConsumptionInventory } from "../lib/types";

export const consumptionInventoryFixture = (): ConsumptionInventory => ({
  revision: "fixture-revision",
  observed_at: "2026-08-04T00:00:00Z",
  target_incidents: [],
  native_plugin_agents: [], host_mcps: [], native_plugins: [],
  consumptions: [
    {
      agent_id: "claude-code",
      asset: { domain: "mcp", key: "github::stdio" },
      ownership: "managed",
      desired: true,
      observed: true,
      status: "synced",
      reason: null,
      affected_agent_ids: ["claude-code"],
      available_actions: [],
    },
    {
      agent_id: "codex",
      asset: { domain: "skill", name: "review-changes" },
      ownership: "managed",
      desired: true,
      observed: false,
      status: "external-removed",
      reason: "skill_target_missing",
      affected_agent_ids: ["codex", "cursor", "gemini"],
      available_actions: ["restore-desired", "detach"],
    },
  ],
  external: [
    {
      agent_id: "claude-code",
      asset: { domain: "mcp", key: "external::http" },
      ownership: "external",
      desired: false,
      observed: true,
      status: "external-added",
      reason: "mcp_external_unmanaged",
      affected_agent_ids: ["claude-code"],
      available_actions: ["adopt-observed"],
    },
  ],
});

export const assetOperationPlanFixture = (): AssetOperationPlan => ({
  operation_id: "00000000-0000-4000-8000-000000000001",
  kind: "set-consumption",
  domain_plan: {
    domain: "mcp",
    before: { "claude-code": ["github::stdio"] },
    after: { "claude-code": ["github::stdio", "filesystem::stdio"] },
  },
  central_changes: [],
  relationship_changes: [
    {
      agent_id: "claude-code",
      asset: { domain: "mcp", key: "filesystem::stdio" },
      action: "add",
    },
  ],
  consumption_state_changes: [],
  model_state_changes: [],
  target_files: ["~/.claude.json"],
  affected_agent_ids: ["claude-code"],
  warnings: [],
  can_commit: true,
  candidate_hash: "candidate",
});
