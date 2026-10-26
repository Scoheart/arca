import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const { run, logError } = createRequire(import.meta.url)("@tauri-apps/cli");
const args = process.argv.slice(2);
if (args[0] === "dev") {
  args.splice(1, 0, "--config", fileURLToPath(new URL("../src-tauri/tauri.dev.conf.json", import.meta.url)));
  if (process.platform === "darwin") {
    const targetOption = args.findIndex((arg) => arg === "--target" || arg === "-t");
    const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)?.[1];
    const target = targetOption >= 0 ? args[targetOption + 1] : host;
    const runner = JSON.stringify(["/usr/bin/swift", fileURLToPath(new URL("./run-dev-app.swift", import.meta.url))]);
    const separator = args.indexOf("--");
    if (separator >= 0) args.splice(separator + 1, 0, "--config", `target.${target}.runner=${runner}`);
    else args.push("--", "--config", `target.${target}.runner=${runner}`);
  }
}

run(args, "npm run tauri").catch((error) => {
  logError(error.message);
  process.exitCode = 1;
});
