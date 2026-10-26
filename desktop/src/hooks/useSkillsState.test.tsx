import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../lib/api";
import type {
  OperationPlan,
  OperationCommitResult,
  SkillCommandError,
  SkillOperationKind,
  SkillsInventory,
  UpdateCheckOutcome,
} from "../lib/types";
import {
  sharedTargetPlanFixture,
  skillsInventoryFixture,
} from "../test/skillsFixtures";
import {
  normalizeSkillCommandError,
  type SkillPlanOperationRequest,
  useSkillsState,
} from "./useSkillsState";

vi.mock("../lib/api", () => ({
  listSkillsInventory: vi.fn(),
  planOperation: vi.fn(),
  commitOperation: vi.fn(),
  cancelOperation: vi.fn(),
  checkSkillUpdates: vi.fn(),
}));

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(reason: unknown): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

const outcomeFixture = (): UpdateCheckOutcome => ({
  performed: true,
  checked: 1,
  available: ["review-changes"],
  skipped_pinned: [],
  errors: {},
  checked_at: "2026-07-16T08:00:00Z",
});

const inventoryNamed = (name: string): SkillsInventory => {
  const inventory = skillsInventoryFixture();
  inventory.items[0] = {
    ...inventory.items[0],
    identity: `central:${name}`,
    name,
  };
  return inventory;
};

const planFor = (kind: SkillOperationKind): OperationPlan => ({
  ...sharedTargetPlanFixture(),
  operation_id: `${kind}-operation`,
  kind,
});

const skillPlanRequests: Array<{
  request: SkillPlanOperationRequest;
  kind: SkillOperationKind;
}> = [
  {
    request: {
      operation: "install_skill",
      request: {
        resolution_id: "resolution-id",
        skill_names: ["review-changes"],
        replace_conflicts: false,
      },
    },
    kind: "install",
  },
  {
    request: {
      operation: "import_skill",
      request: {
        identity: "target:agents-user:review-changes",
        replace_conflicts: true,
      },
    },
    kind: "import",
  },
  {
    request: {
      operation: "assign_skill",
      request: {
        skill_name: "review-changes",
        agent_ids: ["codex"],
        enabled: true,
      },
    },
    kind: "assignment",
  },
  {
    request: {
      operation: "update_skill",
      request: {
        skill_name: "review-changes",
        replace_local_changes: false,
      },
    },
    kind: "update",
  },
  {
    request: {
      operation: "remove_skill",
      request: { skill_name: "review-changes" },
    },
    kind: "remove",
  },
  {
    request: {
      operation: "repair_skill",
      request: {
        skill_name: "review-changes",
        repair: { kind: "central" },
      },
    },
    kind: "repair",
  },
];

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.listSkillsInventory).mockResolvedValue(skillsInventoryFixture());
  vi.mocked(api.planOperation).mockResolvedValue({
    domain: "skill",
    plan: sharedTargetPlanFixture(),
  });
  vi.mocked(api.commitOperation).mockResolvedValue({
    domain: "skill",
    inventory: skillsInventoryFixture(),
  });
  vi.mocked(api.cancelOperation).mockResolvedValue(undefined);
  vi.mocked(api.checkSkillUpdates).mockResolvedValue(outcomeFixture());
});

