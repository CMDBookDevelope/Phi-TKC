<i18n>
en:
  title: Batch Render
  select-charts: Charts
  select-preset: Preset
  add-files: Add
  add-folder: Add Dir
  clear-list: Clear
  clear-done: Clear Done
  start-render: Render
  stop-render: Stop
  retry-failed: Retry
  bulk-edit: Edit
  name: Name
  level: Level
  charter: Charter
  status: Status
  pending: Ready
  rendering: Rendering
  done: Done
  failed: Failed
  total-selected: "Selected: {count}"
  all: All
  none: None
  configure: Config
  save: Save
  actions: Actions
  close: Close
  total-charts: "Total: {count}"
  select-all: Sel All
  deselect-all: Deselect
  progress: Progress
  eta: ETA
  no-charts: Drop chart files/folders here or use buttons above.
  no-results: No results
  search-placeholder: Search...
  selected: "Sel: {count}"
  filtered-results: "Filter: {count}"
  add-files-failed: Add failed
  no-charts-found: No charts found
  charts-added: "{count} added"
  add-folder-failed: Add failed
  no-charts-selected: None selected
  batch-completed: "Done: {count}"
  batch-stopped: Stopped
  ffmpeg-not-found: No FFmpeg
  chart-info-missing: No info
  adding-charts: Adding...
  invalid-chart-file: Bad file
  file-type-error: "Error: {message}"
  config-saved: Saved
  chart-name: Name
  composer: Composer
  illustrator: Artist
  background-dim: Dim
  hold_cover: Hold Cover
  tip: Tip
  aspect: Aspect
  width: Width
  height: Height
  dim: Dim
  preview: Preview
  edit: Edit
  chart-info: Info
  bulk-edit-title: Bulk Edit
  bulk-edit-hint: Leave blank to keep original values.
zh-CN:
  title: 批量渲染
  select-charts: 谱面
  select-preset: 预设
  add-files: 添加
  add-folder: 目录
  clear-list: 清空
  clear-done: 清理完成
  start-render: 渲染
  stop-render: 停止
  retry-failed: 重试
  bulk-edit: 编辑
  name: 名称
  level: 难度
  charter: 谱师
  status: 状态
  pending: 就绪
  rendering: 渲染中
  done: 完成
  failed: 失败
  actions: 操作
  total-selected: "已选择: {count}"
  all: 全选
  none: 取消
  configure: 配置
  close: 关闭
  total-charts: "总计: {count}"
  select-all: 全选
  deselect-all: 取消
  progress: 进度
  eta: 预计
  no-charts: 拖拽谱面至此，或使用上方按钮添加
  no-results: 无结果
  search-placeholder: 搜索...
  selected: "已选: {count}"
  filtered-results: "筛选: {count}"
  save: 保存
  add-files-failed: 添加失败
  no-charts-found: 无谱面
  charts-added: "已添加 {count} 个"
  add-folder-failed: 添加失败
  no-charts-selected: 未选择
  batch-completed: "完成: {count} 个"
  batch-stopped: 已停止
  ffmpeg-not-found: 无FFmpeg
  chart-info-missing: 无信息
  adding-charts: 解析中...
  invalid-chart-file: 无效文件
  file-type-error: "错误: {message}"
  config-saved: 已保存
  chart-name: 谱面名
  composer: 曲师
  illustrator: 画师
  background-dim: 暗度
  hold_cover: 遮罩
  aspect: 比例
  tip: 提示
  width: 宽
  height: 高
  dim: 暗度
  preview: 预览
  edit: 编辑
  chart-info: 信息
  bulk-edit-title: 批量编辑
  bulk-edit-hint: 留空则保持原值。
</i18n>

<script setup lang="ts">
import { mainColor, bgColor, subColor } from '@/colorStore'
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { event } from '@tauri-apps/api';
import moment from 'moment';

import { RULES } from './common';
import type { ChartInfo, RenderConfig } from './model';
import ConfigView from '@/components/ConfigView.vue';

const { t } = useI18n();
const router = useRouter();

interface BatchChart {
  id: string;
  path: string;
  name: string;
  level: string;
  charter: string;
  status: 'pending' | 'rendering' | 'done' | 'failed';
  selected: boolean;
  error?: string;
  chartInfo?: ChartInfo;
  aspectWidth?: string;
  aspectHeight?: string;
}

const charts = ref<BatchChart[]>([]);
const selectedPreset = ref<string>('default');
const presets = ref<{ name: string }[]>([]);
const configViewRef = ref<InstanceType<typeof ConfigView> | null>(null);
const configDialog = ref(false);
const editDialog = ref(false);
const bulkEditDialog = ref(false);
const editingChartId = ref<string | null>(null);
const editingChart = computed(() => editingChartId.value ? charts.value.find(c => c.id === editingChartId.value) : undefined);
const form = ref<any>();
const bulkForm = ref<any>();

