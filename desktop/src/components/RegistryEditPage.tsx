import { ResourceIcon } from "./resourcePresentation";
import { useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { InstallState } from "../hooks/useInstallState";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import type { AssetCommandError, McpIconPreference, RegistryEntry } from "../lib/types";
import { keyOf, type Transport } from "../lib/mcp";
import { requiresAgentReview } from "../lib/agentOperation";
import { EnvEditor } from "./EnvEditor";
import { AssetOperationReviewDialog } from "./AssetOperationReviewDialog";
import { DialogShell } from "./DialogShell";
import { FormSelect } from "./FormSelect";
import { ResourceInspector } from "./ResourceWorkspace";
import { CloudIcon, LayersIcon, NetworkIcon, SaveIcon, TerminalIcon, FolderIcon } from "./icons";
import { ToastNotice, useToast } from "./Toast";
import { McpAvatar } from "./McpIcon";
import { McpPasteInput } from "./McpPasteInput";
import { McpTagInput, appendMcpTag } from "./McpTagEditor";
import { SubscriptionFields } from "./SubscribeDialog";
import { useMcpTags } from "../hooks/useMcpTags";
import { formatError } from "../lib/format";
import { previewPastedConfig, type PastedMcpSummary } from "../lib/api";
import { normalizeMcpTags } from "../lib/mcp";

interface RegistryEditPageProps {
  state: InstallState;
  consumptionState: ConsumptionState;
  /** Entry name to edit; null = create a new entry. */
  name: string | null;
  /** Exact catalog entry when editing from an Inspector, including shadowed copies. */
  entry?: RegistryEntry;
  iconPreference?: McpIconPreference;
  /** Resolved tags for the edited entry when available from workspace state. */
  resolvedTags?: string[];
  /** Existing tag suggestions across all MCP entries. */
  availableTags?: string[];
  /** Optional callback to sync tag customizations for non-manual or existing assets. */
  onTagsSync?: (assetKey: string, nextTags: string[]) => Promise<unknown>;
  /** Which transport variant to edit (a name can have both stdio + http). */
  transport?: Transport;
  onBack: () => void;
  /** Existing MCPs can edit inside the already-open resource Inspector. */
  presentation?: "dialog" | "inspector";
}

const HTTP_TYPES = ["http", "sse", "streamable-http"];

export function RegistryEditPage({
  state,
  consumptionState,
  name,
  entry,
  iconPreference,
  resolvedTags,
  availableTags: availableTagsProp,
  onTagsSync,
  transport: editTransport,
  onBack,
  presentation = "dialog",
}: RegistryEditPageProps) {
  const { entries, customKeys } = state;
  const toast = useToast();
  const { t } = useTranslation();
  const connectionId = useId();

  const isNew = name === null;
  const mcpTagsState = useMcpTags(isNew && availableTagsProp === undefined);
  const existing = useMemo(
    () =>
      entry ??
      (name
        ? entries.find(
            (e) => e.name === name && (e.config.http ? "http" : "stdio") === (editTransport ?? "stdio")
          ) ?? null
        : null),
    [entries, entry, name, editTransport]
  );
  const isCustom = existing
    ? (existing.origin?.kind === "manual" || existing.origin?.kind === "discovered") &&
      customKeys.has(keyOf(existing))
    : false;
  const createsLocalOverride =
    existing?.origin?.kind === "remote" || existing?.origin?.kind === "local";

  const [serverName, setServerName] = useState(existing?.name ?? "");
  const [description, setDescription] = useState(existing?.description ?? "");
  const [transport, setTransport] = useState<Transport>(existing?.config.http ? "http" : "stdio");

  const [command, setCommand] = useState(existing?.config.stdio?.command ?? "");
  const [argsText, setArgsText] = useState((existing?.config.stdio?.args ?? []).join("\n"));
  const [argsEdited, setArgsEdited] = useState(false);
  const [cwd, setCwd] = useState(existing?.config.stdio?.cwd ?? "");
  const [env, setEnv] = useState<Record<string, string>>(existing?.config.stdio?.env ?? {});
  const [envValid, setEnvValid] = useState(true);

  const [httpType, setHttpType] = useState<string>(existing?.config.http?.type ?? "http");
  const [url, setUrl] = useState(existing?.config.http?.url ?? "");
  const [headers, setHeaders] = useState<Record<string, string>>(existing?.config.http?.headers ?? {});
  const [headersValid, setHeadersValid] = useState(true);
  const [repo, setRepo] = useState(existing?.repo ?? "");
  const [tags, setTags] = useState<string[]>(() => normalizeMcpTags(resolvedTags ?? existing?.tags ?? []));
  const [tagsEdited, setTagsEdited] = useState(false);
  const [pendingTagDraft, setPendingTagDraft] = useState("");
  const [subUrl, setSubUrl] = useState("");
  const [subName, setSubName] = useState("");

  const [saving, setSaving] = useState(false);
  const [addMode, setAddMode] = useState<"paste" | "manual" | "subscribe" | "import">("paste");
  const [pasteText, setPasteText] = useState("");
  const isPaste = isNew && addMode === "paste";
  const isManual = !isNew || addMode === "manual";
  const isSubscribe = isNew && addMode === "subscribe";
  const isImport = isNew && addMode === "import";
  const [pasteResult, setPasteResult] = useState<{ text: string; entries: PastedMcpSummary[]; error: string } | null>(null);
  const pasteReady = isPaste && pasteResult?.text === pasteText;
  const pasteEntries = pasteReady ? pasteResult.entries : null;
  const pasteError = pasteReady ? pasteResult.error : "";
  const pasteValid = !!pasteEntries?.length && !pasteError;
  const existingKeys = useMemo(() => new Set(entries.map(keyOf)), [entries]);

  const suggestedTags = useMemo(() => {
    if (availableTagsProp) return availableTagsProp;
    const collected: string[] = [];
    for (const item of state.catalog ?? []) collected.push(...item.entry.tags);
    for (const entryItem of entries) collected.push(...entryItem.tags);
    for (const list of Object.values(mcpTagsState.tagsByKey)) collected.push(...list);
    return normalizeMcpTags(collected).sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  }, [availableTagsProp, state.catalog, entries, mcpTagsState.tagsByKey]);

  useEffect(() => {
    if (!isPaste || !pasteText.trim()) return;
    let active = true;
    const timer = setTimeout(() => {
      void previewPastedConfig(pasteText).then(
        (entries) => { if (active) setPasteResult({ text: pasteText, entries, error: "" }); },
        (error) => { if (active) setPasteResult({ text: pasteText, entries: [], error: formatError(error) }); },
      );
    }, 300);
    return () => { active = false; clearTimeout(timer); };
  }, [isPaste, pasteText]);

  const handlePaste = async () => {
    if (!pasteValid || saving) return;
    setSaving(true);
    try {
      const names = await state.importPaste(pasteText);
      toast.show({ kind: "success", msg: `已添加 ${names.length} 个 MCP` });
      onBack();
    } catch (error) {
      const failure = error as AssetCommandError | null;
      if (failure?.code === "target_convergence_failed" && failure.details?.central_saved === true) {
        const remaining = Number(failure.details.remaining_count ?? 0);
        toast.show({ kind: "error", msg: `已保存 ${failure.details.saved_count} 个 MCP，但后续校验或同步未完成。${remaining > 0 ? `另有 ${remaining} 个尚未添加。` : "可在 MCP 列表中查看。"}`, details: failure });
        if (remaining === 0) onBack();
      } else {
        toast.show({ kind: "error", msg: "添加 MCP 失败", details: error });
      }
    } finally {
      setSaving(false);
    }
  };

  const compact = (o: Record<string, string>) => (Object.keys(o).length > 0 ? o : undefined);

  const resolveEditedTags = () => {
    const trimmed = pendingTagDraft.trim();
    if (!trimmed) return normalizeMcpTags(tags);
    const appended = appendMcpTag(tags, trimmed);
    return appended.error ? normalizeMcpTags(tags) : appended.next;
  };

  const buildEntry = (resolvedTagList = resolveEditedTags()): RegistryEntry => {
    const hasTagEdit = tagsEdited || pendingTagDraft.trim().length > 0;
    const entryTags = isNew
      ? resolvedTagList
      : !hasTagEdit || createsLocalOverride
        ? (existing?.tags ?? [])
        : resolvedTagList;
    return {
      name: serverName.trim(),
      description: description.trim(),
      tags: entryTags,
      config:
        transport === "stdio"
          ? {
              stdio: {
                ...existing?.config.stdio,
                command: command.trim(),
                args: argsEdited || isNew ? argsText.split(/\r?\n/).filter((arg) => arg.length > 0) : existing?.config.stdio?.args,
                env: compact(env),
                cwd: cwd.trim() || undefined,
              },
            }
          : { http: { type: httpType.trim() || "http", url: url.trim(), headers: compact(headers) } },
      // Central edits always write the user's highest-priority manual layer.
      // Normalizing the draft keeps the review payload truthful for source-owned
      // entries whose subscription cache remains untouched.
      origin: { kind: "manual", source: "manual" },
      repo: repo.trim() || undefined,
    };
  };

  let urlInvalid = false;
  if (url.trim()) {
    try {
      const endpoint = new URL(url.trim());
      urlInvalid = HTTP_TYPES.includes(httpType) && !["http:", "https:"].includes(endpoint.protocol);
    } catch { urlInvalid = true; }
  }
  const duplicateName = serverName.trim() && existingKeys.has(`${serverName.trim()}::${transport}`)
    && `${serverName.trim()}::${transport}` !== (existing ? keyOf(existing) : undefined);
  const valid =
    serverName.trim().length > 0 &&
    !duplicateName &&
    (transport === "stdio" ? command.trim().length > 0 && envValid : url.trim().length > 0 && !!httpType.trim() && !urlInvalid && headersValid);
  const subscribeValid = subUrl.trim().length > 0;

  const syncTagsAfterCommit = async (draftKey: string, nextTags: string[]) => {
    if (!onTagsSync) return;
    if (!tagsEdited && !pendingTagDraft.trim()) return;
    try {
      await onTagsSync(draftKey, nextTags);
    } catch (err) {
      toast.show({ kind: "warning", msg: "MCP 已保存，标签同步失败", details: err });
    }
  };

  const handleSave = async () => {
    if (!valid || saving) return;
    const finalTags = resolveEditedTags();
    const draft = buildEntry(finalTags);
    const draftKey = keyOf(draft);
    const existingKey = existing ? keyOf(existing) : undefined;
    // The original composite key is the edit target. A rename may keep the
    // display name used by another transport, but may never replace an
    // existing name + transport identity.
    if (draftKey !== existingKey && entries.some((e) => keyOf(e) === draftKey)) {
      toast.show({ kind: "error", msg: `已存在同名同传输方式的 MCP: ${draft.name} (${transport})` });
      return;
    }
    setSaving(true);
    try {
      const plan = await consumptionState.planUpdate({
        domain: "mcp",
        existing_key: existing ? keyOf(existing) : undefined,
        entry: draft,
      }, { reviewOnlyWhenNeeded: true });
      if (!requiresAgentReview(plan)) {
        await consumptionState.commit({ background: true });
        await syncTagsAfterCommit(draftKey, finalTags);
        onBack();
        void state.refreshRegistry().then(
          () => toast.show({ kind: "success", msg: "MCP 资产已保存。" }),
          (error) => toast.show({ kind: "warning", msg: "MCP 已保存，列表刷新失败", details: error }),
        );
      }
    } catch (err) {
      toast.show({ kind: "error", msg: `保存 MCP「${draft.name}」失败`, details: err });
    } finally {
      setSaving(false);
    }
  };

  const handleSubscribe = async () => {
    if (!subscribeValid || saving) return;
    setSaving(true);
    try {
      const v = await state.subscribe(subUrl.trim(), subName.trim() || undefined);
      toast.show({ kind: "success", msg: t("desktop.subscribeDialog.subscribedToItems", { value0: v.name, value1: v.server_count }) });
      onBack();
    } catch (err) {
      toast.show({ kind: "error", msg: t("desktop.subscribeDialog.subscriptionFailed") + formatError(err) });
    } finally {
      setSaving(false);
    }
  };

  const handleImportLocal = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const v = await state.pickLocalSource();
      if (v) {
        toast.show({ kind: "success", msg: t("desktop.sourcesSidebar.importedItems", { value0: v.name, value1: v.server_count }) });
        onBack();
      }
    } catch (err) {
      toast.show({ kind: "error", msg: t("desktop.sourcesSidebar.importFailed") + formatError(err) });
    } finally {
      setSaving(false);
    }
  };

  const handleRevert = async () => {
    if (!name || saving || !existing) return;
    setSaving(true);
    try {
      const sourceId = existing.origin?.source ?? existing.origin?.kind;
      await consumptionState.planDelete(
        { domain: "mcp", key: keyOf(existing) },
        sourceId,
      );
    } catch (err) {
      toast.show({ kind: "error", msg: `恢复 MCP「${name}」失败`, details: err });
    } finally {
      setSaving(false);
    }
  };

  const form = (
    <fieldset className="arca-mcp-form arca-mcp-editor-form" disabled={saving || consumptionState.committing}>
      {createsLocalOverride && (
        <div className="arca-mcp-override-note" role="note">
          <LayersIcon className="w-4 h-4 flex-shrink-0" />
          <div>
            <strong>{t("mcpEditor.override")}</strong>
            <p>{t("mcpEditor.overrideHint")}</p>
          </div>
        </div>
      )}
      <div className="arca-mcp-connection-picker" role="radiogroup" aria-label={t("mcpEditor.connection")}>
        {(["stdio", "http"] as const).map((value) => (
          <label key={value} data-selected={transport === value || undefined} data-disabled={!isNew || undefined}>
            <input type="radio" name={connectionId} value={value} checked={transport === value}
              disabled={!isNew} onChange={() => setTransport(value)} />
            {value === "stdio" ? <TerminalIcon /> : <NetworkIcon />}
            <span>
              <strong>{t(value === "stdio" ? "mcpEditor.local" : "mcpEditor.remote")}</strong>
              <small>{t(value === "stdio" ? "mcpEditor.localHint" : "mcpEditor.remoteHint")}</small>
            </span>
            <i aria-hidden="true" />
          </label>
        ))}
      </div>
      <div className="arca-mcp-field-grid">
        <label className="arca-mcp-field">
          <span>{t("mcpEditor.name")}</span>
          <input aria-label={t("mcpEditor.name")} className="arca-dialog-input" value={serverName}
            readOnly={!!createsLocalOverride} onChange={(event) => setServerName(event.target.value)}
            placeholder={t("mcpEditor.namePlaceholder")} aria-invalid={!!duplicateName || undefined} spellCheck={false} />
          {duplicateName && <ToastNotice kind="warning" delay={500} message={t("mcpEditor.duplicateKey")} />}
        </label>
        <label className="arca-mcp-field">
          <span>{t("mcpEditor.description")} <small>{t("mcpEditor.optional")}</small></span>
          <input className="arca-dialog-input" value={description} onChange={(event) => setDescription(event.target.value)}
            placeholder={t("mcpEditor.descriptionPlaceholder")} />
        </label>
      </div>
      <div hidden={transport !== "stdio"}>
        <div className="arca-mcp-connection-fields">
          <div className="arca-mcp-field-grid">
            <label className="arca-mcp-field">
              <span>{t("mcpEditor.command")}</span>
              <input className="arca-dialog-input arca-dialog-input-mono" value={command}
                onChange={(event) => setCommand(event.target.value)} placeholder="npx / uvx / node" spellCheck={false} autoCapitalize="off" autoCorrect="off" />
            </label>
            <label className="arca-mcp-field">
              <span>{t("mcpEditor.directory")} <small>{t("mcpEditor.optional")}</small></span>
              <input className="arca-dialog-input arca-dialog-input-mono" value={cwd}
                onChange={(event) => setCwd(event.target.value)} placeholder={t("mcpEditor.directoryPlaceholder")}
                spellCheck={false} autoCapitalize="off" autoCorrect="off" />
            </label>
          </div>
          <label className="arca-mcp-field">
            <span>{t("mcpEditor.args")} <small>{t("mcpEditor.argsHint")}</small></span>
            <textarea aria-label={t("mcpEditor.args")} className="arca-dialog-input arca-mcp-args" rows={3}
              value={argsText} onChange={(event) => { setArgsText(event.target.value); setArgsEdited(true); }}
              placeholder={"-y\n@playwright/mcp@latest"} spellCheck={false} autoCapitalize="off" autoCorrect="off" />
          </label>
          <section className="arca-mcp-field arca-mcp-field-section" aria-label={t("mcpEditor.environment")}>
            <span>{t("mcpEditor.environment")} <small>{t("mcpEditor.optional")}</small></span>
            <EnvEditor value={env} onChange={setEnv} onValidityChange={setEnvValid} disabled={saving} />
          </section>
        </div>
      </div>
      <div hidden={transport !== "http"}>
        <div className="arca-mcp-connection-fields">
          <div className="arca-mcp-remote-fields">
            <label className="arca-mcp-field">
              <span>{t("mcpEditor.url")}</span>
              <input className="arca-dialog-input arca-dialog-input-mono" value={url}
                onChange={(event) => setUrl(event.target.value)} placeholder="https://example.com/mcp"
                aria-invalid={urlInvalid || undefined} spellCheck={false} autoCapitalize="off" autoCorrect="off" />
              {urlInvalid && <ToastNotice kind="warning" delay={500} message={t("mcpEditor.invalidUrl")} />}
            </label>
            <div className="arca-mcp-field">
              <span>{t("mcpEditor.protocol")}</span>
              <FormSelect ariaLabel={t("mcpEditor.protocol")} value={HTTP_TYPES.includes(httpType) ? httpType : "custom"}
                options={[
                  { value: "http", label: t("mcpEditor.streamable") },
                  { value: "sse", label: t("mcpEditor.sse") },
                  { value: "streamable-http", label: t("mcpEditor.explicitStreamable") },
                  { value: "custom", label: t("mcpEditor.custom") },
                ]} onChange={(value) => setHttpType(value === "custom" ? "" : value)} disabled={saving} />
            </div>
          </div>
          {!HTTP_TYPES.includes(httpType) && <label className="arca-mcp-field">
            <span>{t("mcpEditor.customType")}</span>
            <input className="arca-dialog-input arca-dialog-input-mono" value={httpType}
              onChange={(event) => setHttpType(event.target.value)} placeholder="type" spellCheck={false} />
          </label>}
          <section className="arca-mcp-field arca-mcp-field-section" aria-label={t("mcpEditor.headers")}>
            <span>{t("mcpEditor.headers")} <small>{t("mcpEditor.optional")}</small></span>
            <EnvEditor kind="headers" value={headers} onChange={setHeaders} onValidityChange={setHeadersValid} disabled={saving} />
          </section>
        </div>
      </div>
      <label className="arca-mcp-field">
        <span>{t("mcpEditor.repo")} <small>{t("mcpEditor.optional")}</small></span>
        <input className="arca-dialog-input arca-dialog-input-mono" value={repo}
          onChange={(event) => setRepo(event.target.value)} placeholder="https://github.com/owner/repo"
          spellCheck={false} autoCapitalize="off" autoCorrect="off" />
      </label>
      <section className="arca-mcp-field arca-mcp-field-section" aria-label={t("mcpEditor.tags")}>
        <span>{t("mcpEditor.tags")} <small>{t("mcpEditor.optional")}</small></span>
        <McpTagInput
          tags={tags}
          availableTags={suggestedTags}
          onChange={(next) => {
            setTags(next);
            setTagsEdited(true);
          }}
          inputValue={pendingTagDraft}
          onInputValueChange={setPendingTagDraft}
          disabled={saving}
        />
      </section>
    </fieldset>
  );

  const footerStart = !isNew && isCustom ? (
    <button onClick={handleRevert} disabled={saving} className="btn-danger" title={t("mcpEditor.restoreHint")}>
      {t("mcpEditor.restore")}
    </button>
  ) : null;
  const footerEnd = (
    <>
      <button onClick={onBack} disabled={saving} className="btn-secondary">{t("common.cancel")}</button>
      {isSubscribe ? (
        <button onClick={() => void handleSubscribe()} disabled={!subscribeValid || saving} className="btn-primary">
          <CloudIcon className="w-4 h-4" />
          {saving ? t("desktop.subscribeDialog.subscribing") : t("desktop.subscribeDialog.subscribe")}
        </button>
      ) : isImport ? (
        <button onClick={() => void handleImportLocal()} disabled={saving} className="btn-primary">
          <FolderIcon className="w-4 h-4" />
          {saving ? t("mcpEditor.importing") : t("mcpEditor.importChooseFile")}
        </button>
      ) : (
        <button onClick={isPaste ? handlePaste : handleSave} disabled={(isPaste ? !pasteValid : !valid) || saving} className="btn-primary">
          <SaveIcon className="w-4 h-4" />
          {saving ? t(isPaste ? "mcpEditor.importing" : isNew ? "mcpEditor.adding" : "common.saving")
            : isPaste ? pasteEntries?.length ? t("mcpEditor.importCount", { count: pasteEntries.length }) : t("mcpEditor.import")
            : t(isNew ? "mcpEditor.add" : createsLocalOverride ? "mcpEditor.override" : "common.save")}
        </button>
      )}
    </>
  );

  const review = consumptionState.plan ? (
    <AssetOperationReviewDialog
      plan={consumptionState.plan}
      busy={consumptionState.committing}
      error={consumptionState.error}
      cancelLabel="返回编辑"
      onCancel={consumptionState.cancel}
      onCommit={async () => {
        const kind = consumptionState.plan?.kind;
        const plan = consumptionState.plan;
        await consumptionState.commit();
        if (kind !== "delete-asset" && plan?.domain_plan.domain === "mcp") {
          const targetKey = `${serverName.trim()}::${transport}`;
          await syncTagsAfterCommit(targetKey, resolveEditedTags());
        }
        onBack();
        void state.refreshRegistry().then(
          () => toast.show({ kind: "success", msg: kind === "delete-asset" ? "MCP 资产已删除。" : "MCP 资产已保存。" }),
          (error) => toast.show({ kind: "warning", msg: "MCP 操作已完成，列表刷新失败", details: error }),
        );
      }}
    />
  ) : null;
  if (review) return review;

  if (presentation === "inspector" && existing) {
    return (
      <ResourceInspector
        title={existing.name}
        avatar={<McpAvatar assetKey={keyOf(existing)} entry={existing} preference={iconPreference} size={40} />}
        subtitle={`编辑 · ${transport === "stdio" ? "stdio" : "HTTP"} · 全局配置`}
        onClose={onBack}
        footerStart={footerStart}
        footer={footerEnd}
      >
        {form}
      </ResourceInspector>
    );
  }

  const newFooterHint = isSubscribe
    ? t("mcpEditor.subscribeHint")
    : isImport
      ? t("mcpEditor.importActionHint")
      : t("mcpEditor.pasteScope");

  return (
    <DialogShell
      leading={<span className="arca-dialog-shell-glyph"><ResourceIcon domain="mcp" /></span>}
      className="arca-dialog-mcp-editor"
      kind="editor"
      size="wide"
      title={t(isNew ? "mcpEditor.addTitle" : "mcpEditor.editTitle")}
      subtitle={isNew ? undefined : `${transport === "stdio" ? "stdio" : "HTTP"} · ${t("mcpEditor.global")}`}
      busy={saving || consumptionState.committing}
      onClose={onBack}
      footerStart={footerStart ?? (isNew ? newFooterHint : null)}
      footerEnd={footerEnd}
    >
      {isNew && <div className="arca-seg arca-mcp-add-modes" role="group" aria-label={t("mcpEditor.addMode")}>
        <button type="button" className="arca-seg-item" aria-pressed={addMode === "paste"}
          disabled={saving} onClick={() => setAddMode("paste")}>{t("mcpEditor.paste")}</button>
        <button type="button" className="arca-seg-item" aria-pressed={addMode === "manual"}
          disabled={saving} onClick={() => setAddMode("manual")}>{t("mcpEditor.manual")}</button>
        <button type="button" className="arca-seg-item" aria-pressed={addMode === "subscribe"}
          disabled={saving} onClick={() => setAddMode("subscribe")}>{t("mcpEditor.subscribe")}</button>
        <button type="button" className="arca-seg-item" aria-pressed={addMode === "import"}
          disabled={saving} onClick={() => setAddMode("import")}>{t("mcpEditor.importConfig")}</button>
      </div>}
      {isNew && <div hidden={!isPaste}>
        <McpPasteInput value={pasteText} onChange={setPasteText} disabled={saving}
          preview={pasteEntries} error={pasteError} pending={!pasteReady} existingKeys={existingKeys} />
      </div>}
      {isNew && <div hidden={!isSubscribe}>
        <SubscriptionFields
          url={subUrl}
          name={subName}
          disabled={saving}
          urlLabel={t("desktop.subscribeDialog.configurationURL")}
          nameLabel={t("desktop.subscribeDialog.name")}
          namePlaceholder={t("desktop.subscribeDialog.leaveBlankToUseTheURLHostname")}
          presetLabel={t("desktop.subscribeDialog.useArcaPicks")}
          onUrlChange={setSubUrl}
          onNameChange={setSubName}
          onSubmit={() => void handleSubscribe()}
        />
      </div>}
      {isNew && <div hidden={!isImport}>
        <div className="arca-mcp-import-panel">
          <div className="arca-mcp-import-icon" aria-hidden="true">
            <FolderIcon className="w-6 h-6" />
          </div>
          <div className="arca-mcp-import-copy">
            <strong>{t("mcpEditor.importTitle")}</strong>
            <p>{t("mcpEditor.importDescription")}</p>
            <div className="arca-mcp-import-formats" aria-hidden="true">
              <span>JSON</span>
              <span>TOML</span>
              <span>YAML</span>
            </div>
          </div>
        </div>
      </div>}
      <div hidden={!isManual}>{form}</div>
    </DialogShell>
  );
}