describe("useSkillsState", () => {
  it("loads and caches inventory on mount", async () => {
    const initial = inventoryNamed("initial");
    vi.mocked(api.listSkillsInventory).mockResolvedValueOnce(initial);

    const { result } = renderHook(() => useSkillsState());
    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.inventory).toBe(initial));
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
    expect(api.listSkillsInventory).toHaveBeenCalledOnce();
  });

  it("refreshes observed Skill changes without returning the view to loading", async () => {
    const initial = inventoryNamed("initial");
    const changed = inventoryNamed("changed-externally");
    const background = deferred<SkillsInventory>();
    vi.mocked(api.listSkillsInventory)
      .mockResolvedValueOnce(initial)
      .mockReturnValueOnce(background.promise);
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).toBe(initial));

    let refreshing!: Promise<SkillsInventory>;
    act(() => {
      refreshing = result.current.refreshSilently();
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.inventory).toBe(initial);

    background.resolve(changed);
    await act(async () => {
      await refreshing;
    });
    expect(result.current.inventory).toBe(changed);
    expect(result.current.loading).toBe(false);
  });

  it("hydrates from the shared workspace snapshot without another Skill scan", () => {
    const snapshotInventory = inventoryNamed("workspace");
    const { result } = renderHook(() => useSkillsState({ autoLoad: false }));

    act(() => result.current.hydrate(snapshotInventory));

    expect(result.current.inventory).toBe(snapshotInventory);
    expect(result.current.loading).toBe(false);
    expect(api.listSkillsInventory).not.toHaveBeenCalled();
  });

  it.each(skillPlanRequests)(
    "plans $request.operation through the exact unified request envelope",
    async ({ request, kind }) => {
      const planned = planFor(kind);
      vi.mocked(api.planOperation).mockResolvedValueOnce({
        domain: "skill",
        plan: planned,
      });
      const { result } = renderHook(() => useSkillsState());

      let returned!: OperationPlan;
      await act(async () => {
        returned = await result.current.plan(request);
      });

      expect(api.planOperation).toHaveBeenCalledWith(request);
      expect(returned).toBe(planned);
    },
  );

  it("restores the Skill findings token from the unified Core confirmation", () => {
    expect(normalizeSkillCommandError({
      code: "confirmation_required",
      message: "请确认风险。",
      confirmation: {
        kind: "skill_findings",
        token: "findings-from-unified-core",
      },
    })).toEqual({
      code: "confirmation_required",
      message: "请确认风险。",
      findings_hash: "findings-from-unified-core",
    });
  });

  it.each<SkillOperationKind>([
    "install",
    "import",
    "update",
    "remove",
    "assignment",
    "repair",
  ])("dispatches %s through the sole committer with the exact confirmation string", async (kind) => {
    const next = inventoryNamed(`${kind}-committed`);
    vi.mocked(api.commitOperation).mockResolvedValueOnce({
      domain: "skill",
      inventory: next,
    });
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).not.toBeNull());

    await act(async () => {
      await result.current.commit(planFor(kind), "findings-from-core");
    });

    expect(api.commitOperation).toHaveBeenCalledWith({
      domain: "skill",
      kind,
      request: {
        operation_id: `${kind}-operation`,
        candidate_hash: "candidate-hash",
        findings_confirmation: "findings-from-core",
      },
    });
    expect(result.current.inventory).toBe(next);
  });

  it("claims commits synchronously and rejects a duplicate without invoking it", async () => {
    const pending = deferred<OperationCommitResult>();
    vi.mocked(api.commitOperation).mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).not.toBeNull());

    let first!: Promise<SkillsInventory>;
    let second!: Promise<SkillsInventory>;
    act(() => {
      first = result.current.commit(planFor("install"), null);
      second = result.current.commit(planFor("import"), null);
    });

    await expect(second).rejects.toEqual({
      code: "operation_pending",
      message: "已有 Skill 操作正在进行。",
    });
    expect(api.commitOperation).toHaveBeenCalledOnce();
    expect(result.current.pendingOperation).toBe("install-operation");

    pending.resolve({
      domain: "skill",
      inventory: inventoryNamed("committed"),
    });
    await act(async () => {
      await first;
    });
    expect(result.current.pendingOperation).toBeNull();
  });

  it("preserves structured failures, keeps cache, and clears pending after settlement", async () => {
    const initial = skillsInventoryFixture();
    const commandError: SkillCommandError = {
      code: "confirmation_required",
      message: "请确认风险。",
      retry_at: "2026-07-16T09:00:00Z",
      findings_hash: "findings-current",
    };
    vi.mocked(api.listSkillsInventory).mockResolvedValueOnce(initial);
    vi.mocked(api.commitOperation).mockRejectedValueOnce(commandError);
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).toBe(initial));

    await act(async () => {
      await expect(
        result.current.commit(planFor("install"), null),
      ).rejects.toEqual(commandError);
    });

    expect(result.current.inventory).toBe(initial);
    expect(result.current.error).toEqual(commandError);
    expect(result.current.pendingOperation).toBeNull();
  });

  it("normalizes unknown rejection values without leaking their contents", async () => {
    vi.mocked(api.commitOperation).mockRejectedValueOnce(
      new Error("private path: /Users/example/.arca/skills"),
    );
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).not.toBeNull());

    const expected = { code: "unknown", message: "操作失败，请重试。" };
    await act(async () => {
      await expect(
        result.current.commit(planFor("install"), null),
      ).rejects.toEqual(expected);
    });
    expect(result.current.error).toEqual(expected);
  });

  it("does not let an older refresh overwrite a newer commit result", async () => {
    const oldRefresh = deferred<SkillsInventory>();
    const committed = inventoryNamed("authoritative-commit");
    vi.mocked(api.listSkillsInventory).mockReturnValueOnce(oldRefresh.promise);
    vi.mocked(api.commitOperation).mockResolvedValueOnce({
      domain: "skill",
      inventory: committed,
    });
    const { result } = renderHook(() => useSkillsState());

    await act(async () => {
      await result.current.commit(planFor("install"), null);
    });
    expect(result.current.inventory).toBe(committed);

    oldRefresh.resolve(inventoryNamed("stale-refresh"));
    await act(async () => {
      await oldRefresh.promise;
    });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.inventory).toBe(committed);
  });

  it("checks then refreshes but suppresses a check started before a newer commit", async () => {
    const check = deferred<UpdateCheckOutcome>();
    const initial = inventoryNamed("initial");
    const stale = inventoryNamed("stale-check-refresh");
    const committed = inventoryNamed("authoritative-commit");
    vi.mocked(api.listSkillsInventory)
      .mockResolvedValueOnce(initial)
      .mockResolvedValueOnce(stale);
    vi.mocked(api.checkSkillUpdates).mockReturnValueOnce(check.promise);
    vi.mocked(api.commitOperation).mockResolvedValueOnce({
      domain: "skill",
      inventory: committed,
    });
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).toBe(initial));

    let checking!: Promise<UpdateCheckOutcome>;
    act(() => {
      checking = result.current.checkUpdates(true);
    });
    await act(async () => {
      await result.current.commit(planFor("install"), null);
    });
    check.resolve(outcomeFixture());
    await act(async () => {
      await expect(checking).resolves.toEqual(outcomeFixture());
    });

    expect(api.listSkillsInventory).toHaveBeenCalledTimes(2);
    expect(result.current.inventory).toBe(committed);
  });

  it("returns the original check outcome after refreshing inventory", async () => {
    const outcome = outcomeFixture();
    const updated = inventoryNamed("update-available");
    vi.mocked(api.listSkillsInventory)
      .mockResolvedValueOnce(skillsInventoryFixture())
      .mockResolvedValueOnce(updated);
    vi.mocked(api.checkSkillUpdates).mockResolvedValueOnce(outcome);
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).not.toBeNull());

    let returned!: UpdateCheckOutcome;
    await act(async () => {
      returned = await result.current.checkUpdates(true);
    });
    expect(returned).toBe(outcome);
    expect(result.current.inventory).toBe(updated);
  });

  it("keeps an in-flight commit pending when staging cleanup is requested", async () => {
    const pending = deferred<OperationCommitResult>();
    vi.mocked(api.commitOperation).mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).not.toBeNull());

    let committing!: Promise<SkillsInventory>;
    act(() => {
      committing = result.current.commit(planFor("install"), null);
    });
    await act(async () => {
      await result.current.cancel("install-operation");
    });
    expect(api.cancelOperation).toHaveBeenCalledWith({
      domain: "skill",
      operation_id: "install-operation",
    });
    expect(result.current.pendingOperation).toBe("install-operation");

    pending.resolve({
      domain: "skill",
      inventory: inventoryNamed("committed"),
    });
    await act(async () => {
      await committing;
    });
    expect(result.current.pendingOperation).toBeNull();
  });

  it("does not update state after unmount when a refresh settles", async () => {
    const first = deferred<SkillsInventory>();
    const second = deferred<SkillsInventory>();
    vi.mocked(api.listSkillsInventory)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { result, unmount } = renderHook(() => useSkillsState());
    const explicitRefresh = result.current.refresh();
    unmount();

    first.resolve(inventoryNamed("old"));
    second.resolve(inventoryNamed("after-unmount"));
    await expect(explicitRefresh).resolves.toEqual(inventoryNamed("after-unmount"));
    expect(consoleError).not.toHaveBeenCalled();
  });

  it("keeps cached inventory and exposes a normalized failed update check", async () => {
    const initial = skillsInventoryFixture();
    vi.mocked(api.listSkillsInventory).mockResolvedValueOnce(initial);
    vi.mocked(api.checkSkillUpdates).mockRejectedValueOnce({
      code: "network",
      message: "GitHub rate limited the request.",
      retry_at: "2026-07-16T10:00:00Z",
    });
    const { result } = renderHook(() => useSkillsState());
    await waitFor(() => expect(result.current.inventory).toBe(initial));

    await act(async () => {
      await expect(result.current.checkUpdates(true)).rejects.toEqual({
        code: "network",
        message: "GitHub rate limited the request.",
        retry_at: "2026-07-16T10:00:00Z",
      });
    });
    expect(result.current.inventory).toBe(initial);
    expect(result.current.error?.code).toBe("network");
  });
});
