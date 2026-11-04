import { expect, it } from "vitest";
import type { AgentInfo } from "./types.ts";
import {
  MAX_PINNED_AGENTS,
  buildAgentPickerSections,
  movePinnedAgentAfter,
  movePinnedAgentBefore,
  movePinnedAgentBy,
  previewPinnedAgentOrder,
  projectedPinnedAgentOffset,
  togglePinnedAgent,
  type PinnedDropPlacement,
} from "./pinnedAgents.ts";

function agent(id: string, name: string, hasGlobal = true): AgentInfo {
  return {
    id,
    name,
    format: "json",
    key: "mcpServers",
    has_global: hasGlobal,
    has_project: false,
    enabled: true,
    supported_transports: ["stdio", "http"],
    global: hasGlobal ? `~/.${id}/settings.json` : null,
    project: null,
    skills_global_dir: null,
    docs: null,
    note: null,
    category: "coding",
    evidence: "official",
    verified_at: null,
    builtin: true,
  };
}

const agents = [
  agent("codex", "Codex"),
  agent("claude-code", "Claude Code"),
  agent("qoder", "Qoder CLI"),
  agent("freebuff", "Catalog Only", false),
];

it("sections preserve pinned order and include every Agent definition", () => {
  const sections = buildAgentPickerSections(
    agents,
    ["qoder", "missing", "codex", "qoder"],
    "",
  );
  expect(sections.pinned.map(({ id }) => id)).toEqual(["qoder", "codex"]);
  expect(sections.available.map(({ id }) => id)).toEqual(["freebuff", "claude-code"]);
  expect(sections.searchResults).toBeNull();
});

it("search merges pinned and available matches without duplicates", () => {
  const sections = buildAgentPickerSections(agents, ["qoder"], "code");
  expect(sections.searchResults?.map(({ id }) => id)).toEqual(["claude-code", "codex"]);
});

it("search exposes read-only catalog entries such as Cline Desktop and Freebuff", () => {
  const entries = [
    agent("cline-desktop", "Cline Desktop", false),
    agent("freebuff", "Freebuff", false),
  ];
  expect(buildAgentPickerSections(entries, [], "").available.map(({ id }) => id)).toEqual([
    "cline-desktop",
    "freebuff",
  ]);
  expect(buildAgentPickerSections(entries, [], "freebuff").searchResults?.map(({ id }) => id))
    .toEqual(["freebuff"]);
});

it("includes Skills-only and catalog-only Agent definitions", () => {
  const skillsOnly = [
    { ...agent("cortex-code", "Cortex Code", false), skills_global_dir: "~/.snowflake/cortex/skills" },
    { ...agent("dirac", "Dirac", false), skills_global_dir: "~/.agents/skills" },
    { ...agent("minion-code", "Minion Code", false), skills_global_dir: "~/.minion/skills" },
  ];
  const blankSkillsPath = {
    ...agent("blank-skills", "Blank Skills", false),
    builtin: false,
    skills_global_dir: "   ",
  };
  const sections = buildAgentPickerSections(
    [...agents, ...skillsOnly, blankSkillsPath],
    ["cortex-code", "dirac", "minion-code"],
    "",
  );

  expect(sections.pinned.map(({ id }) => id)).toEqual(["cortex-code", "dirac", "minion-code"]);
  expect(sections.available.map(({ id }) => id)).toContain("freebuff");
  expect(sections.available.map(({ id }) => id)).toContain("blank-skills");
});

it("toggle removes existing pins, appends new pins, and enforces the limit", () => {
  const removed = ["codex", "qoder"];
  const appended = ["codex"];
  expect(togglePinnedAgent(removed, "codex")).toEqual(["qoder"]);
  expect(togglePinnedAgent(appended, "qoder")).toEqual(["codex", "qoder"]);
  expect(removed).toEqual(["codex", "qoder"]);
  expect(appended).toEqual(["codex"]);
  const full = Array.from({ length: MAX_PINNED_AGENTS }, (_, index) => `agent-${index}`);
  expect(togglePinnedAgent(full, "overflow")).toEqual(full);
});

it("keyboard and drag ordering are stable at boundaries", () => {
  const ids = ["claude-code", "codex", "qoder"];
  expect(movePinnedAgentBy(ids, "codex", -1)).toEqual(["codex", "claude-code", "qoder"]);
  expect(movePinnedAgentBy(ids, "claude-code", -1)).toEqual(ids);
  expect(movePinnedAgentBefore(ids, "qoder", "claude-code")).toEqual(["qoder", "claude-code", "codex"]);
  expect(movePinnedAgentBefore(ids, "codex", "codex")).toEqual(ids);
});

