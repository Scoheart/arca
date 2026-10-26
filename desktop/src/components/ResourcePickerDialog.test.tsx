import { cleanup, fireEvent, screen } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ResourcePickerDialog } from "./ResourcePickerDialog";

afterEach(cleanup);

const options = [
  { id: "fs", name: "Filesystem", description: "Local files" },
  { id: "web", name: "Web Search", description: "Remote search" },
];

describe("ResourcePickerDialog", () => {
  it("filters, selects, and adds explicitly", () => {
    const onAdd = vi.fn();
    render(<ResourcePickerDialog title="添加 MCP" options={options} onAdd={onAdd} onClose={() => undefined} />);
    const add = screen.getByRole("button", { name: "添加" });
    expect(add).toBeDisabled();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "web" } });
    expect(screen.queryByRole("option", { name: /Filesystem/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: /Web Search/ }));
    fireEvent.click(add);
    expect(onAdd).toHaveBeenCalledWith(options[1]);
  });

  it("shows a no-match state and result count", () => {
    render(<ResourcePickerDialog title="添加 MCP" options={options} onAdd={() => undefined} onClose={() => undefined} />);
    expect(screen.getByText("2 个可选项")).toBeVisible();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "missing" } });
    expect(screen.getByText("没有匹配项")).toBeVisible();
    expect(screen.getByText("0 个可选项")).toBeVisible();
  });
});
