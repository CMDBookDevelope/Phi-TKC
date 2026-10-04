<i18n>
en:
  title: Not available on mobile
  desc: This feature needs to spawn a second process and run an external FFmpeg binary, which iOS/iPadOS and Android do not allow. Please use the desktop version of Phi-TKC to render charts.
  available-hint: "Still usable on mobile: task list, settings (theme / background / language) and about."

zh-CN:
  title: 移动端暂不支持
  desc: 该功能需要启动子进程并调用外部 FFmpeg，iOS/iPadOS 与 Android 不允许这样做。请在桌面版 Phi-TKC 上进行渲染。
  available-hint: 移动端仍可使用：任务列表、设置（主题 / 背景 / 语言）与关于。
</i18n>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';

// `feature` is the already-localised name of the page (translated by App.vue).
defineProps<{ feature: string }>();

const { t } = useI18n();
</script>

<template>
  <div class="mobile-unsupported">
    <div class="unsupported-card glass fluent-glass" v-glass>
      <v-icon icon="mdi-monitor-off" size="56" class="card-icon" />
      <h2 class="card-title">{{ feature }} · {{ t('title') }}</h2>
      <p class="card-desc">{{ t('desc') }}</p>
      <p class="card-hint">{{ t('available-hint') }}</p>
    </div>
  </div>
</template>

<style scoped>
.mobile-unsupported {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: 24px;
  padding-bottom: max(24px, env(safe-area-inset-bottom));
}

.unsupported-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  gap: 12px;
  max-width: 480px;
  padding: 40px 28px;
  border-radius: 20px;
}

.card-icon {
  color: rgba(var(--main-color-rgb), 0.85);
}

.card-title {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 600;
  color: var(--main-color);
}

.card-desc {
  margin: 0;
  line-height: 1.6;
  color: rgba(var(--main-color-rgb), 0.8);
}

.card-hint {
  margin: 0;
  font-size: 0.85rem;
  line-height: 1.5;
  color: rgba(var(--main-color-rgb), 0.55);
}
</style>
