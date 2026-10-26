import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { AgentResourcePanel, type AgentResourceTab } from "./AgentResourcePanel";

afterEach(cleanup);

function Harness() {
  const [tab, setTab] = useState<AgentResourceTab>("mcps");
  const content = { mcps: "MCP content", models: "Model content", skills: "Skill content", instructions: "Native instruction files" };
  return (
    <AgentResourcePanel
      value={tab}
      onChange={setTab}
      counts={{ mcps: 2, models: 3, skills: 4 }}
    >
      {content[tab]}
    </AgentResourcePanel>
  );
}

describe("AgentResourcePanel", () => {
  it("keeps Models, MCPs, Skills in a vertical navigation with counts", () => {
    render(<Harness />);
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["Models3", "MCPs2", "Skills4"]);
    expect(screen.getByRole("tablist")).toHaveAttribute("aria-orientation", "vertical");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("MCP content");
  });

  it("supports roving focus and panel linkage", () => {
    render(<Harness />);
    const tabs = screen.getAllByRole("tab");
    fireEvent.keyDown(tabs[0], { key: "End" });
    expect(tabs[2]).toHaveFocus();
    expect(tabs[2]).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Skill content");
    expect(tabs[2]).toHaveAttribute("aria-controls", screen.getByRole("tabpanel").id);
    fireEvent.keyDown(tabs[2], { key: "ArrowUp" });
    expect(tabs[1]).toHaveFocus();
    expect(screen.getByRole("tabpanel")).toHaveTextContent("MCP content");
    fireEvent.keyDown(tabs[1], { key: "ArrowDown" });
    expect(tabs[2]).toHaveFocus();
  });
});
