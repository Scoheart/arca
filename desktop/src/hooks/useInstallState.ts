import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  addLocalSourceDialog,
  subscribeSource,
  importPastedConfig,
  listAgents,
  getRegistrySnapshot,
  listSources,
  refreshSource,
  removeSource,
  scanInstalled,
  setSourceEnabled,
} from "../lib/api";
import type {
  AgentInfo,
  CatalogItem,
  InstalledMcp,
  RegistryEntry,
  SourceView,
} from "../lib/types";
import { installedKey } from "../lib/mcp";
import { formatErrorDetails } from "../lib/format";

/**
 * Read model for the MCP asset library and observed Agent files. Mutating an
 * Agent's MCP usage is intentionally absent: all relationships go through
 * useConsumptionState's plan/review/commit lifecycle.
 */
export interface InstallState {
  entries: RegistryEntry[];
  catalog: CatalogItem[];
  agents: AgentInfo[];
  installed: InstalledMcp[];
  loading: boolean;
  registryError: string | null;
  agentsForServer(serverKey: string): string[];
  customKeys: Set<string>;
  rescan(): Promise<InstalledMcp[]>;
  refreshAll(): Promise<void>;
  refreshRegistry(): Promise<RegistryEntry[]>;
  refreshAgents(): Promise<AgentInfo[]>;
  sources: SourceView[];
  refreshSources(): Promise<SourceView[]>;
  subscribe(url: string, name?: string): Promise<SourceView>;
  pickLocalSource(): Promise<SourceView | null>;
  rescanDiscovered(): Promise<void>;
  refreshOneSource(id: string): Promise<void>;
  toggleSource(id: string, enabled: boolean): Promise<void>;
  deleteSource(id: string): Promise<void>;
  importPaste(text: string): Promise<string[]>;
}

export function useInstallState({ autoLoad = true }: { autoLoad?: boolean } = {}): InstallState {
  const [entries, setEntries] = useState<RegistryEntry[]>([]);
  const [catalog, setCatalog] = useState<CatalogItem[]>([]);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [installed, setInstalled] = useState<InstalledMcp[]>([]);
  const [loading, setLoading] = useState(true);
  const [registryError, setRegistryError] = useState<string | null>(null);
  const [customKeys, setCustomKeys] = useState<Set<string>>(new Set());
  const [sources, setSources] = useState<SourceView[]>([]);
  const generations = useRef({ registry: 0, sources: 0, agents: 0, installed: 0 });
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const rescan = useCallback(async () => {
    const generation = ++generations.current.installed;
    const next = await scanInstalled();
    if (mounted.current && generation === generations.current.installed) setInstalled(next);
    return next;
  }, []);

  const refreshAgents = useCallback(async () => {
    const generation = ++generations.current.agents;
    const next = await listAgents();
    if (mounted.current && generation === generations.current.agents) setAgents(next);
    return next;
  }, []);

  const refreshRegistry = useCallback(async () => {
    const generation = ++generations.current.registry;
    const sourceGeneration = ++generations.current.sources;
    const current = () => mounted.current && generation === generations.current.registry;
    setRegistryError(null);
    try {
      const next = await getRegistrySnapshot();
      if (current()) {
        setEntries(next.entries);
        setCatalog(next.catalog);
        setCustomKeys(new Set(next.custom_keys));
      }
      if (mounted.current && sourceGeneration === generations.current.sources) setSources(next.sources);
      return next.entries;
    } catch (error) {
      if (current()) setRegistryError(formatErrorDetails(error));
      throw error;
    } finally {
      if (current()) setLoading(false);
    }
  }, []);

  const refreshSources = useCallback(async () => {
    const generation = ++generations.current.sources;
    const next = await listSources();
    if (mounted.current && generation === generations.current.sources) setSources(next);
    return next;
  }, []);

  const refreshAll = useCallback(async () => {
    await Promise.all([
      refreshRegistry().catch(console.error),
      rescan().catch(console.error),
    ]);
  }, [refreshRegistry, rescan]);

  useEffect(() => {
    if (!autoLoad) return;
    Promise.all([
      refreshRegistry().catch(console.error),
      refreshAgents().catch(console.error),
      rescan().catch(console.error),
    ]).catch(() => undefined);
  }, [autoLoad, refreshAgents, refreshRegistry, rescan]);

  const serverToAgents = useMemo(() => {
    const result = new Map<string, string[]>();
    for (const item of installed) {
      if (item.scope !== "global" || !item.enabled) continue;
      const rows = result.get(installedKey(item)) ?? [];
      rows.push(item.agent);
      result.set(installedKey(item), rows);
    }
    return result;
  }, [installed]);
  const agentsForServer = useCallback(
    (serverKey: string) => serverToAgents.get(serverKey) ?? [],
    [serverToAgents],
  );

  const afterSourceChange = useCallback(async () => {
    await refreshRegistry();
  }, [refreshRegistry]);

  const subscribe = useCallback(async (url: string, name?: string) => {
    const source = await subscribeSource(url, name);
    await afterSourceChange();
    return source;
  }, [afterSourceChange]);

  const pickLocalSource = useCallback(async () => {
    const source = await addLocalSourceDialog();
    if (source) await afterSourceChange();
    return source;
  }, [afterSourceChange]);

  const rescanDiscovered = useCallback(async () => {
    await rescan();
  }, [rescan]);

  const refreshOneSource = useCallback(async (id: string) => {
    await refreshSource(id);
    await afterSourceChange();
  }, [afterSourceChange]);

  const toggleSource = useCallback(async (id: string, enabled: boolean) => {
    await setSourceEnabled(id, enabled);
    await afterSourceChange();
  }, [afterSourceChange]);

  const deleteSource = useCallback(async (id: string) => {
    await removeSource(id);
    await afterSourceChange();
  }, [afterSourceChange]);

  const importPaste = useCallback(async (text: string) => {
    try {
      return await importPastedConfig(text);
    } finally {
      // Save completion must not wait for unrelated source/catalog reads.
      // Refresh on partial failure too, since earlier entries may have committed.
      void afterSourceChange().catch(console.error);
    }
  }, [afterSourceChange]);

  return {
    entries,
    catalog,
    agents,
    installed,
    loading,
    registryError,
    agentsForServer,
    customKeys,
    rescan,
    refreshAll,
    refreshRegistry,
    refreshAgents,
    sources,
    refreshSources,
    subscribe,
    pickLocalSource,
    rescanDiscovered,
    refreshOneSource,
    toggleSource,
    deleteSource,
    importPaste,
  };
}
