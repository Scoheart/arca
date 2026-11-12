import { useEffect } from "react";
import { cliStatus, installCli } from "../lib/api";

/**
 * 桌面 App 自带 arca CLI（sidecar）：启动后静默把它软链到 ~/.local/bin/arca。
 * 软链指向包内 → App 自动更新后 CLI 同步更新，且断链（App 挪位置）会在下次
 * 启动时自动修复。
 */
export function useCliTool({ start = true }: { start?: boolean } = {}) {
  useEffect(() => {
    if (!start) return;
    const t = setTimeout(async () => {
      try {
        const status = await cliStatus();
        if (status.bundled && !status.installed) await installCli();
      } catch {
        // 静默失败：CLI 安装是锦上添花，不该打断启动。
      }
    }, 1000);
    return () => clearTimeout(t);
  }, [start]);
}
