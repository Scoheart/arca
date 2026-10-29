import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import * as api from "../lib/api";
import * as modelsDev from "../lib/modelsDev";
import { invalidateModelObservation } from "../lib/modelObservation";
import { ModelsView } from "./ModelsView";
import { ToastProvider } from "./Toast";

vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    listModelProfiles: vi.fn(),
    listModelProviders: vi.fn(),
    listModelProviderInstances: vi.fn(),
    discoverProviderModels: vi.fn(),
    revealModelProviderCredential: vi.fn(),
    inferModelProvider: vi.fn(),
  };
});

vi.mock("../lib/modelsDev", async () => {
  const actual = await vi.importActual<typeof import("../lib/modelsDev")>("../lib/modelsDev");
  return {
    ...actual,
    getCachedModelsDevMetadata: vi.fn(() => ({})),
    loadModelsDevMetadata: vi.fn(async () => ({})),
  };
});

const source = await readFile(resolve(process.cwd(), "src/components/ModelsView.tsx"), "utf8");
const i18nSource = await readFile(resolve(process.cwd(), "src/i18n/index.ts"), "utf8");
const agentSource = await readFile(resolve(process.cwd(), "src/components/AgentView.tsx"), "utf8");
const css = await readFile(resolve(process.cwd(), "src/index.css"), "utf8");

