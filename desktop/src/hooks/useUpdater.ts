import { readLocalSetting, writeLocalSetting } from "../lib/localSettings";
import { useCallback, useEffect, useRef, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import type { Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { updateEnvironment } from "../lib/api";

// Remember the version the user clicked "稍后" on, so the startup check stops
// nagging about it. A manual check (点版本号) always ignores this.
const DISMISS_KEY = "arca-update-dismissed";

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "downloading"; percent: number | null }
  | { kind: "ready"; version: string }
  | {
      kind: "requires-install";
      reason: "disk-image" | "app-translocation" | "read-only-volume";
    }
  | { kind: "error"; operation: "check" | "install" | "restart"; message: string };

export interface UpdaterState {
  phase: UpdatePhase;
  /** Check the release channel. Returns what happened so callers can toast. */
  checkNow: (opts?: { manual?: boolean }) => Promise<"available" | "latest" | "error">;
  /** Download + stage the available update (with progress in `phase`). */
  download: () => Promise<void>;
  /** Relaunch into the updated build. */
  restart: () => Promise<void>;
  /** "稍后" — hide the banner and don't re-prompt for this version on startup. */
  dismiss: () => void;
  /** "下次启动时" — update is staged; just hide the banner. */
  later: () => void;
}

export function useUpdater(
  proxyUrl: string | null = null,
  { autoCheck = true }: { autoCheck?: boolean } = {},
): UpdaterState {
  const [phase, setPhase] = useState<UpdatePhase>({ kind: "idle" });
  const updateRef = useRef<Update | null>(null);
  const proxyUrlRef = useRef(proxyUrl);

  useEffect(() => {
    proxyUrlRef.current = proxyUrl;
  }, [proxyUrl]);

  const checkNow = useCallback(
    async ({ manual = false }: { manual?: boolean } = {}) => {
      setPhase({ kind: "checking" });
      try {
        const update = await check({ proxy: proxyUrlRef.current ?? undefined });
        if (update) {
          if (!manual && readLocalSetting(DISMISS_KEY) === update.version) {
            // User already said "稍后" to this exact version — stay quiet.
            setPhase({ kind: "idle" });
            return "latest" as const;
          }
          updateRef.current = update;
          setPhase({
            kind: "available",
            version: update.version,
            notes: update.body?.trim() || null,
          });
          return "available" as const;
        }
        setPhase({ kind: "idle" });
        return "latest" as const;
      } catch (e) {
        // A silent startup check failing (offline, rate-limit…) should never
        // surface UI; only manual checks report the error.
        setPhase(
          manual
            ? { kind: "error", operation: "check", message: String(e) }
            : { kind: "idle" },
        );
        return "error" as const;
      }
    },
    [],
  );

  const download = useCallback(async () => {
    const update = updateRef.current;
    if (!update) return;
    try {
      const environment = await updateEnvironment();
      if (!environment.canSelfUpdate && environment.reason) {
        setPhase({ kind: "requires-install", reason: environment.reason });
        return;
      }
    } catch {
      // If the guard itself is unavailable, let the signed updater run and
      // preserve its normal error handling below.
    }
    setPhase({ kind: "downloading", percent: null });
    let total: number | null = null;
    let received = 0;
    let reportedPercent: number | null = null;
    try {
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") {
          total = ev.data.contentLength ?? null;
          received = 0;
          reportedPercent = null;
        } else if (ev.event === "Progress") {
          received += ev.data.chunkLength;
          if (total) {
            const percent = Math.min(99, Math.round((received / total) * 100));
            if (percent !== reportedPercent) {
              reportedPercent = percent;
              setPhase({ kind: "downloading", percent });
            }
          }
        } else if (ev.event === "Finished") {
          setPhase({ kind: "downloading", percent: 100 });
        }
      });
      setPhase({ kind: "ready", version: update.version });
    } catch (e) {
      const message = String(e);
      if (/read-only file system|os error 30/i.test(message)) {
        setPhase({ kind: "requires-install", reason: "read-only-volume" });
      } else {
        setPhase({ kind: "error", operation: "install", message });
      }
    }
  }, []);

  const restart = useCallback(async () => {
    try {
      await relaunch();
    } catch (e) {
      setPhase({ kind: "error", operation: "restart", message: String(e) });
    }
  }, []);

  const dismiss = useCallback(() => {
    const v = updateRef.current?.version;
    if (v) writeLocalSetting(DISMISS_KEY, v);
    setPhase({ kind: "idle" });
  }, []);

  const later = useCallback(() => setPhase({ kind: "idle" }), []);

  // Silent check shortly after launch — delayed so it never competes with the
  // startup scan, and any failure is swallowed by checkNow itself.
  useEffect(() => {
    if (!autoCheck) return;
    const t = setTimeout(() => void checkNow(), 2500);
    return () => clearTimeout(t);
  }, [autoCheck, checkNow]);

  return { phase, checkNow, download, restart, dismiss, later };
}