const currentRenderingId = ref<string | null>(null);
const isRenderingQueue = ref(false);
const renderMsg = ref('');
const renderProgress = ref<number>(0);

const bulkEditData = ref({
  aspectWidth: '',
  aspectHeight: '',
  backgroundDim: null as number | null,
  holdCover: null as number | null,
});

const isAddingFiles = ref(false);
const isAddingFolder = ref(false);
const searchQuery = ref('');

const defaultConfig = ref<RenderConfig>(loadDefaultConfig());

function generateId() {
  return Math.random().toString(36).substring(2, 9);
}

async function previewChart(id: string) {
  const chart = charts.value.find(c => c.id === id);
  if (!chart?.chartInfo) return toast(t('chart-info-missing'), 'error');
  try {
    const config = await buildRenderParams();
    await invoke('preview_chart', {
      params: { path: chart.path, info: chart.chartInfo, config: { ...config, autoplay: true } },
    });
  } catch (error) { toastError(error); }
}

function loadDefaultConfig(): RenderConfig {
  const savedConfig = localStorage.getItem('defaultRenderConfig');
  if (savedConfig) {
    try { return JSON.parse(savedConfig) as RenderConfig; } catch (e) { console.error('Failed to parse saved config', e); }
  }
  return {
    resolution: [1920, 1080], ffmpegPreset: 'medium p4 balanced', endingLength: -2.0, disableLoading: true,
    chartDebug: false, flidX: false, chartRatio: 1, bufferSize: 256, fps: 60, hardwareAccel: true,
    videoCodec: 'h264', encoder: 'auto', bitrateControl: 'CRF', bitrate: '28', targetAudio: 48000,
    video: false, audioBit: undefined, audioFormat: 'flac', background: false, aggressive: false,
    challengeColor: 'golden', challengeRank: 45, disableEffect: false, doubleHint: true, fxaa: false,
    noteScale: 1, particle: true, playerAvatar: null, playerName: '', playerRks: 15, sampleCount: 1,
    resPackPath: null, speed: 1, volumeMusic: 1, volumeSfx: 1, combo: 'AUTOPLAY', watermark: '',
    handSplit: false, noteSpeedFactor: 1.0, ffmpegThread: false, showProgressText: false,
    showTimeText: false, uiLine: true, uiScore: true, uiCombo: true, uiLevel: true, uiName: true,
    uiPb: true, uiPause: true, bar: false,
  };
}

function saveDefaultConfig(config: RenderConfig) {
  defaultConfig.value = config;
  localStorage.setItem('defaultRenderConfig', JSON.stringify(config));
  toast(t('config-saved'), 'success');
}

async function getPresets() {
  try {
    const presetsMap = (await invoke('get_presets')) as Record<string, any>;
    presets.value = [{ name: 'default' }, ...Object.keys(presetsMap).map((name) => ({ name }))];
    selectedPreset.value = presets.value[0].name;
  } catch (error) { console.error('Failed to get presets', error); }
}

async function processNewPaths(paths: string[]) {
  const uniquePaths = [...new Set(paths)];
  const existingPaths = new Set(charts.value.map((c) => c.path));
  const newPaths = uniquePaths.filter((path) => !existingPaths.has(path));
  if (newPaths.length === 0) { toast(t('no-charts-found'), 'warning'); return; }
  for (const path of newPaths) { await addChart(path); }
  toast(t('charts-added', { count: newPaths.length }), 'success');
}

async function addFiles() {
  if (isAddingFiles.value) return;
  isAddingFiles.value = true;
  try {
    const files = await open({
      multiple: true,
      filters: [{ name: t('chart-file'), extensions: ['zip', 'json', 'pez'] }],
    });
    if (!files) return;
    const paths = (Array.isArray(files) ? files : [files]).map((f) => (typeof f === 'string' ? f : (f as any).path));
    await processNewPaths(paths);
  } catch (error) { toast(t('add-files-failed'), 'error'); } finally { isAddingFiles.value = false; }
}

async function addFolder() {
  if (isAddingFolder.value) return;
  isAddingFolder.value = true;
  try {
    const folder = await open({ directory: true });
    if (!folder) return;
    const folderPath = typeof folder === 'string' ? folder : (folder as any).path;
    const files = (await invoke('list_chart_files', { path: folderPath })) as string[];
    if (!files || files.length === 0) return toast(t('no-charts-found'), 'warning');
    const validExtensions = ['.json', '.zip', '.pez'];
    const filteredFiles = files.filter((f) => validExtensions.includes(f.toLowerCase().slice(f.lastIndexOf('.'))));
    await processNewPaths(filteredFiles);
  } catch (error) { toast(t('add-folder-failed'), 'error'); } finally { isAddingFolder.value = false; }
}