beforeEach(() => {
  void invalidateModelObservation();
  vi.mocked(api.listModelProfiles).mockResolvedValue([]);
  vi.mocked(api.listModelProviders).mockResolvedValue([
    {
      id: "openrouter",
      name: "OpenRouter",
      base_url: "https://openrouter.ai/api/v1",
      protocols: {
        "openai-responses": { endpoint_path: "/responses" },
      },
      default_base_url: "https://openrouter.ai/api/v1",
      default_protocol: "openai-responses",
      additional_endpoints: [],
      category: "gateway",
      model_discovery_supported: true,
    },
    {
      id: "openai",
      name: "OpenAI",
      base_url: "https://api.openai.com/v1",
      protocols: {
        "openai-responses": { endpoint_path: "/responses" },
      },
      default_base_url: "https://api.openai.com/v1",
      default_protocol: "openai-responses",
      additional_endpoints: [],
      category: "official",
      model_discovery_supported: true,
    },
    {
      id: "alibaba-coding-plan",
      name: "Alibaba Coding Plan (Global)",
      base_url: "https://coding-intl.dashscope.aliyuncs.com",
      protocols: {
        "anthropic-messages": { endpoint_path: "/apps/anthropic/v1/messages" },
        "openai-completions": { endpoint_path: "/v1/chat/completions" },
      },
      default_base_url: "https://coding-intl.dashscope.aliyuncs.com/v1",
      default_protocol: "openai-completions",
      additional_endpoints: [{
        protocol: "anthropic-messages",
        base_url: "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic",
      }],
      category: "official",
      model_discovery_supported: true,
    },
    {
      id: "ollama",
      name: "Ollama",
      base_url: "http://localhost:11434/v1",
      protocols: {
        "openai-completions": { endpoint_path: "/chat/completions" },
      },
      default_base_url: "http://localhost:11434/v1",
      default_protocol: "openai-completions",
      additional_endpoints: [],
      category: "local",
      model_discovery_supported: true,
    },
    {
      id: "custom",
      name: "Custom Provider",
      base_url: "",
      protocols: {
        "openai-responses": { endpoint_path: "/responses" },
      },
      default_base_url: null,
      default_protocol: "openai-responses",
      additional_endpoints: [],
      category: "custom",
      model_discovery_supported: true,
    },
  ]);
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([]);
  vi.mocked(api.discoverProviderModels).mockResolvedValue([]);
  vi.mocked(api.revealModelProviderCredential).mockResolvedValue("");
  vi.mocked(api.inferModelProvider).mockImplementation(async (baseUrl) => {
    const host = (() => {
      try {
        return new URL(baseUrl).hostname;
      } catch {
        return "";
      }
    })();
    if (host === "openrouter.ai") return "openrouter";
    if (host === "api.openai.com") return "openai";
    return "custom";
  });
  vi.mocked(modelsDev.getCachedModelsDevMetadata).mockReturnValue({});
  vi.mocked(modelsDev.loadModelsDevMetadata).mockResolvedValue({});
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function chooseFormSelect(
  user: ReturnType<typeof userEvent.setup>,
  label: string,
  option: string,
) {
  const combobox = screen.getByRole("combobox", { name: label });
  await user.click(combobox);
  await user.click(screen.getByRole("option", { name: option }));
  return combobox;
}

async function openProviderTemplate(
  user: ReturnType<typeof userEvent.setup>,
  provider: string,
) {
  await user.click(screen.getByRole("button", { name: "添加 Provider" }));
  const catalog = screen.getByRole("dialog", { name: "添加 Provider" });
  expect(catalog).toBeVisible();
  await user.click(within(catalog).getByRole("radio", { name: new RegExp(provider) }));
  await user.click(within(catalog).getByRole("button", { name: "创建" }));
  await waitFor(() =>
    expect(screen.getByRole("heading", { name: `添加 ${provider}` })).toHaveFocus()
  );
}

it("maps Models to one compact, scannable list", () => {
  const list = source.slice(source.indexOf("function ModelList"), source.indexOf("function ModelInspector"));
  expect(list).toMatch(/className="arca-asset-list arca-model-list" role="list"/);
  expect(list).toMatch(/role="listitem"/);
  expect(list).toMatch(/className="arca-asset-list-row arca-model-list-row"/);
  expect(list).toMatch(/<strong title=\{displayName\}>\{displayName\}<\/strong>/);
  expect(list).toMatch(/title=\{profile\.model\}>\{profile\.model\}<\/code>/);
  expect(list).not.toMatch(/protocolLabel\(profile\.protocol\)/);
  expect(list).not.toMatch(/profile\.credential_saved/);
  expect(list).not.toMatch(/<ResourceCard/);
  expect(source).toMatch(/className="arca-models-workspace"/);
  expect(source).not.toMatch(/<ResourceTabs/);
  expect(css).toMatch(/\.arca-models-workspace \.arca-model-list \{[\s\S]*?background: transparent;/);
  expect(css).toMatch(/\.arca-models-workspace \.arca-model-list > \[role="listitem"\] \{[\s\S]*?border-bottom:/);
  expect(css).toMatch(/\.arca-models-workspace \.arca-model-list-row \{[\s\S]*?border-radius: 0;[\s\S]*?background: transparent;/);
});

it("keeps the Models workspace visible through loading, error recovery, and empty states", async () => {
  vi.mocked(api.listModelProfiles).mockReturnValueOnce(new Promise<never>(() => undefined));
  const loadingView = render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  expect(screen.getByRole("status", { name: "正在读取模型资产" })).toBeVisible();
  expect(screen.getByRole("button", { name: "添加 Provider" })).toBeVisible();
  loadingView.unmount();

  vi.mocked(api.listModelProfiles)
    .mockRejectedValueOnce(new Error("model registry unavailable"))
    .mockResolvedValueOnce([]);
  const user = userEvent.setup();
  const errorView = render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  expect(await screen.findByText("读取模型资产失败")).toBeVisible();
  expect(screen.getByRole("alert")).toHaveTextContent("model registry unavailable");
  await user.click(screen.getByRole("button", { name: "重试" }));
  expect(await screen.findByText("暂无模型资产")).toBeVisible();
  expect(screen.getByText("新建一个可复用的模型配置。")).toBeVisible();
  errorView.unmount();
});

it("recovers a no-match Models search without losing the compact index", async () => {
  vi.mocked(api.listModelProfiles).mockResolvedValue([{
    id: "qwen-profile",
    name: "Qwen Profile",
    provider: "openrouter",
    protocol: "openai-completions",
    base_url: "https://openrouter.ai/api/v1",
    model: "qwen/qwen3",
    catalog_key: "openrouter/qwen/qwen3",
    credential_saved: true,
  }]);
  const user = userEvent.setup();

  render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  expect(await screen.findByRole("list", { name: "模型资产" })).toBeVisible();
  await user.type(screen.getByPlaceholderText("搜索名称、Model ID 或 Provider"), "not-present");
  expect(screen.getByText("没有匹配项")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "清除筛选" }));
  expect(screen.getByRole("button", { name: "打开模型 Qwen Profile 详情" })).toBeVisible();
});

it("enriches an OpenRouter list row without overriding user token limits", async () => {
  vi.mocked(api.listModelProfiles).mockResolvedValue([{
    id: "qwen-profile",
    name: "OpenRouter",
    provider: "openrouter",
    protocol: "openai-completions",
    base_url: "https://openrouter.ai/api/v1",
    model: "qwen/qwen3",
    context_window: 200_000,
    max_output_tokens: 16_000,
    catalog_key: "openrouter/qwen/qwen3",
    credential_saved: true,
  }]);
  vi.mocked(modelsDev.loadModelsDevMetadata).mockResolvedValue({
    "qwen-profile": {
      name: "Qwen3",
      description: "A capable reasoning and tool-use model.",
      contextWindow: 262_144,
      maxOutputTokens: 32_768,
      reasoning: true,
      toolCall: true,
      inputCost: 0.2,
      outputCost: 0.8,
    },
  });

  render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  const card = await screen.findByRole("button", { name: "打开模型 OpenRouter 详情" });
  await waitFor(() => expect(within(card).getByText("Qwen3")).toBeVisible());
  expect(within(card).getByText("qwen/qwen3")).toBeVisible();
  expect(within(card).queryByText("OpenRouter")).not.toBeInTheDocument();
  expect(within(card).getByLabelText("上下文 200K")).toHaveTextContent("上下文 200K");
  expect(within(card).queryByText("262.1K")).not.toBeInTheDocument();
  expect(within(card).queryByText("$0.2/M 输入")).not.toBeInTheDocument();
  expect(within(card).queryByText("推理")).not.toBeInTheDocument();
  expect(within(card).queryByText("Tools")).not.toBeInTheDocument();
  expect(within(card).queryByText(/models\.dev/)).not.toBeInTheDocument();
});

it("uses neutral protocol classification without a card color rail", () => {
  expect(source).not.toMatch(/className="arca-model-protocol-dot" data-protocol=\{profile\.protocol\}/);
  expect(css).not.toMatch(/\.arca-model-card::before/);
  expect(css).not.toMatch(/\.arca-model-card\[data-protocol=/);
});

it("keeps the sidebar limited to the model library and configured Providers", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([
    {
      id: "openai-personal",
      name: "OpenAI Personal",
      provider: "openai",
      base_url: "https://api.openai.com",
      protocols: { "openai-responses": { endpoint_path: "/v1/responses" } },
      credential_saved: true,
      model_count: 1,
      model_discovery_supported: true,
    },
    {
      id: "openrouter-personal",
      name: "OpenRouter Personal",
      provider: "openrouter",
      base_url: "https://openrouter.ai",
      protocols: { "openai-completions": { endpoint_path: "/api/v1/chat/completions" } },
      credential_saved: false,
      model_count: 1,
      model_discovery_supported: true,
    },
  ]);
  vi.mocked(api.listModelProfiles).mockResolvedValue([
    {
      id: "responses-model",
      name: "Responses Model",
      provider_id: "openai-personal",
      provider: "openai",
      protocol: "openai-responses",
      base_url: "https://api.openai.com/v1",
      model: "gpt-responses",
      reasoning: true,
      catalog_key: "openai/gpt-responses",
      credential_saved: true,
    },
    {
      id: "completions-model",
      name: "Completions Model",
      provider_id: "openrouter-personal",
      provider: "openrouter",
      protocol: "openai-completions",
      base_url: "https://openrouter.ai/api/v1",
      model: "openrouter/free",
      reasoning: false,
      catalog_key: "openrouter/free",
      credential_saved: false,
    },
  ]);
  const user = userEvent.setup();
  const view = render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  await screen.findByRole("button", { name: "打开模型 Completions Model 详情" });
  const sidebarElement = view.container.querySelector(".arca-workspace-sidebar");
  expect(sidebarElement).not.toBeNull();
  const sidebar = within(sidebarElement as HTMLElement);

  expect(sidebarElement?.querySelectorAll(".arca-sidebar-section")).toHaveLength(2);
  expect(sidebar.getByText("模型库")).toBeVisible();
  expect(sidebar.getByText("官方 Provider")).toBeVisible();
  expect(sidebar.queryByText("自定义 Provider")).not.toBeInTheDocument();
  expect(sidebar.getByText("全部模型")).toBeVisible();
  expect(sidebar.queryByText("协议")).not.toBeInTheDocument();
  expect(sidebar.queryByText("全部协议")).not.toBeInTheDocument();

  const openAiProvider = sidebar.getByRole("button", { name: /OpenAI Personal/ });
  expect(openAiProvider.querySelector('[data-provider-icon="openai"]')).toBeInTheDocument();
  await user.click(openAiProvider);
  expect(screen.getByRole("button", { name: "打开模型 Responses Model 详情" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "打开模型 Completions Model 详情" })).not.toBeInTheDocument();
  expect(view.container.querySelector(
    '.arca-model-provider-banner [data-provider-icon="openai"]',
  )).toBeInTheDocument();

  await user.click(sidebar.getByRole("button", { name: /全部模型/ }));
  expect(screen.getByRole("button", { name: "打开模型 Responses Model 详情" })).toBeVisible();
  expect(screen.getByRole("button", { name: "打开模型 Completions Model 详情" })).toBeVisible();
});

it("splits configured Providers into custom then official sidebar groups", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([
    {
      id: "openai-personal",
      name: "OpenAI Personal",
      provider: "openai",
      base_url: "https://api.openai.com",
      protocols: { "openai-responses": { endpoint_path: "/v1/responses" } },
      credential_saved: true,
      model_count: 1,
      model_discovery_supported: true,
    },
    {
      id: "custom-idealab",
      name: "Idealab",
      provider: "custom",
      base_url: "https://idealab.alibaba-inc.com",
      protocols: { "openai-completions": { endpoint_path: "/api/openai/v1/chat/completions" } },
      credential_saved: true,
      model_count: 1,
      model_discovery_supported: false,
    },
  ]);
  vi.mocked(api.listModelProfiles).mockResolvedValue([
    {
      id: "responses-model",
      name: "Responses Model",
      provider_id: "openai-personal",
      provider: "openai",
      protocol: "openai-responses",
      base_url: "https://api.openai.com/v1",
      model: "gpt-responses",
      reasoning: true,
      catalog_key: "openai/gpt-responses",
      credential_saved: true,
    },
    {
      id: "idealab-model",
      name: "Idealab Model",
      provider_id: "custom-idealab",
      provider: "custom",
      protocol: "openai-completions",
      base_url: "https://idealab.alibaba-inc.com/api/openai/v1",
      model: "Peach-07-17-DogFooding",
      reasoning: false,
      catalog_key: "custom/peach",
      credential_saved: true,
    },
  ]);
  const user = userEvent.setup();
  const view = render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  await screen.findByRole("button", { name: "打开模型 Idealab Model 详情" });
  const sidebarElement = view.container.querySelector(".arca-workspace-sidebar");
  expect(sidebarElement).not.toBeNull();
  const sidebar = within(sidebarElement as HTMLElement);
  const sections = [...(sidebarElement?.querySelectorAll(".arca-sidebar-section") ?? [])];

  expect(sections).toHaveLength(3);
  expect(within(sections[0] as HTMLElement).getByText("模型库")).toBeVisible();
  expect(within(sections[1] as HTMLElement).getByText("自定义 Provider")).toBeVisible();
  expect(within(sections[1] as HTMLElement).getByRole("button", { name: /Idealab/ })).toBeVisible();
  expect(within(sections[2] as HTMLElement).getByText("官方 Provider")).toBeVisible();
  expect(within(sections[2] as HTMLElement).getByRole("button", { name: /OpenAI Personal/ })).toBeVisible();

  await user.click(sidebar.getByRole("button", { name: /Idealab/ }));
  expect(screen.getByRole("button", { name: "打开模型 Idealab Model 详情" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "打开模型 Responses Model 详情" })).not.toBeInTheDocument();
});

it("collapses long official Provider lists behind show more", async () => {
  const official = ["Anthropic", "Cerebras", "Cohere", "Google", "Groq", "Mistral", "Ollama"].map((name) => ({
    id: `${name.toLowerCase()}-instance`,
    name,
    provider: name.toLowerCase(),
    base_url: `https://${name.toLowerCase()}.example.test`,
    protocols: { "openai-completions": { endpoint_path: "/v1/chat/completions" } },
    credential_saved: true,
    model_count: 1,
    model_discovery_supported: true,
  }));
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([
    {
      id: "custom-idealab",
      name: "Idealab",
      provider: "custom",
      base_url: "https://idealab.alibaba-inc.com",
      protocols: { "openai-completions": { endpoint_path: "/api/openai/v1/chat/completions" } },
      credential_saved: true,
      model_count: 1,
      model_discovery_supported: false,
    },
    ...official,
  ]);
  vi.mocked(api.listModelProfiles).mockResolvedValue(official.map((provider) => ({
    id: `${provider.id}-model`,
    name: `${provider.name} Model`,
    provider_id: provider.id,
    provider: provider.provider,
    protocol: "openai-completions",
    base_url: provider.base_url,
    model: `${provider.provider}-latest`,
    reasoning: false,
    catalog_key: `${provider.provider}/latest`,
    credential_saved: true,
  })));
  const user = userEvent.setup();
  const { container } = render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  await screen.findByRole("button", { name: "打开模型 Anthropic Model 详情" });
  const sidebarElement = container.querySelector(".arca-workspace-sidebar");
  expect(sidebarElement).not.toBeNull();
  const sidebar = within(sidebarElement as HTMLElement);
  const sections = [...(sidebarElement?.querySelectorAll(".arca-sidebar-section") ?? [])];
  const officialSection = within(sections[2] as HTMLElement);

  expect(within(sections[1] as HTMLElement).getByText("自定义 Provider")).toBeVisible();
  expect(sidebar.getByRole("button", { name: /Idealab/ })).toBeVisible();
  expect(officialSection.getByRole("button", { name: /Anthropic/ })).toBeVisible();
  expect(officialSection.getByRole("button", { name: /Groq/ })).toBeVisible();
  expect(officialSection.queryByRole("button", { name: /Mistral/ })).not.toBeInTheDocument();
  expect(officialSection.queryByRole("button", { name: /Ollama/ })).not.toBeInTheDocument();
  expect(officialSection.getByRole("button", { name: /显示更多 · 2/ })).toBeVisible();

  await user.click(officialSection.getByRole("button", { name: /显示更多 · 2/ }));
  expect(officialSection.getByRole("button", { name: /Mistral/ })).toBeVisible();
  expect(officialSection.getByRole("button", { name: /Ollama/ })).toBeVisible();
  expect(officialSection.getByRole("button", { name: /收起/ })).toBeVisible();

  await user.click(officialSection.getByRole("button", { name: /Ollama/ }));
  expect(screen.getByRole("button", { name: "打开模型 Ollama Model 详情" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "打开模型 Anthropic Model 详情" })).not.toBeInTheDocument();

  await user.click(officialSection.getByRole("button", { name: /收起/ }));
  expect(officialSection.getByRole("button", { name: /Ollama/ })).toBeVisible();
  expect(officialSection.queryByRole("button", { name: /Mistral/ })).not.toBeInTheDocument();
  expect(officialSection.getByRole("button", { name: /显示更多 · 2/ })).toBeVisible();
});

it("keeps the built-in Provider Catalog unfiltered, searchable, and keyboard-selectable", async () => {
  const user = userEvent.setup();
  const consumptionState = { plan: null, planUpdate: vi.fn() } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "添加 Provider" }));
  const catalog = screen.getByRole("dialog", { name: "添加 Provider" });
  expect(catalog).toBeVisible();
  expect(within(catalog).getByRole("radio", { name: /Custom Provider/ })).toHaveAttribute("aria-checked", "true");
  expect(within(catalog).getByRole("radio", { name: /OpenAI/ })).toBeVisible();
  expect(within(catalog).getByRole("radio", { name: /Ollama/ })).toBeVisible();
  expect(within(catalog).getByRole("radio", { name: /Custom Provider/ })).toBeVisible();
  expect(within(catalog).getAllByRole("radio")[0]).toHaveAccessibleName(/Custom Provider/);
  expect(within(catalog).getByRole("radio", { name: /OpenAI/ })
    .querySelector('[data-provider-icon="openai"]')).toBeInTheDocument();
  expect(within(catalog).getByRole("radio", { name: /Custom Provider/ })
    .querySelector('[data-provider-icon="fallback"]')).toBeInTheDocument();
  expect(within(catalog).queryByText("Local endpoint")).not.toBeInTheDocument();
  for (const label of ["Official", "Gateway", "Local", "Custom"]) {
    expect(within(catalog).queryByText(label)).not.toBeInTheDocument();
  }
  expect(catalog.querySelector(".arca-provider-catalog-categories")).not.toBeInTheDocument();
  expect(within(catalog).queryByRole("button", { name: "全部" })).not.toBeInTheDocument();

  const search = within(catalog).getByRole("searchbox", { name: "搜索 Provider" });
  await user.type(search, "apps/anthropic");
  expect(within(catalog).getAllByRole("radio")).toHaveLength(1);
  expect(within(catalog).getByRole("radio", { name: /Alibaba Coding Plan/ })).toBeVisible();

  await user.clear(search);
  await user.type(search, "api.openai.com");
  expect(within(catalog).getAllByRole("radio")).toHaveLength(1);
  expect(within(catalog).getByRole("radio", { name: /OpenAI/ })).toBeVisible();
  expect(within(catalog).getByRole("button", { name: "创建" })).toBeDisabled();

  await user.clear(search);
  expect(within(catalog).getAllByRole("radio")).toHaveLength(5);
  const openAi = within(catalog).getByRole("radio", { name: /OpenAI/ });
  openAi.focus();
  await user.keyboard("{Enter}");
  expect(openAi).toHaveAttribute("aria-checked", "true");
  expect(catalog.querySelector(".arca-provider-catalog-selection")).toHaveTextContent("OpenAI");

  await user.click(within(catalog).getByRole("button", { name: "创建" }));
  await waitFor(() => expect(screen.getByRole("heading", { name: "添加 OpenAI" })).toHaveFocus());
  expect(screen.getByLabelText("Base URL")).toHaveValue("https://api.openai.com/v1");
  expect(screen.getByText("API Key")).toBeVisible();
  expect(source).toMatch(/<DialogShell\s+[\s\S]*?kind="picker"/);
  expect(source).not.toMatch(/ProviderCatalogDrawer|provider-catalog-drawer/);
});

