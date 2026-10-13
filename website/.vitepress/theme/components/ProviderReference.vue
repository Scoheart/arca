<script setup lang="ts">
import { computed, ref } from "vue";
import reference from "../../reference.json";
const props = defineProps<{ lang?: "zh" | "en" }>();
const query = ref("");
const en = computed(() => props.lang === "en");
const rows = computed(() => reference.providers.filter(p => `${p.name} ${p.category}`.toLowerCase().includes(query.value.trim().toLowerCase())));
</script>
<template>
  <div class="arca-reference">
    <div class="arca-reference__toolbar"><input v-model="query" type="search" :aria-label="en ? 'Search Providers' : '搜索 Provider'" :placeholder="en ? 'Search provider or category' : '搜索服务商或类别'" /><span aria-live="polite">{{ rows.length }} {{ en ? 'templates' : '个模板' }}</span></div>
    <div class="arca-reference__scroll" tabindex="0" :aria-label="en ? 'Provider template table' : 'Provider 模板表'">
      <table><thead><tr><th>Provider</th><th>{{ en ? 'Category' : '类别' }}</th><th>Base URL</th><th>{{ en ? 'Links' : '入口' }}</th></tr></thead><tbody>
        <tr v-for="p in rows" :key="p.id"><td>{{ p.name }}<code class="arca-reference__id">{{ p.id }}</code></td><td>{{ p.category }}</td><td><code>{{ p.baseUrl ?? (en ? 'Set for your account' : '按账号填写') }}</code></td><td><a v-if="p.docs" :href="p.docs" target="_blank" rel="noopener noreferrer">{{ en ? 'Docs' : '文档' }}</a><a v-if="p.portal" :href="p.portal.url" target="_blank" rel="noopener noreferrer">{{ en ? 'Open' : '打开入口' }}</a></td></tr>
      </tbody></table><p v-if="!rows.length">{{ en ? 'No matching Provider.' : '没有匹配的 Provider。' }}</p>
    </div>
  </div>
</template>
