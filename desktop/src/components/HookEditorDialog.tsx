import { useState, type FormEvent } from "react";
import type { HookDefinition } from "../lib/types";
import { DialogShell } from "./DialogShell";
import { HookMarkIcon, SaveIcon } from "./icons";
import { ToastNotice } from "./Toast";

interface HookEditorDialogProps {
  hook?: HookDefinition | null;
  existingNames: Set<string>;
  onSave: (definition: HookDefinition) => Promise<void>;
  onClose: () => void;
}

const COMMON_EVENTS = [
  { value: "UserPromptSubmit", label: "UserPromptSubmit (提交用户提示词前)" },
  { value: "PreToolUse", label: "PreToolUse (调用工具执行前)" },
  { value: "PostToolUse", label: "PostToolUse (工具执行完成返回后)" },
  { value: "SessionStart", label: "SessionStart (会话启动或恢复)" },
  { value: "PermissionRequest", label: "PermissionRequest (权限请求拦截)" },
  { value: "Stop", label: "Stop (会话或任务正常结束)" },
  { value: "StopFailure", label: "StopFailure (任务失败中断)" },
  { value: "SessionEnd", label: "SessionEnd (会话完全退出)" },
  { value: "Notification", label: "Notification (系统通知事件)" },
];

export function HookEditorDialog({
  hook,
  existingNames,
  onSave,
  onClose,
}: HookEditorDialogProps) {
  const isEditing = Boolean(hook);
  const [name, setName] = useState(hook?.name ?? "");
  const [displayName, setDisplayName] = useState(hook?.display_name ?? "");
  const [description, setDescription] = useState(hook?.description ?? "");
  const [event, setEvent] = useState(hook?.event ?? "UserPromptSubmit");
  const [matcher, setMatcher] = useState(hook?.matcher ?? "");
  const [command, setCommand] = useState(hook?.command ?? "");
  const [commandWindows, setCommandWindows] = useState(hook?.command_windows ?? "");
  const [timeout, setTimeoutVal] = useState(hook?.timeout ?? 10);
  const [enabled, setEnabled] = useState(hook?.enabled ?? true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) {
      setError("Hook 标识符（Name）不能为空");
      return;
    }
    if (!/^[a-zA-Z0-9_-]+$/.test(trimmedName)) {
      setError("Hook 标识符只能包含英文字母、数字、短横线 (-) 和下划线 (_)");
      return;
    }
    if (!isEditing && existingNames.has(trimmedName)) {
      setError(`已存在名为 "${trimmedName}" 的 Hook`);
      return;
    }
    if (!displayName.trim()) {
      setError("显示名称（Display Name）不能为空");
      return;
    }
    if (!event.trim()) {
      setError("事件类型（Event）不能为空");
      return;
    }
    if (!command.trim()) {
      setError("执行命令（Command）不能为空");
      return;
    }

    const definition: HookDefinition = {
      name: trimmedName,
      display_name: displayName.trim(),
      description: description.trim(),
      event: event.trim(),
      matcher: matcher.trim(),
      command: command.trim(),
      command_windows: commandWindows.trim() || undefined,
      hook_type: hook?.hook_type || "command",
      timeout: Number(timeout) || 10,
      enabled,
      created_at: hook?.created_at || new Date().toISOString(),
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
      title={isEditing ? `编辑 Hook · ${hook?.display_name || hook?.name}` : "新建 Hook"}
      subtitle="配置 Agent 运行周期事件触发与拦截命令"
      leading={<HookMarkIcon className="w-5 h-5" />}
      onClose={onClose}
      busy={saving}
      footerEnd={
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="btn-secondary"
            onClick={onClose}
            disabled={saving}
          >
            取消
          </button>
          <button
            type="submit"
            form="hook-editor-form"
            className="btn-primary inline-flex items-center gap-1.5"
            disabled={saving}
          >
            <SaveIcon className="w-4 h-4" />
            <span>{saving ? "保存中…" : isEditing ? "更新 Hook" : "创建 Hook"}</span>
          </button>
        </div>
      }
    >
      <form id="hook-editor-form" onSubmit={handleSubmit} className="arca-dialog-form">
        {error && <ToastNotice kind="error" message={error} />}

        <div className="arca-form-grid-2">
          <label className="arca-dialog-field">
            <span className="arca-dialog-label">
              唯一标识符 (ID) <span className="text-red-500">*</span>
            </span>
            <input
              type="text"
              className="arca-dialog-input font-mono"
              placeholder="例如: pre-tool-check"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={isEditing || saving}
              required
            />
            <span className="arca-dialog-hint">由字母、数字、破折号组成，保存后不可修改</span>
          </label>

          <label className="arca-dialog-field">
            <span className="arca-dialog-label">
              显示名称 <span className="text-red-500">*</span>
            </span>
            <input
              type="text"
              className="arca-dialog-input"
              placeholder="例如: 敏感工具调用拦截"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              disabled={saving}
              required
            />
            <span className="arca-dialog-hint">在 UI 中清晰标识 Hook 作用</span>
          </label>
        </div>

        <label className="arca-dialog-field">
          <span className="arca-dialog-label">Hook 描述</span>
          <input
            type="text"
            className="arca-dialog-input"
            placeholder="简要说明此 Hook 的触发时机与预期执行逻辑"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            disabled={saving}
          />
        </label>

        <div className="arca-form-grid-2">
          <label className="arca-dialog-field">
            <span className="arca-dialog-label">
              触发事件 (Event) <span className="text-red-500">*</span>
            </span>
            <select
              className="arca-dialog-input"
              value={event}
              onChange={(e) => setEvent(e.target.value)}
              disabled={saving}
            >
              {COMMON_EVENTS.map((ev) => (
                <option key={ev.value} value={ev.value}>
                  {ev.label}
                </option>
              ))}
            </select>
            <span className="arca-dialog-hint">选择在 Agent 生命周期的哪一个阶段触发</span>
          </label>

          <label className="arca-dialog-field">
            <span className="arca-dialog-label">匹配条件 (Matcher)</span>
            <input
              type="text"
              className="arca-dialog-input font-mono"
              placeholder="留空表示匹配所有；或输入工具名/正则"
              value={matcher}
              onChange={(e) => setMatcher(e.target.value)}
              disabled={saving}
            />
            <span className="arca-dialog-hint">留空匹配全部；支持正则表达式或精确工具名</span>
          </label>
        </div>

        <label className="arca-dialog-field">
          <span className="arca-dialog-label">
            执行 Shell 命令 <span className="text-red-500">*</span>
          </span>
          <textarea
            className="arca-dialog-input font-mono text-xs"
            rows={4}
            placeholder="例如: python3 /path/to/check.py || exit 1"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            disabled={saving}
            required
            spellCheck={false}
          />
          <span className="arca-dialog-hint">在宿主 Shell 中执行，退出码为非 0 时拦截生命周期</span>
        </label>

        <div className="arca-form-grid-2">
          <label className="arca-dialog-field">
            <span className="arca-dialog-label">Windows 命令 (可选)</span>
            <input
              type="text"
              className="arca-dialog-input font-mono text-xs"
              placeholder="仅在 Windows 环境下替代运行的命令"
              value={commandWindows}
              onChange={(e) => setCommandWindows(e.target.value)}
              disabled={saving}
            />
          </label>

          <label className="arca-dialog-field">
            <span className="arca-dialog-label">超时时间 (秒)</span>
            <input
              type="number"
              min={1}
              max={300}
              className="arca-dialog-input"
              value={timeout}
              onChange={(e) => setTimeoutVal(Number(e.target.value))}
              disabled={saving}
            />
          </label>
        </div>

        <div className="arca-form-checkbox-row">
          <label className="inline-flex items-center gap-2 cursor-pointer select-none">
            <input
              type="checkbox"
              className="arca-dialog-checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
              disabled={saving}
            />
            <span className="text-sm font-medium">启用此 Hook</span>
          </label>
        </div>
      </form>
    </DialogShell>
  );
}
