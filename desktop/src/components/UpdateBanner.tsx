import type { UpdaterState } from "../hooks/useUpdater";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ToastNotice, useToast } from "./Toast";

const LATEST_RELEASE_URL = "https://github.com/Scoheart/arca/releases/latest";

/** Update lifecycle shares the global notice, including progress and actions. */
export function UpdateBanner({ updater }: { updater: UpdaterState }) {
  const { phase } = updater;
  const toast = useToast();
  if (phase.kind === "idle" || phase.kind === "checking") return null;
  if (phase.kind === "available") return <ToastNotice kind="info" noticeKey="update-available" persistent
    message={`发现新版本 v${phase.version}`} action={<>
      <button type="button" className="btn-ghost" onClick={updater.dismiss}>稍后</button>
      <button type="button" className="btn-primary" onClick={() => void updater.download()}>立即更新</button>
    </>} />;
  if (phase.kind === "downloading") return <ToastNotice kind="info" noticeKey="update-download" persistent
    message={`正在下载更新${phase.percent != null ? ` ${phase.percent}%` : "…"}`}
    action={<div className="arca-toast-progress" role="progressbar" aria-label="更新下载进度"
      aria-valuemin={0} aria-valuemax={100} aria-valuenow={phase.percent ?? undefined}>
      <span style={{ width: `${phase.percent ?? 8}%` }} />
    </div>} />;
  if (phase.kind === "ready") return <ToastNotice kind="success" noticeKey="update-ready" persistent
    message={`v${phase.version} 已就绪 · 重启应用即可完成更新。`} action={<>
      <button type="button" className="btn-ghost" onClick={updater.later}>下次启动时</button>
      <button type="button" className="btn-primary" onClick={() => void updater.restart()}>立即重启</button>
    </>} />;
  if (phase.kind === "requires-install") return <ToastNotice kind="warning" noticeKey="update-install" persistent
    message="请先安装 Arca · 将 Arca.app 拖入应用程序后重新打开，再进行更新。" action={<>
      <button type="button" className="btn-ghost" onClick={updater.later}>关闭</button>
      <button type="button" className="btn-primary" onClick={() => void openUrl(LATEST_RELEASE_URL)
        .catch(() => toast.show({ kind: "error", msg: "无法打开下载页面，请稍后重试。" }))}>下载最新版</button>
    </>} />;
  return <ToastNotice noticeKey="update-error" message={`${phase.operation === "install" ? "安装更新失败"
    : phase.operation === "restart" ? "重启失败" : "检查更新失败"} · ${phase.message}`} action={<>
      <button type="button" className="btn-ghost" onClick={updater.later}>关闭</button>
      <button type="button" className="btn-primary" onClick={() => void (phase.operation === "install"
        ? updater.download() : phase.operation === "restart" ? updater.restart() : updater.checkNow({ manual: true }))}>重试</button>
    </>} />;
}
