import { expect, it } from "vitest";
import type { AgentLaunchInfo } from "./agentLaunch";
import { createLaunchProfilesDraft, launchProfilesChanged, launchProfilesPayload, launchProfilesValid } from "../components/AgentLaunchProfiles";

const info: AgentLaunchInfo = {
  agent_id: "codex-desktop", name: "ChatGPT", category: "desktop", kind: "app", available: true,
  supported: true, host_name: null, install_url: null, directory: null, directory_exists: false,
  configured_target: { kind: "app", path: "/Applications/ChatGPT.app", env: { HTTP_PROXY: "http://127.0.0.1:7890" } },
  resolved_target: { kind: "app", path: "/Applications/ChatGPT.app" },
};

it("keeps the default empty and retains legacy settings in a separate custom profile", () => {
  const draft = createLaunchProfilesDraft(info);
  expect(draft.activeProfileId).toBe("custom");
  expect(draft.profiles[0]).toMatchObject({ id: "default", name: "默认配置", launch: { app: "", appArgs: "", environment: "", newInstance: false, directory: "" } });
  const payload = launchProfilesPayload(info, draft);
  expect(payload.profiles[0]).toEqual({ id: "default", name: "默认配置", target: null, default_directory: null });
  expect(payload.profiles[1].target).toEqual(info.configured_target);
  expect(launchProfilesValid(draft)).toBe(true);
  expect(launchProfilesChanged(info, draft)).toBe(false);
  draft.activeProfileId = "default";
  expect(launchProfilesChanged(info, draft)).toBe(true);
});

it("validates inactive custom drafts and rejects changes to the immutable default", () => {
  const draft = createLaunchProfilesDraft(info);
  draft.activeProfileId = "default";
  draft.profiles[1].launch.environment = "invalid";
  expect(launchProfilesValid(draft)).toBe(false);
  const modified = createLaunchProfilesDraft(info);
  modified.profiles[0].launch.environment = "HTTP_PROXY=http://127.0.0.1:7890";
  expect(launchProfilesValid(modified)).toBe(false);
  expect(launchProfilesPayload(info, modified).profiles[0].target).toBeNull();
});

it("accepts an empty default even when no runtime is installed", () => {
  const empty = { ...info, configured_target: null, resolved_target: null };
  const draft = createLaunchProfilesDraft(empty);
  expect(draft.activeProfileId).toBe("default");
  expect(launchProfilesValid(draft)).toBe(true);
  expect(launchProfilesPayload(empty, draft).profiles[0].target).toBeNull();
});