async function addChart(path: string) {
  if (charts.value.some((c) => c.path === path)) return;
  const newChart: BatchChart = { id: generateId(), path, name: t('adding-charts'), level: '...', charter: '...', status: 'pending', selected: true };
  const placeholderIndex = charts.value.push(newChart) - 1;
  try {
    const chartInfo = (await invoke('parse_chart', { path })) as ChartInfo;
    let aW = String(chartInfo.aspectRatio);
    let aH = '1.0';
    for (const asp of [[16, 9], [4, 3], [8, 5], [3, 2]]) {
      if (Math.abs(asp[0] / asp[1] - chartInfo.aspectRatio) < 1e-4) { aW = String(asp[0]); aH = String(asp[1]); break; }
    }
    charts.value[placeholderIndex] = { ...newChart, name: chartInfo.name, level: chartInfo.level, charter: chartInfo.charter, chartInfo, aspectWidth: aW, aspectHeight: aH };
  } catch (error: any) {
    let errorMessage = t('invalid-chart-file');
    if (error.message?.includes('as zip archive')) errorMessage = t('file-type-error', { message: 'Not a ZIP archive' });
    else if (error.message?.includes('central directory')) errorMessage = t('file-type-error', { message: 'Invalid ZIP format' });
    charts.value[placeholderIndex] = { ...newChart, name: t('failed'), status: 'failed', selected: false, error: errorMessage };
  }
}

function clearList() { if (confirm(t('clear-list') + '?')) charts.value = []; }
function clearDone() { charts.value = charts.value.filter(c => c.status !== 'done'); }
function retryFailed() { charts.value.forEach(c => { if (c.status === 'failed') c.status = 'pending'; }); }
function removeChart(id: string) { charts.value = charts.value.filter(c => c.id !== id); }

async function buildRenderParams() {
  if (!(await invoke('test_ffmpeg'))) throw new Error(t('ffmpeg-not-found'));
  let config = selectedPreset.value === 'default' ? defaultConfig.value : ((await invoke('get_presets')) as any)[selectedPreset.value];
  if (!config) config = defaultConfig.value;
  if (!config.resolution) throw new Error('Resolution missing');
  return config;
}

async function saveConfig() {
  const config = await configViewRef.value?.buildConfig();
  if (config) { saveDefaultConfig(config); configDialog.value = false; }
}

async function startRender() {
  const pendingCount = filteredCharts.value.filter((c) => c.selected && c.status === 'pending').length;
  if (pendingCount === 0) return toast(t('no-charts-selected'), 'warning');
  try {
    const config = await buildRenderParams();
    isRenderingQueue.value = true;
    for (let i = 0; i < charts.value.length; i++) {
      if (!isRenderingQueue.value) break;
      const chart = charts.value[i];
      if (!chart.selected || chart.status !== 'pending') continue;
      currentRenderingId.value = chart.id;
      chart.status = 'rendering';
      renderProgress.value = 0;
      try {
        if (!chart.chartInfo) throw new Error(t('chart-info-missing'));
        await invoke('post_render', { params: { path: chart.path, info: chart.chartInfo, config } });
        chart.status = 'done';
      } catch (error: any) {
        chart.status = 'failed';
        chart.error = error.message || String(error);
        toastError(error);
      }
    }
    if (isRenderingQueue.value) toast(t('batch-completed', { count: pendingCount }), 'success');
  } catch (error) { toastError(error); } finally {
    isRenderingQueue.value = false;
    currentRenderingId.value = null;
    renderMsg.value = '';
    renderProgress.value = 0;
  }
}

function stopRender() { isRenderingQueue.value = false; toast(t('batch-stopped'), 'info'); }

const filteredCharts = computed(() => {
  if (!searchQuery.value.trim()) return charts.value;
  const q = searchQuery.value.toLowerCase();
  return charts.value.filter(c =>
    c.name.toLowerCase().includes(q) || c.level.toLowerCase().includes(q) || c.charter.toLowerCase().includes(q)
  );
});

const selectedCount = computed(() => charts.value.filter(c => c.selected).length);
const allSelected = computed(() => charts.value.length > 0 && selectedCount.value === charts.value.length);
const isIndeterminate = computed(() => selectedCount.value > 0 && selectedCount.value < charts.value.length);

function toggleSelectAll() {
  const target = !allSelected.value;
  charts.value.forEach((chart) => { if (chart.status !== 'rendering') chart.selected = target; });
}

event.listen('render-msg', (msg) => { renderMsg.value = msg.payload as string; });
event.listen('render-progress', (msg) => {
  const p = msg.payload as { progress: number; fps: number; estimate: number };
  renderMsg.value = `FPS: ${p.fps} | ${t('eta')}: ${moment.duration(p.estimate, 'seconds').humanize(true, { ss: 1 })}`;
  renderProgress.value = p.progress * 100;
});

function openEditDialog(id: string) { editingChartId.value = id; editDialog.value = true; }

