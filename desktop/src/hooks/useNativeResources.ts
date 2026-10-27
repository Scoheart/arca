import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { listNativeResources } from "../lib/api";
import type { NativeResourceInventory } from "../lib/types";
import type { ObservationChange } from "../lib/observationRefresh";

/** Host metadata stays available while credential-backed asset reads are pending. */
export function useNativeResources(agentId: string) {
  const [state, setState] = useState<{ agentId: string; inventory: NativeResourceInventory | null; error: boolean }>({ agentId, inventory: null, error: false });
  useEffect(() => {
    let active = true;
    let generation = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = () => {
      if (!active) return;
      const request = ++generation;
      void listNativeResources(agentId).then(inventory => {
        if (active && request === generation) setState({ agentId, inventory, error: false });
      }).catch(() => {
        if (active && request === generation) setState({ agentId, inventory: null, error: true });
      });
    };
    const subscription = listen<ObservationChange>("asset-observation-changed", event => {
      if (!active) return;
      const domains = event.payload?.domains;
      if (domains?.length && !domains.includes("mcp") && !domains.includes("central")) return;
      clearTimeout(timer);
      timer = setTimeout(refresh, 200);
    });
    // Subscribe first, then read, so a change during the first scan is not lost.
    void subscription.then(() => { if (active) refresh(); }).catch(() => { if (active) refresh(); });
    return () => { active = false; clearTimeout(timer); void subscription.then(unlisten => unlisten()).catch(() => undefined); };
  }, [agentId]);
  return state.agentId === agentId ? state : { agentId, inventory: null, error: false };
}
