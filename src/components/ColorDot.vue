<script setup lang="ts">
import { computed } from 'vue';

// Renders the picked colour as a round Windows-Paint-style swatch.
// (Replaces the old ColorSwatch.vue, which was left unreadable on disk.)
const props = defineProps<{
  color: string;
  size?: number;
  active?: boolean;
}>();

const wrapStyle = computed(() => ({
  width: `${props.size ?? 32}px`,
  height: `${props.size ?? 32}px`,
}));

const innerStyle = computed(() => {
  const innerSize = (props.size ?? 32) - 6;
  return {
    width: `${innerSize}px`,
    height: `${innerSize}px`,
    backgroundColor: props.color,
  };
});
</script>

<template>
  <div class="color-dot" :class="{ active }" :style="wrapStyle" @click="$emit('click')">
    <div class="color-dot-inner" :style="innerStyle">
      <div class="color-dot-highlight"></div>
    </div>
  </div>
</template>

<style scoped>
.color-dot {
  position: relative;
  border-radius: 50%;
  /* 外层灰色浮雕环，复刻 Windows 原生凹凸边框 */
  box-shadow:
    inset 0 0 0 2px #fff,
    inset 0 0 0 3px #999,
    0 1px 2px rgba(0, 0, 0, 0.15);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: transform 0.12s ease;
}
.color-dot:hover {
  transform: scale(1.08);
}
.color-dot.active {
  box-shadow:
    inset 0 0 0 2px #fff,
    inset 0 0 0 3px var(--main-color),
    0 1px 2px rgba(0, 0, 0, 0.15);
}
.color-dot-inner {
  border-radius: 50%;
  position: relative;
  overflow: hidden;
}
/* 顶部玻璃高光渐变 */
.color-dot-highlight {
  position: absolute;
  inset: 0 0 50% 0;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.55), rgba(255, 255, 255, 0));
}
</style>
