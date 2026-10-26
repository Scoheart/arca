import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

const relativeFile = (path: string) => new URL(path, import.meta.url);
const css = await readFile(relativeFile("../index.css"), "utf8");
const skillsView = await readFile(
  relativeFile("../components/SkillsView.tsx"),
  "utf8",
);

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

it("scopes the Skills busy spinner and disables it for reduced motion", () => {
  expect(skillsView).toMatch(/className="arca-skill-check-icon/);
  expect(skillsView).toMatch(/data-busy=\{checking \? "true" : undefined\}/);
  expect(skillsView).not.toMatch(
    /style=\{checking \? \{ animation: "spin 0\.8s linear infinite" \}/,
  );

  const busy = declarations(
    css,
    '.arca-skill-check-icon[data-busy="true"]',
  );
  expect(busy).toMatch(/animation:\s*spin\s+0\.8s\s+linear\s+infinite/);

  const reducedMotionBusy = mediaBlocks(
    css,
    "@media (prefers-reduced-motion: reduce)",
  )
    .map((block) =>
      declarations(block, '.arca-skill-check-icon[data-busy="true"]'),
    )
    .find((rule) => rule !== null);
  expect(reducedMotionBusy).toMatch(/animation:\s*none/);
});

it("uses one shared Soft Workbench geometry for every resource domain", () => {
  expect(skillsView).toMatch(/className="arca-skill-workspace"/);

  const sharedSidebar = declarations(css, ".arca-workspace-sidebar");
  const sharedToolbar = declarations(css, ".arca-workspace-toolbar");
  const sharedFilters = declarations(css, ".arca-workspace-filters");
  const fullWidthWorkspace = declarations(
    css,
    '.arca-workspace[data-sidebar="false"]',
  );
  expect(sharedSidebar).toMatch(/border:\s*0/);
  expect(sharedSidebar).toMatch(/border-radius:\s*var\(--radius-canvas\)/);
  expect(sharedToolbar).toMatch(/border:\s*0/);
  expect(sharedToolbar).toMatch(/border-radius:\s*var\(--radius-row\)/);
  expect(sharedFilters).toMatch(/border:\s*0/);
  expect(sharedFilters).toMatch(/border-radius:\s*var\(--radius-row\)/);
  expect(fullWidthWorkspace).toMatch(/grid-template-columns:\s*minmax\(0,\s*1fr\)/);
  expect(fullWidthWorkspace).toMatch(/gap:\s*0/);

  expect(css).not.toMatch(/\.arca-skill-workspace\s+\.arca-workspace-(?:sidebar|toolbar|scroll)/);
  expect(css).not.toMatch(/\.arca-skill-workspace\s+\.arca-resource-(?:grid|tabs|tab)/);
  expect(sharedToolbar).toMatch(/min-height:\s*52px/);
  expect(sharedFilters).toMatch(/height:\s*42px/);

  const content = declarations(css, ".arca-workspace-content");
  const grid = declarations(css, ".arca-resource-grid");
  expect(content).toMatch(/container:\s*resource-stage\s*\/\s*inline-size/);
  expect(grid).toMatch(/grid-template-columns:\s*repeat\(3,\s*minmax\(0,\s*1fr\)\)/);
  expect(grid).toMatch(/gap:\s*8px/);
  expect(grid).toMatch(/padding:\s*6px/);
  expect(grid).toMatch(/border:\s*0/);
  expect(grid).toMatch(/border-radius:\s*var\(--radius-region\)/);
  expect(grid).toMatch(/background:\s*var\(--surface-section\)/);

  const mediumContainerGrid = mediaBlocks(
    css,
    "@container resource-stage (max-width: 760px)",
  )
    .map((block) => declarations(block, ".arca-resource-grid"))
    .find((rule) => rule !== null);
  expect(mediumContainerGrid).toMatch(/grid-template-columns:\s*repeat\(2,\s*minmax\(0,\s*1fr\)\)/);

  const narrowContainerGrid = mediaBlocks(
    css,
    "@container resource-stage (max-width: 500px)",
  )
    .map((block) => declarations(block, ".arca-resource-grid"))
    .find((rule) => rule !== null);
  expect(narrowContainerGrid).toMatch(/grid-template-columns:\s*minmax\(0,\s*1fr\)/);
});

it("lays out the three Skill intake sources as compact peers", () => {
  const sources = declarations(css, ".arca-skill-source-step-compact");
  expect(sources).toMatch(
    /grid-template-columns:\s*minmax\(0,\s*2\.2fr\)\s*repeat\(2,\s*minmax\(118px,\s*1fr\)\)/,
  );
  expect(sources).toMatch(/align-items:\s*stretch/);
  expect(sources).toMatch(/gap:\s*8px/);
  expect(css).not.toMatch(/arca-skill-source-divider|arca-skill-local-sources/);
});