async function saveChartInfo() {
  if (!form.value || !editingChartId.value) return;
  const { valid } = await form.value.validate();
  if (!valid) return;
  const chart = charts.value.find(c => c.id === editingChartId.value);
  if (!chart) return;
  const aspect = tryParseAspect(chart.aspectWidth, chart.aspectHeight);
  if (aspect && chart.chartInfo) chart.chartInfo.aspectRatio = aspect;
  if (chart.chartInfo) { chart.name = chart.chartInfo.name; chart.level = chart.chartInfo.level; chart.charter = chart.chartInfo.charter; }
  editDialog.value = false;
}

function openBulkEditDialog() {
  bulkEditData.value = { aspectWidth: '', aspectHeight: '', backgroundDim: null, holdCover: null };
  bulkEditDialog.value = true;
}

function saveBulkEdit() {
  const aspect = tryParseAspect(bulkEditData.value.aspectWidth, bulkEditData.value.aspectHeight);
  const selectedCharts = charts.value.filter(c => c.selected && c.chartInfo);
  selectedCharts.forEach(chart => {
    if (!chart.chartInfo) return;
    if (aspect) { chart.aspectWidth = bulkEditData.value.aspectWidth; chart.aspectHeight = bulkEditData.value.aspectHeight; chart.chartInfo.aspectRatio = aspect; }
    if (bulkEditData.value.backgroundDim !== null) chart.chartInfo.backgroundDim = bulkEditData.value.backgroundDim;
    if (bulkEditData.value.holdCover !== null) chart.chartInfo.HoldPartialCover = !!bulkEditData.value.holdCover;
  });
  bulkEditDialog.value = false;
  toast(t('config-saved'), 'success');
}

function tryParseAspect(w?: string, h?: string) {
  if (!w || !h) return undefined;
  const numW = parseFloat(w), numH = parseFloat(h);
  return (isNaN(numW) || isNaN(numH)) ? undefined : numW / numH;
}

const STORAGE_KEY = 'batch_render_charts_v2';
onMounted(() => {
  getPresets();
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved) {
    try { charts.value = JSON.parse(saved).map((c: any) => ({ ...c, status: c.status === 'rendering' ? 'failed' : c.status })); }
    catch (e) { }
  }
});

watch(charts, (val) => { localStorage.setItem(STORAGE_KEY, JSON.stringify(val)); }, { deep: true });
</script>

