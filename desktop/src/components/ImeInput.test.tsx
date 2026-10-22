import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ImeInput } from "./ImeInput";

afterEach(cleanup);

describe("IME keyboard ownership", () => {
  it("tracks composition even when native flags are missing; next Enter submits immediately", () => {
    const submit = vi.fn();
    render(<ImeInput aria-label="input" onKeyDown={event => { if (event.key === "Enter") submit(); }} />);
    const input = screen.getByRole("textbox");
    for (let i = 0; i < 2; i++) {
      fireEvent.compositionStart(input);
      fireEvent.change(input, { target: { value: "中文" } });
      fireEvent.keyDown(input, { key: "Enter", isComposing: false });
      expect(submit).toHaveBeenCalledTimes(i);
      fireEvent.compositionEnd(input);
      fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
      expect(submit).toHaveBeenCalledTimes(i + 1);
    }
  });

  it("handles WebKit compositionend before keydown 229 without swallowing the next Enter", () => {
    const action = vi.fn();
    render(<ImeInput onKeyDown={action} />);
    const input = screen.getByRole("textbox");
    fireEvent.compositionStart(input);
    fireEvent.compositionEnd(input);
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229, isComposing: false });
    expect(action).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    expect(action).toHaveBeenCalledTimes(1);
  });

  it("honors isComposing independently and keeps English, shortcuts and cancelled composition usable", () => {
    const action = vi.fn(); const bubble = vi.fn();
    render(<div onKeyDown={bubble}><ImeInput onKeyDown={action} /></div>);
    const input = screen.getByRole("textbox");
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    fireEvent.compositionStart(input);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(action).not.toHaveBeenCalled();
    expect(bubble).not.toHaveBeenCalled();
    fireEvent.compositionEnd(input, { data: "" });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "k", metaKey: true });
    expect(action).toHaveBeenCalledTimes(2);
    expect(bubble).toHaveBeenCalledTimes(2);
  });

  it("resets on blur and prevents implicit form submission only for an IME Enter", () => {
    const action = vi.fn();
    render(<form><ImeInput onKeyDown={action} /><button>Save</button></form>);
    const input = screen.getByRole("textbox");
    fireEvent.compositionStart(input);
    expect(fireEvent.keyDown(input, { key: "Enter" })).toBe(false);
    fireEvent.blur(input);
    expect(fireEvent.keyDown(input, { key: "Enter" })).toBe(true);
    expect(action).toHaveBeenCalledTimes(1);
  });
});
