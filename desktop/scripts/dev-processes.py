#!/usr/bin/env python3
"""macOS: inspect/stop only this checkout's Tauri dev app, watcher and Vite server."""
import ctypes
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

DESKTOP = Path(__file__).resolve().parents[1]
ROOT = DESKTOP.parent
APP_NAME = json.loads((DESKTOP / "src-tauri/tauri.dev.conf.json").read_text())["productName"]
APP_PATHS = {str(folder / suffix) for folder in [DESKTOP / "src-tauri/target/debug", ROOT / "target/debug"]
             for suffix in ["desktop", f"{APP_NAME}.app/Contents/MacOS/desktop"]}
LIBPROC = ctypes.CDLL("/usr/lib/libproc.dylib")


def output(*args):
    return subprocess.run(args, capture_output=True, text=True).stdout.strip()


def identity(pid):
    buffer = ctypes.create_string_buffer(4096)
    if LIBPROC.proc_pidpath(pid, buffer, len(buffer)) <= 0:
        return None
    executable = buffer.value.decode()
    if executable in APP_PATHS:
        return {"pid": pid, "kind": "app", "path": executable}
    if Path(executable).name not in {"node", "nodejs", "bun"}:
        return None
    cwd = next((line[1:] for line in output("/usr/sbin/lsof", "-a", "-p", str(pid), "-d", "cwd", "-Fn").splitlines() if line.startswith("n")), "")
    if cwd not in {str(DESKTOP), str(DESKTOP / "src-tauri")}:
        return None
    command = output("ps", "-p", str(pid), "-o", "args=")
    if re.search(r"(?:^|[/\s])tauri(?:\.js|\.mjs)?\s+dev(?:\s|$)", command):
        return {"pid": pid, "kind": "watcher", "path": executable}
    if cwd == str(DESKTOP) and re.search(r"/vite(?:\s|$|/)", command):
        return {"pid": pid, "kind": "server", "path": executable}
    return None


def processes(apps_only=False):
    return [entry for raw in output("pgrep", "-x", "desktop" if apps_only else "node|nodejs|bun|desktop").splitlines()
            if raw.isdigit() and (entry := identity(int(raw))) is not None]


def stop(entry):
    pid = entry["pid"]
    if identity(pid) != entry:
        return
    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            return
        for _ in range(30):
            if identity(pid) != entry:
                return
            time.sleep(0.1)
    raise RuntimeError(f"Development {entry['kind']} {pid} did not exit")


def main():
    mode = sys.argv[1] if len(sys.argv) == 2 else ""
    if mode == "list":
        print(json.dumps(processes()))
    elif mode == "stop":
        for kind in ("watcher", "app", "server"):
            for entry in processes():
                if entry["kind"] == kind:
                    stop(entry)
    elif mode in {"pid", "wait"}:
        for _ in range(180 if mode == "wait" else 1):
            apps = [entry for entry in processes(apps_only=True) if entry["kind"] == "app"]
            if len(apps) == 1:
                print(apps[0]["pid"])
                return
            if mode == "wait":
                time.sleep(1)
        raise RuntimeError("No unique development app is running")
    else:
        raise RuntimeError("Usage: dev-processes.py <list|stop|pid|wait>")


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        sys.exit(str(error))
