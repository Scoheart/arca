import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

const relativeFile = (path: string) => new URL(path, import.meta.url);
const css = await readFile(relativeFile("../index.css"), "utf8");
const layout = await readFile(relativeFile("../components/Layout.tsx"), "utf8");

function declarations(source: string, selector: string): string | null {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return source.match(new RegExp(`${escaped}\\s*\\{([^{}]*)\\}`))?.[1] ?? null;
}

function mediaBlocks(source: string, heading: string): string[] {
  const blocks: string[] = [];
  let searchFrom = 0;

  while (true) {
    const start = source.indexOf(heading, searchFrom);
    if (start === -1) return blocks;
    const openingBrace = source.indexOf("{", start + heading.length);
    if (openingBrace === -1) return blocks;

    let depth = 0;
    let end = openingBrace;
    for (; end < source.length; end += 1) {
      if (source[end] === "{") depth += 1;
      if (source[end] === "}") depth -= 1;
      if (depth === 0) break;
    }
    blocks.push(source.slice(start, end + 1));
    searchFrom = end + 1;
  }
}

it("uses a refresh glyph for the checking state instead of spinning the download arrow", () => {
  expect(layout).toMatch(/className="arca-update-check-icon"/);
  expect(layout).toMatch(/data-busy=\{checkingUpdate \? "true" : undefined\}/);
  expect(layout).toMatch(/checkingUpdate\s*\? <RefreshIcon/);
  expect(layout).toMatch(/: <DownloadIcon/);
  expect(layout).not.toMatch(/<DownloadIcon[\s\S]{0,120}animation:/);
});

it("centers the update spinner and disables motion when requested", () => {
  expect(declarations(css, ".arca-update-check-icon")).toMatch(/transform-origin:\s*center/);
  expect(declarations(css, '.arca-update-check-icon[data-busy="true"]')).toMatch(
    /animation:\s*spin\s+\.8s\s+linear\s+infinite/,
  );

  const reducedMotion = mediaBlocks(css, "@media (prefers-reduced-motion: reduce)")
    .map((block) => declarations(block, '.arca-update-check-icon[data-busy="true"]'))
    .find((rule) => rule !== null);
  expect(reducedMotion).toMatch(/animation:\s*none/);
});
