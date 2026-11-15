<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useTheme } from 'vuetify';

const props = withDefaults(defineProps<{
  modelValue: number;
  color?: string;
  size?: number;
  width?: number;
}>(), { color: 'primary', size: 20, width: 2 });

const canvas = ref<HTMLCanvasElement>();
const theme = useTheme();
const progress = computed(() => Number.isFinite(props.modelValue)
  ? Math.max(0, Math.min(100, props.modelValue)) : 0);
const diameter = computed(() => Number.isFinite(props.size) ? Math.max(1, props.size) : 20);
const strokeWidth = computed(() => Number.isFinite(props.width)
  ? Math.max(1, Math.min(diameter.value, props.width)) : 2);
const paintColor = computed(() => theme.current.value.colors[props.color] ?? props.color);
let observer: ResizeObserver | undefined;

// Draw the determinate task arc directly, with the same percentage and paint
// as RenderProgressBar. The track's alpha does not depend on an SVG CSS layer.
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
  context.strokeStyle = paintColor.value;
  context.lineWidth = Math.min(width, height, strokeWidth.value * scale);
  const radius = (Math.min(width, height) - context.lineWidth) / 2;
  const start = -Math.PI / 2;
  context.beginPath();
  context.arc(width / 2, height / 2, radius, start, start + Math.PI * 2);
  context.globalAlpha = 0.18;
  context.stroke();
  if (progress.value > 0) {
    context.beginPath();
    context.arc(width / 2, height / 2, radius, start, start + Math.PI * 2 * progress.value / 100);
    context.globalAlpha = 1;
    context.stroke();
  }
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
watch([progress, diameter, strokeWidth, paintColor], paint, { flush: 'post' });
onUnmounted(() => {
  observer?.disconnect();
  window.removeEventListener('resize', paint);
});
</script>

<template>
  <span class="render-progress-circular" :style="{ width: `${diameter}px`, height: `${diameter}px` }"
    role="progressbar" aria-valuemin="0" aria-valuemax="100" :aria-valuenow="progress">
    <canvas ref="canvas" aria-hidden="true" />
  </span>
</template>

<style scoped>
.render-progress-circular {
  display: inline-block;
  flex-shrink: 0;
}
canvas {
  display: block;
  width: 100%;
  height: 100%;
}
</style>