<template>
  <div class="batch-layout">
    <!-- Top Bar -->
    <div class="batch-header">
      <div class="header-row">
        <div class="search-wrap">
          <fv-animated-icon icon="Filter" :font-size="16" class="search-icon" />
          <input v-model="searchQuery" :placeholder="t('search-placeholder')" class="md3-search" />
        </div>

        <div class="header-actions">
          <fv-button :background="bgColor" :border-radius="30" :foreground="mainColor" :disabled="isAddingFiles" @click="addFiles">
            <fv-animated-icon icon="FolderHorizontal" :font-size="18" />
            <span>{{ t('add-files') }}</span>
          </fv-button>
          <fv-button :background="bgColor" :border-radius="30" :foreground="mainColor" :disabled="isAddingFolder" @click="addFolder">
            <fv-animated-icon icon="FolderHorizontal" :font-size="18" />
            <span>{{ t('add-folder') }}</span>
          </fv-button>
          <fv-button :background="bgColor" :border-radius="30" :foreground="mainColor" @click="configDialog = true">
            <fv-animated-icon icon="Settings" :font-size="18" />
            <span>{{ t('configure') }}</span>
          </fv-button>
        </div>
      </div>
    </div>

    <!-- Selection bar -->
    <div class="selection-bar">
      <div class="sel-left">
        <input type="checkbox" :checked="allSelected" :indeterminate="isIndeterminate" @click="toggleSelectAll" class="md3-checkbox" />
        <span class="sel-count">{{ t('selected', { count: selectedCount }) }} / {{ t('total-charts', { count: charts.length }) }}</span>

        <fv-button v-if="selectedCount > 1" class="md3-btn md3-btn-text md3-btn-sm" :border-radius="30" @click="openBulkEditDialog">
          <fv-animated-icon icon="Sort" :font-size="16" />
          <span>{{ t('bulk-edit') }}</span>
        </fv-button>
      </div>
      <div class="sel-right">
        <fv-button v-if="charts.some(c => c.status === 'failed')" :border-radius="30" class="md3-btn md3-btn-text md3-btn-sm" @click="retryFailed">
          <fv-animated-icon icon="Refresh" :font-size="16" /><span>{{ t('retry-failed') }}</span>
        </fv-button>
        <fv-button :disabled="charts.some(c => c.status === 'done')" :border-radius="30" class="md3-btn md3-btn-text md3-btn-sm" @click="clearDone">
          <fv-animated-icon icon="CheckMark" :font-size="16" /><span>{{ t('clear-done') }}</span>
        </fv-button>
        <fv-button :background="bgColor" :foreground="mainColor" :border-radius="30" @click="clearList" :disabled="charts.length === 0">
          <fv-animated-icon icon="Delete" :font-size="16" /><span>{{ t('clear-list') }}</span>
        </fv-button>
      </div>
    </div>

    <!-- Render action -->
    <div class="render-bar">
      <template v-if="isRenderingQueue">
        <span class="rendering-chip">
          <fv-AnimatedIcon icon="Refresh" modelValue="bounceRotate" :font-size="14" />
          {{ t('rendering') }}
        </span>
        <fv-button :background="bgColor" :border-radius="30" :foreground="mainColor" @click="stopRender">
          <fv-animated-icon icon="Blocked" :font-size="18" /><span>{{ t('stop-render') }}</span>
        </fv-button>
      </template>
      <template v-else>
        <fv-button :background="mainColor" :border-radius="30" :foreground="bgColor" :disabled="selectedCount === 0" @click="startRender">
          <fv-animated-icon icon="Play" :font-size="18" />
          <span>{{ t('start-render') }}</span>
        </fv-button>
        <span class="render-count">({{ selectedCount }} Tasks)</span>
      </template>
    </div>

    <!-- List -->
    <div class="chart-list">
      <div v-if="charts.length === 0" class="empty-state">
        <fv-animated-icon icon="Page" :font-size="64" style="opacity: 0.15;" />
        <p>{{ t('no-charts') }}</p>
      </div>

      <div v-else class="list-scroll">
        <div
          v-for="item in filteredCharts"
          :key="item.id"
          class="chart-row"
          :class="{ 'is-rendering': item.status === 'rendering', 'is-failed': item.status === 'failed' }"
        >
          <div v-if="item.status === 'rendering' && item.id === currentRenderingId" class="progress-bg" :style="{ width: `${renderProgress}%` }"></div>

          <div class="row-content">
            <input type="checkbox" v-model="item.selected" :disabled="item.status === 'rendering'" class="md3-checkbox" />

            <div class="row-info">
              <div class="row-title">
                <span class="row-name" :title="item.name">{{ item.name }}</span>
                <span class="level-badge">{{ item.level }}</span>
              </div>
              <div class="row-sub">
                <fv-animated-icon icon="MusicNote" :font-size="14" />
                <span>{{ item.charter }}</span>
                <template v-if="item.status === 'rendering' && item.id === currentRenderingId">
                  <span class="render-status">{{ renderProgress.toFixed(1) }}% - {{ renderMsg }}</span>
                </template>
                <template v-else-if="item.error">
                  <span class="error-status" :title="item.error">{{ item.error }}</span>
                </template>
              </div>
            </div>

            <span class="status-chip" :class="'status-' + item.status">{{ t(item.status) }}</span>

            <div class="row-actions">
              <fv-button :border-radius="30" icon="Sort" :background="bgColor" :foreground="mainColor" :disabled="!item.chartInfo || item.status === 'rendering'" @click="openEditDialog(item.id)">{{ t('edit') }}</fv-button>
              <fv-button :border-radius="30" icon="Play" :background="bgColor" :foreground="mainColor" :disabled="!item.chartInfo || item.status === 'rendering'" @click="previewChart(item.id)">{{ t('preview') }}</fv-button>
              <fv-button :border-radius="30" icon="ChromeClose" :background="bgColor" :foreground="subColor" :disabled="item.status === 'rendering'" @click="removeChart(item.id)">{{ t('close') }}</fv-button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Config Dialog -->
    <div v-if="configDialog" class="dialog-overlay">
      <div class="md3-dialog-full dialog-fullscreen-inner">
        <div class="dialog-header border-b">
          <h3 class="dialog-title">{{ t('configure') }}</h3>
          <div style="flex: 1;"></div>
          <fv-button :border-radius="30" icon="ChromeClose" class="dialog-close-btn" @click="configDialog = false" />
        </div>
        <div class="pa-0 h-100 dialog-body">
          <ConfigView ref="configViewRef" />
        </div>
        <div class="pa-4 border-t dialog-footer">
          <div style="flex: 1;"></div>
          <fv-button :border-radius="30" :background="bgColor" :foreground="mainColor" @click="configDialog = false">{{ t('close') }}</fv-button>
          <fv-button :border-radius="30" :background="mainColor" :foreground="bgColor" @click="saveConfig">{{ t('save') }}</fv-button>
        </div>
      </div>
    </div>

    <!-- Edit Dialog -->
    <div v-if="editDialog" class="dialog-overlay" @click.self="editDialog = false">
      <div class="md3-dialog dialog-modal-inner" v-if="editingChart?.chartInfo">
        <div class="dialog-header border-b">
          <h3 class="dialog-title">{{ t('chart-info') }}</h3>
          <div style="flex: 1;"></div>
          <fv-button :border-radius="30" icon="ChromeClose" class="dialog-close-btn" @click="editDialog = false" />
        </div>
        <div class="pa-6 dialog-body">
          <form ref="form" @submit.prevent>
            <div class="input-grid">
              <div class="input-grid-col" style="flex: 2;">
                <label class="field-label-sm">{{ t('chart-name') }}</label>
                <input v-model="editingChart.chartInfo!.name" class="blur-glass md3-field" required />
              </div>
              <div class="input-grid-col">
                <label class="field-label-sm">{{ t('level') }}</label>
                <input v-model="editingChart.chartInfo!.level" class="blur-glass md3-field" required />
              </div>
              <div class="input-grid-col">
                <label class="field-label-sm">{{ t('charter') }}</label>
                <input v-model="editingChart.chartInfo!.charter" class="blur-glass md3-field" required />
              </div>
              <div class="input-grid-col">
                <label class="field-label-sm">{{ t('composer') }}</label>
                <input v-model="editingChart.chartInfo!.composer" class="blur-glass md3-field" />
              </div>
              <div class="input-grid-col">
                <label class="field-label-sm">{{ t('illustrator') }}</label>
                <input v-model="editingChart.chartInfo!.illustrator" class="blur-glass md3-field" />
              </div>
            </div>
            <hr class="hr-divider" />
            <div class="edit-two-col">
              <div class="edit-col-left">
                <div class="field-label-sm">{{ t('aspect') }}</div>
                <div style="display: flex; align-items: center; gap: 8px;">
                  <input type="number" v-model="editingChart.aspectWidth" class="blur-glass md3-field" style="width: 80px;" />
                  <span>:</span>
                  <input type="number" v-model="editingChart.aspectHeight" class="blur-glass md3-field" style="width: 80px;" />
                </div>
              </div>
              <div class="edit-col-right">
                <label class="field-label-sm">{{ t('dim') }}</label>
                <fv-slider thumb-label="always" :min="0" :max="1" :step="0.01" v-model="editingChart.chartInfo!.backgroundDim" class="mt-4" />
              </div>
              <div class="edit-col-full">
                <div style="display: flex; align-items: center; justify-content: space-between;">
                  <span class="field-label-sm">{{ t('hold_cover') }}</span>
                  <input type="checkbox" :checked="editingChart.chartInfo!.HoldPartialCover" @change="editingChart.chartInfo!.HoldPartialCover = ($event.target as HTMLInputElement).checked" class="md3-checkbox" />
                </div>
              </div>
              <div class="edit-col-full">
                <label class="field-label-sm">{{ t('tip') }}</label>
                <input v-model="editingChart.chartInfo!.tip" class="blur-glass md3-field" />
              </div>
            </div>
          </form>
        </div>
        <div class="pa-4 border-t dialog-footer">
          <div style="flex: 1;"></div>
          <fv-button :border-radius="30" :background="bgColor" :foreground="mainColor" @click="editDialog = false">{{ t('close') }}</fv-button>
          <fv-button :border-radius="30" :background="mainColor" :foreground="bgColor" @click="saveChartInfo">{{ t('save') }}</fv-button>
        </div>
      </div>
    </div>

    <!-- Bulk Edit Dialog -->
    <div v-if="bulkEditDialog" class="dialog-overlay" @click.self="bulkEditDialog = false">
      <div class="md3-dialog dialog-modal-inner">
        <div class="dialog-header border-b">
          <h3 class="dialog-title">{{ t('bulk-edit-title') }}</h3>
          <div style="flex: 1;"></div>
          <fv-button :border-radius="30" icon="ChromeClose" class="dialog-close-btn" @click="bulkEditDialog = false" />
        </div>
        <div class="pa-6 dialog-body">
          <div class="alert-info">
            <fv-animated-icon icon="Info" :font-size="16" />
            <span>{{ t('bulk-edit-hint') }}</span>
          </div>
          <form ref="bulkForm" @submit.prevent>
            <div class="field-label-sm mb-1">{{ t('aspect') }}</div>
            <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 16px;">
              <input type="number" placeholder="Original" v-model="bulkEditData.aspectWidth" class="blur-glass md3-field" style="flex: 1;" />
              <span>:</span>
              <input type="number" placeholder="Original" v-model="bulkEditData.aspectHeight" class="blur-glass md3-field" style="flex: 1;" />
            </div>
            <div class="field-label-sm mb-1">{{ t('dim') }}</div>
            <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 8px;">
              <fv-slider :min="0" :max="1" :step="0.01" :model-value="bulkEditData.backgroundDim ?? 0" @update:model-value="bulkEditData.backgroundDim = $event" style="flex: 1;" />
              <fv-button :border-radius="30" v-if="bulkEditData.backgroundDim !== null" icon="ChromeClose" class="icon-btn" @click="bulkEditData.backgroundDim = null" />
            </div>
            <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 16px;">
              <span class="field-label-sm">{{ t('hold_cover') }}</span>
              <div class="btn-group">
                <fv-button :border-radius="30" type="button" class="group-btn" :class="{ 'is-active': bulkEditData.holdCover === null }" @click="bulkEditData.holdCover = null">Keep</fv-button>
                <fv-button :border-radius="30" type="button" class="group-btn" :class="{ 'is-active': bulkEditData.holdCover === 1 }" @click="bulkEditData.holdCover = 1">On</fv-button>
                <fv-button :border-radius="30" type="button" class="group-btn" :class="{ 'is-active': bulkEditData.holdCover === 0 }" @click="bulkEditData.holdCover = 0">Off</fv-button>
              </div>
            </div>
          </form>
        </div>
        <div class="pa-4 border-t dialog-footer">
          <div style="flex: 1;"></div>
          <fv-button :border-radius="30" :background="bgColor" :foreground="mainColor" @click="bulkEditDialog = false">{{ t('close') }}</fv-button>
          <fv-button :border-radius="30" :background="mainColor" :foreground="bgColor" @click="saveBulkEdit">{{ t('save') }}</fv-button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.batch-layout {
  display: flex;
  flex-direction: column;
  height: 100vh;
  width: 100%;
  padding: 20px;
  box-sizing: border-box;
  font-family: 'Segoe UI', 'Microsoft YaHei', 'PingFang SC', sans-serif;
}


