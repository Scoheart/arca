import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AgentInfo } from "../lib/types";
import {
  assetOperationPlanFixture,
  consumptionInventoryFixture,
} from "../test/consumptionFixtures";
import { AgentConfigurationDialog } from "./AgentConfigurationDialog";

const apiMocks = vi.hoisted(() => ({
  planOperation: vi.fn(),
  commitOperation: vi.fn(),
  cancelOperation: vi.fn(),
}));

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    planOperation: apiMocks.planOperation,
    commitOperation: apiMocks.commitOperation,
    cancelOperation: apiMocks.cancelOperation,
  };
});

beforeEach(() => {
  apiMocks.planOperation.mockResolvedValue({
    domain: "asset",
    plan: assetOperationPlanFixture(),
  });
  apiMocks.commitOperation.mockResolvedValue({
    domain: "asset",
    inventory: consumptionInventoryFixture(),
    converged: true,
  });
  apiMocks.cancelOperation.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const amp: AgentInfo = {
  id: "amp",
  name: "Amp",
  format: "json",
  key: "amp.mcpServers",
  has_global: true,
  has_project: false,
  enabled: true,
  supported_transports: ["stdio", "http"],
  global: "~/.config/amp/settings.json",
  project: null,
  skills_global_dir: null,
  docs: null,
  note: null,
  category: "coding",
  evidence: "official",
  verified_at: null,
  builtin: true,
};

it("edits and submits the MCP path and configuration key together", async () => {
  render(
    <AgentConfigurationDialog
      agent={amp}
      modelAgent={null}
      onClose={vi.fn()}
      onSaved={vi.fn()}
    />,
  );

  expect(screen.getByLabelText("MCP 文件路径")).toHaveValue("~/.config/amp/settings.json");
  const keyInput = screen.getByLabelText("MCP 配置键");
  expect(keyInput).toHaveValue("amp.mcpServers");

  await userEvent.clear(keyInput);
  expect(screen.getByRole("button", { name: "继续" })).toBeDisabled();
  await userEvent.type(keyInput, "  custom.mcpServers  ");
  await userEvent.click(screen.getByRole("button", { name: "继续" }));

  await waitFor(() => {
    expect(apiMocks.planOperation).toHaveBeenCalledWith({
      operation: "update_agent_capabilities",
      request: {
        agent_id: "amp",
        patch: {
          mcp: {
            path: "~/.config/amp/settings.json",
            key: "custom.mcpServers",
          },
        },
      },
    });
  });
});

it("adds and removes multiple Skills compatibility directories", async () => {
  const codex: AgentInfo = {
    ...amp,
    id: "codex",
    name: "Codex",
    global: "~/.codex/config.toml",
    skills_global_dir: "~/.agents/skills",
    skills_global_dirs: ["~/.agents/skills", "~/.claude/skills"],
  };
  render(
    <AgentConfigurationDialog
      agent={codex}
      modelAgent={null}
      onClose={vi.fn()}
      onSaved={vi.fn()}
    />,
  );

  expect(screen.getByLabelText("Skills 2")).toHaveValue("~/.claude/skills");
  await userEvent.click(screen.getByRole("button", { name: "移除 Skills 目录 2" }));
  expect(screen.queryByLabelText("Skills 2")).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "添加 Skills 目录" }));
  const second = screen.getByLabelText("Skills 2");
  expect(screen.getByRole("button", { name: "继续" })).toBeDisabled();
  await userEvent.type(second, "~/.codex/skills");
  await userEvent.click(screen.getByRole("button", { name: "继续" }));

  await waitFor(() => {
    expect(apiMocks.planOperation).toHaveBeenCalledWith({
      operation: "update_agent_capabilities",
      request: {
        agent_id: "codex",
        patch: {
          mcp: {
            path: "~/.codex/config.toml",
            key: "amp.mcpServers",
          },
          skill: {
            global_dir: "~/.agents/skills",
            alias_dirs: ["~/.codex/skills"],
          },
        },
      },
    });
  });
});

it("submits a Model-only Agent without inventing MCP configuration", async () => {
  const grok: AgentInfo = {
    ...amp,
    id: "grok-build",
    name: "Grok Build",
    has_global: false,
    global: null,
  };
  render(
    <AgentConfigurationDialog
      agent={grok}
      modelAgent={{
        id: "grok-build",
        name: "Grok Build",
        mode: "managed",
        storage_authority: "native-registry",
        installed: true,
        config_path: "~/.grok-build/config.json",
        config_paths: ["~/.grok-build/config.json"],
        docs: "https://example.invalid",
        assigned_profile: null,
        assigned_profiles: [],
        active_profile: null,
        supports_multiple: false,
        credential_mode: "guided",
        supported_protocols: ["openai-responses"],
        note: "",
      }}
      onClose={vi.fn()}
      onSaved={vi.fn()}
    />,
  );

  expect(screen.getByLabelText("MCP")).toBeDisabled();
  const model = screen.getByLabelText("Model");
  fireEvent.change(model, { target: { value: "/tmp/grok-build/custom.json" } });
  await userEvent.click(screen.getByRole("button", { name: "继续" }));

  await waitFor(() => {
    expect(apiMocks.planOperation).toHaveBeenCalledWith({
      operation: "update_agent_capabilities",
      request: {
        agent_id: "grok-build",
        patch: {
          model: { paths: ["/tmp/grok-build/custom.json"] },
        },
      },
    });
  });
});

it("commits the reviewed Agent configuration through the unified asset envelope", async () => {
  const plan = assetOperationPlanFixture();
  const onClose = vi.fn();
  const onSaved = vi.fn();
  render(
    <AgentConfigurationDialog
      agent={amp}
      modelAgent={null}
      onClose={onClose}
      onSaved={onSaved}
    />,
  );

  await userEvent.click(screen.getByRole("button", { name: "继续" }));
  await userEvent.click(await screen.findByRole("button", { name: "添加 MCP" }));

  await waitFor(() => {
    expect(apiMocks.commitOperation).toHaveBeenCalledWith({
      domain: "asset",
      request: {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
      },
    });
  });
  expect(onSaved).toHaveBeenCalledOnce();
  expect(onClose).toHaveBeenCalledOnce();
});

it("cancels the reviewed Agent configuration through the unified asset envelope", async () => {
  const plan = assetOperationPlanFixture();
  render(
    <AgentConfigurationDialog
      agent={amp}
      modelAgent={null}
      onClose={vi.fn()}
      onSaved={vi.fn()}
    />,
  );

  await userEvent.click(screen.getByRole("button", { name: "继续" }));
  await userEvent.click(await screen.findByRole("button", { name: "返回编辑" }));

  await waitFor(() => {
    expect(apiMocks.cancelOperation).toHaveBeenCalledWith({
      domain: "asset",
      operation_id: plan.operation_id,
    });
  });
});