it("renders compact Provider options with a quiet sticky action bar", () => {
  expect(css).toMatch(/\.arca-provider-catalog-item\s*\{[\s\S]*?min-height: 54px/);
  expect(css).toMatch(/grid-template-columns: 28px minmax\(0, 1fr\) 16px; align-items: center/);
  expect(css).toMatch(/\.arca-provider-catalog-copy\s*\{[\s\S]*?align-content: center; gap: 3px/);
  expect(css).toMatch(/\.arca-provider-catalog-grid\s*\{[\s\S]*?gap: 7px/);
  expect(css).toMatch(/\.arca-dialog-shell-footer\s*\{[\s\S]*?border-top:/);
  expect(css).toMatch(/\.arca-provider-catalog-search \.arca-model-field\s*\{[\s\S]*?padding-left: 36px/);
  expect(css).toMatch(/\.arca-provider-catalog-copy code\s*\{[\s\S]*?text-overflow: ellipsis; white-space: nowrap/);
  expect(css).not.toMatch(/\.arca-provider-catalog-categories/);
});

it("uses explicit dialog variants instead of descendant shell detection", () => {
  expect(source).toMatch(/className="arca-dialog-provider-catalog"/);
  expect(source).toMatch(/className="arca-dialog-provider-editor"/);
  expect(css).not.toMatch(/\.arca-dialog-shell:has\(\.arca-provider-(catalog|form)\)/);
  expect(css).not.toMatch(/\.arca-dialog-shell:has\(\.arca-agent-create\)/);
});

it("keeps Keychain status on the Provider rather than Model cards", () => {
  expect(source).toMatch(/className="arca-model-provider-credential"/);
  expect(source).toMatch(/data-saved=\{provider\.credential_saved \? "true" : "false"\}/);
  expect(source).toMatch(/<KeyIcon className="w-3\.5 h-3\.5" \/>/);
  expect(source).not.toMatch(/className="arca-model-list-credential"/);
  expect(css).not.toMatch(/\.arca-model-list-credential/);
});

it("renders model details as one continuous field list without section cards", async () => {
  vi.mocked(api.listModelProfiles).mockResolvedValue([{
    id: "qwen3-7-plus",
    name: "Qwen3 7 Plus",
    provider: "max-ai",
    model_vendor: "Qwen",
    protocol: "openai-responses",
    base_url: "https://models.example.test/v1",
    model: "qwen3.7-plus",
    env_key: "MAX_AI_API_KEY",
    context_window: 200_000,
    max_output_tokens: 32_000,
    reasoning: true,
    catalog_key: "max-ai/qwen3.7-plus",
    credential_saved: true,
  }]);
  const user = userEvent.setup();

  render(
    <ToastProvider>
      <ModelsView />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "打开模型 Qwen3 7 Plus 详情" }));

  const inspector = screen.getByRole("complementary", { name: "Qwen3 7 Plus 详情" });
  const fields = within(inspector).getByRole("region", { name: "模型详情字段" });
  expect(within(inspector).getByLabelText("资源指标")).toBeVisible();
  expect(within(inspector).getByText("200K")).toBeVisible();
  expect(within(inspector).getByText("32K")).toBeVisible();
  expect(fields).toHaveClass("arca-model-inspector-fields");
  expect(fields.querySelectorAll(".arca-inspector-field")).toHaveLength(6);
  expect(fields.querySelectorAll(".arca-inspector-section")).toHaveLength(0);
  for (const label of [
    "模型提供商",
    "协议",
    "推理",
    "模型 ID",
    "完整请求 URL",
    "环境变量",
  ]) {
    expect(within(fields).getByText(label)).toBeVisible();
  }
  expect(within(fields).queryByText("API Key")).not.toBeInTheDocument();
  expect(within(fields).queryByText("已保存到 Keychain")).not.toBeInTheDocument();
  for (const removed of ["模型开发商", "目录来源", "Profile ID", "Catalog Key"]) {
    expect(within(fields).queryByText(removed)).not.toBeInTheDocument();
  }
  expect(within(fields).queryByText(/models\.dev/)).not.toBeInTheDocument();
  expect(within(inspector).queryByRole("heading", { name: "资产信息" })).not.toBeInTheDocument();
  expect(within(inspector).queryByRole("heading", { name: "接口" })).not.toBeInTheDocument();
  expect(within(inspector).queryByRole("heading", { name: "技术详情" })).not.toBeInTheDocument();
  expect(within(inspector).queryByRole("button", { name: "删除" })).not.toBeInTheDocument();
  expect(within(inspector).queryByText("cURL 请求")).not.toBeInTheDocument();
  expect(within(inspector).getByRole("button", { name: "编辑" })).toBeVisible();
  expect(within(inspector).getByRole("button", { name: "复制模型 ID" })).toBeVisible();
});

it("keeps new Provider forms API-key-only while preserving legacy sources internally", () => {
  const modelDialog = source.slice(
    source.indexOf("function ModelProfileDialog"),
    source.indexOf("function ModelProviderDialog"),
  );
  const providerDialog = source.slice(source.indexOf("function ModelProviderDialog"));
  expect(modelDialog).not.toMatch(/models\.apiKeyEnv|models\.apiKey"\)/);
  expect(providerDialog).toMatch(/initial\?\.api_key_source/);
  expect(providerDialog).toMatch(/models\.apiKey/);
  expect(providerDialog).toMatch(/kind: "arca-store"/);
  expect(providerDialog).not.toMatch(/credentialSourceOptions/);
  expect(providerDialog).not.toMatch(/authRequirementOptions/);
  expect(providerDialog).not.toMatch(/changeCredentialSource/);
  expect(providerDialog).not.toMatch(/testCredentialSource/);
  expect(providerDialog).not.toMatch(/models\.authRequirement/);
  expect(providerDialog).not.toMatch(/models\.credentialSource/);
  expect(providerDialog).not.toMatch(/models\.apiKeyEnv/);
  expect(providerDialog).not.toMatch(/models\.credentialFilePath/);
  expect(providerDialog).not.toMatch(/models\.credentialHelperCommand/);
  expect(providerDialog).toMatch(/authWithoutCredential/);
  expect(providerDialog).toMatch(/hasNewCredential[\s\S]*?"required"[\s\S]*?clearCredential[\s\S]*?authWithoutCredential[\s\S]*?initialSource \|\| preservedCredential[\s\S]*?initialAuthRequirement/);
  expect(i18nSource).toContain('credentialHelp: "API Key 安全保存到系统 Keychain。"');
});

it("requires Model forms to select a persisted Provider relationship", () => {
  const profileDialog = source.indexOf("function ModelProfileDialog");
  const providerField = source.slice(
    source.indexOf('<span>{t("models.provider")}</span>', profileDialog),
    source.indexOf('<span>{t("models.protocol")}</span>', profileDialog),
  );
  expect(providerField).toMatch(/<FormSelect\s+[\s\S]*?ariaLabel=\{t\("models\.provider"\)\}/);
  expect(providerField).not.toMatch(/自动识别/);
  expect(providerField).toMatch(/placeholder=\{t\("models\.providerPlaceholder"\)\}/);
  expect(providerField).toMatch(/options=\{providerInstances\.map/);
  expect(providerField).not.toMatch(/CUSTOM_PROVIDER_OPTION|customProviderId/);
  expect(providerField).not.toMatch(/<select|datalist/);
  expect(providerField).toMatch(/models\.providerRequired/);
  expect(source).toMatch(/provider_id: provider\.id/);
});

it("uses one custom select surface for model provider, protocol, and reasoning", () => {
  expect(source).toMatch(/<FormSelect\s+[\s\S]*?ariaLabel=\{t\("models\.provider"\)\}/);
  expect(source).toMatch(/<FormSelect\s+[\s\S]*?ariaLabel=\{t\("models\.protocol"\)\}/);
  expect(source).toMatch(/<FormSelect\s+[\s\S]*?ariaLabel=\{t\("models\.reasoningMode"\)\}/);
  expect(source).not.toMatch(/<select/);
  expect(css).toMatch(/\.arca-form-select-menu/);
  expect(css).toMatch(/\.arca-form-select\[data-open="true"\]\s*\{\s*z-index: 621/);
  expect(css).toMatch(/background: var\(--surface-popover\)/);
});

it("fills a standalone Provider form from the selected catalog template", async () => {
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "provider-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "OpenRouter");
  expect(screen.getByRole("heading", { name: "添加 OpenRouter" })).toBeVisible();
  expect(screen.queryByRole("combobox", { name: "Provider 类型" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("自定义模型提供商 ID")).not.toBeInTheDocument();
  expect(screen.getByLabelText("Base URL")).toHaveValue("https://openrouter.ai/api/v1");
  expect(screen.getAllByLabelText("Base URL")).toHaveLength(1);
  expect(screen.getByLabelText("Models 列表 URL")).toHaveValue("");
  expect(screen.getByRole("switch", { name: /OpenAI Responses/ })).toBeChecked();
  expect(screen.getByRole("switch", { name: /Anthropic Messages/ })).not.toBeChecked();
  expect(screen.getByLabelText("OpenAI Responses Endpoint Path")).toHaveValue(
    "/responses",
  );
  const requestUrl = screen.getByLabelText("完整请求 URL");
  expect(requestUrl.tagName).toBe("OUTPUT");
  expect(requestUrl).toHaveTextContent("https://openrouter.ai/api/v1/responses");
  expect(requestUrl).not.toHaveClass("arca-model-field");
  const protocolList = screen.getByRole("region", { name: "支持的协议" })
    .querySelector(".arca-provider-protocol-list");
  expect(protocolList).not.toBeNull();
  expect(within(protocolList as HTMLElement).getByText("/v1/messages")).toBeVisible();
  expect(within(protocolList as HTMLElement).getByText("/responses")).toBeVisible();
  expect(within(protocolList as HTMLElement).getByText("/chat/completions")).toBeVisible();
  const responseSwitch = screen.getByRole("switch", { name: "OpenAI Responses" });
  const responseRow = responseSwitch.closest(".arca-provider-protocol");
  const responsePath = within(responseRow as HTMLElement).getByText("/responses");
  const visualSwitch = responseRow?.querySelector(".arca-provider-protocol-switch");
  expect(visualSwitch).not.toBeNull();
  expect(responsePath.compareDocumentPosition(visualSwitch as Node) & Node.DOCUMENT_POSITION_FOLLOWING)
    .toBeTruthy();
  const credential = screen.getByRole("region", { name: "凭据" });
  expect(credential).toHaveClass("arca-provider-credential");
  const apiKey = within(credential).getByLabelText("API Key");
  expect(apiKey).toHaveAttribute("type", "password");
  expect(within(credential).getByRole("tab", { name: "API Key" })).toHaveAttribute("aria-selected", "true");
  expect(within(credential).getByRole("tab", { name: "Env" })).toHaveAttribute("aria-selected", "false");
  await user.click(within(credential).getByRole("button", { name: "显示 API Key" }));
  expect(apiKey).toHaveAttribute("type", "text");
  await user.click(within(credential).getByRole("tab", { name: "Env" }));
  const env = within(credential).getByLabelText("API Key 环境变量");
  expect(env).toBeVisible();
  expect(credential.querySelectorAll(".arca-provider-credential-input input")).toHaveLength(1);
  await user.type(env, "OPENROUTER_API_KEY");
  await user.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model-provider",
    credential: undefined,
    provider: expect.objectContaining({
      env_key: "OPENROUTER_API_KEY",
    }),
  })));
});

it("lets a Custom Provider save an explicit model catalog URL", async () => {
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "provider-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "Custom Provider");
  await user.type(screen.getByLabelText("Base URL"), "https://gateway.example.test");
  await user.type(
    screen.getByLabelText("Models 列表 URL"),
    "https://catalog.example.test/v1/models?available=true",
  );
  await user.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model-provider",
    provider: expect.objectContaining({
      model_catalog_url: "https://catalog.example.test/v1/models?available=true",
    }),
  })));
});

it("keeps the empty protocol requirement inside the compact section header", async () => {
  const user = userEvent.setup();
  const consumptionState = { plan: null, planUpdate: vi.fn() } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "OpenRouter");
  await user.click(screen.getByRole("switch", { name: /OpenAI Responses/ }));

  const protocols = screen.getByRole("region", { name: "支持的协议" });
  const header = protocols.querySelector(".arca-provider-section-head");
  expect(header).not.toBeNull();
  expect(within(header as HTMLElement).getByRole("status")).toHaveTextContent("至少启用一种协议。");
  expect(within(header as HTMLElement).queryByText("已启用 0 个")).not.toBeInTheDocument();
  expect(protocols.querySelector(".arca-provider-protocol-list + .arca-provider-protocol-error"))
    .not.toBeInTheDocument();
});

