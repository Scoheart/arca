import { useEffect, useRef, useState } from "react";
import { describeError, runModel } from "./api";
import type { RequestSnapshot, RoundRequest, RunResult, RunRound, SharedInput } from "./types";

const MAX_ROUNDS = 12;
const HISTORY_CHARACTER_BUDGET = 4 * 1024 * 1024;

interface ActiveRequest {
  roundId: number;
  controller: AbortController;
  text: string;
  frame?: number;
}

function requestKey(roundId: number, requestId: number) {
  return roundId + "/" + requestId;
}

function responseText(result: RunResult) {
  return result.status === "done" ? result.response.text : result.text;
}

function keepRecent(rounds: RunRound[]) {
  let remaining = HISTORY_CHARACTER_BUDGET;
  return rounds.slice(0, MAX_ROUNDS - 1).filter((round) => {
    remaining -= round.prompt.length + round.instructions.length;
    for (const request of round.requests) {
      remaining -= request.parameters.extra.length + responseText(request.result).length;
    }
    return remaining >= 0;
  });
}

export function useRounds(token: string | null) {
  const [rounds, setRounds] = useState<RunRound[]>([]);
  const [viewId, setViewId] = useState<number>();
  const nextId = useRef(1);
  const active = useRef(new Map<string, ActiveRequest>());
  const round = rounds.find((row) => row.id === viewId) ?? rounds[0];
  const runningRound = rounds.find((row) => row.requests.some(({ result }) => result.status === "running"));

  useEffect(() => {
    const pool = active.current;
    return () => {
      for (const request of pool.values()) {
        request.controller.abort();
        if (request.frame !== undefined) cancelAnimationFrame(request.frame);
      }
      pool.clear();
    };
  }, []);

  function update(roundId: number, requestId: number, result: RunResult) {
    setRounds((all) => all.map((item) => {
      if (item.id !== roundId) return item;
      return {
        ...item,
        requests: item.requests.map((row) => row.id === requestId ? { ...row, result } : row),
      };
    }));
  }

  async function execute(round: RunRound, request: RequestSnapshot, updateStarted: boolean) {
    if (!token) return;
    const key = requestKey(round.id, request.id);
    if (active.current.has(key)) return;
    const startedAt = performance.now();
    const entry: ActiveRequest = { roundId: round.id, controller: new AbortController(), text: "" };
    active.current.set(key, entry);
    if (updateStarted) update(round.id, request.id, { status: "running", startedAt, text: "" });

    try {
      const response = await runModel(token, request.parameters, request.model, round, entry.controller.signal, (delta) => {
        entry.text += delta;
        if (entry.frame !== undefined) return;
        entry.frame = requestAnimationFrame(() => {
          entry.frame = undefined;
          if (!entry.controller.signal.aborted) {
            update(round.id, request.id, { status: "running", startedAt, text: entry.text });
          }
        });
      });
      if (!entry.controller.signal.aborted) update(round.id, request.id, { status: "done", response });
    } catch (error) {
      if (!entry.controller.signal.aborted) {
        update(round.id, request.id, { status: "error", error: describeError(error), text: entry.text });
      }
    } finally {
      if (entry.frame !== undefined) cancelAnimationFrame(entry.frame);
      if (active.current.get(key) === entry) active.current.delete(key);
    }
  }

  function start(shared: SharedInput, requests: RequestSnapshot[]) {
    if (!token || active.current.size || !requests.length) return false;
    const id = nextId.current++;
    const startedAt = performance.now();
    const round: RunRound = {
      ...shared,
      id,
      createdAt: Date.now(),
      requests: requests.map((request) => ({
        ...request,
        result: { status: "running", startedAt, text: "" },
      })),
    };
    setRounds((all) => [round, ...keepRecent(all)]);
    setViewId(id);
    for (const request of requests) void execute(round, request, false);
    return true;
  }

  function retry(round: RunRound, request: RoundRequest) {
    if ([...active.current.values()].some((entry) => entry.roundId !== round.id)) return;
    if (request.result.status === "error" || request.result.status === "cancelled") {
      void execute(round, request, true);
    }
  }

  function stop(roundId: number, requestId: number) {
    const key = requestKey(roundId, requestId);
    const entry = active.current.get(key);
    if (!entry) return;
    entry.controller.abort();
    if (entry.frame !== undefined) cancelAnimationFrame(entry.frame);
    active.current.delete(key);
    update(roundId, requestId, { status: "cancelled", text: entry.text });
  }

  return { rounds, round, runningRound, select: setViewId, start, retry, stop };
}

export function exportRound(round: RunRound) {
  const content = JSON.stringify({
    schema_version: 1,
    round: round.id,
    created_at: new Date(round.createdAt).toISOString(),
    prompt: round.prompt,
    instructions: round.instructions,
    requests: round.requests.map(({ label, model, parameters, result }) => ({
      label,
      model: { name: model.name, model_id: model.model, provider: model.provider, protocol: model.protocol },
      parameters,
      result,
    })),
  }, null, 2);
  const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
  const download = document.createElement("a");
  download.href = url;
  download.download = "arca-playground-" + new Date(round.createdAt).toISOString().slice(0, 10) + "-" + round.id + ".json";
  download.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