/* ===== Header ===== */
.batch-header {
  flex-shrink: 0;
  background: transparent;
  backdrop-filter: blur(16px);
  border: 1px solid rgba(var(--main-color-rgb), 0.08);
  border-radius: 20px;
  padding: 14px 18px;
  margin-bottom: 12px;
}

.header-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.search-wrap {
  position: relative;
  flex: 1;
  max-width: 320px;
  min-width: 160px;
}

.search-icon {
  position: absolute;
  left: 12px;
  top: 50%;
  transform: translateY(-50%);
  color: rgba(var(--main-color-rgb), 0.4);
  pointer-events: none;
  font-size: 18px;
}

.md3-search {
  width: 100%;
  padding: 8px 12px 8px 36px;
  background: rgba(var(--main-color-rgb), 0.06);
  border: 1px solid rgba(var(--main-color-rgb), 0.1);
  border-radius: 16px;
  color: rgba(var(--main-color-rgb), 0.9);
  font-size: 13px;
  font-family: inherit;
  transition: border-color 0.2s ease;
}
.md3-search:focus {
  outline: none;
  border-color: rgba(var(--main-color-rgb),0.5);
}
.md3-search::placeholder {
  color: rgba(var(--main-color-rgb), 0.35);
}

.header-actions {
  display: flex;
  gap: 8px;
  margin-left: auto;
}