it("keeps plan-specific protocols under one Base URL", async () => {
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "provider-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "Alibaba Coding Plan");
  expect(screen.getByLabelText("Base URL")).toHaveValue(
    "https://coding-intl.dashscope.aliyuncs.com",
  );
  expect(screen.getByRole("switch", { name: /Anthropic Messages/ })).toBeChecked();
  expect(screen.getByRole("switch", { name: /OpenAI Chat Completions/ })).toBeChecked();
  expect(screen.getByLabelText("Anthropic Messages Endpoint Path")).toHaveValue(
    "/apps/anthropic/v1/messages",
  );
  await user.click(screen.getByRole("button", { name: /OpenAI Chat Completions/ }));
  expect(screen.getByLabelText("OpenAI Chat Completions Endpoint Path")).toHaveValue(
    "/v1/chat/completions",
  );
  await user.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model-provider",
    provider: expect.objectContaining({
      provider: "alibaba-coding-plan",
      base_url: "https://coding-intl.dashscope.aliyuncs.com",
      protocols: {
        "anthropic-messages": { endpoint_path: "/apps/anthropic/v1/messages" },
        "openai-completions": { endpoint_path: "/v1/chat/completions" },
      },
    }),
  })));
});

it("enables, previews, and submits protocol Endpoint Paths", async () => {
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "provider-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "OpenRouter");
  await user.click(screen.getByRole("switch", { name: /Anthropic Messages/ }));
  await user.click(screen.getByRole("button", { name: /Anthropic Messages/ }));
  expect(screen.getAllByLabelText("Base URL")).toHaveLength(1);
  expect(screen.getByLabelText("Base URL")).toHaveValue("https://openrouter.ai/api/v1");
  const path = screen.getByLabelText("Anthropic Messages Endpoint Path");
  expect(path).toHaveValue("/v1/messages");
  await user.clear(path);
  await user.type(path, "custom/messages");
  expect(path).toHaveValue("custom/messages");
  const protocolEditor = path.closest(".arca-provider-protocol-editor");
  expect(protocolEditor).not.toBeNull();
  expect(within(protocolEditor as HTMLElement).getByLabelText("完整请求 URL"))
    .toHaveTextContent("https://openrouter.ai/api/v1/custom/messages");
  await user.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model-provider",
    provider: expect.objectContaining({
      base_url: "https://openrouter.ai/api/v1",
      protocols: expect.objectContaining({
        "anthropic-messages": { endpoint_path: "/custom/messages" },
        "openai-responses": { endpoint_path: "/responses" },
      }),
    }),
  })));
});

