import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConsumptionView } from "../lib/types";
import { AgentConsumptionPanel } from "./AgentConsumptionPanel";

afterEach(cleanup);

const syncedRow: ConsumptionView = {
  agent_id: "codex",
  asset: { domain: "mcp", key: "github::stdio" },
  ownership: "managed",
  desired: true,
  observed: true,
  enabled: true,
  status: "synced",
  reason: null,
  affected_agent_ids: ["codex"],
  available_actions: [],
};

const externalRow: ConsumptionView = {
  ...syncedRow,
  asset: { domain: "mcp", key: "computer-use::stdio" },
  desired: false,
  ownership: "external",
  status: "external-added",
  reason: "mcp_adoptable",
  available_actions: ["adopt-observed"],
};

describe("AgentConsumptionPanel", () => {
  it("offers one master MCP switch without changing external entries", async () => {
    const onBulkEnabledChange = vi.fn();
    render(
      <AgentConsumptionPanel
        domain="mcp"
        title="MCP"
        manageLabel="添加 MCP"
        rows={[syncedRow]}
        external={[externalRow]}
        present={() => ({ name: "MCP" })}
        onManage={vi.fn()}
        bulkToggleLabel="全部"
        bulkEnabled
        onBulkEnabledChange={onBulkEnabledChange}
      />,
    );

    const toggle = screen.getByRole("switch", { name: "停用全部 MCP" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await userEvent.click(toggle);
    expect(onBulkEnabledChange).toHaveBeenCalledWith(false);
  });

  it("places the explicit bulk MCP removal next to the add action", async () => {
    const onBulkRemove = vi.fn();
    render(
      <AgentConsumptionPanel
        domain="mcp"
        title="MCP"
        manageLabel="添加 MCP"
        rows={[syncedRow]}
        external={[externalRow]}
        present={() => ({ name: "MCP" })}
        onManage={vi.fn()}
        bulkRemoveLabel="移除全部 MCP"
        bulkRemoveTitle="清空 Codex 的全部 MCP，包括外部配置"
        onBulkRemove={onBulkRemove}
      />,
    );

    const button = screen.getByRole("button", { name: "移除全部 MCP" });
    expect(button).toHaveAttribute("title", "清空 Codex 的全部 MCP，包括外部配置");
    await userEvent.click(button);
    expect(onBulkRemove).toHaveBeenCalledOnce();
  });

  it("uses direct Agent actions and keeps healthy sync state quiet", async () => {
    const onManage = vi.fn();
    const onRemove = vi.fn();
    const onEnabledChange = vi.fn();
    render(
      <AgentConsumptionPanel
        domain="mcp"
        title="MCP"
        description="1 个已添加到 Codex。"
        manageLabel="添加 MCP"
        rows={[syncedRow]}
        external={[]}
        present={() => ({ name: "GitHub", description: "GitHub tools" })}
        onManage={onManage}
        onEnabledChange={onEnabledChange}
        onRemove={onRemove}
        removeLabel={(name) => `从 Codex 移除 ${name}`}
        columns={3}
      />,
    );

    expect(screen.getByRole("list")).toHaveAttribute("data-columns", "3");
    expect(screen.getByRole("button", { name: "添加 MCP" })).toBeVisible();
    expect(screen.getByText("GitHub tools")).toBeVisible();
    expect(screen.queryByText("已同步")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("switch", { name: "停用 GitHub" }));
    expect(onEnabledChange).toHaveBeenCalledWith(syncedRow, false);
    await userEvent.click(screen.getByRole("button", { name: "从 Codex 移除 GitHub" }));
    expect(onRemove).toHaveBeenCalledWith(syncedRow.asset);
  });

  it("does not expose an asset identity when the card has no description", () => {
    render(
      <AgentConsumptionPanel
        domain="mcp"
        title="MCP"
        manageLabel="添加 MCP"
        rows={[syncedRow]}
        external={[]}
        present={() => ({ name: "GitHub" })}
        onManage={vi.fn()}
      />,
    );

    const card = screen.getByText("GitHub").closest<HTMLElement>("li");
    expect(card).not.toBeNull();
    expect(within(card!).queryByText("github::stdio")).not.toBeInTheDocument();
    expect(card!.querySelector(".arca-consumption-copy small")).toBeNull();
  });

  it("uses asset-specific empty copy", () => {
    render(
      <AgentConsumptionPanel
        domain="skill"
        title="Skills"
        description="0 个已添加到 Codex。"
        manageLabel="添加 Skill"
        rows={[]}
        external={[]}
        present={() => ({ name: "unused" })}
        onManage={vi.fn()}
        emptyTitle="还没有添加 Skill"
        emptyDescription="从 Skills 资产库选择并添加到 Codex。"
      />,
    );

    expect(screen.getByText("还没有添加 Skill")).toBeVisible();
    expect(screen.getByText("从 Skills 资产库选择并添加到 Codex。")).toBeVisible();
  });

  it("renders a domain-specific row action", async () => {
    const onActivate = vi.fn();
    render(
      <AgentConsumptionPanel
        domain="model"
        title="Models"
        manageLabel="添加 Model"
        rows={[{ ...syncedRow, asset: { domain: "model", profile_id: "work" }, active: false }]}
        external={[]}
        present={() => ({ name: "Work" })}
        onManage={vi.fn()}
        renderAction={(item) => (
          <button type="button" onClick={() => onActivate(item)}>设为当前</button>
        )}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "设为当前" }));
    expect(onActivate).toHaveBeenCalledOnce();
  });

  it("renders every external asset as a read-only disabled card", () => {
    const onEnabledChange = vi.fn();
    const onOpenAsset = vi.fn();
    const onRemove = vi.fn();
    render(
      <AgentConsumptionPanel
        domain="mcp"
        title="MCP"
        manageLabel="添加 MCP"
        rows={[syncedRow]}
        external={[externalRow]}
        externalMode="cards"
        present={(asset) => ({ name: asset.domain === "mcp" ? asset.key : "asset" })}
        onManage={vi.fn()}
        onEnabledChange={onEnabledChange}
        onOpenAsset={onOpenAsset}
        onRemove={onRemove}
      />,
    );

    const cards = screen.getAllByRole("listitem");
    expect(cards).toHaveLength(2);
    expect(cards[1]).toHaveAttribute("data-status", "external-added");
    expect(cards[1]).toHaveAttribute("data-enabled", "false");
    expect(within(cards[1]).queryByText("外部新增")).not.toBeInTheDocument();
    expect(cards[1].querySelector(".arca-consumption-status")).toBeNull();
    expect(within(cards[1]).queryByRole("switch")).not.toBeInTheDocument();
    expect(within(cards[1]).queryByRole("button")).not.toBeInTheDocument();
    expect(onEnabledChange).not.toHaveBeenCalled();
    expect(onOpenAsset).not.toHaveBeenCalled();
    expect(onRemove).not.toHaveBeenCalled();
  });

  it("keeps one recovery action visible and moves alternatives into a compact menu", async () => {
    const onConverge = vi.fn();
    const changedRow: ConsumptionView = {
      ...syncedRow,
      status: "external-changed",
      reason: "mcp_owned_fields_drift",
      available_actions: ["adopt-observed", "restore-desired", "detach"],
    };
    render(
      <AgentConsumptionPanel
        domain="mcp"
        title="MCP"
        manageLabel="添加 MCP"
        rows={[changedRow]}
        external={[]}
        present={() => ({ name: "GitHub" })}
        onManage={vi.fn()}
        onConverge={onConverge}
      />,
    );

    const card = screen.getByText("GitHub").closest<HTMLElement>("li");
    expect(card).not.toBeNull();
    expect(within(card!).getByLabelText("外部已修改")).toBeVisible();
    expect(within(card!).queryByText("外部已修改")).not.toBeInTheDocument();
    expect(within(card!).getByRole("button", { name: /^恢复 Arca：/ })).toBeVisible();
    expect(within(card!).queryByRole("button", { name: /^收录 Arca：/ })).not.toBeInTheDocument();
    expect(within(card!).queryByRole("button", { name: /^解除管理：/ })).not.toBeInTheDocument();

    await userEvent.click(within(card!).getByRole("button", { name: /^更多处理方式：/ }));
    expect(within(card!).getByRole("menuitem", { name: /^收录 Arca：/ })).toBeVisible();
    expect(within(card!).getByRole("menuitem", { name: /^解除管理：/ })).toBeVisible();
    await userEvent.click(within(card!).getByRole("menuitem", { name: /^收录 Arca：/ }));
    expect(onConverge).toHaveBeenCalledWith(changedRow, "adopt-observed");
  });

  it("rejects cross-domain rows at the panel boundary", () => {
    const modelRow: ConsumptionView = {
      ...syncedRow,
      asset: { domain: "model", profile_id: "claude-opus-4-7" },
      active: true,
    };
    const skillRow: ConsumptionView = {
      ...syncedRow,
      asset: { domain: "skill", name: "dws" },
      enabled: null,
      affected_agent_ids: Array.from({ length: 9 }, (_, index) => `agent-${index}`),
    };

    render(
      <AgentConsumptionPanel
        domain="model"
        title="Models"
        manageLabel="添加 Model"
        rows={[modelRow, skillRow]}
        external={[]}
        present={(asset) => ({
          name: asset.domain === "model"
            ? asset.profile_id
            : asset.domain === "model-provider"
              ? asset.provider_id
              : asset.domain === "skill"
                ? asset.name
                : asset.key,
        })}
        onManage={vi.fn()}
      />,
    );

    expect(screen.getAllByText("claude-opus-4-7")[0]).toBeVisible();
    expect(screen.queryByText("dws")).not.toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
  });
});


it("filters Agent resources while keeping resource-specific action names", async () => {
  render(<AgentConsumptionPanel domain="mcp" title="MCPs" manageLabel="添加 MCPs"
    rows={[]} external={[externalRow, { ...externalRow, asset: { domain: "mcp", key: "second::http" } }]}
    externalMode="cards" present={(asset) => ({ name: asset.domain === "mcp" ? asset.key.split("::")[0] : "" })}
    onManage={vi.fn()} onConverge={vi.fn()} />);
  expect(screen.getByRole("button", { name: "收录 Arca：computer-use · stdio" })).toBeVisible();
  expect(screen.getByRole("button", { name: "收录 Arca：second · http" })).toBeVisible();
  await userEvent.type(screen.getByRole("searchbox", { name: "搜索当前 MCPs" }), "second");
  expect(screen.queryByRole("button", { name: /收录 Arca：computer-use/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "收录 Arca：second · http" })).toBeVisible();
});
