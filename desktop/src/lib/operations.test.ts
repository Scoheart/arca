import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  cancelOperation,
  commitOperation,
  getWorkspaceSnapshot,
  planOperation,
} from "./api";
import type {
  CancelOperationRequest,
  CommitOperationRequest,
  PlanOperationRequest,
} from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
});

describe("unified operation wire contract", () => {
  it("passes the complete tagged plan envelope under request", async () => {
    const assetRequest: PlanOperationRequest = {
      operation: "set_agent_consumption",
      request: {
        agent_id: "codex",
        selection: {
          domain: "mcp",
          asset_keys: ["local::stdio"],
        },
      },
    };
    await planOperation(assetRequest);
    expect(invokeMock).toHaveBeenLastCalledWith("plan_operation", {
      request: assetRequest,
    });

    const clearModelsRequest: PlanOperationRequest = {
      operation: "clear_agent_models",
      request: { agent_id: "pi" },
    };
    await planOperation(clearModelsRequest);
    expect(invokeMock).toHaveBeenLastCalledWith("plan_operation", {
      request: clearModelsRequest,
    });

    const skillRequest: PlanOperationRequest = {
      operation: "assign_skill",
      request: {
        skill_name: "review-changes",
        agent_ids: ["codex"],
        enabled: true,
      },
    };
    await planOperation(skillRequest);
    expect(invokeMock).toHaveBeenLastCalledWith("plan_operation", {
      request: skillRequest,
    });
  });

  it("keeps asset and Skill commit/cancel discriminants intact", async () => {
    const assetCommit: CommitOperationRequest = {
      domain: "asset",
      request: {
        operation_id: "asset-operation",
        candidate_hash: "asset-candidate",
      },
    };
    await commitOperation(assetCommit);
    expect(invokeMock).toHaveBeenLastCalledWith("commit_operation", {
      request: assetCommit,
    });

    const skillCommit: CommitOperationRequest = {
      domain: "skill",
      kind: "assignment",
      request: {
        operation_id: "skill-operation",
        candidate_hash: "skill-candidate",
        findings_confirmation: null,
      },
    };
    await commitOperation(skillCommit);
    expect(invokeMock).toHaveBeenLastCalledWith("commit_operation", {
      request: skillCommit,
    });

    const cancellations: CancelOperationRequest[] = [
      { domain: "asset", operation_id: "asset-operation" },
      { domain: "skill", operation_id: "skill-operation" },
    ];
    for (const request of cancellations) {
      await cancelOperation(request);
      expect(invokeMock).toHaveBeenLastCalledWith("cancel_operation", {
        request,
      });
    }
  });

  it("loads the revisioned workspace through the unified query", async () => {
    await getWorkspaceSnapshot();
    expect(invokeMock).toHaveBeenCalledWith("get_workspace_snapshot", undefined);
  });
});