it("rejects absolute, fragmented, and traversal Endpoint Paths in the Provider form", async () => {
  const user = userEvent.setup();
  const consumptionState = { plan: null, planUpdate: vi.fn() } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "Custom Provider");
  await user.type(screen.getByLabelText("Base URL"), "https://gateway.example.test");
  const path = screen.getByLabelText("OpenAI Responses Endpoint Path");
  const protocolEditor = path.closest(".arca-provider-protocol-editor");
  expect(protocolEditor).not.toBeNull();
  const save = screen.getByRole("button", { name: "保存" });

  for (const invalid of [
    "https://other.example/responses",
    "/v1/responses#fragment",
    "/v1/responses?mode=fast",
    "/v1/%2e%2e/responses",
  ]) {
    await user.clear(path);
    await user.type(path, invalid);
    expect(screen.getByText(/请输入相对路径/)).toBeVisible();
    expect(protocolEditor?.querySelector(".arca-provider-route-builder"))
      .toHaveAttribute("data-invalid", "true");
    expect(within(protocolEditor as HTMLElement).queryByText(invalid)).not.toBeInTheDocument();
    expect(save).toBeDisabled();
  }
});

it("reads a saved Provider API key only on explicit reveal and preserves it when unchanged", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "openrouter-team",
    name: "OpenRouter Team",
    provider: "openrouter",
    base_url: "https://openrouter.ai/api/v1",
    protocols: { "openai-responses": { endpoint_path: "/responses" } },
    credential_saved: true,
    model_count: 0,
    model_discovery_supported: true,
  }]);
  vi.mocked(api.revealModelProviderCredential).mockResolvedValue("saved-test-value");
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "provider-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  const view = render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  const sidebar = within(view.container.querySelector(".arca-workspace-sidebar") as HTMLElement);
  await user.click(await sidebar.findByTitle("OpenRouter Team"));
  await user.click(screen.getByRole("button", { name: "编辑 Provider" }));

  const credential = screen.getByRole("region", { name: "API Key" });
  const apiKey = within(credential).getByLabelText("API Key");
  expect(apiKey).toHaveAttribute("type", "password");
  expect(apiKey).toHaveValue("");
  expect(apiKey).toHaveAttribute("placeholder", "••••••••");
  expect(api.revealModelProviderCredential).not.toHaveBeenCalled();
  await user.click(within(credential).getByRole("button", { name: "显示 API Key" }));
  await waitFor(() => expect(apiKey).toHaveValue("saved-test-value"));
  expect(api.revealModelProviderCredential).toHaveBeenCalledWith("openrouter-team");
  expect(apiKey).toHaveAttribute("type", "text");
  expect(apiKey).toHaveValue("saved-test-value");

  await user.click(within(credential).getByRole("button", { name: "隐藏 API Key" }));
  expect(apiKey).toHaveAttribute("type", "password");
  expect(apiKey).toHaveValue("");

  await user.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model-provider",
    credential: undefined,
  }), { reviewOnlyWhenNeeded: true }));
});

async function openSavedProviderEditor(user: ReturnType<typeof userEvent.setup>) {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "saved-provider", name: "Saved Provider", provider: "openrouter",
    base_url: "https://openrouter.ai/api/v1",
    protocols: { "openai-responses": { endpoint_path: "/responses" } },
    auth_requirement: "required", api_key_source: { kind: "arca-store" },
    credential_saved: true, model_count: 0, model_discovery_supported: true,
  }]);
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "credential-plan", can_commit: true,
    warnings: [], relationship_changes: [], affected_agent_ids: [], model_state_changes: [] });
  const commit = vi.fn().mockResolvedValue(undefined);
  const consumptionState = { plan: null, planUpdate, commit } as unknown as ConsumptionState;
  const view = render(<ToastProvider><ModelsView consumptionState={consumptionState} /></ToastProvider>);
  const sidebar = within(view.container.querySelector(".arca-workspace-sidebar") as HTMLElement);
  await user.click(await sidebar.findByTitle("Saved Provider"));
  await user.click(screen.getByRole("button", { name: "编辑 Provider" }));
  const credentialSection = screen.getByRole("region", { name: "API Key" });
  return { planUpdate, commit, input: within(credentialSection).getByLabelText("API Key") };
}

it.each(["", "   ", "replacement-test-key"])("replaces or explicitly deletes a saved API key without revealing it: %j", async (value) => {
  vi.mocked(api.revealModelProviderCredential).mockResolvedValue("saved-test-key");
  const user = userEvent.setup();
  const { input, planUpdate, commit } = await openSavedProviderEditor(user);
  expect(input).toHaveValue("");
  expect(api.revealModelProviderCredential).not.toHaveBeenCalled();
  expect(screen.queryByRole("checkbox", { name: "清除已存密钥" })).not.toBeInTheDocument();
  if (!value.trim()) await user.click(screen.getByRole("button", { name: "删除已保存的 API Key" }));
  if (value) await user.type(input, value);
  expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model-provider", existing_id: "saved-provider",
    credential: value.trim() ? value : "",
    provider: expect.objectContaining({
      auth_requirement: "required",
      api_key_source: value.trim() ? { kind: "arca-store" } : undefined,
    }),
  }), { reviewOnlyWhenNeeded: true }));
  await waitFor(() => expect(commit).toHaveBeenCalled());
});

it("allows preserving a saved key after a denied explicit read and supports manual retry", async () => {
  vi.mocked(api.revealModelProviderCredential)
    .mockRejectedValueOnce(new Error("Keychain unavailable"))
    .mockResolvedValueOnce("saved-test-key");
  const user = userEvent.setup();
  const { input, planUpdate } = await openSavedProviderEditor(user);
  await user.click(screen.getByRole("button", { name: "显示 API Key" }));
  await screen.findByRole("alert");
  expect(input).toBeEnabled();
  expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
  expect(planUpdate).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "重试" }));
  await waitFor(() => expect(input).toHaveValue("saved-test-key"));
  await user.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    credential: undefined,
    provider: expect.objectContaining({ api_key_source: { kind: "arca-store" } }),
  }), { reviewOnlyWhenNeeded: true }));
});