it("keyboard ordering moves down and preserves the lower boundary", () => {
  const ids = ["claude-code", "codex", "qoder"];
  const snapshot = [...ids];

  expect(movePinnedAgentBy(ids, "codex", 1)).toEqual(["claude-code", "qoder", "codex"]);
  expect(movePinnedAgentBy(ids, "qoder", 1)).toEqual(ids);
  expect(movePinnedAgentBy(ids, "missing", 1)).toEqual(ids);
  expect(ids).toEqual(snapshot);
});

it("drag ordering is stable for unknown source and target IDs", () => {
  const ids = ["claude-code", "codex", "qoder"];
  const snapshot = [...ids];

  expect(movePinnedAgentBefore(ids, "missing", "codex")).toEqual(ids);
  expect(movePinnedAgentBefore(ids, "codex", "missing")).toEqual(ids);
  expect(ids).toEqual(snapshot);
});

it("move-after places the first pinned Agent after the final row", () => {
  const ids = ["claude-code", "codex", "qoder"];

  expect(movePinnedAgentAfter(ids, "claude-code", "qoder")).toEqual([
    "codex",
    "qoder",
    "claude-code",
  ]);
  expect(ids).toEqual(["claude-code", "codex", "qoder"]);
});

it("move-after preserves order for invalid source, target, and self moves", () => {
  const ids = ["claude-code", "codex", "qoder"];

  expect(movePinnedAgentAfter(ids, "missing", "codex")).toEqual(ids);
  expect(movePinnedAgentAfter(ids, "codex", "missing")).toEqual(ids);
  expect(movePinnedAgentAfter(ids, "codex", "codex")).toEqual(ids);
  expect(ids).toEqual(["claude-code", "codex", "qoder"]);
});

it("reorder operations preserve original input arrays", () => {
  const keyboardIds = ["claude-code", "codex", "qoder"];
  const dragIds = ["claude-code", "codex", "qoder"];
  const keyboardSnapshot = [...keyboardIds];
  const dragSnapshot = [...dragIds];

  expect(movePinnedAgentBy(keyboardIds, "codex", 1)).toEqual(["claude-code", "qoder", "codex"]);
  expect(movePinnedAgentBefore(dragIds, "qoder", "claude-code")).toEqual(["qoder", "claude-code", "codex"]);
  expect(keyboardIds).toEqual(keyboardSnapshot);
  expect(dragIds).toEqual(dragSnapshot);
});

it("drag preview follows before and after targets across multiple rows", () => {
  const ids = ["claude-code", "codex", "qoder", "pi"];
  const first = previewPinnedAgentOrder(ids, "qoder", "claude-code", "before");
  const second = previewPinnedAgentOrder(first, "qoder", "codex", "after");

  expect(first).toEqual(["qoder", "claude-code", "codex", "pi"]);
  expect(second).toEqual(["claude-code", "codex", "qoder", "pi"]);
  expect(ids).toEqual(["claude-code", "codex", "qoder", "pi"]);
});

it("projects visual offsets without changing the committed DOM order", () => {
  const committed = ["claude-code", "codex", "qoder", "pi"];
  const projected = previewPinnedAgentOrder(
    committed,
    "qoder",
    "claude-code",
    "before",
  );

  expect(projectedPinnedAgentOffset(committed, projected, "qoder")).toBe(-2);
  expect(projectedPinnedAgentOffset(committed, projected, "claude-code")).toBe(1);
  expect(projectedPinnedAgentOffset(committed, projected, "codex")).toBe(1);
  expect(projectedPinnedAgentOffset(committed, projected, "pi")).toBe(0);
  expect(projectedPinnedAgentOffset(committed, projected, "missing")).toBe(0);
  expect(committed).toEqual(["claude-code", "codex", "qoder", "pi"]);
});

it("recalculates every hover projection from the committed order", () => {
  const committed = ["claude-code", "codex", "qoder", "pi"];

  expect(
    previewPinnedAgentOrder(committed, "qoder", "claude-code", "before"),
  ).toEqual(["qoder", "claude-code", "codex", "pi"]);
  expect(
    previewPinnedAgentOrder(committed, "qoder", "pi", "after"),
  ).toEqual(["claude-code", "codex", "pi", "qoder"]);
  expect(committed).toEqual(["claude-code", "codex", "qoder", "pi"]);
});

it("drag preview preserves order for invalid and self targets", () => {
  const ids = ["claude-code", "codex", "qoder"];
  const placements: PinnedDropPlacement[] = ["before", "after"];

  for (const placement of placements) {
    expect(previewPinnedAgentOrder(ids, "codex", "codex", placement)).toEqual(ids);
    expect(previewPinnedAgentOrder(ids, "missing", "codex", placement)).toEqual(ids);
    expect(previewPinnedAgentOrder(ids, "codex", "missing", placement)).toEqual(ids);
  }
});
