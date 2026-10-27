import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { HostMcpObservation, NativePluginObservation, PluginPresence } from "../lib/types";
import { SearchBar } from "./ui";
import { ResourceIcon } from "./resourcePresentation";
import "./NativePluginsPanel.css";

export function HostMcpCard({ item }: { item: HostMcpObservation }) {
  const { t } = useTranslation();
  return <li data-domain="mcp" data-host-managed="true" className="arca-host-mcp-card">
    <span className="arca-consumption-icon"><ResourceIcon domain="mcp" className="w-7 h-7" /></span>
    <span className="arca-consumption-copy">
      <span className="arca-consumption-title"><strong>{item.name}</strong></span>
      <span className="arca-native-label">{t(`nativePlugins.presence_${item.presence}`)} · {t("nativePlugins.readOnly")}</span>
      <code className="arca-native-id">{item.server_id}</code>
      <small>{item.provider} · {item.versions.join(", ")}</small>
      <small>{t(item.presence === "cache_only" ? "nativePlugins.cacheRuntime" : "nativePlugins.hostRuntime")}</small>
      <details className="arca-native-sources"><summary>{t("nativePlugins.sources", { count: item.source_paths.length })}</summary>
        {item.source_paths.map(path => <code key={path}>{path}</code>)}
      </details>
    </span>
  </li>;
}

function Presence({ value }: { value: PluginPresence }) {
  const { t } = useTranslation();
  return <span className="arca-native-label">{t(`nativePlugins.presence_${value}`)} · {t("nativePlugins.readOnly")}</span>;
}

export function NativePluginsPanel({ plugins }: { plugins: NativePluginObservation[] }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const groups = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    const groups = new Map<string, NativePluginObservation[]>();
    for (const plugin of plugins) {
      const text = [plugin.name, plugin.plugin_id, plugin.version, plugin.revision, ...plugin.components.flatMap(c => [c.kind, c.name, c.native_id ?? ""])].join(" ").toLocaleLowerCase();
      if (needle && !text.includes(needle)) continue;
      const key = `${plugin.agent_id}:${plugin.plugin_id}`;
      groups.set(key, [...groups.get(key) ?? [], plugin]);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [plugins, query]);
  return <section className="arca-agent-section arca-agent-resource-content arca-native-plugins" aria-label="Plugins">
    <p className="arca-native-intro">{t("nativePlugins.description")}</p>
    <div className="arca-consumption-search"><SearchBar value={query} onChange={setQuery} placeholder={t("nativePlugins.search")} />
      <span role="status" aria-live="polite">{t("nativePlugins.pluginCount", { count: groups.length })}</span>
    </div>
    {groups.length === 0 ? <p className="arca-consumption-empty">{t(query ? "nativePlugins.noMatch" : "nativePlugins.empty")}</p> :
      <ul className="arca-native-plugin-grid">{groups.map(([key, versions]) => <li key={key} className="arca-native-plugin-card">
        <h4>{versions[0].name}</h4>
        <code className="arca-native-id">{versions[0].plugin_id}</code>
        <div className="arca-native-badges">{[...new Set(versions.map(v => v.presence))].map(presence => <Presence key={presence} value={presence} />)}</div>
        <p>{[...new Set(versions.map(v => v.version))].join(", ")}</p>
        <p className="arca-native-muted">{t("nativePlugins.snapshotCount", { count: versions.length })}</p>
        {versions.some(v => v.presence === "cache_only") && <p className="arca-native-muted">{t("nativePlugins.cacheRuntime")}</p>}
        <details><summary>{t("nativePlugins.componentsAndSources")}</summary>
          {versions.map(version => <div className="arca-native-version" key={`${version.revision}:${version.presence}:${version.version}`}>
            <p><Presence value={version.presence} /></p>
            <strong>{version.version}</strong> <code title={version.revision}>{version.revision}</code>
            <p className="arca-native-muted">{version.enabled === null ? t("nativePlugins.enablementUnknown") : t(version.enabled ? "nativePlugins.enabled" : "nativePlugins.disabled")}</p>
            {version.source_paths.map(path => <code className="arca-native-path" key={path}>{path}</code>)}
            <ul className="arca-native-components">{version.components.map(component => <li key={`${component.kind}:${component.source_path}:${component.name}`}>
              <span className="arca-native-label">{t(`nativePlugins.kind_${component.kind}`)}</span> <span>{component.name}</span>
              {component.native_id && <code className="arca-native-path">{component.native_id}</code>}
              <code className="arca-native-path">{component.source_path}</code>
            </li>)}</ul>
          </div>)}
        </details>
      </li>)}</ul>}
  </section>;
}