/* ===== Selection Bar ===== */
.selection-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  flex-shrink: 0;
}

.sel-left, .sel-right {
  display: flex;
  align-items: center;
  gap: 10px;
}

.sel-count {
  font-size: 13px;
  color: rgba(var(--main-color-rgb), 0.55);
}

/* ===== Render Bar ===== */
.render-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  flex-shrink: 0;
}

.rendering-chip {
  animation: pulse-glow 2s infinite;
}
@keyframes pulse-glow {
  0% { box-shadow: 0 0 0 0 rgba(var(--main-color-rgb), 0.4); }
  70% { box-shadow: 0 0 0 8px rgba(var(--main-color-rgb), 0); }
  100% { box-shadow: 0 0 0 0 rgba(var(--main-color-rgb), 0); }
}

.spin-anim { animation: spin 1.5s linear infinite; }
@keyframes spin { 100% { transform: rotate(360deg); } }

/* ===== Chart List ===== */
.chart-list {
  flex: 1;
  overflow: hidden;
  position: relative;
}

.list-scroll {
  height: 100%;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  gap: 16px;
  color: rgba(var(--main-color-rgb), 0.35);
  font-size: 14px;
}

/* ===== Chart Row (MD3 Card) ===== */
.chart-row {
  position: relative;
  background: transparent;
  backdrop-filter: blur(12px);
  border: 1px solid rgba(var(--main-color-rgb), 0.08);
  border-radius: 16px;
  overflow: hidden;
  transition: all 0.2s ease;
}

.chart-row:hover {
  background: rgba(var(--main-color-rgb), 0.04);
  border-color: rgba(var(--main-color-rgb), 0.15);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
}

.chart-row.is-failed {
  border-left: 3px solid var(--sub-color);
}

.chart-row.is-rendering {
  border: 1px solid rgba(var(--main-color-rgb), 0.3);
}

.progress-bg {
  position: absolute;
  top: 0; left: 0;
  height: 100%;
  background: linear-gradient(90deg, rgba(var(--main-color-rgb),0.08), rgba(var(--main-color-rgb),0.18));
  z-index: 0;
  transition: width 0.3s linear;
}

.row-content {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  padding: 12px 14px;
  gap: 12px;
}

