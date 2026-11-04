import { beforeEach, describe, expect, it, vi } from "vitest";
import { openLocationWithFallback } from "./FileEditorSelect";

const openerMocks = vi.hoisted(() => ({
  openPath: vi.fn(),
}));

const coreMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

const pathMocks = vi.hoisted(() => ({
  homeDir: vi.fn().mockResolvedValue("/Users/test"),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: openerMocks.openPath,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: coreMocks.invoke,
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: pathMocks.homeDir,
}));

describe("openLocationWithFallback", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    openerMocks.openPath.mockResolvedValue(undefined);
    coreMocks.invoke.mockResolvedValue(undefined);
    pathMocks.homeDir.mockResolvedValue("/Users/test");
  });

  it("resolves ~ and opens folder with system default launcher", async () => {
    await openLocationWithFallback("~/.pi/agent/skills", "folder");
    expect(coreMocks.invoke).toHaveBeenCalledWith("ensure_file_path", {
      path: "/Users/test/.pi/agent/skills",
      kind: "folder",
    });
    expect(openerMocks.openPath).toHaveBeenCalledWith("/Users/test/.pi/agent/skills");
  });

  it("opens file with specified preferred editor when it succeeds", async () => {
    await openLocationWithFallback("~/.pi/agent/mcp.json", "file", "/Applications/Qoder IDE.app");
    expect(coreMocks.invoke).toHaveBeenCalledWith("ensure_file_path", {
      path: "/Users/test/.pi/agent/mcp.json",
      kind: "file",
    });
    expect(openerMocks.openPath).toHaveBeenCalledWith(
      "/Users/test/.pi/agent/mcp.json",
      "/Applications/Qoder IDE.app",
    );
  });

  it("falls back to app name and then system default when editor fails", async () => {
    openerMocks.openPath
      .mockRejectedValueOnce(new Error('Launcher "/usr/bin/open" failed with ExitStatus(unix_wait_status(256))'))
      .mockRejectedValueOnce(new Error('Launcher "/usr/bin/open" failed with ExitStatus(unix_wait_status(256))'))
      .mockResolvedValueOnce(undefined);

    await openLocationWithFallback("~/.pi/agent/mcp.json", "file", "/Applications/Qoder IDE.app");

    expect(openerMocks.openPath).toHaveBeenNthCalledWith(
      1,
      "/Users/test/.pi/agent/mcp.json",
      "/Applications/Qoder IDE.app",
    );
    expect(openerMocks.openPath).toHaveBeenNthCalledWith(
      2,
      "/Users/test/.pi/agent/mcp.json",
      "Qoder IDE",
    );
    expect(openerMocks.openPath).toHaveBeenNthCalledWith(
      3,
      "/Users/test/.pi/agent/mcp.json",
    );
  });

  it("falls back to enclosing parent directory if opening file fails completely", async () => {
    openerMocks.openPath
      .mockRejectedValueOnce(new Error("Editor error"))
      .mockRejectedValueOnce(new Error("Editor name error"))
      .mockRejectedValueOnce(new Error("Default app error"))
      .mockResolvedValueOnce(undefined);

    await openLocationWithFallback("~/.pi/agent/mcp.json", "file", "/Applications/Qoder IDE.app");

    expect(openerMocks.openPath).toHaveBeenLastCalledWith("/Users/test/.pi/agent");
  });
});
