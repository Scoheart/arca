import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

const relativeFile = (path: string) => new URL(path, import.meta.url);

const css = await readFile(relativeFile("../index.css"), "utf8");
const component = await readFile(
  relativeFile("../components/AgentNavigation.tsx"),
  "utf8",
);
const app = await readFile(relativeFile("../App.tsx"), "utf8");
const layout = await readFile(relativeFile("../components/Layout.tsx"), "utf8");
const icons = await readFile(relativeFile("../components/icons.tsx"), "utf8");
const types = await readFile(relativeFile("./types.ts"), "utf8");

function declarations(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = source.match(
    new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^{}]*)\\}`),
  );
  expect(match, `expected ${selector} rule`).toBeTruthy();
  if (!match) throw new Error(`expected ${selector} rule`);
  return match[1];
}

function pixelValue(source: string, property: string): number {
  const match = source.match(new RegExp(`${property}:\\s*(\\d+)px`));
  expect(match, `expected ${property} in ${source}`).toBeTruthy();
  if (!match) throw new Error(`expected ${property}`);
  return Number(match[1]);
}

it("keeps Models, MCPs, Skills order before Agent navigation", () => {
  expect(types).toMatch(
    /\| \{ kind: "skills"; intent\?: SkillNavigationIntent \}/,
  );
  expect(icons).toMatch(/export function SparklesIcon\(/);
  expect(layout).toMatch(/onSelectSkills: \(\) => void/);

  const segmentStart = layout.indexOf('className="arca-seg arca-skill-seg');
  const agentNavigationStart = layout.indexOf("<AgentNavigation");
  expect(segmentStart, "expected the Skills-specific resource segment").not.toBe(-1);
  expect(agentNavigationStart, "expected AgentNavigation to remain separate").toBeGreaterThan(
    segmentStart,
  );

  const segment = layout.slice(segmentStart, agentNavigationStart);
  const labels = ["Models", "MCPs", "Skills"].map((label) => segment.indexOf(label));
  expect(labels.every((index) => index >= 0)).toBe(true);
  expect(labels).toEqual([...labels].sort((left, right) => left - right));
  expect(segment).toMatch(/data-active=\{view\.kind === "skills"/);
  expect(segment).toMatch(/onClick=\{onSelectSkills\}/);
  expect(segment).toMatch(/<SparklesIcon/);
  expect(segment).not.toMatch(/Beta|arca-seg-beta/);
  expect(css).not.toMatch(/arca-seg-beta/);
});

it("routes every workspace without a whole-app MCP loading gate", () => {
  expect(app.match(/\buseSkillsState\(\{ autoLoad: false \}\)/g) ?? []).toHaveLength(1);
  expect(app).toMatch(/onSelectSkills=\{\(\) => setView\(\{ kind: "skills" \}\)\}/);
  expect(app).toMatch(/<SkillsView\s+state=\{skillsState\}/);
  expect(app).toMatch(/intent=\{view\.intent\}/);
  expect(app).toMatch(/onIntentConsumed=\{consumeResourceIntent\}/);
  expect(app).not.toMatch(/state\.loading\s*\?\s*\(/);
  expect(app).toMatch(/startupSync=\{startupSync\}/);
  for (const task of [
    "agents",
    "agent-capabilities",
    "relationships",
    "skills",
    "registry",
    "sources",
  ]) {
    expect(app).toContain(`id: "${task}"`);
  }
  expect(app).not.toMatch(/skillsState\.hydrate\(snapshot\.assets\.skills\)/);
  expect(app).not.toMatch(/startupSync\.refreshTasks/);
  expect(app).toContain("skillsState.refreshSilently");
  expect(app).toContain("taskIdsForObservation(event.payload)");
  expect(app).toContain("focusRefreshDue(lastFocusRefreshAt, now)");
});

it("keeps topbar controls at their wide-layout sizes without compact overrides", () => {
  const topbar = declarations(css, ".arca-topbar");
  expect(pixelValue(topbar, "gap")).toBe(8);
  expect(pixelValue(topbar, "padding-left")).toBe(16);
  expect(pixelValue(declarations(css, ".arca-icon-btn"), "width")).toBe(30);
  expect(pixelValue(declarations(css, ".arca-icon-btn"), "height")).toBe(30);
  const pickerAnchor = declarations(css, ".arca-agent-picker-anchor");
  expect(pixelValue(pickerAnchor, "width")).toBe(220);
  expect(pixelValue(pickerAnchor, "min-width")).toBe(40);
  expect(pickerAnchor).toMatch(/flex:\s*0\s+1\s+220px/);
  expect(pickerAnchor).toMatch(/container-name:\s*arca-agent-picker-anchor/);
  expect(pickerAnchor).toMatch(/container-type:\s*inline-size/);
  expect(declarations(css, ".arca-agent-picker-trigger")).toMatch(/width:\s*100%/);
  expect(component).toMatch(/aria-label=\{selectedAgent\?\.name \?\? "选择 Agent"\}/);
  const pickerCluster = declarations(css, ".arca-agent-picker-cluster");
  expect(pickerCluster).toMatch(/position:\s*relative/);
  expect(pickerCluster).toMatch(/max-width:\s*100%/);
  expect(component).toMatch(
    /className="arca-agent-picker-cluster"[\s\S]*className="arca-pinned-agent-bar"[\s\S]*className="arca-agent-picker-anchor"[\s\S]*className="arca-agent-picker"/,
  );
  const picker = declarations(css, ".arca-agent-picker");
  expect(picker).toMatch(/right:\s*0/);
  expect(picker).toMatch(/left:\s*auto/);
  expect(picker).toMatch(/width:\s*100%/);
  expect(pixelValue(picker, "min-width")).toBe(220);
  expect(pixelValue(declarations(css, ".arca-agent-picker-trigger"), "height")).toBe(40);
  expect(pixelValue(declarations(css, ".arca-pinned-agent"), "width")).toBe(34);
  expect(pixelValue(declarations(css, ".arca-pinned-agent"), "height")).toBe(34);
  expect(pixelValue(declarations(css, ".arca-update-check"), "height")).toBe(32);

  expect(css).not.toMatch(/\.arca-topbar \.arca-(?:wordmark|seg|skill-seg|seg-item|icon-btn|update-check)/);
  expect(css.match(/\.arca-agent-picker-trigger\s*\{/g) ?? []).toHaveLength(2);
  expect(css.match(/\.arca-pinned-agent-glyph\s*\{/g) ?? []).toHaveLength(1);
  expect(css).not.toMatch(/@media \(max-width: (?:980|840)px\)/);
  expect(layout).toMatch(/className="arca-update-check-label"/);
  expect(layout).toMatch(/aria-label=\{version \? t\("layout\.checkUpdateVersion"/);
  expect(layout.match(/className="arca-resource-label"/g) ?? []).toHaveLength(3);
});

it("absorbs narrow widths in whole pinned Agent increments", () => {
  expect(layout).toMatch(/className="arca-topbar-navigation-lane"/);
  const lane = declarations(css, ".arca-topbar-navigation-lane");
  expect(lane).toMatch(/min-width:\s*0/);
  expect(lane).toMatch(/flex:\s*1\s+1\s+auto/);
  expect(lane).toMatch(/container-name:\s*arca-agent-lane/);
  expect(lane).toMatch(/container-type:\s*inline-size/);

  const navigation = declarations(css, ".arca-agent-navigation");
  expect(navigation).toMatch(/width:\s*100%/);
  expect(navigation).toMatch(/min-width:\s*0/);
  const pinned = declarations(css, ".arca-pinned-agent-bar");
  expect(pinned).toMatch(/flex:\s*0\s+0\s+auto/);
  expect(pinned).toMatch(/overflow:\s*hidden/);
  expect(pinned).not.toMatch(/overflow-x:\s*auto/);
  expect(css).not.toMatch(/scroll-snap-(?:align|type)/);
  for (const [laneWidth, firstHidden] of [
    [277, 6],
    [240, 5],
    [203, 4],
    [166, 3],
    [129, 2],
  ]) {
    expect(css).toMatch(
      new RegExp(
        `@container arca-agent-lane \\(max-width: ${laneWidth}px\\)\\s*\\{[\\s\\S]*?` +
        `\\.arca-pinned-agent:nth-child\\(n \\+ ${firstHidden}\\)\\s*\\{\\s*display:\\s*none`,
      ),
    );
  }
  expect(css).toMatch(
    /@container arca-agent-lane \(max-width: 92px\)\s*\{[\s\S]*?\.arca-pinned-agent-bar\s*\{\s*display:\s*none/,
  );
  expect(
    css.match(/@container arca-agent-lane \(max-width: \d+px\)/g) ?? [],
  ).toHaveLength(7);
  expect(css).toMatch(
    /@container arca-agent-lane \(max-width: 333px\)\s*\{[\s\S]*?\.arca-agent-picker-anchor\s*\{[^}]*width:\s*40px[^}]*flex:\s*0\s+0\s+40px/,
  );
  expect(declarations(css, ".arca-agent-picker-anchor")).toMatch(/flex:\s*0\s+1\s+220px/);
  expect(layout).toMatch(
    /picker settles at one icon, then pinned Agents disappear one\s+whole shortcut at a time until their strip has no footprint/,
  );
});

it("collapses the Agent picker to an accessible icon without shrinking its popup", () => {
  expect(css).toMatch(
    /@container arca-agent-picker-anchor \(max-width: 96px\)\s*\{[\s\S]*?\.arca-agent-picker-trigger\s*\{[^}]*justify-content:\s*center[^}]*gap:\s*0[^}]*padding:\s*0/,
  );
  expect(css).toMatch(
    /\.arca-agent-picker-trigger-name,\s*\.arca-agent-picker-chevron\s*\{\s*display:\s*none/,
  );
  const picker = declarations(css, ".arca-agent-picker");
  expect(pixelValue(picker, "min-width")).toBe(220);
  expect(picker).toMatch(/right:\s*0/);
  expect(component).toMatch(/aria-label=\{selectedAgent\?\.name \?\? "选择 Agent"\}/);
});

it("popup action focus rule includes the search clear button", () => {
  const focusRule = Array.from(css.matchAll(/([^{}]+)\{([^{}]*)\}/g)).find(
    ([, selectors, declarations]) =>
      selectors.includes(".arca-agent-picker-select:focus-visible") &&
      declarations.includes("outline:"),
  );

  expect(focusRule, "expected the popup action focus-visible rule").toBeTruthy();
  if (!focusRule) throw new Error("expected the popup action focus-visible rule");
  const selectors = focusRule[1].split(",").map((selector) => selector.trim());
  expect(
    selectors,
    "search clear button must receive the popup action focus-visible treatment",
  ).toContain(".arca-agent-picker-search-clear:focus-visible");
});

it("pinned Agent glyph remains 30px at every viewport width", () => {
  expect(component).toMatch(
    /className="arca-pinned-agent-glyph">\s*<AgentGlyph[^>]*size=\{30\}\s*\/>\s*<\/span>/,
  );

  const baseGlyph = declarations(css, ".arca-pinned-agent-glyph");
  expect(baseGlyph).toMatch(/width:\s*30px/);
  expect(baseGlyph).toMatch(/height:\s*30px/);
  const renderedGlyph = declarations(css, ".arca-pinned-agent-glyph > *");
  expect(renderedGlyph).toMatch(/width:\s*100%\s*!important/);
  expect(renderedGlyph).toMatch(/height:\s*100%\s*!important/);

  expect(css.match(/\.arca-pinned-agent-glyph\s*\{/g) ?? []).toHaveLength(1);
});

it("drag preview keeps DOM order stable and projects only picker rows", () => {
  expect(component).toMatch(/const \[previewIds, setPreviewIds\]/);
  expect(component).toMatch(/const previewIdsRef = useRef/);
  expect(component).not.toMatch(/orderedPinnedAgents/);
  expect(
    component.match(/sections\.pinned\.map\(\(agent\) =>/g) ?? [],
    "top shortcuts and picker rows must keep the committed order during preview",
  ).toHaveLength(2);
  expect(component).toMatch(/previewPinnedAgentOrder/);
  expect(component).toMatch(
    /previewPinnedAgentOrder\(pinnedIds, draggedId, targetId, placement\)/,
  );
  expect(component).toMatch(/projectedPinnedAgentOffset/);
  expect(component).toMatch(/--arca-agent-order-offset/);
  expect(component).toMatch(/setDragImage\(\s*event\.currentTarget/);
  expect(component).toMatch(/className="arca-agent-picker-slot"/);
  expect(component).toMatch(/data-sorting=/);
  expect(component).toMatch(/data-drag-source=/);
  expect(component).toMatch(/data-drop-position=/);
  expect(component).toMatch(/data-settling=\{settling/);
  expect(component).toMatch(/requestAnimationFrame/);

  const slot = declarations(css, ".arca-agent-picker-slot");
  expect(slot).toMatch(/position:\s*relative/);
  const row = declarations(css, ".arca-agent-picker-row");
  expect(row).toMatch(/position:\s*relative/);
  expect(row).toMatch(/transform:\s*translateY\(var\(--arca-agent-order-offset/);
  expect(css).toMatch(
    /\.arca-agent-picker-slot\[data-sorting="true"\]:not\(\[data-drag-source="true"\]\) > \.arca-agent-picker-row\s*\{[^}]*pointer-events:\s*none/,
  );
  expect(css).toMatch(/\.arca-agent-picker-row\[data-drop-position="before"\]::before/);
  expect(css).toMatch(/\.arca-agent-picker-row\[data-drop-position="after"\]::after/);
  expect(css).toMatch(
    /\.arca-agent-picker-list\[data-settling="true"\] \.arca-agent-picker-row\s*\{[^}]*transition:\s*none/,
  );

  const handle = declarations(css, ".arca-agent-order-handle");
  expect(pixelValue(handle, "width")).toBeGreaterThanOrEqual(36);
  expect(pixelValue(handle, "height")).toBeGreaterThanOrEqual(40);
});

it("reduced motion disables drag-preview transitions", () => {
  const reducedStart = css.indexOf("@media (prefers-reduced-motion: reduce)");
  expect(reducedStart).not.toBe(-1);
  expect(css.slice(reducedStart)).toMatch(
    /\.arca-agent-picker-row\s*\{[^}]*transition:\s*none/,
  );
});
