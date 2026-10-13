---
name: run-desktop
description: Launch, screenshot, and drive the Arca Tauri desktop GUI headlessly (Xvfb + scrot + xdotool). Use when asked to run/start/open/screenshot the desktop app, or to visually verify a desktop (desktop/ Tauri+React) change in this repo — including clicking buttons and filling forms to confirm a feature works end-to-end.
---

# Run the Arca desktop app (headless)

The repo has two front-ends: a TS CLI/TUI (`src/`, run with `node dist/index.js`)
and a **Tauri desktop GUI** (`desktop/`, React + Rust). This skill runs the
desktop GUI in a headless container — there is no real display, so it renders to
a virtual X server (Xvfb), captures screenshots (scrot), and simulates input
(xdotool).

All steps are wrapped in `gui.sh` (next to this file). Prefer it over raw
commands — it handles Xvfb startup, the WebKit software-rendering flags, the
detached launch, and the build-ready wait.

## Prerequisites (usually already present)

`Xvfb scrot xdotool` and the app deps. If missing:
`sudo apt-get install -y xvfb scrot xdotool` and `cd desktop && npm install`.

## Quick start

```bash
SKILL=.claude/skills/run-desktop/gui.sh
bash $SKILL start          # ensure Xvfb :99 + launch tauri dev, wait until the window is up
bash $SKILL shot           # prints the PNG path; Read it to see the UI
```

`start` blocks until it sees ``Running `target/debug/desktop` `` in the log, or
reports a Rust build failure with a log tail. The **first** build takes a few
minutes; later runs are fast.

## Driving the UI

Coordinates are in the virtual screen (default 1280x900). Take a `shot` first,
read the PNG to find the element, then click/type:

```bash
bash $SKILL click 1156 67        # e.g. the top-right "+" (Add Agent) button
bash $SKILL type "my-tool"       # type into the focused field
bash $SKILL key Return           # or Escape, ctrl+a, Tab, ...
bash $SKILL shot /tmp/after.png  # verify the result
```

Use `SendUserFile` to show the user the screenshots that matter.

## Rebuild after code changes

- **Frontend** (`desktop/src/**`): Vite hot-reloads automatically — just take a
  new `shot`. No restart needed.
- **Rust** (`desktop/src-tauri/**`) **or** bundled data (`data/*.json`, which the
  Rust core `include_str!`s): the running binary is stale. Run
  `bash $SKILL restart` to rebuild and relaunch. (`tauri dev` only watches
  `src-tauri/`, so it will NOT auto-pick-up `data/` edits.)

## Manage / clean up

```bash
bash $SKILL status     # show Xvfb + app PIDs and readiness
bash $SKILL stop       # kill app + vite (leaves Xvfb up for the next run)
```

## Notes / gotchas

- `libEGL warning: DRI3 ...` lines in the log are harmless (software rendering).
- The app's data comes from `data/registry.json` + `data/agents.json`. With no
  `~/.arca/` user override present, those committed defaults are what render.
- Headless launch needs `WEBKIT_DISABLE_COMPOSITING_MODE=1` and
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` (the script sets these) or the window stays
  blank.
- Override the display/size with env vars: `DISPLAY=:98 SCREEN=1440x900x24 bash $SKILL start`.
