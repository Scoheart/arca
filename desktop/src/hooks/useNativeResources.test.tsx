import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { useNativeResources } from "./useNativeResources";
import type { NativeResourceInventory } from "../lib/types";

const mocks = vi.hoisted(() => ({ read: vi.fn(), listen: vi.fn().mockResolvedValue(() => undefined) }));
vi.mock("../lib/api", () => ({ listNativeResources: mocks.read }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const inventory = (id: string): NativeResourceInventory => ({ native_plugin_agents: [id], native_plugins: [], host_mcps: [], native_skills: [], unavailable: false });

it("reads host metadata independently and discards a previous Agent's late response", async () => {
  let finishFirst!: (value: NativeResourceInventory) => void;
  mocks.read.mockImplementation((id: string) => id === "cursor" ? new Promise(resolve => { finishFirst = resolve; }) : Promise.resolve(inventory(id)));
  const { result, rerender } = renderHook(({ id }) => useNativeResources(id), { initialProps: { id: "cursor" } });
  await waitFor(() => expect(mocks.read).toHaveBeenCalledWith("cursor"));
  rerender({ id: "other" });
  await waitFor(() => expect(result.current.inventory?.native_plugin_agents).toEqual(["other"]));
  await act(async () => finishFirst(inventory("cursor")));
  expect(result.current.inventory?.native_plugin_agents).toEqual(["other"]);
});
