import { useEffect, useRef, useState } from "react";
import { usePreferenceRevision } from "../lib/preferenceObservation";
import { invoke } from "@tauri-apps/api/core";
import { FormSelect } from "./FormSelect";
import { formatError } from "../lib/format";
import { ToastNotice, useToast } from "./Toast";
import { TerminalIcon } from "./icons";
import terminalIcon from "../assets/terminals/terminal.png";
import ghosttyIcon from "../assets/terminals/ghostty.png";
import warpIcon from "../assets/terminals/warp.png";
const terminalIcons: Record<string, string> = { terminal: terminalIcon, ghostty: ghosttyIcon, warp: warpIcon };
function TerminalGlyph({ id }: { id: string }) {
  const src = terminalIcons[id];
  return src ? <img className="arca-terminal-icon" src={src} alt="" aria-hidden="true" draggable={false} />
    : <TerminalIcon className="arca-terminal-icon" />;
}
interface TerminalSettings { selected: string; options: { id: string; name: string; installed: boolean }[] }
export function TerminalSelect() {
  const [settings, setSettings] = useState<TerminalSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  const sharedRevision = usePreferenceRevision();
  const pending = useRef(false);
  const generation = useRef(0);
  const toast = useToast();
  useEffect(() => {
    let active = true;
    const request = ++generation.current;
    if (pending.current) return;
    setError("");
    void invoke<TerminalSettings>("get_terminal_settings").then((result) => { if (active && request === generation.current) setSettings(result); })
      .catch((error) => { if (active && request === generation.current) setError(formatError(error)); });
    return () => { active = false; };
  }, [revision, sharedRevision]);
  async function choose(terminalId: string) {
    if (pending.current || terminalId === settings?.selected) return;
    pending.current = true;
    ++generation.current;
    setBusy(true); setError("");
    try {
      const result = await invoke<TerminalSettings>("set_terminal_preference", { terminalId });
      setSettings(result);
      toast.show({ kind: "success", msg: `CLI 将使用 ${result.options.find((item) => item.id === result.selected)?.name ?? result.selected} 启动` });
    } catch (error) { setError(formatError(error)); }
    finally { pending.current = false; setBusy(false); }
  }
  return <div className="arca-terminal-select">
    {settings && <FormSelect ariaLabel="默认终端" value={settings.selected} disabled={busy}
      triggerContent={<span className="arca-terminal-selection"><TerminalGlyph id={settings.selected} />
        <span>{settings.options.find((item) => item.id === settings.selected)?.name ?? settings.selected}</span></span>}
      options={settings.options.filter((item) => item.installed || item.id === settings.selected)
        .map((item) => ({ value: item.id, label: item.name + (item.installed ? "" : " · 未安装"), icon: <TerminalGlyph id={item.id} /> }))}
      onChange={(value) => void choose(value)} />}
    {!settings && !error && <span role="status">读取中…</span>}
    {error && <ToastNotice message={error} action={<button type="button" className="btn-ghost" onClick={() => setRevision((value) => value + 1)}>重试</button>} />}
  </div>;
}
