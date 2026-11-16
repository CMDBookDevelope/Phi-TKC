<i18n>
en:
  title: Rhythm color scheme
  color: 'Color {number}'
  extract: Extract 3 colors from artwork
  automatic: Colors follow each chart's artwork. Click a swatch to adjust them.
  custom: These three colors are also shared by all charts in a batch.
  reset: Follow chart artwork again
  choose-chart: Select a chart to extract colors from its artwork.
  source: 'Artwork source: {name}'
  failed: Unable to extract colors from the chart artwork.
zh-CN:
  title: 节奏解析颜色方案
  color: '颜色 {number}'
  extract: 从曲绘提取三种主色
  automatic: 配色随每个谱面的曲绘自动变化，点击色块可手动调整。
  custom: 使用这三种颜色，批量渲染中的所有谱面也会共用此配色。
  reset: 恢复随谱面取色
  choose-chart: 请先选择谱面，再从曲绘取色。
  source: '取色来源：{name}'
  failed: 无法从谱面曲绘提取颜色。
</i18n>

<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import { toastError } from '@/common';
import { isRhythmColors, type RhythmColors } from '@/rhythmConfig';
import ColorDot from './ColorDot.vue';

const props = defineProps<{ modelValue: RhythmColors | null; chartPath?: string; chartLabel?: string; revision?: number }>();
const emit = defineEmits<{ 'update:modelValue': [colors: RhythmColors | null] }>();
const { t } = useI18n();
const artworkColors = ref<RhythmColors | null>(null);
const busy = ref(false);
const extractionError = ref(false);
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

function resetColors() {
  invalidate();
  emit('update:modelValue', null);
}

watch(() => props.modelValue, (value) => {
  invalidate();
  if (!value && !artworkColors.value) void extractColors(false);
});
watch([() => props.chartPath, () => props.revision], () => {
  invalidate();
  artworkColors.value = null;
  extractionError.value = false;
  if (!props.modelValue) void extractColors(false);
}, { immediate: true });
onUnmounted(invalidate);
</script>

<template>
  <div class="rhythm-colors">
    <div class="colors-label">{{ t('title') }}</div>
    <div class="colors-row">
      <v-menu v-for="(color, index) in colors" :key="index" :close-on-content-click="false" location="bottom">
        <template #activator="{ props: menuProps }">
          <button v-bind="menuProps" type="button" class="color-swatch" :aria-label="t('color', { number: index + 1 })">
            <ColorDot :color="color" :size="32" />
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
      <v-btn v-if="modelValue" variant="text" size="small" @click="resetColors">{{ t('reset') }}</v-btn>
    </div>
    <p v-if="!chartPath" class="colors-hint">{{ t('choose-chart') }}</p>
    <p v-else-if="sourceName" class="colors-hint">{{ t('source', { name: sourceName }) }}</p>
    <p v-if="extractionError" class="colors-error" role="status">{{ t('failed') }}</p>
  </div>
</template>

<style scoped>
.rhythm-colors { display: flex; flex-direction: column; gap: 10px; }
.colors-label { font-size: 13px; font-weight: 600; }
.colors-row { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
.color-swatch { display: flex; align-items: center; justify-content: center; gap: 8px; padding: 8px; min-height: 48px; border: 1px solid rgba(var(--main-color-rgb), 0.2); border-radius: 12px; background: rgba(var(--main-color-rgb), 0.04); color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
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
