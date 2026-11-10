import { useState, type FormEvent } from "react";
import type { SubagentDefinition } from "../lib/types";
import { DialogShell } from "./DialogShell";
import { SubagentMarkIcon, SaveIcon } from "./icons";
import { ToastNotice } from "./Toast";

interface SubagentEditorDialogProps {
  subagent?: SubagentDefinition | null;
  existingNames: Set<string>;
  onSave: (definition: SubagentDefinition) => Promise<void>;
  onClose: () => void;
}

export function SubagentEditorDialog({
  subagent,
  existingNames,
  onSave,
  onClose,
}: SubagentEditorDialogProps) {
  const isEditing = Boolean(subagent);
  const [name, setName] = useState(subagent?.name ?? "");
  const [displayName, setDisplayName] = useState(subagent?.display_name ?? "");
  const [description, setDescription] = useState(subagent?.description ?? "");
  const [model, setModel] = useState(subagent?.model ?? "inherit");
  const [readonly, setReadonly] = useState(subagent?.readonly ?? false);
  const [allowSubagents, setAllowSubagents] = useState(subagent?.allow_subagents ?? false);
  const [toolsInput, setToolsInput] = useState(subagent?.tools?.join(", ") ?? "");
  const [mcpInput, setMcpInput] = useState(subagent?.mcp_dependencies?.join(", ") ?? "");
  const [systemPrompt, setSystemPrompt] = useState(subagent?.system_prompt ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) {
      setError("Subagent 标识符（Name）不能为空");
      return;
    }
    if (!/^[a-zA-Z0-9_-]+$/.test(trimmedName)) {
      setError("Subagent 标识符只能包含英文字母、数字、短横线 (-) 和下划线 (_)");
      return;
    }
    if (!isEditing && existingNames.has(trimmedName)) {
      setError(`已存在名为 "${trimmedName}" 的 Subagent`);
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
    const mcp_dependencies = mcpInput
      .split(",")
      .map((m) => m.trim())
      .filter(Boolean);

    const definition: SubagentDefinition = {
      name: trimmedName,
      display_name: displayName.trim(),
      description: description.trim(),
      system_prompt: systemPrompt.trim(),
      model: model.trim() || undefined,
      readonly,
      allow_subagents: allowSubagents,
      tools,
      mcp_dependencies,
      created_at: subagent?.created_at || new Date().toISOString(),
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
      title={isEditing ? `编辑 Subagent · ${subagent?.display_name || subagent?.name}` : "新建 Subagent"}
      subtitle="配置可跨 Agent 独立调度的子代理角色与能力定义"
      leading={<SubagentMarkIcon className="w-5 h-5" />}
      onClose={onClose}
      busy={saving}
      footerEnd={
        <div className="flex items-center gap-2">
          <button type="button" className="btn-secondary" onClick={onClose} disabled={saving}>
            取消
          </button>
          <button
            type="submit"
            form="subagent-editor-form"
            className="btn-primary flex items-center gap-1.5"
            disabled={saving}
          >
            <SaveIcon className="w-3.5 h-3.5" />
            <span>{saving ? "保存中…" : isEditing ? "保存变更" : "创建 Subagent"}</span>
          </button>
        </div>
      }
    >
      <form id="subagent-editor-form" onSubmit={handleSubmit} className="arca-subagent-editor-form space-y-4">
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
              placeholder="e.g. Code Reviewer"
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
            placeholder="简短描述该子代理的职能和适用任务场景"
            className="w-full arca-input text-sm"
          />
        </label>

        <div className="grid grid-cols-2 gap-4">
          <label className="block">
            <span className="text-xs font-semibold text-muted block mb-1">偏好模型 (Model)</span>
            <input
              type="text"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              disabled={saving}
              placeholder="inherit, fast, smart 或具体 Profile ID"
              className="w-full arca-input text-sm font-mono"
            />
          </label>

          <div className="flex items-center gap-6 pt-5">
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

            <label className="flex items-center gap-2 cursor-pointer select-none text-sm">
              <input
                type="checkbox"
                checked={allowSubagents}
                onChange={(e) => setAllowSubagents(e.target.checked)}
                disabled={saving}
                className="arca-checkbox"
              />
              <span>允许衍生子代理 (Recursive)</span>
            </label>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
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
              MCP 服务依赖 (MCP Dependencies，英文逗号分隔)
            </span>
            <input
              type="text"
              value={mcpInput}
              onChange={(e) => setMcpInput(e.target.value)}
              disabled={saving}
              placeholder="e.g. github, puppeteer"
              className="w-full arca-input text-xs font-mono"
            />
          </label>
        </div>

        <label className="block">
          <span className="text-xs font-semibold text-muted block mb-1">
            系统提示词 (System Prompt Instructions) <span className="text-rose-500">*</span>
          </span>
          <textarea
            value={systemPrompt}
            onChange={(e) => setSystemPrompt(e.target.value)}
            disabled={saving}
            rows={10}
            placeholder="输入针对该 Subagent 角色的完整 System Prompt 指令…"
            className="w-full arca-input font-mono text-xs leading-relaxed"
            required
          />
        </label>
      </form>
    </DialogShell>
  );
}
