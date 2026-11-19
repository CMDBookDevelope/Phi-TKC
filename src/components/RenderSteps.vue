<i18n>
en:
  progress: Render progress
  steps: [Chart info, Parameters, Rendering]
zh-CN:
  progress: 渲染步骤
  steps: [配置谱面, 渲染参数, 渲染视频]
</i18n>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';

const props = defineProps<{ step: number }>();
const { t } = useI18n();
</script>

<template>
  <div class="render-step-group" role="group" :aria-label="t('progress')" :style="{ '--current-step': props.step }">
    <span class="step-pill-indicator" aria-hidden="true">
      <span class="step-label-window">
        <Transition name="step-label">
          <span :key="props.step" class="step-pill-label">{{ t(`steps[${props.step}]`) }}</span>
        </Transition>
      </span>
    </span>
    <span v-for="index in 3" :key="index" class="step-dot-slot"
      :class="{ 'is-current': props.step === index - 1, 'is-done': props.step > index - 1 }"
      :aria-current="props.step === index - 1 ? 'step' : undefined"
      :aria-label="t(`steps[${index - 1}]`)" :title="t(`steps[${index - 1}]`)">
      <span class="step-dot"></span>
    </span>
  </div>
</template>

<style scoped>
.render-step-group {
  --step-open-width: 132px;
  --step-closed-width: 24px;
  --step-gap: 8px;
  --step-easing: cubic-bezier(0.65, 0, 0.35, 1);
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--step-gap);
  flex: 0 0 auto;
  width: calc(var(--step-open-width) + 2 * var(--step-closed-width) + 2 * var(--step-gap));
  height: 30px;
}
.step-dot-slot {
  position: relative;
  flex: 0 0 auto;
  width: var(--step-closed-width);
  height: 30px;
  z-index: 1;
  transition: width 0.5s var(--step-easing);
}
.step-dot-slot.is-current { width: var(--step-open-width); }
.step-pill-indicator {
  position: absolute;
  left: calc(var(--current-step) * (var(--step-closed-width) + var(--step-gap)));
  top: 1px;
  width: var(--step-open-width);
  height: 28px;
  overflow: hidden;
  border-radius: 999px;
  border: 1px solid rgba(var(--main-color-rgb), 0.16);
  background: rgba(var(--main-color-rgb), 0.12);
  transition: left 0.5s var(--step-easing);
}
.step-label-window {
  position: absolute;
  left: calc(var(--step-closed-width) + 4px);
  right: 8px;
  top: 0;
  bottom: 0;
  overflow: hidden;
}
.step-pill-label {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  white-space: nowrap;
  font-size: 13px;
  font-weight: 500;
  color: var(--main-color);
}
.step-label-enter-active,
.step-label-leave-active {
  transition: opacity 0.5s var(--step-easing), transform 0.5s var(--step-easing);
}
.step-label-enter-from { opacity: 0; transform: translateX(18px); }
.step-label-leave-to { opacity: 0; transform: translateX(-18px); }
.step-dot {
  position: absolute;
  left: calc(var(--step-closed-width) / 2 - 4px);
  top: 11px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: rgba(var(--main-color-rgb), 0.3);
  transition: background-color 0.5s var(--step-easing), transform 0.5s var(--step-easing), box-shadow 0.5s var(--step-easing);
}
.is-done .step-dot { background: var(--main-color); }
.is-current .step-dot {
  background: var(--sub-color);
  transform: scale(1.15);
  box-shadow: 0 0 8px rgba(var(--sub-color-rgb), 0.4);
}
@media (max-width: 600px) {
  .render-step-group {
    --step-open-width: 112px;
    --step-closed-width: 18px;
    --step-gap: 6px;
  }
}
@media (prefers-reduced-motion: reduce) {
  .step-dot-slot,
  .step-pill-indicator,
  .step-label-enter-active,
  .step-label-leave-active,
  .step-dot { transition: none; }
}
</style>