it("filters Model protocols by Provider and previews the selected request URL", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([
    {
      id: "multi-provider",
      name: "Multi Provider",
      provider: "custom",
      base_url: "https://multi.example.test/api",
      protocols: {
        "anthropic-messages": { endpoint_path: "/anthropic/v1/messages" },
        "openai-responses": { endpoint_path: "/openai/responses" },
      },
      credential_saved: false,
      model_count: 0,
      model_discovery_supported: false,
    },
    {
      id: "chat-provider",
      name: "Chat Provider",
      provider: "custom",
      base_url: "https://chat.example.test",
      protocols: {
        "openai-completions": { endpoint_path: "/v2/chat/completions" },
      },
      credential_saved: false,
      model_count: 0,
      model_discovery_supported: false,
    },
  ]);
  const user = userEvent.setup();
  const consumptionState = { plan: null, planUpdate: vi.fn() } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "添加模型" }));
  const protocol = screen.getByRole("combobox", { name: "协议" });
  expect(protocol).toHaveTextContent("Anthropic Messages");
  expect(screen.getByLabelText("完整请求 URL")).toHaveValue(
    "https://multi.example.test/api/anthropic/v1/messages",
  );

  await user.click(protocol);
  expect(screen.getByRole("option", { name: "Anthropic Messages" })).toBeVisible();
  expect(screen.getByRole("option", { name: "OpenAI Responses" })).toBeVisible();
  expect(screen.queryByRole("option", { name: "OpenAI Chat Completions" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("option", { name: "OpenAI Responses" }));
  expect(screen.getByLabelText("完整请求 URL")).toHaveValue(
    "https://multi.example.test/api/openai/responses",
  );

  await chooseFormSelect(user, "模型提供商", "Chat Provider");
  expect(protocol).toHaveTextContent("OpenAI Chat Completions");
  expect(screen.getByLabelText("完整请求 URL")).toHaveValue(
    "https://chat.example.test/v2/chat/completions",
  );
});

it("switches an existing Model to another persisted Provider relationship", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([
    {
      id: "custom-provider",
      name: "Custom Team",
      provider: "custom",
      base_url: "https://gateway.example.test",
      protocols: { "openai-responses": { endpoint_path: "/v1/responses" } },
      credential_saved: false,
      model_count: 1,
      model_discovery_supported: false,
    },
    {
      id: "openrouter-provider",
      name: "OpenRouter Team",
      provider: "openrouter",
      base_url: "https://openrouter.ai",
      protocols: { "openai-responses": { endpoint_path: "/api/v1/responses" } },
      credential_saved: true,
      model_count: 0,
      model_discovery_supported: true,
    },
  ]);
  vi.mocked(api.listModelProfiles).mockResolvedValue([{
    id: "existing-model",
    name: "Existing Model",
    provider_id: "custom-provider",
    provider: "custom",
    protocol: "openai-responses",
    base_url: "https://gateway.example.test/v1",
    model: "existing-model-id",
    reasoning: false,
    catalog_key: "custom/existing-model-id",
    credential_saved: false,
  }]);
  const user = userEvent.setup();
  const consumptionState = { plan: null, planUpdate: vi.fn() } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "打开模型 Existing Model 详情" }));
  await user.click(screen.getByRole("button", { name: "编辑" }));
  await waitFor(() => expect(screen.getByRole("combobox", { name: "模型提供商" })).toBeVisible());
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  const editor = screen.getByRole("complementary", { name: "编辑模型 详情" });
  expect(editor).toBeVisible();
  expect(within(editor).getByRole("heading", { name: "编辑模型" })).toBeVisible();
  expect(within(editor).getByText("模型必须引用一个 Provider；连接与凭据由 Provider 统一管理。")).toBeVisible();
  expect(within(editor).queryByText("编辑 · OpenAI Responses")).not.toBeInTheDocument();
  expect(screen.queryByRole("dialog", { name: "编辑模型" })).not.toBeInTheDocument();

  const provider = screen.getByRole("combobox", { name: "模型提供商" });
  expect(provider).toHaveTextContent("Custom Team");
  await chooseFormSelect(user, "模型提供商", "OpenRouter Team");
  expect(provider).toHaveTextContent("OpenRouter Team");
  expect(screen.getByLabelText("完整请求 URL")).toHaveValue(
    "https://openrouter.ai/api/v1/responses",
  );
  await user.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(consumptionState.planUpdate).toHaveBeenCalledWith(
    expect.objectContaining({
      existing_id: "existing-model",
      profile: expect.objectContaining({ provider_id: "openrouter-provider" }),
    }),
  ));
});

it("discovers provider models when creating and keeps manual Model ID input authoritative", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "openrouter-team-a",
    name: "OpenRouter Team",
    provider: "openrouter",
    base_url: "https://openrouter.ai",
    protocols: { "openai-responses": { endpoint_path: "/api/v1/responses" } },
    credential_saved: true,
    model_count: 0,
    model_discovery_supported: true,
  }]);
  vi.mocked(api.discoverProviderModels).mockResolvedValue([
    { id: "anthropic/claude-sonnet-4", name: "Claude Sonnet 4", context_length: 200000 },
    { id: "openai/gpt-5", name: "GPT-5", context_length: 400000 },
  ]);
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "model-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "添加模型" }));
  await waitFor(() => expect(api.discoverProviderModels).toHaveBeenCalledWith("openrouter-team-a"));
  expect(await screen.findByRole("option", { name: /Claude Sonnet 4.*anthropic\/claude-sonnet-4/ })).toBeVisible();
  expect(screen.queryByText("找到 2 个模型")).not.toBeInTheDocument();

  const modelId = screen.getByRole("combobox", { name: "模型 ID" });
  const modelPicker = modelId.closest(".arca-provider-model-picker");
  expect(modelPicker).toHaveClass("arca-model-form-wide");
  expect(modelPicker?.parentElement).toHaveClass("arca-model-form-grid");
  expect(screen.getByRole("combobox", { name: "协议" }).closest(".arca-model-form-grid"))
    .toBe(modelPicker?.parentElement);
  expect(css).toMatch(/\.arca-provider-model-picker\s*\{[^}]*grid-column: 1 \/ -1/);
  await user.type(modelId, "claude");
  await user.click(screen.getByRole("option", { name: /Claude Sonnet 4.*anthropic\/claude-sonnet-4/ }));
  expect(modelId).toHaveValue("anthropic/claude-sonnet-4");
  expect(screen.getByLabelText("名称（可选）")).toHaveValue("");
  expect(screen.getByLabelText("上下文窗口")).toHaveValue(200000);

  await user.clear(modelId);
  await user.type(modelId, "my-private-model-id");
  expect(screen.getByLabelText("上下文窗口")).toHaveValue(null);
  await user.click(screen.getByRole("button", { name: "添加" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    profile: expect.objectContaining({
      provider_id: "openrouter-team-a",
      model: "my-private-model-id",
      context_window: undefined,
    }),
  })));
});

it("tries discovery for a custom Provider and keeps manual save available after failure", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "custom-gateway",
    name: "Custom Gateway",
    provider: "custom",
    base_url: "https://gateway.example.test",
    protocols: { "openai-responses": { endpoint_path: "/v1/responses" } },
    credential_saved: false,
    model_count: 0,
    model_discovery_supported: true,
  }]);
  vi.mocked(api.discoverProviderModels).mockRejectedValue(
    new Error("model_discovery_http: Provider model catalog returned HTTP 404"),
  );
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "manual-model-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "添加模型" }));
  await waitFor(() => expect(api.discoverProviderModels).toHaveBeenCalledWith("custom-gateway"));
  expect(await screen.findByText(/无法获取模型列表.*仍可手动填写/)).toBeVisible();
  expect(screen.getByRole("button", { name: "刷新模型列表" })).toBeVisible();
  const modelId = screen.getByRole("combobox", { name: "模型 ID" });
  await user.type(modelId, "gpt-manual-fallback");
  expect(modelId).toHaveValue("gpt-manual-fallback");
  const add = screen.getByRole("button", { name: "添加" });
  expect(add).toBeEnabled();
  await user.click(add);
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    profile: expect.objectContaining({ model: "gpt-manual-fallback" }),
  })));
});

it("keeps Azure deployment names manual and never probes an unsupported catalog", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "azure-resource", name: "Azure resource", provider: "azure-openai",
    base_url: "https://resource.openai.azure.com/openai/v1",
    protocols: { "openai-responses": { endpoint_path: "/responses" } },
    credential_saved: true, model_count: 0, model_discovery_supported: false,
  }]);
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "azure-model" });
  render(<ToastProvider><ModelsView consumptionState={{ plan: null, planUpdate } as unknown as ConsumptionState} /></ToastProvider>);
  await user.click(await screen.findByRole("button", { name: "添加模型" }));
  expect(screen.getByText(/填写 Azure 中已创建的部署名称/)).toBeVisible();
  expect(screen.queryByRole("button", { name: "刷新模型列表" })).not.toBeInTheDocument();
  await user.type(screen.getByRole("textbox", { name: "模型 ID" }), "production-deployment");
  expect(api.discoverProviderModels).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "添加" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    profile: expect.objectContaining({ model: "production-deployment", provider_id: "azure-resource" }),
  })));
});

