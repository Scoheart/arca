import { useState, type FormEvent } from "react";
import type { StartupAgentDefinition } from "../lib/types";
import { DialogShell } from "./DialogShell";
import { AgentIcon, SaveIcon } from "./icons";
import { ToastNotice } from "./Toast";

interface StartupAgentEditorDialogProps {
  startupAgent?: StartupAgentDefinition | null;
  existingNames: Set<string>;
  onSave: (definition: StartupAgentDefinition) => Promise<void>;
  onClose: () => void;
}

export function StartupAgentEditorDialog({
  startupAgent,
  existingNames,
  onSave,
  onClose,
}: StartupAgentEditorDialogProps) {
  const isEditing = Boolean(startupAgent);
  const [name, setName] = useState(startupAgent?.name ?? "");
  const [displayName, setDisplayName] = useState(startupAgent?.display_name ?? "");
  const [description, setDescription] = useState(startupAgent?.description ?? "");
  const [model, setModel] = useState(startupAgent?.model ?? "");
  const [mode, setMode] = useState(startupAgent?.mode ?? "");
  const [readonly, setReadonly] = useState(startupAgent?.readonly ?? false);
  const [toolsInput, setToolsInput] = useState(startupAgent?.tools?.join(", ") ?? "");
  const [systemPrompt, setSystemPrompt] = useState(startupAgent?.system_prompt ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) {
      setError("Agent 标识符（Name）不能为空");
      return;
    }
    if (!/^[a-zA-Z0-9_-]+$/.test(trimmedName)) {
      setError("Agent 标识符只能包含英文字母、数字、短横线 (-) 和下划线 (_)");
      return;
    }
    if (!isEditing && existingNames.has(trimmedName)) {
      setError(`已存在名为 "${trimmedName}" 的 Agent`);
      return;
    }
    if (!displayName.trim()) {
      setError("显示名称（Display Name）不能为空");
      return;
    }
    if (!systemPrompt.trim()) {
      setError("系统提示词（System Prompt）不能为空");
      return;
    }

    const tools = toolsInput
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);

    const definition: StartupAgentDefinition = {
      name: trimmedName,
      display_name: displayName.trim(),
      description: description.trim(),
      system_prompt: systemPrompt.trim(),
      model: model.trim() || undefined,
      mode: mode.trim() || undefined,
      readonly,
      tools: tools.length > 0 ? tools : undefined,
      created_at: startupAgent?.created_at || new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };

    setSaving(true);
    setError(null);
    try {
      await onSave(definition);
      onClose();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <DialogShell
      kind="editor"
      size="wide"
      title={isEditing ? `编辑 Agent · ${startupAgent?.display_name || startupAgent?.name}` : "新建启动 Agent"}
      subtitle="定义可作为工具默认身份或在启动时通过 --agent 传入的主代理规范"
      leading={<AgentIcon className="w-5 h-5" />}
      onClose={onClose}
      busy={saving}
      footerEnd={
        <div className="flex items-center gap-2">
          <button type="button" className="btn-secondary" onClick={onClose} disabled={saving}>
            取消
          </button>
          <button
            type="submit"
            form="startup-agent-editor-form"
            className="btn-primary flex items-center gap-1.5"
            disabled={saving}
          >
            <SaveIcon className="w-3.5 h-3.5" />
            <span>{saving ? "保存中…" : isEditing ? "保存变更" : "创建 Agent"}</span>
          </button>
        </div>
      }
    >
      <form id="startup-agent-editor-form" onSubmit={handleSubmit} className="arca-startup-agent-editor-form space-y-4">
        {error && <ToastNotice kind="error" message={error} />}

        <div className="grid grid-cols-2 gap-4">
          <label className="block">
            <span className="text-xs font-semibold text-muted block mb-1">
              标识符 (Name / Kebab-case) <span className="text-rose-500">*</span>
            </span>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={isEditing || saving}
              placeholder="e.g. code-reviewer"
              className="w-full arca-input font-mono text-sm"
              required
            />
          </label>

          <label className="block">
            <span className="text-xs font-semibold text-muted block mb-1">
              显示名称 (Display Name) <span className="text-rose-500">*</span>
            </span>
            <input
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              disabled={saving}
              placeholder="e.g. 代码审查员"
              className="w-full arca-input text-sm"
              required
            />
          </label>
        </div>

        <label className="block">
          <span className="text-xs font-semibold text-muted block mb-1">角色描述 (Description)</span>
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            disabled={saving}
            placeholder="简短描述该 Agent 的职能、工作模式和适用场景"
            className="w-full arca-input text-sm"
          />
        </label>

        <div className="grid grid-cols-3 gap-4">
          <label className="block">
            <span className="text-xs font-semibold text-muted block mb-1">推荐模型 (Model)</span>
            <input
              type="text"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              disabled={saving}
              placeholder="claude-3-5-sonnet, deepseek-chat"
              className="w-full arca-input text-sm font-mono"
            />
          </label>

          <label className="block">
            <span className="text-xs font-semibold text-muted block mb-1">模式标识 (Mode)</span>
            <input
              type="text"
              value={mode}
              onChange={(e) => setMode(e.target.value)}
              disabled={saving}
              placeholder="primary, plan, build 等"
              className="w-full arca-input text-sm font-mono"
            />
          </label>

          <div className="flex items-center pt-5">
            <label className="flex items-center gap-2 cursor-pointer select-none text-sm">
              <input
                type="checkbox"
                checked={readonly}
                onChange={(e) => setReadonly(e.target.checked)}
                disabled={saving}
                className="arca-checkbox"
              />
              <span>只读沙盒模式 (Read-only)</span>
            </label>
          </div>
        </div>

        <label className="block">
          <span className="text-xs font-semibold text-muted block mb-1">
            允许调用的工具 (Tools 白名单，英文逗号分隔)
          </span>
          <input
            type="text"
            value={toolsInput}
            onChange={(e) => setToolsInput(e.target.value)}
            disabled={saving}
            placeholder="read_file, grep, run_command (留空使用默认)"
            className="w-full arca-input text-xs font-mono"
          />
        </label>

        <label className="block">
          <span className="text-xs font-semibold text-muted block mb-1">
            系统提示词 (System Prompt Instructions) <span className="text-rose-500">*</span>
          </span>
          <textarea
            value={systemPrompt}
            onChange={(e) => setSystemPrompt(e.target.value)}
            disabled={saving}
            rows={10}
            placeholder="输入针对该 Agent 的系统提示词指令…"
            className="w-full arca-input font-mono text-xs leading-relaxed"
            required
          />
        </label>
      </form>
    </DialogShell>
  );
}
