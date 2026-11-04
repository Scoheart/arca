import { describe, expect, it } from "vitest";
import { assetOperationPlanFixture } from "../test/consumptionFixtures";
import { requiresAgentReview } from "./agentOperation";

function plan() {
  const value = assetOperationPlanFixture();
  value.warnings = [];
  value.can_commit = true;
  value.relationship_changes = [];
  value.model_state_changes = [];
  value.affected_agent_ids = ["codex"];
  return value;
}

describe("Agent operation review policy", () => {
  it("lets routine additions and removals use progress feedback", () => {
    const addition = plan();
    addition.relationship_changes = [{
      agent_id: "codex",
      asset: { domain: "mcp", key: "github::stdio" },
      action: "add",
    }];
    expect(requiresAgentReview(addition)).toBe(false);

    const removal = plan();
    removal.relationship_changes = [{
      agent_id: "codex",
      asset: { domain: "mcp", key: "github::stdio" },
      action: "remove",
    }];
    expect(requiresAgentReview(removal)).toBe(false);
  });

  it("does not interrupt a safe shared-directory addition", () => {
    const addition = plan();
    addition.affected_agent_ids = ["claude-code", "codex"];
    addition.relationship_changes = [
      {
        agent_id: "codex",
        asset: { domain: "skill", name: "review-changes" },
        action: "add",
      },
      {
        agent_id: "claude-code",
        asset: { domain: "skill", name: "review-changes" },
        action: "add",
      },
    ];
    expect(requiresAgentReview(addition)).toBe(false);
  });

  it("keeps review for conflicts, shared removals, and removing the current Model", () => {
    const conflict = plan();
    conflict.warnings = ["codex: mcp_config_drift"];
    expect(requiresAgentReview(conflict)).toBe(true);

    const sharedRemoval = plan();
    sharedRemoval.affected_agent_ids = ["claude-code", "codex"];
    sharedRemoval.relationship_changes = [{
      agent_id: "codex",
      asset: { domain: "skill", name: "review-changes" },
      action: "remove",
    }];
    expect(requiresAgentReview(sharedRemoval)).toBe(true);

    const currentModel = plan();
    currentModel.model_state_changes = [{
      agent_id: "pi",
      profile_id: "work",
      before: { added: true, enabled: true, active: true },
      after: { added: false, enabled: false, active: false },
      reason: "model_removed",
    }];
    expect(requiresAgentReview(currentModel)).toBe(true);

  });

  it("leaves clear operations to their dedicated confirmation flow", () => {
    for (const kind of ["clear-mcp", "clear-models"] as const) {
      const clear = plan();
      clear.kind = kind;
      expect(requiresAgentReview(clear)).toBe(false);
      clear.can_commit = false;
      expect(requiresAgentReview(clear)).toBe(true);
    }
  });
});
