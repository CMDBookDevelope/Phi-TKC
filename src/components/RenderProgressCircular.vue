<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useTheme } from 'vuetify';

const props = withDefaults(defineProps<{
  complete?: boolean;
  color?: string;
  size?: number;
  width?: number;
}>(), { complete: false, color: 'primary', size: 20, width: 2 });

const canvas = ref<HTMLCanvasElement>();
const theme = useTheme();
const diameter = computed(() => Number.isFinite(props.size) ? Math.max(1, props.size) : 20);
const strokeWidth = computed(() => Number.isFinite(props.width)
  ? Math.max(1, Math.min(diameter.value, props.width)) : 2);
const paintColor = computed(() => theme.current.value.colors[props.color] ?? props.color);
let observer: ResizeObserver | undefined;
let animationFrame: number | undefined;
let mounted = false;
let lastArc = { start: -Math.PI / 2, sweep: Math.PI * 2 * 0.0001 };
let completion: { started: number; arc: typeof lastArc } | undefined;
const fillDuration = 250;
const thinDuration = 180;
const checkDuration = 300;
const completionDuration = fillDuration + thinDuration + checkDuration;

function easeOutQuad(value: number) {
  const t = Math.max(0, Math.min(1, value));
  return t * (2 - t);
}

function drawCheck(context: CanvasRenderingContext2D, x: number, y: number, size: number, progress: number) {
  if (progress <= 0) return;
  const a = { x: x - size * 0.22, y };
  const b = { x: x - size * 0.05, y: y + size * 0.17 };
  const c = { x: x + size * 0.26, y: y - size * 0.17 };
  const first = Math.hypot(b.x - a.x, b.y - a.y);
  const second = Math.hypot(c.x - b.x, c.y - b.y);
  const drawn = (first + second) * progress;
  context.beginPath();
  context.moveTo(a.x, a.y);
  if (drawn <= first) {
    context.lineTo(a.x + (b.x - a.x) * drawn / first, a.y + (b.y - a.y) * drawn / first);
  } else {
    context.lineTo(b.x, b.y);
    const t = Math.min(1, (drawn - first) / second);
    context.lineTo(b.x + (c.x - b.x) * t, b.y + (c.y - b.y) * t);
  }
  context.stroke();
}

// A rounded arc continuously grows, turns, and contracts. Its clock is shared
// by the page's loading indicators, independently of the rendered percentage.
function paint(timestamp: number) {
  const element = canvas.value;
  if (!element || element.clientWidth === 0 || element.clientHeight === 0) return !props.complete || !!completion;
  const scale = Math.max(1, window.devicePixelRatio || 1);
  const width = Math.round(element.clientWidth * scale);
  const height = Math.round(element.clientHeight * scale);
  if (element.width !== width) element.width = width;
  if (element.height !== height) element.height = height;
  const context = element.getContext('2d');
  if (!context) return false;

  context.clearRect(0, 0, width, height);
  context.save();
  context.strokeStyle = paintColor.value;
  const baseWidth = Math.min(width, height, strokeWidth.value * scale);
  context.lineWidth = baseWidth;
  context.lineCap = 'round';
  context.lineJoin = 'round';
  if (props.complete) {
    // Newly mounted historical tasks use the final icon. A live transition
    // continues from the last painted loading arc, without restarting it.
    const elapsed = completion ? Math.max(0, timestamp - completion.started) : completionDuration;
    const fill = easeOutQuad(elapsed / fillDuration);
    const thin = easeOutQuad((elapsed - fillDuration) / thinDuration);
    const check = easeOutQuad((elapsed - fillDuration - thinDuration) / checkDuration);
    const arc = completion?.arc ?? { start: -Math.PI / 2, sweep: Math.PI * 2 };
    const thinWidth = Math.min(baseWidth, Math.max(scale, baseWidth / 2));
    context.lineWidth = baseWidth + (thinWidth - baseWidth) * thin;
    const radius = (Math.min(width, height) - context.lineWidth) / 2;
    const sweep = arc.sweep + (Math.PI * 2 - arc.sweep) * fill;
    context.beginPath();
    context.arc(width / 2, height / 2, radius, arc.start, arc.start + sweep);
    context.stroke();
    context.lineWidth = Math.min(width, height, Math.max(thinWidth, scale * 1.6));
    drawCheck(context, width / 2, height / 2, Math.min(width, height), check);
    context.restore();
    if (elapsed >= completionDuration) completion = undefined;
    return !!completion;
  }

  const radius = (Math.min(width, height) - context.lineWidth) / 2;
  // Match WinUI's indeterminate ring: a two-second cycle, 900-degree
  // rotation, and a half-circle trim grown first and caught by the tail.
  const phase = (timestamp % 2000) / 2000;
  const minSweep = Math.PI * 2 * 0.0001;
  const sweep = phase < 0.5
    ? minSweep + (Math.PI - minSweep) * phase * 2
    : Math.max(minSweep, Math.PI * 2 * (1 - phase));
  const tail = Math.max(0, phase - 0.5) * Math.PI * 2;
  const start = phase * Math.PI * 5 + tail - Math.PI / 2;
  lastArc = { start, sweep };
  context.beginPath();
  context.arc(width / 2, height / 2, radius, start, start + sweep);
  context.stroke();
  context.restore();
  return true;
}

const repaint = () => paint(performance.now());

function animate(timestamp: number) {
  animationFrame = undefined;
  if (!mounted || document.hidden) return;
  if (paint(timestamp)) animationFrame = requestAnimationFrame(animate);
}

function syncAnimation() {
  if (document.hidden) {
    if (animationFrame !== undefined) cancelAnimationFrame(animationFrame);
    animationFrame = undefined;
  } else if (mounted && animationFrame === undefined) {
    if (paint(performance.now())) animationFrame = requestAnimationFrame(animate);
  }
}

onMounted(() => {
  mounted = true;
  if (typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(repaint);
    if (canvas.value) observer.observe(canvas.value);
  }
  window.addEventListener('resize', repaint);
  document.addEventListener('visibilitychange', syncAnimation);
  syncAnimation();
});
watch([diameter, strokeWidth, paintColor], repaint, { flush: 'post' });
watch(() => props.complete, (complete, previous) => {
  if (!mounted) return;
  completion = complete && !previous ? { started: performance.now(), arc: { ...lastArc } } : undefined;
  if (!document.hidden) repaint();
  syncAnimation();
}, { flush: 'post' });
onUnmounted(() => {
  mounted = false;
  if (animationFrame !== undefined) cancelAnimationFrame(animationFrame);
  observer?.disconnect();
  window.removeEventListener('resize', repaint);
  document.removeEventListener('visibilitychange', syncAnimation);
});
</script>

<template>
  <span class="render-progress-circular" :style="{ width: `${diameter}px`, height: `${diameter}px` }"
    :role="props.complete ? 'img' : 'progressbar'">
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
