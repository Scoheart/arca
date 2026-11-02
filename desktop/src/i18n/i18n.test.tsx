import { render, waitFor } from "@testing-library/react";
import { useTranslation } from "react-i18next";
import { describe, expect, it } from "vitest";
import i18n, { translationResources } from ".";
import { AgentLaunchFields, createAgentLaunchDraft } from "../components/AgentLaunchFields";
import { DialogShell } from "../components/DialogShell";
import { ToastProvider } from "../components/Toast";
import type { AgentLaunchInfo } from "../lib/agentLaunch";

function leafKeys(value: unknown, prefix = ""): string[] {
  if (!value || typeof value !== "object") return [prefix];
  return Object.entries(value).flatMap(([key, child]) =>
    leafKeys(child, prefix ? `${prefix}.${key}` : key)
  );
}

describe("Desktop i18n contract", () => {
  it("localizes a portaled launch editor and validation without a DOM translation bridge", async () => {
    const info: AgentLaunchInfo = {default_target: { kind: "cli", command: "/fixture/agent", args: [] }, profiles: [{ id: "default", name: "默认配置", target: null, default_directory: null }, { id: "custom", name: "Fixture", target: null, default_directory: null }], active_profile_id: "custom", profiles_revision: "fixture-revision",
      agent_id: "fixture", name: "Fixture", category: "cli", kind: "cli", available: true,
      installed: true, supported: true, host_name: null, install_url: null, directory: null,
      directory_exists: false, configured_target: null,
      resolved_target: { kind: "cli", command: "/fixture/agent", args: [] },
    };
    await i18n.changeLanguage("en-US");
    const view = render(
      <ToastProvider>
        <DialogShell kind="editor" title="Agent settings" onClose={() => {}}>
          <AgentLaunchFields info={info} draft={{ ...createAgentLaunchDraft(info), environment: "invalid-line" }}
            disabled={false} onChange={() => {}} onBusyChange={() => {}} onError={() => {}} />
        </DialogShell>
      </ToastProvider>
    );
    try {
      expect(view.getByRole("button", { name: "Close" })).toBeVisible();
      expect(view.getByRole("textbox", { name: "Environment variables" })).toBeVisible();
      await waitFor(() => expect(view.getByRole("alert")).toHaveTextContent("Line 1 requires NAME=VALUE"));
      await i18n.changeLanguage("zh-CN");
      await waitFor(() => expect(view.getByRole("button", { name: "关闭" })).toBeVisible());
    } finally { view.unmount(); await i18n.changeLanguage("zh-CN"); }
  });
  it("keeps zh-CN and en-US typed dictionaries structurally identical", () => {
    expect(leafKeys(translationResources["en-US"]).sort())
      .toEqual(leafKeys(translationResources["zh-CN"]).sort());
  });

  it("renders typed Desktop copy in English and can switch back to Chinese", async () => {
    function ModelProviderLabel() {
      const { t } = useTranslation();
      return (
        <div>
          <span>{t("models.provider")}</span>
          <span data-testid="sidebar-add">{t("desktop.sourcesSidebar.addMCPs")}</span>
          <span data-testid="launch-doc">{t("desktop.agentLaunchAction.installationDocumentation")}</span>
        </div>
      );
    }

    await i18n.changeLanguage("en-US");
    const view = render(<ModelProviderLabel />);
    expect(view.getByText("Model provider")).toBeVisible();
    expect(view.getByTestId("sidebar-add").textContent).toBe("Add MCPs");
    expect(view.getByTestId("launch-doc").textContent).toBe("Installation documentation");

    await i18n.changeLanguage("zh-CN");
    await waitFor(() => expect(view.getByText("模型提供商")).toBeVisible());
    expect(view.getByTestId("sidebar-add").textContent).toBe("添加 MCPs");
    expect(view.getByTestId("launch-doc").textContent).toBe("安装文档");
  });
});
