import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { formatErrorDetails } from "../lib/format";

export type StartupTaskStatus = "pending" | "running" | "complete" | "error";

export interface StartupTask {
  id: string;
  label: string;
  run: () => Promise<unknown>;
}

export interface StartupTaskView {
  id: string;
  label: string;
  status: StartupTaskStatus;
  error: string | null;
}

export interface StartupSyncState {
  tasks: StartupTaskView[];
  completed: number;
  total: number;
  activeLabel: string | null;
  failed: number;
  syncing: boolean;
  slow: boolean;
  settled: boolean;
  retryFailed(): Promise<void>;
}

interface StartupSyncOptions {
  foreground: StartupTask[];
  deferred: StartupTask[];
  foregroundConcurrency?: number;
  slowAfterMs?: number;
  deferredDelayMs?: number;
}

async function runBounded(
  tasks: StartupTask[],
  concurrency: number,
  run: (task: StartupTask) => Promise<void>,
): Promise<void> {
  let cursor = 0;
  const worker = async () => {
    while (cursor < tasks.length) {
      const task = tasks[cursor++];
      await run(task);
    }
  };
  await Promise.all(
    Array.from(
      { length: Math.min(Math.max(1, concurrency), tasks.length) },
      worker,
    ),
  );
}

const wait = (milliseconds: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

export function useStartupSync({
  foreground,
  deferred,
  foregroundConcurrency = 2,
  slowAfterMs = 3_000,
  deferredDelayMs = 450,
}: StartupSyncOptions): StartupSyncState {
  // Startup is intentionally a one-shot snapshot. Parent renders can replace
  // wrapper objects while individual scans are running; restarting the whole
  // pipeline would duplicate fresh disk reads and starve the first interaction.
  const initial = useRef({
    foreground,
    deferred,
    foregroundConcurrency,
    slowAfterMs,
    deferredDelayMs,
  });
  const allTasks = useMemo(
    () => [...initial.current.foreground, ...initial.current.deferred],
    [],
  );
  const taskById = useMemo(
    () => new Map(allTasks.map((task) => [task.id, task])),
    [allTasks],
  );
  const [statuses, setStatuses] = useState<Record<string, StartupTaskView>>(() =>
    Object.fromEntries(
      allTasks.map((task) => [
        task.id,
        { id: task.id, label: task.label, status: "pending", error: null },
      ]),
    ),
  );
  const statusesRef = useRef(statuses);
  statusesRef.current = statuses;
  const [slow, setSlow] = useState(false);
  const mounted = useRef(true);
  const started = useRef(false);
  const refreshing = useRef(false);
  const runGeneration = useRef(0);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const runOne = useCallback(async (task: StartupTask, generation: number) => {
    if (!mounted.current || generation !== runGeneration.current) return;
    if (mounted.current && generation === runGeneration.current) {
      setStatuses((current) => ({
        ...current,
        [task.id]: {
          id: task.id,
          label: task.label,
          status: "running",
          error: null,
        },
      }));
    }
    try {
      await task.run();
      if (mounted.current && generation === runGeneration.current) {
        setStatuses((current) => ({
          ...current,
          [task.id]: {
            id: task.id,
            label: task.label,
            status: "complete",
            error: null,
          },
        }));
      }
    } catch (error) {
      if (mounted.current && generation === runGeneration.current) {
        setStatuses((current) => ({
          ...current,
          [task.id]: {
            id: task.id,
            label: task.label,
            status: "error",
            error: formatErrorDetails(error),
          },
        }));
      }
    }
  }, []);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    const options = initial.current;
    const generation = ++runGeneration.current;
    setStatuses(
      Object.fromEntries(
        allTasks.map((task) => [
          task.id,
          { id: task.id, label: task.label, status: "pending", error: null },
        ]),
      ),
    );
    setSlow(false);
    const slowTimer = setTimeout(() => {
      if (mounted.current && generation === runGeneration.current) setSlow(true);
    }, options.slowAfterMs);

    void (async () => {
      await runBounded(
        options.foreground,
        options.foregroundConcurrency,
        (task) => runOne(task, generation),
      );
      if (options.deferred.length > 0) {
        await wait(options.deferredDelayMs);
        for (const task of options.deferred) {
          if (!mounted.current || generation !== runGeneration.current) return;
          await runOne(task, generation);
        }
      }
      clearTimeout(slowTimer);
      if (mounted.current && generation === runGeneration.current) setSlow(false);
    })();

    // Do not cancel the one-shot pipeline during React StrictMode's simulated
    // unmount/remount. Every state write is still guarded by `mounted`, and a
    // real unmount prevents deferred tasks from starting.
  }, [allTasks, runOne]);

  const refreshTasks = useCallback(async (taskIds: string[]) => {
    if (refreshing.current || Object.values(statusesRef.current).some(
      (task) => task.status === "pending" || task.status === "running",
    )) return;
    const selected = [...new Set(taskIds)]
      .map((id) => taskById.get(id))
      .filter((task): task is StartupTask => task != null);
    if (selected.length === 0) return;
    refreshing.current = true;
    const generation = ++runGeneration.current;
    setStatuses((current) => {
      const next = { ...current };
      for (const task of selected) {
        next[task.id] = {
          id: task.id,
          label: task.label,
          status: "pending",
          error: null,
        };
      }
      return next;
    });
    setSlow(false);
    const slowTimer = setTimeout(() => {
      if (mounted.current && generation === runGeneration.current) setSlow(true);
    }, slowAfterMs);
    try {
      await runBounded(
        selected,
        Math.min(foregroundConcurrency, selected.length),
        (task) => runOne(task, generation),
      );
    } finally {
      clearTimeout(slowTimer);
      refreshing.current = false;
      if (mounted.current && generation === runGeneration.current) setSlow(false);
    }
  }, [foregroundConcurrency, runOne, slowAfterMs, taskById]);

  const retryFailed = useCallback(
    () => refreshTasks(
      Object.values(statuses)
        .filter((task) => task.status === "error")
        .map((task) => task.id),
    ),
    [refreshTasks, statuses],
  );

  const tasks = allTasks.map(
    (task) =>
      statuses[task.id] ?? {
        id: task.id,
        label: task.label,
        status: "pending" as const,
        error: null,
      },
  );
  const completed = tasks.filter((task) => task.status === "complete").length;
  const failed = tasks.filter((task) => task.status === "error").length;
  const syncing = tasks.some(
    (task) => task.status === "pending" || task.status === "running",
  );
  const settled = !syncing;
  const activeLabel =
    tasks.find((task) => task.status === "running")?.label ??
    tasks.find((task) => task.status === "pending")?.label ??
    null;

  return {
    tasks,
    completed,
    total: tasks.length,
    activeLabel,
    failed,
    syncing,
    slow,
    settled,
    retryFailed,
  };
}
