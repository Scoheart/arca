import { defineConfig } from "vitepress";
import reference from "./reference.json";

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "Arca",
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: ["README.md"],
  sitemap: { hostname: "https://mux.scoheart.com" },

  head: [
    ["meta", { name: "theme-color", content: "#7C5CFC" }],
    ["link", { rel: "icon", href: "/favicon.png", type: "image/png" }],
    ["meta", { property: "og:title", content: "Arca — Agent Asset Manager" }],
    ["meta", { property: "og:image", content: "https://mux.scoheart.com/favicon.png" }],
    [
      "meta",
      {
        property: "og:description",
        content: "Manage Models, MCPs, Skills, and local Agent traces from one workspace.",
      },
    ],
  ],

  themeConfig: {
    socialLinks: [{ icon: "github", link: "https://github.com/Scoheart/mux" }],
    search: { provider: "local" },
    externalLinkIcon: true,
  },

  locales: {
    root: {
      label: "简体中文",
      lang: "zh-CN",
      description: "集中管理 Models、MCPs、Skills 与本地 Agent Trace · Arca 官方文档",
      themeConfig: {
        nav: [
          { text: "首页", link: "/" },
          { text: "指南", link: "/guide/what-is-arca", activeMatch: "/guide/" },
          { text: "支持的 Agent", link: "/guide/agents" },
          { text: "演示", link: "/guide/demo" },
          { text: `v${reference.version}`, link: "https://github.com/Scoheart/mux/releases/latest" },
        ],
        sidebar: {
          "/guide/": [
            {
              text: "开始",
              items: [
                { text: "Arca 是什么", link: "/guide/what-is-arca" },
                { text: "安装", link: "/guide/install" },
                { text: "核心概念", link: "/guide/concepts" },
              ],
            },
            {
              text: "使用",
              items: [
                { text: "桌面 App 指南", link: "/guide/desktop" },
                { text: "Models 与 Providers", link: "/guide/models" },
                { text: "Skills", link: "/guide/skills" },
                { text: "Agent Trace 与网络记录", link: "/guide/traces" },
                { text: "演示视频", link: "/guide/demo" },
                { text: "命令行 / TUI", link: "/guide/cli" },
                { text: "支持的 Agent", link: "/guide/agents" },
              ],
            },
            {
              text: "参考",
              items: [{ text: "Provider 模板", link: "/guide/providers" }, { text: "常见问题", link: "/guide/faq" }],
            },
          ],
        },
        editLink: {
          pattern: "https://github.com/Scoheart/mux/edit/main/website/:path",
          text: "在 GitHub 上编辑此页",
        },
        docFooter: { prev: "上一页", next: "下一页" },
        outline: { label: "本页导航", level: [2, 3] },
        lastUpdatedText: "最后更新",
        returnToTopLabel: "回到顶部",
        darkModeSwitchLabel: "外观",
        lightModeSwitchTitle: "切换到浅色模式",
        darkModeSwitchTitle: "切换到深色模式",
        sidebarMenuLabel: "菜单",
        langMenuLabel: "切换语言",
        footer: {
          message: "MIT Licensed",
          copyright: "© 2026 Scoheart · Arca",
        },
      },
    },

    en: {
      label: "English",
      lang: "en-US",
      link: "/en/",
      description: "Manage Models, MCPs, Skills, and local Agent traces · Official Arca docs",
      themeConfig: {
        nav: [
          { text: "Home", link: "/en/" },
          { text: "Guide", link: "/en/guide/what-is-arca", activeMatch: "/en/guide/" },
          { text: "Agents", link: "/en/guide/agents" },
          { text: "Demo", link: "/en/guide/demo" },
          { text: `v${reference.version}`, link: "https://github.com/Scoheart/mux/releases/latest" },
        ],
        sidebar: {
          "/en/guide/": [
            {
              text: "Getting started",
              items: [
                { text: "What is Arca", link: "/en/guide/what-is-arca" },
                { text: "Installation", link: "/en/guide/install" },
                { text: "Core concepts", link: "/en/guide/concepts" },
              ],
            },
            {
              text: "Usage",
              items: [
                { text: "Desktop app", link: "/en/guide/desktop" },
                { text: "Models & Providers", link: "/en/guide/models" },
                { text: "Skills", link: "/en/guide/skills" },
                { text: "Agent Trace & network records", link: "/en/guide/traces" },
                { text: "Demo video", link: "/en/guide/demo" },
                { text: "CLI / TUI", link: "/en/guide/cli" },
                { text: "Supported agents", link: "/en/guide/agents" },
              ],
            },
            {
              text: "Reference",
              items: [{ text: "Provider templates", link: "/en/guide/providers" }, { text: "FAQ", link: "/en/guide/faq" }],
            },
          ],
        },
        editLink: {
          pattern: "https://github.com/Scoheart/mux/edit/main/website/:path",
          text: "Edit this page on GitHub",
        },
        footer: {
          message: "MIT Licensed",
          copyright: "© 2026 Scoheart · Arca",
        },
      },
    },
  },
});
