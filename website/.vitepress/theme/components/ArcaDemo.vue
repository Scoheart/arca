<script setup lang="ts">
import { computed, ref } from "vue";
import { withBase } from "vitepress";
const props = defineProps<{ lang?: "zh" | "en" }>();
const en = computed(() => props.lang === "en");
const video = ref<HTMLVideoElement | null>(null);
const chapters = computed(() => [
  { time: 0, label: 'Models' }, { time: 6, label: 'MCPs' },
  { time: 14, label: 'Skills' }, { time: 23, label: en.value ? 'Provider editor' : 'Provider 编辑' },
  { time: 37, label: en.value ? 'Agent picker' : 'Agent 选择器' },
]);
function jump(time: number) {
  const player = video.value;
  if (!player) return;
  const seek = () => { player.currentTime = time; void player.play().catch(() => {}); };
  if (player.readyState >= 1) seek();
  else { player.addEventListener('loadedmetadata', seek, { once: true }); player.load(); }
}
</script>
<template>
  <div class="arca-demo">
    <video ref="video" controls playsinline preload="none" width="1440" height="984" :poster="withBase('/media/mux-1.10.0-models.jpg')" :aria-label="en ? 'Historical MUX 1.10.0 walkthrough' : '改名前的历史演示'">
      <source :src="withBase('/media/mux-1.10.0-overview.mp4')" type="video/mp4" />
      <track kind="captions" :src="withBase('/media/mux-1.10.0-zh.vtt')" srclang="zh-CN" label="简体中文" :default="!en" />
      <track kind="captions" :src="withBase('/media/mux-1.10.0-en.vtt')" srclang="en" label="English" :default="en" />
      {{ en ? 'Your browser cannot play this video.' : '浏览器无法播放此视频。' }}
    </video>
    <div class="arca-demo__chapters" :aria-label="en ? 'Video chapters' : '视频章节'"><button v-for="chapter in chapters" :key="chapter.time" type="button" @click="jump(chapter.time)"><span>{{ Math.floor(chapter.time / 60) }}:{{ String(chapter.time % 60).padStart(2, '0') }}</span>{{ chapter.label }}</button></div>
    <p class="arca-demo__caption">{{ en ? 'Recorded in the released 1.10.0 desktop app with public sample assets. No live model requests.' : '使用正式版 1.10.0 与公开示例资产录制；未发送实际模型请求。' }} <a :href="withBase('/media/mux-1.10.0-overview.mp4')" download>{{ en ? 'Download MP4' : '下载 MP4' }}</a></p>
  </div>
</template>
