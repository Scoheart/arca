<script setup lang="ts">
import { computed } from "vue";
import { withBase } from "vitepress";
import reference from "../../reference.json";
const props = defineProps<{ lang?: "zh" | "en" }>();
const en = computed(() => props.lang === "en");
const prefix = computed(() => en.value ? '/en/guide/' : '/guide/');
const rows = computed(() => en.value ? [
  { title: 'One connection. Several Agents.', desc: 'Create a Provider once, add Models, and choose which compatible Agents use them. Account portals and protocol paths stay visible and editable.', href: 'models', cta: 'Models & Providers', image: 'provider' },
  { title: 'A Skill library with clear sources.', desc: 'Download or import one central copy. Browse by source, read three-line descriptions, and open consumers through their Agent icons.', href: 'skills', cta: 'Manage Skills', image: 'skills' },
  { title: 'Tools in their native configuration.', desc: 'Add, paste, import, or subscribe to MCPs. Assign from the central library while preserving each Agent’s unrelated settings.', href: 'desktop', cta: 'Desktop guide', image: 'mcps' },
] : [
  { title: '一份连接，交给多个 Agent。', desc: '创建 Provider、添加模型，再选择兼容 Agent。服务商入口、模型列表与协议路径直接展示，随时可以编辑。', href: 'models', cta: 'Models 与 Providers', image: 'provider' },
  { title: '每个 Skill，都能找到来源。', desc: '下载或导入一份中央副本。左侧按来源浏览，卡片展示三行简介，Agent 图标直接连接到使用它的客户端。', href: 'skills', cta: '管理 Skills', image: 'skills' },
  { title: '让工具进入原生配置。', desc: '添加、粘贴、导入或订阅 MCPs，再从中央库分配给 Agent。原生格式由 Arca 适配，无关设置继续保留。', href: 'desktop', cta: '桌面 App 指南', image: 'mcps' },
]);
</script>

<template>
  <div class="arca-home">
    <section id="product-demo" class="arca-product-demo" aria-labelledby="demo-title">
      <div class="arca-product-demo__meta"><span>v{{ reference.version }} · macOS Apple Silicon</span><a :href="withBase(prefix + 'agents')">{{ reference.agents.length }} {{ en ? 'audited Agents' : '个核验 Agent' }} →</a></div>
      <h2 id="demo-title">{{ en ? 'Your Agent assets. One library.' : 'Agent 资产，一个库。' }}</h2>
      <p>{{ en ? 'Models, MCPs, Skills, and an Agent picker — in one desktop workspace.' : 'Models、MCPs、Skills 与 Agent 选择器，都在同一个桌面工作区。' }}</p>
      <div class="arca-asset-map" :aria-label="en ? 'Asset management diagram' : '资产管理流程图'">
        <div class="arca-asset-map__library"><span>Models</span><span>MCPs</span><span>Skills</span></div>
        <div class="arca-asset-map__arrow" aria-hidden="true">↓</div>
        <strong>{{ en ? 'Your Agents' : '你的 Agents' }}</strong>
        <small>{{ en ? 'Create once. Assign where needed.' : '创建一次，按需分配。' }}</small>
      </div>
    </section>
    <section class="arca-story">
      <header class="arca-story__header" data-reveal><h2 class="arca-story__heading">{{ en ? 'Configure once. Choose where it goes.' : '配置一次，决定交给谁。' }}</h2></header>
      <article v-for="(row, i) in rows" :key="row.href" class="arca-row" :class="{ 'is-reverse': i % 2 === 1 }" data-reveal>
        <div class="arca-row__visual arca-asset-map arca-asset-map--compact">
          <span class="arca-eyebrow">ARCA</span>
          <strong>{{ row.image === 'provider' ? 'Provider → Models' : row.image === 'skills' ? 'Source → Skills' : 'Source → MCPs' }}</strong>
          <span class="arca-asset-map__arrow" aria-hidden="true">↓</span>
          <span>{{ en ? 'Compatible Agents' : '兼容的 Agents' }}</span>
        </div>
        <div class="arca-row__text"><h3 class="arca-row__title">{{ row.title }}</h3><p class="arca-row__desc">{{ row.desc }}</p><a class="arca-try" :href="withBase(prefix + row.href)">{{ row.cta }} <span aria-hidden="true">→</span></a></div>
      </article>
    </section>
    <section class="arca-automation" data-reveal>
      <div><span class="arca-eyebrow">DESKTOP + CLI</span><h2>{{ en ? 'Give your Agent the same controls.' : '也让你的 Agent 操作这份资源库。' }}</h2><p>{{ en ? 'Batch status reads, inspect local traces, and review a plan before committing it in the original process. Desktop and CLI share the same assets and safety rules.' : '批量查询状态、读取本地 Trace，把计划交给人或其他 Agent 审阅，再执行原计划。桌面与 CLI 共用资产和写入规则。' }}</p><a class="arca-try" :href="withBase(prefix + 'cli')">{{ en ? 'CLI reference' : 'CLI 使用指南' }} →</a></div>
      <pre><code>arca status --agent codex --agent opencode --json
arca trace list --agent codex --json
arca operation review --file request.json --json</code></pre>
    </section>
    <section class="arca-cta" data-reveal><h2 class="arca-cta__title">{{ en ? 'Start with the Agents you already use.' : '从你正在使用的 Agent 开始。' }}</h2><p class="arca-cta__desc">{{ en ? 'Install Arca, create a central asset, then choose its consumers.' : '安装 Arca，创建一份中央资产，再选择需要使用它的客户端。' }}</p><div class="arca-cta__actions"><a class="arca-btn arca-btn--brand" :href="withBase(prefix + 'install')">{{ en ? 'Install Arca' : '安装 Arca' }}</a><a class="arca-btn arca-btn--alt" :href="withBase(prefix + 'providers')">{{ en ? 'Provider templates' : 'Provider 模板' }}</a></div></section>
  </div>
</template>

<style scoped>
.arca-asset-map { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 18px; padding: 40px 24px; border: 1px solid var(--vp-c-divider); border-radius: 18px; background: var(--vp-c-bg-soft); text-align: center; }
.arca-asset-map__library { display: flex; flex-wrap: wrap; justify-content: center; gap: 14px; }
.arca-asset-map__library > span { padding: 18px 28px; border: 1px solid var(--vp-c-divider); border-radius: 12px; background: var(--vp-c-bg); font-size: 18px; font-weight: 600; }
.arca-asset-map__arrow { color: var(--vp-c-text-3); font-size: 28px; }
.arca-asset-map > strong { font-size: 22px; font-weight: 600; }
.arca-asset-map > small { color: var(--vp-c-text-2); }
.arca-asset-map--compact { min-height: 260px; }
</style>
