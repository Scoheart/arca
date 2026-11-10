import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { AgentResourcePanel, type AgentResourceTab } from "./AgentResourcePanel";

afterEach(cleanup);

function Harness() {
  const [tab, setTab] = useState<AgentResourceTab>("mcps");
  const content: Record<AgentResourceTab, string> = {
    mcps: "MCP content",
    models: "Model content",
    skills: "Skill content",
    subagents: "Subagent content",
    hooks: "Hook content",
    instructions: "Native instruction files",
    plugins: "Plugin content",
  };
  return (
    <AgentResourcePanel
      value={tab}
      onChange={setTab}
      counts={{ mcps: 2, models: 3, skills: 4, subagents: 1, hooks: 5 }}
    >
      {content[tab]}
    </AgentResourcePanel>
  );
}

describe("AgentResourcePanel", () => {
  it("keeps Models, MCPs, Skills, Subagents, Hooks in a vertical navigation with counts", () => {
    render(<Harness />);
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Models3",
      "MCPs2",
      "Skills4",
      "Subagents1",
      "Hooks5",
    ]);
    expect(screen.getByRole("tablist")).toHaveAttribute("aria-orientation", "vertical");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("MCP content");
  });

  it("supports roving focus and panel linkage", () => {
    render(<Harness />);
    const tabs = screen.getAllByRole("tab");
    fireEvent.keyDown(tabs[0], { key: "End" });
    expect(tabs[4]).toHaveFocus();
    expect(tabs[4]).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Hook content");
    expect(tabs[4]).toHaveAttribute("aria-controls", screen.getByRole("tabpanel").id);
    fireEvent.keyDown(tabs[4], { key: "ArrowUp" });
    expect(tabs[3]).toHaveFocus();
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Subagent content");
    fireEvent.keyDown(tabs[3], { key: "ArrowDown" });
    expect(tabs[4]).toHaveFocus();
  });
});