.row-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.row-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.row-name {
  font-size: 14px;
  font-weight: 600;
  color: rgba(var(--main-color-rgb), 0.9);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row-sub {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: rgba(var(--main-color-rgb), 0.45);
}

.render-status {
  color: var(--main-color);
  font-weight: 500;
}

.error-status {
  color: var(--sub-color);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}

.field-label-sm {
  font-size: 12px;
  font-weight: 600;
  color: rgba(var(--main-color-rgb), 0.5);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

/* ===== Dialogs ===== */
.md3-dialog {
  background: transparent !important;
  backdrop-filter: blur(24px) !important;
  border: 1px solid rgba(var(--main-color-rgb), 0.08) !important;
  border-radius: 28px !important;
}

.md3-dialog-full {
  background: transparent !important;
  backdrop-filter: blur(24px) !important;
}

.border-b { border-bottom: 1px solid rgba(var(--main-color-rgb), 0.06) !important; }
.border-t { border-top: 1px solid rgba(var(--main-color-rgb), 0.06) !important; }

/* ===== Spacing utilities (replacing Vuetify) ===== */
.pa-0 { padding: 0 !important; }
.pa-4 { padding: 16px !important; }
.pa-6 { padding: 24px !important; }
.h-100 { height: 100% !important; }
.mb-1 { margin-bottom: 4px !important; }
.mb-4 { margin-bottom: 16px !important; }
.mt-4 { margin-top: 16px !important; }
.mt-2 { margin-top: 8px !important; }

/* ===== Dialogs (custom overlay) ===== */
.dialog-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
}
.dialog-fullscreen-inner {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.dialog-modal-inner {
  max-width: 600px;
  width: 90%;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.dialog-header {
  display: flex;
  align-items: center;
  padding: 12px 20px;
  flex-shrink: 0;
}
.dialog-title {
  font-size: 18px;
  font-weight: 600;
  color: rgba(var(--main-color-rgb), 0.9);
  margin: 0;
}
.dialog-close-btn {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: 50%;
  cursor: pointer;
  transition: background 0.15s ease;
}
.dialog-close-btn:hover {
  background: rgba(var(--main-color-rgb), 0.08);
}
.dialog-body {
  flex: 1;
  overflow-y: auto;
}
.dialog-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

/* ===== Custom Checkbox ===== */
.md3-checkbox {
  width: 18px;
  height: 18px;
  cursor: pointer;
  accent-color: var(--sub-color, var(--main-color));
  flex-shrink: 0;
}

/* ===== Level Badge (replaces v-chip for level) ===== */
.level-badge {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 8px;
  font-size: 11px;
  font-weight: 600;
  border: 1px solid rgba(var(--main-color-rgb),0.4);
  color: var(--main-color);
  background: transparent;
  flex-shrink: 0;
}

/* ===== Status Chip (replaces v-chip for status) ===== */
.status-chip {
  display: inline-flex;
  align-items: center;
  padding: 4px 12px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 500;
  flex-shrink: 0;
}
.status-chip.status-pending {
  background: rgba(var(--main-color-rgb), 0.08);
  color: rgba(var(--main-color-rgb), 0.6);
}
.status-chip.status-rendering {
  background: rgba(var(--main-color-rgb), 0.15);
  color: var(--main-color);
}
.status-chip.status-done {
  background: rgba(var(--main-color-rgb), 0.15);
  color: var(--main-color);
}
.status-chip.status-failed {
  background: rgba(var(--sub-color-rgb), 0.15);
  color: var(--sub-color);
}

/* ===== Input Grid (replaces v-row/v-col) ===== */
.input-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}
.input-grid-col {
  flex: 1;
  min-width: 120px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

/* ===== Edit Dialog Two Column Layout ===== */
.edit-two-col {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-top: 12px;
}
.edit-col-left {
  flex: 0 0 100%;
}
.edit-col-right {
  flex: 1;
  min-width: 200px;
}
.edit-col-full {
  flex: 0 0 100%;
}

/* ===== Divider ===== */
.hr-divider {
  border: none;
  border-top: 1px solid rgba(var(--main-color-rgb), 0.06);
  margin: 16px 0;
}

/* ===== Alert Info (replaces v-alert) ===== */
.alert-info {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  background: rgba(33, 150, 243, 0.08);
  border: 1px solid rgba(33, 150, 243, 0.15);
  border-radius: 12px;
  color: rgba(var(--main-color-rgb), 0.7);
  font-size: 13px;
  margin-bottom: 16px;
}

/* ===== Button Group (replaces v-btn-toggle) ===== */
.btn-group {
  display: inline-flex;
  border-radius: 20px;
  overflow: hidden;
  border: 1px solid rgba(var(--main-color-rgb),0.3);
}
.group-btn {
  padding: 6px 16px;
  border: none;
  background: transparent;
  color: rgba(var(--main-color-rgb), 0.6);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s ease;
  font-family: inherit;
}
.group-btn:hover {
  background: rgba(var(--main-color-rgb),0.08);
}
.group-btn.is-active {
  background: rgba(var(--main-color-rgb),0.15);
  color: var(--main-color);
}

/* ===== Responsive ===== */
@media (max-width: 768px) {
  .batch-layout { padding: 12px; }
  .header-actions { flex-wrap: wrap; }
  .header-actions .md3-btn span { display: none; }
  .sel-right .md3-btn span { display: none; }
}
</style>
