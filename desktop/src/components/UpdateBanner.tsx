import type { UpdaterState } from "../hooks/useUpdater";
import { openUrl } from "@tauri-apps/plugin-opener";
import { DownloadIcon, FolderIcon, RefreshIcon, XIcon } from "./icons";

const LATEST_RELEASE_URL = "https://github.com/Scoheart/mux/releases/latest";

/**
 * Non-blocking update card, floated below the header at the top-right —
 * available → downloading (progress) → ready (restart) without ever
 * modal-blocking the app.
 */
export function UpdateBanner({ updater }: { updater: UpdaterState }) {
  const { phase } = updater;
  if (phase.kind === "idle" || phase.kind === "checking") return null;

  return (
    <div
      className="fixed right-5 z-50 rounded-mac"
      style={{
        top: 64,
        width: 340,
        background: "var(--glass-fill-strong)",
        backdropFilter: "blur(var(--glass-blur)) saturate(var(--glass-saturate))",
        WebkitBackdropFilter: "blur(var(--glass-blur)) saturate(var(--glass-saturate))",
        border: "1px solid var(--glass-border)",
        boxShadow: "var(--shadow-sheet), var(--glass-highlight)",
        color: "var(--text-primary)",
      }}
    >
      {phase.kind === "available" && (
        <div className="p-4">
          <div className="flex items-start gap-2.5">
            <div
              className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0"
              style={{ background: "var(--color-blue)" }}
            >
              <DownloadIcon className="w-3.5 h-3.5 text-white" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold">发现新版本 v{phase.version}</div>
            </div>
          </div>
          <div className="flex justify-end gap-2 mt-3">
            <button type="button" className="btn-ghost" onClick={updater.dismiss}>
              稍后
            </button>
            <button type="button" className="btn-primary" onClick={() => void updater.download()}>
              立即更新
            </button>
          </div>
        </div>
      )}

      {phase.kind === "downloading" && (
        <div className="p-4">
          <div className="text-sm font-semibold">
            正在下载更新{phase.percent != null ? ` ${phase.percent}%` : "…"}
          </div>
          <div
            className="mt-2.5 h-1.5 rounded-full overflow-hidden"
            style={{ background: "color-mix(in srgb, var(--text-primary) 12%, transparent)" }}
            role="progressbar"
            aria-label="更新下载进度"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={phase.percent ?? undefined}
          >
            <div
              className="h-full rounded-full"
              style={{
                width: `${phase.percent ?? 8}%`,
                background: "var(--color-blue)",
              }}
            />
          </div>
          <div className="mt-2 text-xs" style={{ color: "var(--text-secondary)" }}>
            后台下载中，您可继续使用
          </div>
        </div>
      )}

      {phase.kind === "ready" && (
        <div className="p-4">
          <div className="flex items-start gap-2.5">
            <div
              className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0"
              style={{ background: "#34C759" }}
            >
              <RefreshIcon className="w-3.5 h-3.5 text-white" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold">v{phase.version} 已就绪</div>
              <div className="mt-0.5 text-xs" style={{ color: "var(--text-secondary)" }}>
                重启应用即可完成更新。
              </div>
            </div>
          </div>
          <div className="flex justify-end gap-2 mt-3">
            <button type="button" className="btn-ghost" onClick={updater.later}>
              下次启动时
            </button>
            <button type="button" className="btn-primary" onClick={() => void updater.restart()}>
              立即重启
            </button>
          </div>
        </div>
      )}

      {phase.kind === "requires-install" && (
        <div className="p-4">
          <div className="flex items-start gap-2.5">
            <div
              className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0"
              style={{ background: "var(--color-blue)" }}
            >
              <FolderIcon className="w-3.5 h-3.5 text-white" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold">请先安装 Arca</div>
              <div className="mt-1 text-xs" style={{ color: "var(--text-secondary)" }}>
                当前应用位于只读磁盘映像或系统隔离目录，无法原地更新。请退出 Arca，
                将 Arca.app 拖入“应用程序”后重新打开；现有配置不会丢失。
              </div>
            </div>
          </div>
          <div className="flex justify-end gap-2 mt-3">
            <button type="button" className="btn-ghost" onClick={updater.later}>
              关闭
            </button>
            <button
              type="button"
              className="btn-primary"
              onClick={() => void openUrl(LATEST_RELEASE_URL)}
            >
              下载最新版
            </button>
          </div>
        </div>
      )}

      {phase.kind === "error" && (
        <div className="p-4">
          <div className="flex items-start gap-2.5">
            <div
              className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0"
              style={{ background: "#FF3B30" }}
            >
              <XIcon className="w-3.5 h-3.5 text-white" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold">
                {phase.operation === "install"
                  ? "安装更新失败"
                  : phase.operation === "restart"
                    ? "重启失败"
                    : "检查更新失败"}
              </div>
              <div
                className="mt-1 text-xs break-words overflow-y-auto"
                style={{ color: "var(--text-secondary)", maxHeight: 96 }}
              >
                {phase.message}
              </div>
            </div>
          </div>
          <div className="flex justify-end gap-2 mt-3">
            <button type="button" className="btn-ghost" onClick={updater.later}>
              关闭
            </button>
            <button
              type="button"
              className="btn-primary"
              onClick={() =>
                void (phase.operation === "install"
                  ? updater.download()
                  : phase.operation === "restart"
                    ? updater.restart()
                    : updater.checkNow({ manual: true }))
              }
            >
              重试
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
