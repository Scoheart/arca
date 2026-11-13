import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { DialogShell } from "./DialogShell";
import { ImeInput, isImeKey } from "./ImeInput";
import { McpAvatar } from "./McpIcon";
import { ToastNotice } from "./Toast";
import { SearchBar } from "./ui";
import { CheckIcon, PlusIcon, TagIcon, TrashIcon, XIcon } from "./icons";
import {
  keyOf,
  MAX_MCP_TAG_LEN,
  MAX_MCP_TAGS_PER_ASSET,
  normalizeMcpTags,
  resolveMcpTags,
  transportOf,
} from "../lib/mcp";
import type {
  CatalogItem,
  McpIconPreference,
  McpIconPreferences,
  McpTagPreferences,
  RegistryEntry,
} from "../lib/types";

function hasFoldedTag(tags: readonly string[], candidate: string): boolean {
  const needle = candidate.trim().toLowerCase();
  return needle.length > 0 && tags.some((tag) => tag.trim().toLowerCase() === needle);
}

export function appendMcpTag(
  current: readonly string[],
  raw: string,
): { next: string[]; added: boolean; error: string | null } {
  const trimmed = raw.trim();
  if (!trimmed) return { next: [...current], added: false, error: null };
  if ([...trimmed].length > MAX_MCP_TAG_LEN || /[\u0000-\u001f\u007f]/.test(trimmed)) {
    return {
      next: [...current],
      added: false,
      error: "limit",
    };
  }
  if (hasFoldedTag(current, trimmed)) {
    return { next: [...current], added: true, error: null };
  }
  if (current.length >= MAX_MCP_TAGS_PER_ASSET) {
    return {
      next: [...current],
      added: false,
      error: "limit",
    };
  }
  return {
    next: normalizeMcpTags([...current, trimmed]),
    added: true,
    error: null,
  };
}

