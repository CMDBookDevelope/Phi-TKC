<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useTheme } from 'vuetify';

const props = withDefaults(defineProps<{
  modelValue: number;
  color?: string;
  height?: number;
}>(), { color: 'primary', height: 6 });

const canvas = ref<HTMLCanvasElement>();
const theme = useTheme();
const progress = computed(() => Number.isFinite(props.modelValue)
  ? Math.max(0, Math.min(100, props.modelValue)) : 0);
const barHeight = computed(() => Math.max(1, props.height));
const paintColor = computed(() => theme.current.value.colors[props.color] ?? props.color);
let observer: ResizeObserver | undefined;

// The rendering task's percentage determines the painted pixels directly.
// Keep this separate from Vuetify loaders: their full-width background/buffer
// layers rely on CSS opacity, which can become a solid foreground in WebViews.
function paint() {
  const element = canvas.value;
  if (!element || element.clientWidth === 0 || element.clientHeight === 0) return;
  const scale = Math.max(1, window.devicePixelRatio || 1);
  const width = Math.round(element.clientWidth * scale);
  const height = Math.round(element.clientHeight * scale);
  if (element.width !== width) element.width = width;
  if (element.height !== height) element.height = height;
  const context = element.getContext('2d');
  if (!context) return;

  context.clearRect(0, 0, width, height);
  context.save();
  const radius = Math.min(height / 2, width / 2);
  context.beginPath();
  context.moveTo(radius, 0);
  context.lineTo(width - radius, 0);
  context.quadraticCurveTo(width, 0, width, radius);
  context.lineTo(width, height - radius);
  context.quadraticCurveTo(width, height, width - radius, height);
  context.lineTo(radius, height);
  context.quadraticCurveTo(0, height, 0, height - radius);
  context.lineTo(0, radius);
  context.quadraticCurveTo(0, 0, radius, 0);
  context.closePath();
  context.clip();
  context.fillStyle = paintColor.value;
  context.globalAlpha = 0.18;
  context.fillRect(0, 0, width, height);
  context.globalAlpha = 1;
  const filled = Math.round(width * progress.value / 100);
  if (filled > 0) context.fillRect(0, 0, filled, height);
  context.restore();
}

onMounted(() => {
  if (typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(paint);
    if (canvas.value) observer.observe(canvas.value);
  }
  window.addEventListener('resize', paint);
  paint();
});
watch([progress, barHeight, paintColor], paint, { flush: 'post' });
onUnmounted(() => {
  observer?.disconnect();
  window.removeEventListener('resize', paint);
});
</script>

<template>
  <div class="render-progress-bar" :style="{ height: `${barHeight}px` }"
    role="progressbar" aria-valuemin="0" aria-valuemax="100" :aria-valuenow="progress">
    <canvas ref="canvas" aria-hidden="true" />
  </div>
</template>

<style scoped>
.render-progress-bar {
  display: block;
  width: 100%;
}
canvas {
  display: block;
  width: 100%;
  height: 100%;
}
</style>
