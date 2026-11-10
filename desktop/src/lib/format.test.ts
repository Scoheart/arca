import { expect, it } from "vitest";
import { formatError } from "./format";

it("formats structured command errors instead of showing object coercion", () => {
  expect(formatError({ message: "asset operation failed" })).toBe(
    "asset operation failed",
  );
  expect(formatError({ error: { detail: "rolled back" } })).toBe("rolled back");
  expect(formatError({ code: "unknown", retryable: false })).toBe(
    '{"code":"unknown","retryable":false}',
  );
});

it("cleans raw system launcher exit status errors into readable messages", () => {
  expect(
    formatError('Launcher "/usr/bin/open" "/path/to/file" "-a" "/Applications/App.app" failed with ExitStatus(unix_wait_status(256))')
  ).toBe("指定应用无法打开该文件或目录。");
});
