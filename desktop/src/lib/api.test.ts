import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  listModelProfiles,
  revealModelProviderCredential,
  setAgentCredentialDelivery,
  setModelCredentialDelivery,
  validateModelCredentialSource,
} from "./api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

beforeEach(() => {
  invokeMock.mockReset();
});

describe("Model Profile wire contract", () => {
  it("restores empty strings omitted by compact Rust serialization", async () => {
    invokeMock.mockResolvedValueOnce([{
      id: "legacy-profile",
      name: "Legacy Profile",
      protocol: "anthropic-messages",
      model: "vendor/model",
      catalog_key: "/vendor/model",
      credential_saved: false,
    }]);

    await expect(listModelProfiles()).resolves.toEqual([{
      id: "legacy-profile",
      name: "Legacy Profile",
      provider: "",
      protocol: "anthropic-messages",
      base_url: "",
      model: "vendor/model",
      catalog_key: "/vendor/model",
      credential_saved: false,
    }]);
    expect(invokeMock).toHaveBeenCalledWith("list_model_profiles", undefined);
  });

  it("uses a dedicated command to reveal one Provider credential", async () => {
    invokeMock.mockResolvedValueOnce("saved-test-value");

    await expect(revealModelProviderCredential("team-provider"))
      .resolves.toBe("saved-test-value");
    expect(invokeMock).toHaveBeenCalledWith(
      "reveal_model_provider_credential",
      { providerId: "team-provider" },
    );
  });

  it("validates source metadata without putting a secret in the request", async () => {
    invokeMock.mockResolvedValueOnce({
      source_kind: "file",
      source_identity: "/private/provider.key",
      message: "valid",
    });
    await validateModelCredentialSource({ kind: "file", path: "/private/provider.key" });
    expect(invokeMock).toHaveBeenCalledWith("validate_model_credential_source", {
      source: { kind: "file", path: "/private/provider.key" },
    });
  });

  it("updates one Agent-specific credential delivery policy", async () => {
    invokeMock.mockResolvedValueOnce({ agent: "opencode", profile: "work" });

    await setModelCredentialDelivery("opencode", "work", "plaintext", true);

    expect(invokeMock).toHaveBeenCalledWith("set_model_credential_delivery", {
      agentId: "opencode",
      profileId: "work",
      delivery: "plaintext",
      confirmPlaintext: true,
    });
  });

  it("updates the Agent default credential delivery", async () => {
    invokeMock.mockResolvedValueOnce({ agent: "opencode", profile: "" });

    await setAgentCredentialDelivery("opencode", "env", false);

    expect(invokeMock).toHaveBeenCalledWith("set_agent_credential_delivery", {
      agentId: "opencode",
      delivery: "env",
      confirmPlaintext: false,
    });
  });

});
