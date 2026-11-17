<i18n>
en:
  title: Rhythm color scheme
  color: 'Color {number}'
  extract: Extract 3 colors from artwork
  automatic: Colors follow each chart's artwork. Click a swatch to adjust them.
  custom: These three colors are also shared by all charts in a batch.
  choose-chart: Select a chart to extract colors from its artwork.
  source: 'Artwork source: {name}'
  pick: Pick a color from the artwork
  pick-active: Select a color ring, then sample the artwork
  failed: Unable to extract colors from the chart artwork.
zh-CN:
  title: 节奏解析颜色方案
  color: '颜色 {number}'
  extract: 从曲绘提取三种主色
  automatic: 配色随每个谱面的曲绘自动变化，点击色块可手动调整。
  custom: 使用这三种颜色，批量渲染中的所有谱面也会共用此配色。
  choose-chart: 请先选择谱面，再从曲绘取色。
  source: '取色来源：{name}'
  pick: 从曲绘吸取颜色
  pick-active: 先选择一个颜色环，再点击曲绘取色
  failed: 无法从谱面曲绘提取颜色。
</i18n>

<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { toastError } from '@/common';
import { isRhythmColors, type RhythmColors } from '@/rhythmConfig';
import ColorDot from './ColorDot.vue';

const props = defineProps<{ modelValue: RhythmColors | null; chartPath?: string; chartLabel?: string; chartCover?: string | null; revision?: number }>();
const emit = defineEmits<{ 'update:modelValue': [colors: RhythmColors | null] }>();
const { t } = useI18n();
const artworkColors = ref<RhythmColors | null>(null);
const busy = ref(false);
const extractionError = ref(false);
const picking = ref(false);
const selectedColor = ref(0);
const coverImage = ref<HTMLImageElement>();
// Neutral swatches can be edited before a chart is selected; they are never
// submitted while the config is in automatic mode.
const colors = computed(() => props.modelValue ?? artworkColors.value ?? ['#FFFFFF', '#B3B3B3', '#666666'] as RhythmColors);
const sourceName = computed(() => props.chartLabel || props.chartPath?.split(/[\\/]/).filter(Boolean).pop());
let generation = 0;

function invalidate() {
  generation += 1;
  busy.value = false;
}

async function extractColors(commit: boolean) {
  const path = props.chartPath;
  if (!path) return;
  const request = ++generation;
  busy.value = true;
  extractionError.value = false;
  try {
    const result = await invoke<unknown>('extract_rhythm_colors', { path });
    if (request !== generation || path !== props.chartPath) return;
    if (!isRhythmColors(result)) throw new Error(t('failed'));
    artworkColors.value = [...result];
    if (commit) emit('update:modelValue', [...result]);
  } catch (error) {
    if (request !== generation || path !== props.chartPath) return;
    extractionError.value = true;
    if (commit) toastError(error);
  } finally {
    if (request === generation) busy.value = false;
  }
}

