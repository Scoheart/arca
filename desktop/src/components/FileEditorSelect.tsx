import { readLocalSetting, writeLocalSetting } from "../lib/localSettings";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { formatError } from "../lib/format";
import { useToast } from "./Toast";
import { claimLayerKeyboardEvent } from "./ui";

import { AgentGlyph } from "./brandIcons";
import { ChevronDownIcon, EditIcon, FolderIcon, PlusIcon } from "./icons";

import textEditIcon from "../assets/editors/textedit.png";
import xcodeIcon from "../assets/editors/xcode.png";
import androidStudioIcon from "../assets/editors/android-studio.png";
import webStormIcon from "../assets/editors/webstorm.png";

const EDITOR_ICONS: Record<string, string> = {
  TextEdit: textEditIcon,
  Xcode: xcodeIcon,
  "Android Studio": androidStudioIcon,
  WebStorm: webStormIcon,
};

const STORAGE_KEY = "arca.file-editor";
const EDITORS = ["Visual Studio Code", "Cursor", "Sublime Text", "Zed", "TextEdit"];
const ICONS: Record<string, string> = { "Visual Studio Code": "vscode", Cursor: "cursor", Zed: "zed", Qoder: "qoder", "Qoder IDE": "qoder", Devin: "windsurf", Antigravity: "antigravity", ZCode: "zcode" };
function EditorIcon({ editor }: { editor: string }) {
  editor = editorLabel(editor);
  if (EDITOR_ICONS[editor]) return <img src={EDITOR_ICONS[editor]} alt="" aria-hidden="true" draggable={false}
    style={{ width: 20, height: 20, objectFit: "contain", flexShrink: 0 }} />;
  if (ICONS[editor]) return <span aria-hidden="true"><AgentGlyph id={ICONS[editor]} size={20} /></span>;
  if (editor === "Sublime Text") return <span className="arca-editor-sublime" aria-hidden="true">S</span>;
  return editor ? <EditIcon className="w-4 h-4" /> : <FolderIcon className="w-4 h-4" />;
}

export function preferredFileEditor(): string | undefined {
  try {
    return readLocalSetting(STORAGE_KEY) || undefined;
  } catch {
    return undefined;
  }
}

function editorLabel(editor: string): string {
  return editor.split(/[\\/]/).pop()?.replace(/\.app$/i, "") || editor;
}

function editorDisplayName(editor: string): string {
  const name = editorLabel(editor);
  return name === "Visual Studio Code" ? "VS Code" : name;
}

export function FileEditorSelect({ showLabel = false }: { showLabel?: boolean }) {
  const { t } = useTranslation();
  const [editor, setEditor] = useState(() => preferredFileEditor() ?? "");
  const [installed, setInstalled] = useState<Array<{name: string; path: string}>>([]);
  const [choosing, setChoosing] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!expanded) return;
    void invoke<Array<{name: string; path: string}>>("list_file_editors")
      .then(setInstalled).catch(() => {});
    root.current?.querySelector<HTMLButtonElement>('[role="menuitemradio"][aria-checked="true"]')?.focus();
    const dismiss = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setExpanded(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [expanded]);
  const { show: showToast } = useToast();

  async function change(value: string) {
    setExpanded(false);
    trigger.current?.focus();
    if (value === editor) return;
    setChoosing(true);
    try {
      let selected = value;
      if (value === "__choose__") {
        const path = await open({
          title: t("fileEditor.choose"),
          defaultPath: "/Applications",
          filters: [{ name: t("fileEditor.applications"), extensions: ["app"] }],
          multiple: false,
          directory: false,
        });
        if (!path) return;
        selected = path;
      }
      if (selected === editor) return;
      if (selected) writeLocalSetting(STORAGE_KEY, selected);
      else writeLocalSetting(STORAGE_KEY, null);
      setEditor(selected);
      showToast({ kind: "success", msg: selected
        ? t("fileEditor.saved", { editor: editorDisplayName(selected) })
        : t("fileEditor.reset") });
    } catch (error) {
      showToast({ kind: "error", msg: t("fileEditor.saveFailed", { error: formatError(error) }) });
    } finally {
      setChoosing(false);
    }
  }

  const available = installed.length ? installed.map((item) => item.path) : EDITORS;
  const options = ["", ...available, ...(editor && !available.includes(editor) ? [editor] : [])];
  const selectedLabel = editor ? editorDisplayName(editor) : t("fileEditor.systemDefault");
  return (
    <div className="arca-editor-menu-wrap" ref={root}
      onBlur={(event) => {
        // Safari can report null when clicking a button. Do not unmount it before click.
        if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) setExpanded(false);
      }}
      onKeyDown={(event) => {
        if (expanded && event.key === "Escape") {
          claimLayerKeyboardEvent(event.nativeEvent);
          event.preventDefault(); setExpanded(false); trigger.current?.focus();
        }
        if (expanded && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
          event.preventDefault();
          const items = Array.from(root.current?.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]') ?? []);
          const index = items.indexOf(document.activeElement as HTMLButtonElement);
          const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
          items[next]?.focus();
        }
      }}>
      <button type="button" ref={trigger} className="arca-model-field arca-form-select-trigger arca-editor-trigger"
        data-open={expanded ? "true" : undefined}
        aria-label={t("fileEditor.selected", { editor: selectedLabel })}
        title={t("fileEditor.choose")} aria-haspopup="menu" aria-expanded={expanded}
        disabled={choosing} onClick={() => setExpanded((value) => !value)}
        onKeyDown={(event) => { if (!expanded && event.key === "ArrowDown") { event.preventDefault(); setExpanded(true); } }}>
        <span className="arca-form-select-value"><span className="arca-editor-selection">
          <span className="arca-editor-glyph"><EditorIcon editor={editor} /></span>{showLabel && <span className="arca-editor-selected-label">{selectedLabel}</span>}
        </span></span><ChevronDownIcon className="arca-form-select-chevron" />
      </button>
      {expanded && <div className="arca-editor-menu" role="menu" aria-label={t("fileEditor.label")}>
        {options.map((value) => <button type="button" role="menuitemradio" aria-checked={value === editor}
          onMouseDown={(event) => event.preventDefault()}
          key={value || "default"} onClick={() => { void change(value); }}>
          <span className="arca-editor-glyph"><EditorIcon editor={value} /></span>
          <span>{value ? editorDisplayName(value) : t("fileEditor.systemDefault")}</span>
          {value === editor && <span className="arca-editor-check" aria-hidden="true">✓</span>}
        </button>)}
        <div role="separator" className="arca-editor-separator" />
        <button type="button" role="menuitem" onMouseDown={(event) => event.preventDefault()} onClick={() => { void change("__choose__"); }}>
          <PlusIcon className="w-4 h-4" /><span>{t("fileEditor.otherApplication")}</span>
        </button>
      </div>}
    </div>
  );
}
