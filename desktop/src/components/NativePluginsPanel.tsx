import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { HostMcpObservation, NativePluginObservation, PluginComponent, PluginPresence } from "../lib/types";
import { SearchBar } from "./ui";
import { ResourceIcon } from "./resourcePresentation";
import { DialogShell } from "./DialogShell";
import { ChevronDownIcon, PackageIcon } from "./icons";
import "./NativePluginsPanel.css";

export function HostMcpCard({ item }: { item: HostMcpObservation }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return <li data-domain="mcp" data-host-managed="true" className="arca-host-mcp-card">
    <ResourceRow name={item.name} icon={<ResourceIcon domain="mcp" className="w-6 h-6" />}
      onOpen={() => setOpen(true)} open={open}>
      <Presence values={[item.presence]} />
      {item.origin === "plugin" && <span className="arca-native-row-context" title={item.provider}>{item.provider}</span>}
    </ResourceRow>
    {open && <DialogShell kind="review" size="wide" className="arca-native-dialog" title={item.name}
      leading={<ResourceIcon domain="mcp" className="w-6 h-6" />} subtitle={t("nativePlugins.mcpDetails")}
      onClose={() => setOpen(false)}
      footerEnd={<button type="button" className="btn-ghost" onClick={() => setOpen(false)}>{t("common.close")}</button>}>
      <p className="arca-native-notice">{t(item.presence === "cache_only" ? "nativePlugins.cacheRuntime" : "nativePlugins.hostRuntime")}</p>
      <dl className="arca-native-facts">
        <Fact label={t("nativePlugins.serverId")}><code>{item.server_id}</code></Fact>
        <Fact label={t("nativePlugins.provider")}>{item.provider}</Fact>
        <Fact label={t("nativePlugins.versions")}>{item.versions.join(", ") || "—"}</Fact>
        <Fact label={t("nativePlugins.sourceState")}><Presence values={[item.presence]} /></Fact>
      </dl>
      <SourcePaths paths={item.source_paths} />
    </DialogShell>}
  </li>;
}

function Presence({ values }: { values: PluginPresence[] }) {
  const { t } = useTranslation();
  const label = [...new Set(values)].map(value => t(`nativePlugins.presence_${value}`)).join(" · ");
  return <span className="arca-native-presence" title={label}>{label}</span>;
}

function ResourceRow({ name, icon, children, onOpen, open }: {
  name: string; icon: ReactNode; children: ReactNode; onOpen(): void; open: boolean;
}) {
  const { t } = useTranslation();
  return <button type="button" className="arca-native-row" aria-label={t("nativePlugins.viewDetails", { name })}
    aria-haspopup="dialog" aria-expanded={open} onClick={event => {
      // WebKit/AX activation can leave focus behind; capture this row as the modal opener.
      event.currentTarget.focus({ preventScroll: true });
      onOpen();
    }}>
    <span className="arca-native-row-icon" aria-hidden="true">{icon}</span>
    <span className="arca-native-row-copy">
      <strong title={name}>{name}</strong>
      <span className="arca-native-row-meta">{children}</span>
    </span>
    <span className="arca-native-row-affordance" title={t("nativePlugins.readOnly")} aria-hidden="true">
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3"><rect x="3.5" y="7" width="9" height="6.5" rx="1.5" /><path d="M5.5 7V4.5a2.5 2.5 0 0 1 5 0V7" /></svg>
      <ChevronDownIcon className="arca-native-row-chevron" />
    </span>
  </button>;
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return <div><dt>{label}</dt><dd>{children}</dd></div>;
}

function SourcePaths({ paths }: { paths: string[] }) {
  const { t } = useTranslation();
  return <section className="arca-native-detail-section">
    <h3>{t("nativePlugins.sourcesHeading")}</h3>
    <ul className="arca-native-source-list">{[...new Set(paths)].map(path => <li key={path}><code>{path}</code></li>)}</ul>
  </section>;
}

function Components({ components }: { components: PluginComponent[] }) {
  const { t } = useTranslation();
  const sources = new Map<string, PluginComponent[]>();
  for (const component of components) sources.set(component.source_path, [...sources.get(component.source_path) ?? [], component]);
  return <section className="arca-native-detail-section">
    <h3>{t("nativePlugins.components")}</h3>
    {components.length === 0 ? <p className="arca-native-muted">{t("nativePlugins.noComponents")}</p> :
      [...sources].map(([path, entries]) => <div className="arca-native-component-group" key={path}>
        <code className="arca-native-component-source">{path}</code>
        <ul>{entries.map(component => <li key={`${component.kind}:${component.name}:${component.native_id ?? ""}`}>
          <span className="arca-native-component-kind">{t(`nativePlugins.kind_${component.kind}`)}</span>
          <span className="arca-native-component-name">{component.name}</span>
          {component.native_id && component.native_id !== component.name && <code>{component.native_id}</code>}
        </li>)}</ul>
      </div>)}
  </section>;
}