function setColor(index: number, value: unknown) {
  if (typeof value !== 'string' || !/^#[0-9a-f]{6}$/i.test(value)) return;
  invalidate();
  const next: RhythmColors = [...colors.value];
  next[index] = value.toUpperCase();
  emit('update:modelValue', next);
}

function startPicking() {
  if (!props.chartPath || !props.chartCover) return;
  picking.value = true;
}

async function sampleArtwork(event: MouseEvent | TouchEvent) {
  if (!picking.value || !props.chartPath || !coverImage.value) return;
  const point = 'touches' in event ? (event.changedTouches[0] ?? event.touches[0]) : event;
  if (!point) return;
  const image = coverImage.value;
  const rect = image.getBoundingClientRect();
  const scale = Math.min(rect.width / image.naturalWidth, rect.height / image.naturalHeight);
  if (!Number.isFinite(scale) || scale <= 0) return;
  const renderedWidth = image.naturalWidth * scale;
  const renderedHeight = image.naturalHeight * scale;
  const offsetX = (rect.width - renderedWidth) / 2;
  const offsetY = (rect.height - renderedHeight) / 2;
  const localX = point.clientX - rect.left - offsetX;
  const localY = point.clientY - rect.top - offsetY;
  if (localX < 0 || localY < 0 || localX >= renderedWidth || localY >= renderedHeight) return;
  const x = Math.floor(localX / scale);
  const y = Math.floor(localY / scale);
  try {
    const color = await invoke<unknown>('sample_rhythm_color', { path: props.chartPath, x, y });
    if (typeof color !== 'string' || !/^#[0-9a-f]{6}$/i.test(color)) throw new Error(t('failed'));
    setColor(selectedColor.value, color);
    picking.value = false;
  } catch (error) {
    toastError(error);
  }
}

watch(() => props.modelValue, (value) => {
  invalidate();
  if (!value && !artworkColors.value) void extractColors(false);
});
watch([() => props.chartPath, () => props.revision], () => {
  invalidate();
  artworkColors.value = null;
  extractionError.value = false;
  picking.value = false;
  if (!props.modelValue) void extractColors(false);
}, { immediate: true });
onUnmounted(invalidate);
</script>

<template>
  <div class="rhythm-colors">
    <div class="colors-label">{{ t('title') }}</div>
    <div class="artwork-preview" :class="{ 'is-picking': picking }">
      <img v-if="chartCover" ref="coverImage" :src="convertFileSrc(chartCover)" alt="" @click="sampleArtwork" @touchend.prevent="sampleArtwork" />
      <div v-else class="artwork-placeholder"><v-icon icon="mdi-image-outline" size="24" /></div>
      <v-tooltip location="left" :text="t(picking ? 'pick-active' : 'pick')">
        <template #activator="{ props: tooltipProps }">
          <button v-bind="tooltipProps" type="button" class="eyedropper-button" :aria-pressed="picking"
            :disabled="!chartPath || !chartCover" @click="startPicking">
            <v-icon :icon="picking ? 'mdi-eyedropper-variant' : 'mdi-eyedropper'" size="18" />
          </button>
        </template>
      </v-tooltip>
    </div>
    <div class="colors-row">
      <v-menu v-for="(color, index) in colors" :key="index" :close-on-content-click="false" location="bottom">
        <template #activator="{ props: menuProps }">
          <button v-bind="menuProps" type="button" class="color-swatch" :class="{ 'is-selected': selectedColor === index }"
            :aria-label="t('color', { number: index + 1 })" :aria-pressed="selectedColor === index" @click="selectedColor = index">
            <ColorDot :color="color" :size="34" />
            <span>{{ t('color', { number: index + 1 }) }}</span>
          </button>
        </template>
        <v-color-picker :model-value="color" @update:model-value="setColor(index, $event)" mode="hex" :modes="['hex']" hide-inputs hide-mode-switch />
      </v-menu>
    </div>
    <p class="colors-hint">{{ props.modelValue ? t('custom') : t('automatic') }}</p>
    <div class="colors-actions">
      <v-btn variant="tonal" color="primary" prepend-icon="mdi-palette" :loading="busy" :disabled="!chartPath || busy"
        @click="extractColors(true)">{{ t('extract') }}</v-btn>
    </div>
    <p v-if="!chartPath" class="colors-hint">{{ t('choose-chart') }}</p>
    <p v-else-if="sourceName" class="colors-hint">{{ t('source', { name: sourceName }) }}</p>
    <p v-if="extractionError" class="colors-error" role="status">{{ t('failed') }}</p>
  </div>
</template>

<style scoped>
.rhythm-colors { display: flex; flex-direction: column; gap: 10px; }
.colors-label { font-size: 13px; font-weight: 600; }
.artwork-preview { position: relative; display: grid; place-items: center; width: 100%; height: 190px; overflow: hidden; border: 1px solid rgba(var(--main-color-rgb), 0.2); border-radius: 8px; background: rgba(0, 0, 0, 0.18); }
.artwork-preview img { display: block; width: 100%; height: 100%; object-fit: contain; }
.artwork-preview.is-picking img { cursor: crosshair; }
.artwork-placeholder { display: grid; place-items: center; width: 100%; height: 100%; opacity: 0.45; }
.eyedropper-button { position: absolute; top: 8px; right: 8px; display: grid; place-items: center; width: 36px; height: 36px; border: 1px solid rgba(255,255,255,0.24); border-radius: 50%; color: white; background: rgba(20,24,30,0.74); cursor: pointer; }
.eyedropper-button:disabled { opacity: 0.4; cursor: default; }
.eyedropper-button[aria-pressed="true"] { color: rgb(var(--v-theme-primary)); outline: 2px solid currentColor; }
.colors-row { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
.color-swatch { display: flex; align-items: center; justify-content: center; gap: 8px; padding: 8px; min-height: 48px; border: 1px solid rgba(var(--main-color-rgb), 0.2); border-radius: 8px; background: rgba(var(--main-color-rgb), 0.04); color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
.color-swatch.is-selected { border-color: var(--main-color); box-shadow: 0 0 0 2px rgba(var(--main-color-rgb), 0.24); }
.color-swatch:hover { background: rgba(var(--main-color-rgb), 0.1); }
.color-swatch:focus-visible { outline: 2px solid var(--main-color); outline-offset: 2px; }
.colors-actions { display: flex; flex-wrap: wrap; gap: 6px; }
.colors-hint { margin: 0; font-size: 12px; line-height: 1.6; opacity: 0.7; overflow-wrap: anywhere; }
.colors-error { margin: 0; font-size: 12px; color: rgb(var(--v-theme-error)); }
@media (max-width: 600px) {
  .color-swatch { flex-direction: column; gap: 6px; }
  .colors-actions :deep(.v-btn) { max-width: 100%; }
}
</style>