it("shows cloud setup guidance without saving an account placeholder as an endpoint", async () => {
  vi.mocked(api.listModelProviders).mockResolvedValue([{
    id: "azure-openai", name: "Azure OpenAI", default_base_url: null,
    default_protocol: "openai-responses", additional_endpoints: [], base_url: null,
    protocols: { "openai-responses": { endpoint_path: "/responses" } },
    category: "official", model_discovery_supported: false,
    setup: { base_url_placeholder: "https://<resource>.openai.azure.com/openai/v1", hint: "azure" },
  }]);
  const user = userEvent.setup();
  render(<ToastProvider><ModelsView consumptionState={{ plan: null, planUpdate: vi.fn() } as unknown as ConsumptionState} /></ToastProvider>);
  await openProviderTemplate(user, "Azure OpenAI");
  const base = screen.getByLabelText("Base URL");
  expect(base).toHaveValue("");
  expect(base).toHaveAttribute("placeholder", "https://<resource>.openai.azure.com/openai/v1");
  expect(screen.getByText(/填写 Azure 资源的/)).toBeVisible();
  await user.type(screen.getByLabelText("API Key"), "fixture-only-key");
  fireEvent.change(base, { target: { value: "https://<resource>.openai.azure.com/openai/v1" } });
  expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
  fireEvent.change(base, { target: { value: "https://resource.openai.azure.com/openai/v1" } });
  expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
});

it("discovers provider models on refresh but never lets a stale Provider response win", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([
    {
      id: "openrouter-team-a",
      name: "OpenRouter Team",
      provider: "openrouter",
      base_url: "https://openrouter.ai",
      protocols: { "openai-responses": { endpoint_path: "/api/v1/responses" } },
      credential_saved: true,
      model_count: 0,
      model_discovery_supported: true,
    },
    {
      id: "openai-personal",
      name: "OpenAI Personal",
      provider: "openai",
      base_url: "https://api.openai.com",
      protocols: { "openai-responses": { endpoint_path: "/v1/responses" } },
      credential_saved: true,
      model_count: 0,
      model_discovery_supported: true,
    },
  ]);
  let resolveOpenRouter: ((models: Array<{ id: string; name?: string; context_length?: number }>) => void) | undefined;
  let resolveOpenAi: ((models: Array<{ id: string; name?: string; context_length?: number }>) => void) | undefined;
  vi.mocked(api.discoverProviderModels).mockImplementation((providerId) => new Promise((resolveModels) => {
    if (providerId === "openrouter-team-a") resolveOpenRouter = resolveModels;
    else resolveOpenAi = resolveModels;
  }));
  const user = userEvent.setup();
  const consumptionState = {
    plan: null,
    planUpdate: vi.fn().mockResolvedValue({ operation_id: "stale-model-plan" }),
  } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "添加模型" }));
  await waitFor(() => expect(resolveOpenRouter).toBeTypeOf("function"));
  await chooseFormSelect(user, "模型提供商", "OpenAI Personal");
  await waitFor(() => expect(resolveOpenAi).toBeTypeOf("function"));

  resolveOpenAi?.([{ id: "gpt-current", name: "Current GPT" }]);
  expect(await screen.findByRole("option", { name: /Current GPT.*gpt-current/ })).toBeVisible();
  resolveOpenRouter?.([{ id: "stale-openrouter", name: "Stale OpenRouter" }]);
  await waitFor(() => expect(screen.queryByText(/Stale OpenRouter/)).not.toBeInTheDocument());

  await user.click(screen.getByRole("button", { name: "刷新模型列表" }));
  await waitFor(() => expect(api.discoverProviderModels).toHaveBeenCalledTimes(3));
});

it("discovers provider models only after refresh when editing an existing Model", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "openrouter-team-a",
    name: "OpenRouter Team",
    provider: "openrouter",
    base_url: "https://openrouter.ai",
    protocols: { "openai-responses": { endpoint_path: "/api/v1/responses" } },
    credential_saved: true,
    model_count: 1,
    model_discovery_supported: true,
  }]);
  vi.mocked(api.listModelProfiles).mockResolvedValue([{
    id: "existing-model",
    name: "Existing Model",
    provider_id: "openrouter-team-a",
    provider: "openrouter",
    protocol: "openai-responses",
    base_url: "https://openrouter.ai",
    model: "existing/model",
    catalog_key: "openrouter/existing/model",
    credential_saved: true,
  }]);
  vi.mocked(api.discoverProviderModels).mockResolvedValue([
    { id: "new/model", name: "New Model" },
  ]);
  const user = userEvent.setup();
  const consumptionState = {
    plan: null,
    planUpdate: vi.fn().mockResolvedValue({ operation_id: "edit-model-plan" }),
  } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "打开模型 Existing Model 详情" }));
  await user.click(screen.getByRole("button", { name: "编辑" }));
  await waitFor(() => expect(screen.getByRole("combobox", { name: "模型 ID" })).toBeVisible());
  expect(api.discoverProviderModels).not.toHaveBeenCalled();

  await user.click(screen.getByRole("button", { name: "刷新模型列表" }));
  await waitFor(() => expect(api.discoverProviderModels).toHaveBeenCalledWith("openrouter-team-a"));
  expect(await screen.findByRole("option", { name: /New Model.*new\/model/ })).toBeVisible();
});

it("submits an independent custom Provider through the central asset plan", async () => {
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "model-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "Custom Provider");
  await user.type(
    screen.getByLabelText("Base URL"),
    "https://models.example.test/v1/",
  );
  await user.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledTimes(1));
  expect(planUpdate).toHaveBeenCalledWith({
    domain: "model-provider",
    existing_id: undefined,
    provider: expect.objectContaining({
      id: "",
      provider: "custom",
      base_url: "https://models.example.test/v1",
      protocols: expect.objectContaining({
        "openai-responses": { endpoint_path: "/responses" },
      }),
    }),
    credential: undefined,
  });
});

it("uses the Google template default for new Models and saves an explicit protocol choice", async () => {
  const protocols = {
    "gemini-interactions": { endpoint_path: "/v1beta/interactions" },
    "gemini-generate-content": { endpoint_path: "/v1beta/models/{model}:generateContent" },
    "openai-completions": { endpoint_path: "/v1beta/openai/chat/completions" },
  };
  vi.mocked(api.listModelProviders).mockResolvedValue([{
    id: "google", name: "Google AI Studio", base_url: "https://generativelanguage.googleapis.com",
    default_base_url: "https://generativelanguage.googleapis.com/v1beta",
    default_protocol: "gemini-interactions", protocols, additional_endpoints: [],
    category: "official", model_discovery_supported: true,
  }]);
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "google-work", name: "Google Work", provider: "google", auth_requirement: "required",
    base_url: "https://generativelanguage.googleapis.com", protocols,
    credential_saved: false, model_count: 0, model_discovery_supported: false,
  }]);
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "google-model" });
  const user = userEvent.setup();
  const view = render(<ToastProvider><ModelsView consumptionState={{ plan: null, planUpdate } as unknown as ConsumptionState} /></ToastProvider>);
  const sidebar = within(view.container.querySelector(".arca-workspace-sidebar") as HTMLElement);
  await user.click(await sidebar.findByTitle("Google Work"));
  await user.click(screen.getByRole("button", { name: /手动添加 Model/ }));
  expect(screen.getByRole("combobox", { name: "协议" })).toHaveTextContent("Gemini Interactions");
  expect(screen.getByText("https://generativelanguage.googleapis.com/v1beta/interactions")).toBeVisible();
  await chooseFormSelect(user, "协议", "Gemini generateContent");
  await user.type(screen.getByRole("textbox", { name: "模型 ID" }), "gemini-example");
  await user.click(screen.getByRole("button", { name: "添加" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    profile: expect.objectContaining({ protocol: "gemini-generate-content", provider_id: "google-work" }),
  }), expect.anything()));
});

it("opens the Google Provider editor on its Interactions endpoint", async () => {
  vi.mocked(api.listModelProviders).mockResolvedValue([{
    id: "google", name: "Google AI Studio", base_url: "https://generativelanguage.googleapis.com",
    default_base_url: "https://generativelanguage.googleapis.com/v1beta", default_protocol: "gemini-interactions",
    protocols: {
      "gemini-interactions": { endpoint_path: "/v1beta/interactions" },
      "gemini-generate-content": { endpoint_path: "/v1beta/models/{model}:generateContent" },
      "openai-completions": { endpoint_path: "/v1beta/openai/chat/completions" },
    },
    additional_endpoints: [], category: "official", model_discovery_supported: true,
  }]);
  const user = userEvent.setup();
  render(<ToastProvider><ModelsView consumptionState={{ plan: null } as unknown as ConsumptionState} /></ToastProvider>);
  await openProviderTemplate(user, "Google AI Studio");
  expect(screen.getByLabelText("完整请求 URL")).toHaveTextContent(
    "https://generativelanguage.googleapis.com/v1beta/interactions",
  );
});