export function NativePluginsPanel({ plugins }: { plugins: NativePluginObservation[] }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const groups = useMemo(() => {
    const groups = new Map<string, NativePluginObservation[]>();
    for (const plugin of plugins) {
      const key = `${plugin.agent_id}:${plugin.plugin_id}`;
      groups.set(key, [...groups.get(key) ?? [], plugin]);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [plugins]);
  const needle = query.trim().toLocaleLowerCase();
  const filtered = groups.filter(([, versions]) => !needle || versions.some(plugin =>
    [plugin.name, plugin.plugin_id, plugin.version, plugin.revision, ...plugin.components.flatMap(c => [c.kind, c.name, c.native_id ?? ""])].join(" ").toLocaleLowerCase().includes(needle)));
  const selected = groups.find(([key]) => key === selectedKey)?.[1];
  const names = new Map<string, number>();
  for (const [, versions] of groups) names.set(versions[0].name, (names.get(versions[0].name) ?? 0) + 1);
  return <section className="arca-agent-section arca-agent-resource-content arca-native-plugins" aria-label="Plugins">
    <p className="arca-native-intro">{t("nativePlugins.description")}</p>
    <div className="arca-consumption-search"><SearchBar value={query} onChange={setQuery} placeholder={t("nativePlugins.search")} />
      <span role="status" aria-live="polite">{t("nativePlugins.pluginCount", { count: filtered.length })}</span>
    </div>
    {filtered.length === 0 ? <p className="arca-consumption-empty">{t(query ? "nativePlugins.noMatch" : "nativePlugins.empty")}</p> :
      <ul className="arca-native-plugin-grid">{filtered.map(([key, versions]) => {
        const kinds = [...new Set(versions.flatMap(version => version.components.map(component => component.kind)))];
        const summary = kinds.map(kind => t(`nativePlugins.kind_${kind}`)).join(" · ");
        return <li key={key} className="arca-native-plugin-card">
          <ResourceRow name={versions[0].name} icon={<PackageIcon className="w-5 h-5" />} open={selectedKey === key}
            onOpen={() => setSelectedKey(key)}>
            <Presence values={versions.map(version => version.presence)} />
            {(names.get(versions[0].name) ?? 0) > 1 ?
              <code className="arca-native-row-context" title={versions[0].plugin_id}>{versions[0].plugin_id}</code> :
              summary && <span className="arca-native-row-context" title={summary}>{summary}</span>}
          </ResourceRow>
        </li>;
      })}</ul>}
    {selected && <DialogShell kind="review" size="wide" className="arca-native-dialog" title={selected[0].name}
      leading={<PackageIcon className="w-5 h-5" />} subtitle={t("nativePlugins.readOnly")}
      onClose={() => setSelectedKey(null)}
      footerEnd={<button type="button" className="btn-ghost" onClick={() => setSelectedKey(null)}>{t("common.close")}</button>}>
      <p className="arca-native-notice">{t("nativePlugins.hostRuntime")}</p>
      {selected.some(version => version.presence === "cache_only") && <p className="arca-native-notice">{t("nativePlugins.cacheRuntime")}</p>}
      <dl className="arca-native-facts">
        <Fact label={t("nativePlugins.pluginId")}><code>{selected[0].plugin_id}</code></Fact>
        <Fact label={t("nativePlugins.snapshots")}>{selected.length}</Fact>
      </dl>
      {selected.map(version => <section className="arca-native-version" key={`${version.revision}:${version.presence}:${version.version}`}>
        <header><strong>{version.version}</strong><Presence values={[version.presence]} /></header>
        <p className="arca-native-muted">{version.enabled === null ? t("nativePlugins.enablementUnknown") : t(version.enabled ? "nativePlugins.enabled" : "nativePlugins.disabled")}</p>
        <dl className="arca-native-facts"><Fact label={t("nativePlugins.revision")}><code>{version.revision}</code></Fact></dl>
        <SourcePaths paths={version.source_paths} />
        <Components components={version.components} />
      </section>)}
    </DialogShell>}
  </section>;
}
