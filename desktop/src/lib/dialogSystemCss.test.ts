import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

const css = await readFile(new URL("../index.css", import.meta.url), "utf8");

it("keeps dialog geometry shared and inspectors content-driven", () => {
  expect(css).toMatch(/--arca-dialog-header-height:\s*56px/);
  expect(css).toMatch(/\.arca-workspace-inspector-surface\s*\{[^}]*max-height:\s*inherit/);
  expect(css).not.toMatch(/\.arca-workspace-inspector-surface\s*\{[^}]*height:\s*min\(680px/);
});

it("does not style dialog shells by descendant feature detection", () => {
  expect(css).not.toMatch(/\.arca-dialog-shell:has\(\.arca-provider-(catalog|form)\)/);
  expect(css).not.toMatch(/\.arca-dialog-shell:has\(\.arca-agent-create\)/);
});

it("keeps ordinary inspector groups and picker rows flat", () => {
  expect(css).toMatch(/\.arca-inspector-section\s*\{[^}]*background:\s*transparent/);
  expect(css).toMatch(/\.arca-picker-option\s*\{[^}]*border-bottom:/);
  expect(css).toMatch(/\.arca-picker-option\s*\{[^}]*background:\s*transparent/);
  expect(css).toMatch(/\.arca-model-form,[\s\S]*?\.arca-mcp-form\s*\{[^}]*padding:\s*0;[^}]*background:\s*transparent/);
  expect(css).toMatch(/\.arca-asset-review > section\s*\{[^}]*border-bottom:[^}]*background:\s*transparent/);
});

it("keeps destructive reviews on the shared compact frame", () => {
  expect(css).toMatch(/\.arca-review-dialog-danger \.arca-dialog-shell-header\s*\{[^}]*min-height:\s*var\(--arca-dialog-header-height\)/);
  expect(css).toMatch(/\.arca-review-dialog-danger \.arca-dialog-shell-footer\s*\{[^}]*min-height:\s*var\(--arca-dialog-footer-height\)/);
});