export function McpTagInput({
  tags,
  availableTags = [],
  onChange,
  disabled = false,
  autoFocus = false,
  inputValue,
  onInputValueChange,
}: {
  tags: string[];
  availableTags?: string[];
  onChange: (nextTags: string[]) => void;
  disabled?: boolean;
  autoFocus?: boolean;
  inputValue?: string;
  onInputValueChange?: (value: string) => void;
}) {
  const { t } = useTranslation();
  const [localInput, setLocalInput] = useState("");
  const [validationError, setValidationError] = useState<string | null>(null);
  const draft = inputValue ?? localInput;
  const setDraft = onInputValueChange ?? setLocalInput;

  const suggestions = useMemo(
    () => availableTags.filter((candidate) => !hasFoldedTag(tags, candidate)),
    [availableTags, tags],
  );

  const commitDraft = () => {
    if (disabled || !draft.trim()) return;
    const result = appendMcpTag(tags, draft);
    if (result.error) {
      setValidationError(
        t("mcpTags.limitHint", {
          maxLen: MAX_MCP_TAG_LEN,
          maxCount: MAX_MCP_TAGS_PER_ASSET,
        }),
      );
      return;
    }
    setValidationError(null);
    setDraft("");
    if (result.next.length !== tags.length || result.next.some((tag, i) => tag !== tags[i])) {
      onChange(result.next);
    }
  };

  const removeTag = (target: string) => {
    if (disabled) return;
    setValidationError(null);
    onChange(tags.filter((tag) => tag.toLowerCase() !== target.toLowerCase()));
  };

  const toggleSuggestion = (tag: string) => {
    if (disabled) return;
    const result = appendMcpTag(tags, tag);
    if (result.error) {
      setValidationError(
        t("mcpTags.limitHint", {
          maxLen: MAX_MCP_TAG_LEN,
          maxCount: MAX_MCP_TAGS_PER_ASSET,
        }),
      );
      return;
    }
    setValidationError(null);
    onChange(result.next);
  };

  return (
    <div className="arca-mcp-tag-editor">
      {tags.length > 0 && (
        <div className="arca-mcp-tag-pills" role="list" aria-label={t("mcpTags.label")}>
          {tags.map((tag) => (
            <span key={tag} role="listitem" className="arca-mcp-tag-pill">
              <TagIcon className="w-3 h-3" />
              <span title={tag}>{tag}</span>
              <button
                type="button"
                className="arca-mcp-tag-pill-remove"
                aria-label={t("mcpTags.removeTag", { tag })}
                disabled={disabled}
                onClick={() => removeTag(tag)}
              >
                <XIcon className="w-3 h-3" />
              </button>
            </span>
          ))}
        </div>
      )}

      <div className="arca-mcp-tag-input-row">
        <ImeInput
          type="text"
          className="arca-dialog-input"
          value={draft}
          autoFocus={autoFocus}
          data-modal-initial-focus={autoFocus || undefined}
          disabled={disabled}
          aria-label={t("mcpTags.tagName")}
          placeholder={t("mcpTags.addPlaceholder")}
          onChange={(event) => {
            setValidationError(null);
            setDraft(event.target.value);
          }}
          onKeyDown={(event) => {
            if (isImeKey(event)) return;
            if (event.key === "Enter") {
              event.preventDefault();
              commitDraft();
            }
          }}
        />
        <button
          type="button"
          className="btn-secondary arca-mcp-tag-add-btn"
          disabled={disabled || !draft.trim()}
          onClick={commitDraft}
        >
          <PlusIcon className="w-3.5 h-3.5" />
          <span>{t("mcpTags.add")}</span>
        </button>
      </div>

      {validationError && (
        <p className="arca-mcp-field-error" role="alert">{validationError}</p>
      )}

      {suggestions.length > 0 && (
        <div className="arca-mcp-tag-suggestions">
          <span className="arca-mcp-tag-suggestions-label">{t("mcpTags.suggestions")}</span>
          <div className="arca-mcp-tag-suggestions-list">
            {suggestions.map((tag) => (
              <button
                key={tag}
                type="button"
                className="arca-mcp-tag-suggestion"
                disabled={disabled}
                onClick={() => toggleSuggestion(tag)}
              >
                <PlusIcon className="w-3 h-3" />
                <span>{tag}</span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export function McpTagPickerDialog({
  assetKey,
  entry,
  iconPreference,
  initialTags,
  availableTags,
  onSave,
  onClose,
}: {
  assetKey: string;
  entry: RegistryEntry;
  iconPreference?: McpIconPreference;
  initialTags: string[];
  availableTags: string[];
  onSave: (nextTags: string[]) => Promise<unknown>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [draftTags, setDraftTags] = useState<string[]>(() => normalizeMcpTags(initialTags));
  const [pendingInput, setPendingInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    let finalTags = draftTags;
    if (pendingInput.trim()) {
      const appended = appendMcpTag(draftTags, pendingInput);
      if (appended.error) {
        setError(
          t("mcpTags.limitHint", {
            maxLen: MAX_MCP_TAG_LEN,
            maxCount: MAX_MCP_TAGS_PER_ASSET,
          }),
        );
        return;
      }
      finalTags = appended.next;
    }

    setBusy(true);
    setError(null);
    try {
      await onSave(finalTags);
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const subtitleText = entry.config.http?.url
    ?? [entry.config.stdio?.command, ...(entry.config.stdio?.args ?? [])].filter(Boolean).join(" ");

  return (
    <DialogShell
      kind="picker"
      size="sm"
      className="arca-dialog-mcp-tags"
      title={t("mcpTags.pickerTitle")}
      subtitle={t("mcpTags.pickerSubtitle", { name: entry.name })}
      busy={busy}
      onClose={onClose}
      status={error && <ToastNotice message={error} />}
      footerEnd={
        <>
          <button type="button" className="btn-secondary" disabled={busy} onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="button" className="btn-primary" disabled={busy} onClick={() => void handleSave()}>
            {busy ? t("mcpTags.saving") : t("mcpTags.save")}
          </button>
        </>
      }
    >
      <div className="arca-mcp-tag-dialog-body">
        <div className="arca-mcp-tag-target">
          <McpAvatar assetKey={assetKey} entry={entry} preference={iconPreference} size={36} />
          <div className="arca-mcp-tag-target-copy">
            <strong>{entry.name}</strong>
            {subtitleText && <code>{subtitleText}</code>}
          </div>
        </div>
        <McpTagInput
          tags={draftTags}
          availableTags={availableTags}
          onChange={setDraftTags}
          inputValue={pendingInput}
          onInputValueChange={setPendingInput}
          disabled={busy}
          autoFocus
        />
      </div>
    </DialogShell>
  );
}

interface UniqueMcpAsset {
  key: string;
  entry: RegistryEntry;
  currentTags: string[];
}

export function McpTagManagerDialog({
  catalog,
  tagPreferences,
  iconPreferences,
  initialTag,
  availableTags,
  onSaveBatch,
  onClose,
}: {
  catalog: CatalogItem[];
  tagPreferences: McpTagPreferences;
  iconPreferences: McpIconPreferences;
  initialTag?: string | null;
  availableTags: string[];
  onSaveBatch: (updates: McpTagPreferences) => Promise<unknown>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const uniqueAssets = useMemo<UniqueMcpAsset[]>(() => {
    const byKey = new Map<string, CatalogItem>();
    for (const item of catalog) {
      const k = keyOf(item.entry);
      const prev = byKey.get(k);
      if (!prev || (!prev.in_effect && item.in_effect)) {
        byKey.set(k, item);
      }
    }
    const collator = new Intl.Collator(undefined, { sensitivity: "base" });
    return [...byKey.entries()]
      .map(([key, item]) => ({
        key,
        entry: item.entry,
        currentTags: resolveMcpTags(item.entry, tagPreferences),
      }))
      .sort((a, b) => collator.compare(a.entry.name, b.entry.name) || a.key.localeCompare(b.key));
  }, [catalog, tagPreferences]);

  const [sourceTag, setSourceTag] = useState<string | null>(initialTag ?? null);
  const [tagName, setTagName] = useState(initialTag ?? "");
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(() => {
    if (!initialTag) return new Set();
    return new Set(
      uniqueAssets
        .filter((asset) => hasFoldedTag(asset.currentTags, initialTag))
        .map((asset) => asset.key),
    );
  });
  const [filterQuery, setFilterQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const switchEditingTag = (nextSourceTag: string | null) => {
    setError(null);
    setSourceTag(nextSourceTag);
    setTagName(nextSourceTag ?? "");
    if (!nextSourceTag) {
      setSelectedKeys(new Set());
      return;
    }
    setSelectedKeys(
      new Set(
        uniqueAssets
          .filter((asset) => hasFoldedTag(asset.currentTags, nextSourceTag))
          .map((asset) => asset.key),
      ),
    );
  };

  const visibleAssets = useMemo(() => {
    const needle = filterQuery.trim().toLowerCase();
    if (!needle) return uniqueAssets;
    return uniqueAssets.filter((asset) =>
      `${asset.entry.name}\n${asset.entry.description}\n${asset.currentTags.join("\n")}`
        .toLowerCase()
        .includes(needle),
    );
  }, [filterQuery, uniqueAssets]);

  const toggleAsset = (key: string) => {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleDeleteTag = async () => {
    if (!sourceTag) return;
    const updates: McpTagPreferences = {};
    for (const asset of uniqueAssets) {
      if (hasFoldedTag(asset.currentTags, sourceTag)) {
        updates[asset.key] = asset.currentTags.filter(
          (tag) => tag.toLowerCase() !== sourceTag.toLowerCase(),
        );
      }
    }
    if (Object.keys(updates).length === 0) {
      onClose();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onSaveBatch(updates);
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const handleSave = async () => {
    const targetTag = tagName.trim();
    if (!targetTag) return;
    if ([...targetTag].length > MAX_MCP_TAG_LEN || /[\u0000-\u001f\u007f]/.test(targetTag)) {
      setError(
        t("mcpTags.limitHint", {
          maxLen: MAX_MCP_TAG_LEN,
          maxCount: MAX_MCP_TAGS_PER_ASSET,
        }),
      );
      return;
    }

    const updates: McpTagPreferences = {};
    for (const asset of uniqueAssets) {
      const hadSource = sourceTag ? hasFoldedTag(asset.currentTags, sourceTag) : false;
      const shouldHaveTarget = selectedKeys.has(asset.key);
      const base = sourceTag
        ? asset.currentTags.filter((tag) => tag.toLowerCase() !== sourceTag.toLowerCase())
        : asset.currentTags;
      let nextTags = base;
      if (shouldHaveTarget) {
        const appended = appendMcpTag(base, targetTag);
        if (appended.error) {
          setError(
            t("mcpTags.limitHint", {
              maxLen: MAX_MCP_TAG_LEN,
              maxCount: MAX_MCP_TAGS_PER_ASSET,
            }),
          );
          return;
        }
        nextTags = appended.next;
      } else if (!hadSource) {
        continue;
      }

      const changed =
        nextTags.length !== asset.currentTags.length ||
        nextTags.some((tag, i) => tag !== asset.currentTags[i]);
      if (changed) {
        updates[asset.key] = nextTags;
      }
    }

    if (Object.keys(updates).length === 0) {
      onClose();
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await onSaveBatch(updates);
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const canSave = tagName.trim().length > 0 && (sourceTag !== null || selectedKeys.size > 0);

  return (
    <DialogShell
      kind="editor"
      size="md"
      className="arca-dialog-mcp-tag-manager"
      title={t("mcpTags.manageTitle")}
      subtitle={t("mcpTags.manageSubtitle")}
      busy={busy}
      onClose={onClose}
      status={error && <ToastNotice message={error} />}
      footerStart={
        sourceTag ? (
          <button
            type="button"
            className="btn-danger"
            disabled={busy}
            onClick={() => void handleDeleteTag()}
          >
            <TrashIcon className="w-4 h-4" />
            <span>{t("mcpTags.deleteTag")}</span>
          </button>
        ) : null
      }
      footerEnd={
        <>
          <button type="button" className="btn-secondary" disabled={busy} onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="btn-primary"
            disabled={busy || !canSave}
            onClick={() => void handleSave()}
          >
            {busy ? t("mcpTags.saving") : t("mcpTags.save")}
          </button>
        </>
      }
    >
      <div className="arca-mcp-tag-manager">
        {availableTags.length > 0 && (
          <div className="arca-mcp-tag-suggestions">
            <span className="arca-mcp-tag-suggestions-label">{t("mcpTags.selectExisting")}</span>
            <div className="arca-mcp-tag-suggestions-list">
              <button
                type="button"
                className="arca-mcp-tag-suggestion"
                data-selected={sourceTag === null ? "true" : undefined}
                disabled={busy}
                onClick={() => switchEditingTag(null)}
              >
                <PlusIcon className="w-3 h-3" />
                <span>{t("desktop.sourcesSidebar.createTag")}</span>
              </button>
              {availableTags.map((tag) => (
                <button
                  key={tag}
                  type="button"
                  className="arca-mcp-tag-suggestion"
                  data-selected={sourceTag === tag ? "true" : undefined}
                  disabled={busy}
                  onClick={() => switchEditingTag(tag)}
                >
                  <TagIcon className="w-3 h-3" />
                  <span>{tag}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        <label className="arca-mcp-field">
          <span>{t("mcpTags.tagName")}</span>
          <ImeInput
            type="text"
            className="arca-dialog-input"
            value={tagName}
            disabled={busy}
            autoFocus
            data-modal-initial-focus
            placeholder={t("mcpTags.tagNamePlaceholder")}
            onChange={(event) => {
              setError(null);
              setTagName(event.target.value);
            }}
          />
        </label>

        <div className="arca-mcp-tag-manager-list-head">
          <span>{t("mcpTags.assignedMcps", { count: selectedKeys.size })}</span>
          <div className="arca-mcp-tag-manager-bulk">
            <button
              type="button"
              className="arca-mcp-tag-bulk-btn"
              disabled={busy || visibleAssets.length === 0}
              onClick={() => {
                setSelectedKeys((prev) => {
                  const next = new Set(prev);
                  for (const asset of visibleAssets) next.add(asset.key);
                  return next;
                });
              }}
            >
              {t("mcpTags.selectAll")}
            </button>
            <button
              type="button"
              className="arca-mcp-tag-bulk-btn"
              disabled={busy || selectedKeys.size === 0}
              onClick={() => setSelectedKeys(new Set())}
            >
              {t("mcpTags.clearSelection")}
            </button>
          </div>
        </div>

        <SearchBar
          value={filterQuery}
          onChange={setFilterQuery}
          placeholder={t("mcpTags.searchMcps")}
        />

        <div className="arca-mcp-tag-checklist" role="list" aria-label={t("centralAssets.mcpList")}>
          {visibleAssets.map((asset) => {
            const checked = selectedKeys.has(asset.key);
            return (
              <button
                key={asset.key}
                type="button"
                role="listitem"
                aria-pressed={checked}
                className="arca-mcp-tag-check-row"
                data-selected={checked ? "true" : undefined}
                disabled={busy}
                onClick={() => toggleAsset(asset.key)}
              >
                <span className="arca-mcp-tag-checkbox" aria-hidden="true">
                  {checked && <CheckIcon className="w-3 h-3" />}
                </span>
                <McpAvatar
                  assetKey={asset.key}
                  entry={asset.entry}
                  preference={iconPreferences[asset.key]}
                  size={28}
                />
                <span className="arca-mcp-tag-check-copy">
                  <strong title={asset.entry.name}>{asset.entry.name}</strong>
                  <span>{transportOf(asset.entry).toUpperCase()}</span>
                </span>
                {asset.currentTags.length > 0 && (
                  <span className="arca-mcp-tag-check-existing" title={asset.currentTags.join(" · ")}>
                    {asset.currentTags.slice(0, 2).join(" · ")}
                    {asset.currentTags.length > 2 ? ` +${asset.currentTags.length - 2}` : ""}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>
    </DialogShell>
  );
}
