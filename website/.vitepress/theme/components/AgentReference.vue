<script setup lang="ts">
import { computed, ref } from "vue";
import reference from "../../reference.json";
const props = defineProps<{ lang?: "zh" | "en"; modelsOnly?: boolean }>();
const query = ref("");
const scope = ref("all");
const en = computed(() => props.lang === "en");
const rows = computed(() => reference.agents.filter((agent) => {
  if (props.modelsOnly && !agent.model) return false;
  if (scope.value === "mcp" && !agent.mcp) return false;
  if (scope.value === "skill" && !agent.skills) return false;
  if (scope.value === "model" && !agent.model) return false;
  const needle = query.value.trim().toLowerCase();
  return !needle || `${agent.name} ${agent.id} ${agent.category}`.toLowerCase().includes(needle);
}));
const protocolNames: Record<string, string> = {
  "anthropic-messages": "Messages", "openai-responses": "Responses",
  "openai-completions": "Chat Completions", "gemini-generate-content": "Gemini",
};
const typeNames: Record<string, string> = { cli: "CLI", desktop: "Desktop", ide: "IDE", plugin: "Plugin" };
</script>

<template>
  <div class="arca-reference">
    <div class="arca-reference__toolbar">
      <input v-model="query" type="search" :aria-label="en ? 'Search Agents' : '搜索 Agent'" :placeholder="en ? 'Search name or ID' : '搜索名称或 ID'" />
      <select v-if="!modelsOnly" v-model="scope" :aria-label="en ? 'Capability' : '能力'">
        <option value="all">{{ en ? 'All capabilities' : '全部能力' }}</option>
        <option value="mcp">MCPs</option><option value="model">Models</option><option value="skill">Skills</option>
      </select>
      <span aria-live="polite">{{ rows.length }} {{ en ? 'Agents' : '个 Agent' }}</span>
    </div>
    <div class="arca-reference__scroll" tabindex="0" :aria-label="en ? 'Agent capability table' : 'Agent 能力表'">
      <table>
        <thead><tr><th>Agent</th><th>{{ en ? 'Type' : '形态' }}</th><th>Models</th><th v-if="!modelsOnly">MCPs</th><th v-if="!modelsOnly">Skills</th><th v-if="modelsOnly">{{ en ? 'Config / selection' : '配置与选择' }}</th></tr></thead>
        <tbody>
          <tr v-for="agent in rows" :key="agent.id">
            <td><a v-if="agent.docs" :href="agent.docs" target="_blank" rel="noopener noreferrer">{{ agent.name }}</a><span v-else>{{ agent.name }}</span><code class="arca-reference__id">{{ agent.id }}</code></td>
            <td>{{ typeNames[agent.category] ?? agent.category }}</td>
            <td v-if="agent.model"><span>{{ agent.model.mode === 'managed' ? (en ? 'Managed' : '自动配置') : (en ? 'Guided' : '官方引导') }}</span><small>{{ agent.model.protocols.map(p => protocolNames[p] ?? p).join(' · ') }}</small></td><td v-else>—</td>
            <template v-if="!modelsOnly"><td><code v-if="agent.mcp">{{ agent.mcp.path }}</code><span v-else>—</span></td><td><code v-if="agent.skills">{{ agent.skills.path }}</code><span v-else>—</span></td></template>
            <td v-else><code v-for="path in agent.model?.paths" :key="path" class="arca-reference__id">{{ path }}</code><small v-if="agent.model?.mode === 'managed'">{{ agent.model.globalSelection ? (en ? 'Global current model' : '可选择全局当前模型') : (en ? 'Select in the Agent conversation' : '在 Agent 会话中选用') }}</small></td>
          </tr>
        </tbody>
      </table>
      <p v-if="!rows.length">{{ en ? 'No matching Agent.' : '没有匹配的 Agent。' }}</p>
    </div>
  </div>
</template>
