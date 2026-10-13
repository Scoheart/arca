import { readLocalSetting, writeLocalSetting } from "./localSettings";
/** Light/dark theme handling. The dark palette lives under `.dark` on <html>
 *  (wired via the `@custom-variant dark` in index.css); we toggle that class. */
export type Theme = "light" | "dark";

const KEY = "arca-theme";

/** Saved choice, else follow the OS preference. */
export function getInitialTheme(): Theme {
  const saved = readLocalSetting(KEY);
  if (saved === "light" || saved === "dark") return saved;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Apply + persist a theme. */
export function applyTheme(theme: Theme): void {
  document.documentElement.classList.toggle("dark", theme === "dark");
  writeLocalSetting(KEY, theme);
}