it("configures a Gemini native GenerateContent endpoint on a custom Provider", async () => {
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "gemini-provider-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await openProviderTemplate(user, "Custom Provider");
  fireEvent.change(screen.getByLabelText("Base URL"), {
    target: { value: "http://127.0.0.1:18080/v1beta" },
  });
  const geminiSwitch = screen.getByRole("switch", { name: "Gemini generateContent" });
  await user.click(geminiSwitch);
  const protocolRow = geminiSwitch.closest("article") as HTMLElement;
  await user.click(within(protocolRow).getByRole("button", { name: /Gemini generateContent/ }));
  expect(screen.getByLabelText("完整请求 URL")).toHaveTextContent(
    "http://127.0.0.1:18080/v1beta/models/{model}:generateContent",
  );
  await user.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model-provider",
    provider: expect.objectContaining({
      provider: "custom",
      base_url: "http://127.0.0.1:18080/v1beta",
      protocols: expect.objectContaining({
        "gemini-generate-content": {
          endpoint_path: "/models/{model}:generateContent",
        },
      }),
    }),
  })));
});

it("keeps Model fields local while writing an explicit Provider reference", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "openrouter-team-a",
    name: "OpenRouter Team",
    provider: "openrouter",
    base_url: "https://openrouter.ai",
    protocols: { "openai-responses": { endpoint_path: "/api/v1/responses" } },
    credential_saved: true,
    model_count: 0,
    model_discovery_supported: true,
  }]);
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "model-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "添加模型" }));
  expect(screen.queryByText("高级设置")).not.toBeInTheDocument();
  expect(screen.getByLabelText("上下文窗口")).toHaveValue(null);
  expect(screen.getByLabelText("最大输出")).toHaveValue(null);
  expect(screen.queryByPlaceholderText("MY_API_KEY")).not.toBeInTheDocument();
  expect(screen.queryByText("API Key")).not.toBeInTheDocument();
  expect(screen.getByRole("combobox", { name: "推理" })).toHaveTextContent("自动");

  await chooseFormSelect(user, "推理", "关闭");
  await user.type(screen.getByPlaceholderText("model-name"), "explicit-reasoning-off");
  await user.click(screen.getByRole("button", { name: "添加" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledTimes(1));
  const submitted = planUpdate.mock.calls[0][0].profile;
  expect(submitted).toEqual(expect.objectContaining({
    provider_id: "openrouter-team-a",
    reasoning: false,
  }));
  expect(submitted).not.toHaveProperty("context_window");
  expect(submitted).not.toHaveProperty("max_output_tokens");
});

it("adds another Model to an existing Provider without repeating shared connection fields", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "openrouter-team-a",
    name: "OpenRouter Team",
    provider: "openrouter",
    base_url: "https://openrouter.ai",
    protocols: {
      "openai-completions": { endpoint_path: "/api/v1/chat/completions" },
    },
    credential_saved: true,
    model_count: 1,
    model_discovery_supported: true,
  }]);
  vi.mocked(api.listModelProfiles).mockResolvedValue([{
    id: "existing-openrouter-model",
    name: "Existing OpenRouter Model",
    provider_id: "openrouter-team-a",
    provider: "openrouter",
    protocol: "openai-completions",
    base_url: "https://openrouter.ai/api/v1",
    model: "openrouter/free",
    catalog_key: "openrouter/openrouter/free",
    credential_saved: true,
  }]);
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "model-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  const view = render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );

  const sidebar = within(view.container.querySelector(".arca-workspace-sidebar") as HTMLElement);
  await user.click(await sidebar.findByTitle("OpenRouter Team"));
  await user.click(screen.getByRole("button", { name: "添加模型" }));

  expect(screen.getByRole("heading", { name: "添加模型" })).toBeVisible();
  const provider = screen.getByRole("textbox", { name: "模型提供商" });
  expect(provider).toHaveValue("OpenRouter Team");
  expect(provider).toHaveAttribute("readonly");
  expect(screen.queryByRole("combobox", { name: "模型提供商" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Base URL")).not.toBeInTheDocument();
  expect(screen.queryByText("API Key")).not.toBeInTheDocument();
  expect(screen.queryByText("API Key 环境变量")).not.toBeInTheDocument();

  fireEvent.change(screen.getByPlaceholderText("model-name"), {
    target: { value: "anthropic/claude-sonnet-4" },
  });
  await user.click(screen.getByRole("button", { name: "添加" }));
  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith(expect.objectContaining({
    domain: "model",
    profile: expect.objectContaining({
      provider_id: "openrouter-team-a",
      provider: "openrouter",
      protocol: "openai-completions",
      base_url: "https://openrouter.ai",
      model: "anthropic/claude-sonnet-4",
    }),
  })));
});

it("edits one Provider configuration through a single central asset plan", async () => {
  vi.mocked(api.listModelProviderInstances).mockResolvedValue([{
    id: "openrouter-team-a",
    name: "OpenRouter Team",
    provider: "openrouter",
    base_url: "https://openrouter.ai",
    protocols: {
      "openai-completions": { endpoint_path: "/api/v1/chat/completions" },
      "anthropic-messages": { endpoint_path: "/api/v1/messages" },
    },
    env_key: "OPENROUTER_API_KEY",
    credential_saved: true,
    model_count: 3,
    model_discovery_supported: true,
  }]);
  const user = userEvent.setup();
  const planUpdate = vi.fn().mockResolvedValue({ operation_id: "provider-plan" });
  const consumptionState = { plan: null, planUpdate } as unknown as ConsumptionState;

  const view = render(
    <ToastProvider>
      <ModelsView consumptionState={consumptionState} />
    </ToastProvider>,
  );
  const sidebar = within(view.container.querySelector(".arca-workspace-sidebar") as HTMLElement);
  await user.click(await sidebar.findByTitle("OpenRouter Team"));
  await user.click(screen.getByRole("button", { name: "编辑 Provider" }));

  expect(screen.getByRole("switch", { name: /Anthropic Messages/ })).toBeChecked();
  expect(screen.getByRole("switch", { name: /OpenAI Chat Completions/ })).toBeChecked();
  const baseUrl = screen.getByLabelText("Base URL");
  expect(baseUrl).toHaveValue("https://openrouter.ai");
  fireEvent.change(baseUrl, {
    target: { value: "https://gateway.example.test/v2/" },
  });
  fireEvent.change(screen.getByLabelText("Anthropic Messages Endpoint Path"), {
    target: { value: "/anthropic/v1/messages" },
  });
  await user.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(planUpdate).toHaveBeenCalledWith({
    domain: "model-provider",
    existing_id: "openrouter-team-a",
    provider: expect.objectContaining({
      id: "openrouter-team-a",
      name: "OpenRouter Team",
      provider: "openrouter",
      base_url: "https://gateway.example.test/v2",
      protocols: expect.objectContaining({
        "openai-completions": { endpoint_path: "/api/v1/chat/completions" },
        "anthropic-messages": { endpoint_path: "/anthropic/v1/messages" },
      }),
      env_key: "OPENROUTER_API_KEY",
    }),
    credential: undefined,
  }));
});

it("routes profile lifecycle through central asset plans", () => {
  expect(source).toMatch(/consumptionState\.planUpdate/);
  expect(source).toMatch(/consumptionState\.planDelete/);
  expect(source).not.toMatch(/saveModelProfile|deleteModelProfile|applyModelProfile/);
});

it("keeps the top-level Models workspace asset-only", () => {
  expect(source).toMatch(/searchPlaceholder=\{t\("models\.search"\)\}/);
  expect(source).toMatch(/label=\{t\("models\.asset"\)\}/);
  expect(source).not.toMatch(/listModelAgents|planForAgent|planForAsset/);
  expect(source).not.toMatch(/AssetConsumerDialog|管理 Agent|Agent 模型|使用中|未使用/);
  expect(css).not.toMatch(/\.arca-model-agent-grid/);
});

it("owns multi-Model display and switching inside the Agent panel", () => {
  expect(agentSource).toMatch(/title="Models"/);
  expect(agentSource).toMatch(/同一时间使用其中一个/);
  expect(agentSource).not.toMatch(/planModelEnabled/);
  expect(agentSource).toMatch(/setActiveModel/);
  expect(agentSource).toMatch(/toggleKind="current"/);
  expect(agentSource).not.toMatch(/设为当前/);
});
