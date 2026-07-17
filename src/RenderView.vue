<i18n>
  en:
    already-running: Phi TKC is already running

    prev-step: Previous
    next-step: Next
    steps:
      choose: 'Choose the chart'
      config: 'Configure chart'
      options: 'Render options'
      render: 'Render'

    choose:
      archive: Archive
      folder: Folder

    chart-file: Chart file
    choose-drop: Drag and drop chart file here
    parsing: Parsing chart...

    chart-name: Chart name
    charter: Charter
    composer: Composer
    illustrator: Illustrator
    level: Level
    dim: Background dim
    hold_cover: Hold Head Partial Cover

    tip: Tip
    tip-placeholder: Leave empty to choose randomly

    width: Width
    height: Height

    file:
      title: File
      chart: Chart file (empty for default)
      music: Music (empty for default)
      illustration: Illustration (empty for default)

    preview: Preview
    render: Render
    play: Play

    render-started: Rendering has started!
    see-tasks: See tasks
    next-chart: Render Next Chart
    render-status: 'Progress: {progress}% | FPS: {fps} | ETA: {estimate}'

    ffmpeg-not-found: You haven't installed ffmpeg yet. Please download FFmpeg.exe and set Path correctly.

  zh-CN:
    already-running: Phi TKC 已经在运行

    prev-step: 上一步
    next-step: 下一步
    steps:
      choose: '选择谱面'
      config: '配置谱面'
      options: '渲染参数'
      render: '渲染视频'

    choose:
      archive: 压缩包
      folder: 文件夹

    chart-file: 谱面文件
    choose-drop: 拖拽谱面文件到此处
    parsing: 解析谱面中...

    chart-name: 谱面名
    charter: 谱师
    composer: 曲师
    illustrator: 画师
    level: 难度
    dim: 背景昏暗程度
    hold_cover: Hold 头部遮罩

    tip: Tip
    tip-placeholder: 留空则随机选择

    width: 宽
    height: 高

    preview: 演示
    render: 渲染
    play: 游玩

    render-started: 视频开始渲染了！
    see-tasks: 查看任务列表
    next-chart: 渲染下一个谱面
    render-status: '进度：{progress}% | FPS: {fps} | 预计：{estimate}'

    ffmpeg-not-found: 笨蛋怎么没安装 FFmpeg。请下载 FFmpeg.exe 并设置 Path。

</i18n>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue';
import { useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { event } from '@tauri-apps/api';
import { toastError, RULES, toast, anyFilter, isString } from './common';
import type { ChartInfo, FileDropEvent, Task } from './model';
import ConfigView from './components/ConfigView.vue';
import moment from 'moment';
import * as dialog from '@tauri-apps/plugin-dialog';
import * as shell from '@tauri-apps/plugin-shell';
import { listen } from '@tauri-apps/api/event';
import gsap from 'gsap';
import { bgColor, mainColor, subColor } from './stores/colorStore';

const { t } = useI18n();

if (!(await invoke('is_the_only_instance'))) {
  await dialog.message(t('already-running'));
  await invoke('exit_program');
}

const router = useRouter();

const steps = ['choose', 'config', 'options', 'render'];
const stepIndex = ref(1),
  step = computed(() => steps[stepIndex.value - 1]);

const chartInfo = ref<ChartInfo>();
const pstBackgroundDim = ref(0)
let chartPath = '';

const choosingChart = ref(false),
  parsingChart = ref(false);

const flipCardRef = ref<HTMLElement>();

function handleDimChange(val: number) {
  // 1. 先更新中间临时变量
  pstBackgroundDim.value = val

  // 2. 判断chartInfo存在再赋值，避免空报错
    if (chartInfo.value) {
      chartInfo.value.backgroundDim = val / 100
    }
  }

// 页面初始化时同步初始值（双向同步兜底）
if (chartInfo.value) {
pstBackgroundDim.value = chartInfo.value.backgroundDim * 100
}

async function chooseChart(folder?: boolean) {
  if (choosingChart.value) return;
  choosingChart.value = true;
  try {
    let file = folder
      ? await dialog.open({ directory: true })
      : await dialog.open({
          filters: [
            {
              name: t('choose.filter-name'),
              extensions: ['zip', 'pez'],
            },
            anyFilter(),
          ],
        });
    if (!file) return;
    await loadChart(file as string);
  } finally {
    choosingChart.value = false;
  }
}

async function loadChart(file: string) {
  try {
    parsingChart.value = true;
    chartPath = file;
    renderCover.value = null;
    chartInfo.value = (await invoke('parse_chart', { path: file })) as ChartInfo;
    aspectWidth.value = String(chartInfo.value.aspectRatio);
    aspectHeight.value = '1.0';
    for (let asp of [
      [16, 9],
      [4, 3],
      [8, 5],
      [3, 2],
    ]) {
      if (Math.abs(asp[0] / asp[1] - chartInfo.value.aspectRatio) < 1e-4) {
        aspectWidth.value = String(asp[0]);
        aspectHeight.value = String(asp[1]);
        break;
      }
    }

    await nextTick();
    performFlip();
  } catch (e) {
    toastError(e);
  } finally {
    parsingChart.value = false;
  }
}

function performFlip() {
  stepIndex.value = 2;
  nextTick(() => {
    if (flipCardRef.value) {
      gsap.from(flipCardRef.value, {
        scale: 0.9,
        opacity: 0,
        y: 20,
        duration: 0.5,
        ease: 'power2.out',
      });
    }
  });
}

function unflipCard() {
  stepIndex.value = 1;
}

const aspectWidth = ref('0'),
  aspectHeight = ref('0');

const form = ref();

const configView = ref<typeof ConfigView>();
async function buildParams() {
  let config = await configView.value!.buildConfig();
  if (!config) return null;
  const aspect = tryParseAspect();
  if (aspect !== undefined) {
    chartInfo.value!.aspectRatio = aspect;
  }
  return {
    path: chartPath,
    info: chartInfo.value,
    config,
  };
}

watch([aspectWidth, aspectHeight], ([newWidth, newHeight]) => {
  try {
    const width = parseFloat(newWidth);
    const height = parseFloat(newHeight);
    if (!isNaN(width) && !isNaN(height) && chartInfo.value) {
      chartInfo.value.aspectRatio = width / height;
    }
  } catch (e) {
    console.error('Failed to update aspect ratio:', e);
  }
});

async function postRender() {
  try {
    if (!(await invoke('test_ffmpeg'))) {
      await dialog.message(t('ffmpeg-not-found'));
      await invoke('open_app_folder');
      await shell.open('https://mivik.moe/ffmpeg-windows/');
      return false;
    }
    let params = await buildParams();
    if (!params) return false;
    const outputPath = localStorage.getItem('outputPath');
    await invoke('post_render', { params, outputPath: outputPath || null });
    return true;
  } catch (e) {
    toastError(e);
    return false;
  }
}

async function previewChart() {
  try {
    let params = await buildParams();
    if (!params) return false;
    params.config.autoplay = true;
    await invoke('preview_chart', { params });
    return true;
  } catch (e) {
    toastError(e);
    return false;
  }
}

async function playChart() {
  try {
    let params = await buildParams();
    if (!params) return false;
    params.config.autoplay = false;
    await invoke('preview_chart', { params });
    return true;
  } catch (e) {
    toastError(e);
    return false;
  }
}

const renderMsg = ref(''),
  renderProgress = ref<number>(),
  renderDuration = ref<number>();
const renderCover = ref<string | null>(null);
let coverPollInterval: ReturnType<typeof setInterval> | null = null;
event.listen('render-msg', (msg) => (renderMsg.value = msg.payload as string));
event.listen('render-progress', (msg) => {
  let payload = msg.payload as { progress: number; fps: number; estimate: number };
  renderMsg.value = t('render-status', {
    progress: (payload.progress * 100).toFixed(2),
    fps: payload.fps,
    estimate: moment.duration(payload.estimate, 'seconds').humanize(true, { ss: 1 }),
  });
  renderProgress.value = payload.progress * 100;
});

event.listen('render-done', (msg) => {
  stepIndex.value++;
  renderDuration.value = Math.round(msg.payload as number);
});

async function fetchRenderCover() {
  try {
    const tasks = await invoke<Task[]>('get_tasks');
    if (tasks && tasks.length > 0) {
      const task = tasks.find(t => t.path === chartPath);
      if (task?.cover) {
        renderCover.value = task.cover;
      }
    }
  } catch (e) {
    console.error('Failed to fetch render cover:', e);
  }
}

function startCoverPolling() {
  stopCoverPolling();
  fetchRenderCover();
  coverPollInterval = setInterval(fetchRenderCover, 700);
}

function stopCoverPolling() {
  if (coverPollInterval) {
    clearInterval(coverPollInterval);
    coverPollInterval = null;
  }
}

watch(step, (s) => {
  if (s === 'render') startCoverPolling();
  else stopCoverPolling();
});

onUnmounted(() => stopCoverPolling());

async function moveNext() {
  if (step.value === 'config') {
    // VFluent3 v-form无validate API，手动校验必填字段
    if (!chartInfo.value) {
      toast(t('has-error'), 'error');
      return;
    }
    const { name, level, charter } = chartInfo.value;
    // 判断三个必填是否为空/纯空格
    const isInvalid = !name?.trim() || !level?.trim() || !charter?.trim();
    if (isInvalid) {
      toast(t('has-error'), 'error');
      return;
    }
    // 校验通过，切换页面
    stepIndex.value++;
    return;
  }
  if (step.value === 'options') {
    if (await postRender()) {
      stepIndex.value++;
    }
    return;
  }
}


function goBack() {
  if (step.value === 'config') {
    unflipCard();
    return;
  }
  if (stepIndex.value > 1) {
    stepIndex.value--;
  }
}

let chartInQuery = router.currentRoute.value.query.chart;
if (isString(chartInQuery)) {
  onMounted(() => loadChart(chartInQuery as string));
}

function tryParseAspect(): number | undefined {
  try {
    let width = parseFloat(aspectWidth.value);
    let height = parseFloat(aspectHeight.value);
    if (isNaN(width) || isNaN(height)) return undefined;
    return width / height;
  } catch (e) {
    return undefined;
  }
}

const fileHovering = ref(false);
listen('tauri://drag-over', () => (fileHovering.value = step.value === 'choose'));
listen('tauri://drag-leave', () => (fileHovering.value = false));
listen('tauri://drag-drop', async (event) => {
  const files = (event.payload as FileDropEvent).paths;
  if (step.value === 'choose') {
    fileHovering.value = false;
    await loadChart(files[0]);
  } else if (step.value === 'config' || step.value === 'options' || step.value === 'render') {
    fileHovering.value = false;
    stepIndex.value = 1;
    await loadChart(files[0]);
  }
});

function resetAndGoChoose() {
  stepIndex.value = 1;
  chartInfo.value = undefined;
  chartPath = '';
  renderCover.value = null;
}
</script>

<template>
  <div class="render-container">
    <!-- MD3 Top Action Bar -->
    <header class="md3-top-bar" v-if="step !== 'choose'">
      <div class="bar-left">
        <button class="bar-btn bar-btn-text" v-if="step === 'config' || step === 'options'" @click="goBack">
          <v-icon icon="mdi-arrow-left" size="20" />
          <span>{{ t('prev-step') }}</span>
        </button>
      </div>

      <div class="bar-center">
        <div class="step-chips">
          <span
            v-for="(s, i) in steps"
            :key="s"
            class="step-chip"
            :class="{ 'is-active': step === s, 'is-done': stepIndex > i + 1 }"
          >
            <span class="chip-dot"></span>
            <span class="chip-label" v-if="step === s">{{ t('steps.' + s) }}</span>
          </span>
        </div>
      </div>

      <div class="bar-right">
        <button class="bar-btn bar-btn-tonal" v-if="step === 'options'" @click="playChart">
          <v-icon icon="mdi-gamepad-variant" size="18" />
          <span>{{ t('play') }}</span>
        </button>
        <button class="bar-btn bar-btn-tonal" v-if="step === 'options'" @click="previewChart">
          <v-icon icon="mdi-eye" size="18" />
          <span>{{ t('preview') }}</span>
        </button>
        <button class="bar-btn bar-btn-filled" v-if="step === 'config' || step === 'options'" @click="moveNext">
          <span>{{ step === 'options' ? t('render') : t('next-step') }}</span>
          <v-icon icon="mdi-arrow-right" size="18" />
        </button>
      </div>
    </header>

    <!-- Content area -->
    <main class="render-content">
      <!-- Step 1: Choose (Bookshelf) -->
      <div class="step-panel" :class="{ 'is-active': step === 'choose' }">
        <div class="clear-glass bookshelf glass" v-glass>
          <div class="glass fluent-glass shelf-label" v-glass>{{ t('steps.choose') }}</div>
          <div class="shelf-row">
            <!-- Archive card (book-style) -->
            <div class="glass book-card" v-glass @click="chooseChart(false)">
              <div class="fluent-glass book-face">
                <v-icon icon="mdi-folder-zip-outline" size="40" class="book-icon" />
                <span class="book-title">{{ t('choose.archive') }}</span>
                <span class="book-desc">.zip / .pez</span>
              </div>
            </div>
            <!-- Folder card (book-style) -->
            <div class="book-card glass" v-glass @click="chooseChart(true)">
              <div class="fluent-glass book-face">
                <v-icon icon="mdi-folder-outline" size="40" class="book-icon" />
                <span class="book-title">{{ t('choose.folder') }}</span>
                <span class="book-desc">{{ t('choose.folder') }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Step 2: Config -->
      <div class="step-panel" :class="{ 'is-active': step === 'config' }">
        <div class="config-card-wrapper" ref="flipCardRef">
          <div class="glass fluent-glass config-card" v-glass v-if="chartInfo">
            <v-form ref="form" class="config-form" @submit.prevent>
                <h2 class="config-title">{{ t('steps.config') }}</h2>
                <div class="config-grid">
                  <div class="config-section">
                    <div class="field-group">
                      <label class="field-label">{{ t('chart-name') }} *</label>
                      <input v-model="chartInfo.name" class="blur-glass md3-field" :class="{ 'has-value': chartInfo.name }" />
                    </div>
                    <div class="field-group">
                      <label class="field-label">{{ t('level') }} *</label>
                      <input v-model="chartInfo.level" class="blur-glass md3-field" :class="{ 'has-value': chartInfo.level }" />
                    </div>
                    <div class="field-group">
                      <label class="field-label">{{ t('charter') }} *</label>
                      <input v-model="chartInfo.charter" class="blur-glass md3-field" :class="{ 'has-value': chartInfo.charter }" />
                    </div>
                    <div class="field-group">
                      <label class="field-label">{{ t('composer') }}</label>
                      <input v-model="chartInfo.composer" class="blur-glass md3-field" :class="{ 'has-value': chartInfo.composer }" />
                    </div>
                    <div class="field-group">
                      <label class="field-label">{{ t('illustrator') }}</label>
                      <input v-model="chartInfo.illustrator" class="blur-glass md3-field" :class="{ 'has-value': chartInfo.illustrator }" />
                    </div>
                  </div>
                  <div class="config-section">
                    <div class="field-group">
                      <label class="field-label">{{ t('tip') }}</label>
                      <input v-model="chartInfo.tip" class="blur-glass md3-field" :placeholder="t('tip-placeholder')" />
                    </div>
                    <div class="field-group">
                      <label class="field-label">{{ t('dim') }} — {{ Math.round(pstBackgroundDim) }}%</label>
                      <fv-slider
                        class="slider"
                        :mininum="0"
                        :unit=1
                        :maxinum="100" 
                        :color="mainColor"
                        :background="bgColor"
                        :icon-wrapper-background="bgColor"
                        v-model:value="pstBackgroundDim"
                        @update:model-value = "handleDimChange"
                        >
                      </fv-slider>
                    </div>
                    <div class="field-group field-toggle">
                      <label class="field-label">{{ t('hold_cover') }}</label>
                      <fv-ToggleSwitch
                        :borderColor="mainColor"
                        :ringBackground="mainColor"
                        :switchOnBackground="subColor"
                        :onForeground="subColor"
                        :offForeground="mainColor"
                        class="switch"
                        v-model:value= "chartInfo.HoldPartialCover"
                        @click="chartInfo.HoldPartialCover = !chartInfo.HoldPartialCover"
                      />
                    </div>
                  </div>
                </div>
              </v-form>
          </div>
        </div>
      </div>

      <!-- Step 3: Options -->
      <div class="fluent-glass glass step-panel" v-glass :class="{ 'is-active': step === 'options' }">
        <div class="options-wrapper">
          <ConfigView ref="configView" :init-aspect-ratio="tryParseAspect()" />
        </div>
      </div>

      <!-- Step 4: Render -->
      <div class="step-panel" :class="{ 'is-active': step === 'render' }">
        <div class="render-card" v-if="chartInfo">
          <!-- Left: Illustration -->
          <div class="render-cover">
            <img v-if="renderCover" :src="convertFileSrc(renderCover)" class="cover-img" />
            <div v-else class="cover-placeholder">
              <v-icon icon="mdi-music-note-outline" size="48" color="rgba(255,255,255,0.15)" />
            </div>
          </div>
          <!-- Right: Info -->
          <div class="render-info">
            <div class="render-header">
              <v-icon icon="mdi-video" size="24" color="primary" />
              <h3>{{ t('render-started') }}</h3>
            </div>
            <div class="info-rows">
              <div class="info-row"><span class="info-label">{{ t('chart-name') }}</span><span class="info-value">{{ chartInfo.name }}</span></div>
              <div class="info-row"><span class="info-label">{{ t('level') }}</span><span class="info-value">{{ chartInfo.level }}</span></div>
              <div class="info-row"><span class="info-label">{{ t('charter') }}</span><span class="info-value">{{ chartInfo.charter }}</span></div>
              <div class="info-row" v-if="chartInfo.composer"><span class="info-label">{{ t('composer') }}</span><span class="info-value">{{ chartInfo.composer }}</span></div>
              <div class="info-row" v-if="chartInfo.illustrator"><span class="info-label">{{ t('illustrator') }}</span><span class="info-value">{{ chartInfo.illustrator }}</span></div>
            </div>
            <div v-if="renderProgress !== undefined" class="render-progress">
              <v-progress-linear :model-value="renderProgress" color="primary" height="6" rounded />
              <p class="progress-text">{{ renderMsg }}</p>
            </div>
            <div class="render-actions">
              <button class="bar-btn bar-btn-tonal" @click="router.push({ name: 'tasks' })">
                <v-icon icon="mdi-view-list" size="18" />
                <span>{{ t('see-tasks') }}</span>
              </button>
              <button class="bar-btn bar-btn-filled" @click="resetAndGoChoose">
                <v-icon icon="mdi-plus" size="18" />
                <span>{{ t('next-chart') }}</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </main>

    <!-- File drag overlay -->
    <v-overlay v-model="fileHovering" contained class="fluent-glass align-center justify-center drop-overlay" persistent :close-on-content-click="false">
      <div class="glass drop-zone" v-glass>
        <fv-AnimatedIcon :font-size=32 icon="OpenFile" modelValue="bounceRotate" customize-animation="棍母。。。" />
        <!-- 我故意的，不让你点嘻嘻 -->
        <p>{{ t('choose-drop') }}</p>
      </div>
    </v-overlay>

    <!-- Parsing overlay -->
    <v-overlay v-model="parsingChart" contained class="align-center justify-center parse-overlay" persistent :close-on-content-click="false">
      <div class="parse-card">
        <v-progress-circular indeterminate color="primary" size="36" width="3" />
        <span>{{ t('parsing') }}</span>
      </div>
    </v-overlay>
  </div>
</template>

<style scoped>
.render-container {
  display: flex;
  flex-direction: column;
  height: 100vh;
  width: 100%;
  background: transparent;
  font-family: 'Segoe UI', 'Microsoft YaHei', 'PingFang SC', sans-serif;
  color: #e3e2e6;
}

/* ===== MD3 Top Action Bar ===== */
.md3-top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 56px;
  padding: 0 20px;
  background: rgba(var(--bg-color-rgb), 0.88);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  flex-shrink: 0;
  z-index: 10;
}

.bar-left, .bar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.bar-center {
  flex: 1;
  display: flex;
  justify-content: center;
}

.step-chips {
  display: flex;
  align-items: center;
  gap: 8px;
}

.step-chip {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-radius: 16px;
  transition: all 0.3s ease;
}

.chip-dot {
  width: 8px; height: 8px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.25);
  transition: all 0.3s ease;
}

.step-chip.is-active {
  background: rgba(130, 177, 255, 0.12);
}
.step-chip.is-active .chip-dot {
  background: #82b1ff;
  box-shadow: 0 0 8px rgba(130, 177, 255, 0.5);
}
.step-chip.is-done .chip-dot {
  background: #7dd87d;
}

.chip-label {
  font-size: 13px;
  font-weight: 500;
  color: rgba(255, 255, 255, 0.8);
}

/* MD3 Button variants */
.bar-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border: none;
  border-radius: 20px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s cubic-bezier(0.2, 0, 0, 1);
  white-space: nowrap;
  font-family: inherit;
  letter-spacing: 0.1px;
}

/* ===== Content Area ===== */
.render-content {
  flex: 1;
  overflow-y: auto;
  position: relative;
  padding: 24px;
}

.step-panel {
  position: absolute;
  inset: 0;
  padding: 24px;
  opacity: 0;
  transform: translateY(16px);
  pointer-events: none;
  transition: opacity 0.35s cubic-bezier(0.2, 0, 0, 1), transform 0.35s cubic-bezier(0.2, 0, 0, 1);
  overflow-y: auto;
}
.step-panel.is-active {
  opacity: 1;
  transform: translateY(0);
  pointer-events: auto;
}

/* ===== Bookshelf (Step 1) ===== */
.bookshelf {
  border-radius: 16px;
  display: flex;
  width: 80%;
  max-width: 500px;
  max-height: 300px;
  flex-direction: column;
  padding: 20px;
  justify-content: center;
  height: auto;
}

.shelf-label {
  padding: 12px 20px;
  font-size: 22px;
  font-weight: 600;
  color: var(--main-color);
  margin-bottom: 32px;
  letter-spacing: 0.5px;
}

.shelf-row {
  display: flex;
  flex-direction: column; /* 改为竖直排列 */
  gap: 16px; /* 调整卡片之间的间距 */
  width: 100%; /* 确保占满父容器宽度 */
  height: 100%; /* 确保占满父容器高度 */
}

.book-card {
  min-width: 100%;
  max-height: 62px; /* 高度根据内容自适应 */
  border-radius: 16px; /* 调整圆角 */
  cursor: pointer;
  position: relative;
  transition: transform 0.3s ease, box-shadow 0.3s ease;
  display: flex; /* 添加 flex 布局 */
  flex-direction: column; /* 保持竖直布局 */
}

.book-face {
  min-width: 100%;
  height: 100%;
  padding: 18px 20px;
  border-radius: 16px;
  border-width: 1px;
  border-style: solid;
  border-color: rgba(var(--main-color-rgb), 0.08);
}


.book-title {
  font-size: 16px;
  font-weight: 600;
  color: rgba(var(--main-color-rgb), 0.9);
}

.book-desc {
  margin: 10px;
  font-size: 12px;
  color: rgba(var(--main-color-rgb), 0.45);
}

/* ===== Config Card (Step 2) ===== */
.config-card-wrapper {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  padding: 8px;
}

.config-card {
  width: 100%;
  border-radius: 28px;
  padding: 32px;
}

.config-form {
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.config-title {
  font-size: 22px;
  font-weight: 700;
  color: rgba(var(--main-color-rgb), 0.9);
  margin: 0;
}

.config-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 24px;
}

.config-section {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.field-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.field-label {
  font-size: 12px;
  font-weight: 800;
  color: rgba(var(--main-color-rgb), 0.55);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.md3-field {
  width: 100%;
  border-top: 1px solid rgba(var(--main-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--main-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--main-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--main-color-rgb), 0.05);
  padding: 12px 16px;
  border-radius: 16px;
  font-size: 14px;
  font-family: inherit;
  transition: border-color 0.2s ease, box-shadow 0.2s ease;
}
.md3-field:focus {
  outline: none;
  border-top: 1px solid rgba(var(--sub-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--sub-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--sub-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--sub-color-rgb), 0.05);
  box-shadow: 0 0 0 3px rgba(var(--sub-color-rgb), 0.15);
}
.md3-field::placeholder {
  color: rgba(var(--main-color-rgb), 0.3);
}

.aspect-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.aspect-field {
  flex: 1;
  text-align: center;
}
.aspect-sep {
  font-size: 18px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.4);
}

.slider {
  width: 100%;
  appearance: none;
  color: var(--main-color);
  border-radius: 2px;
  cursor: pointer;
  margin-top: 20px;
}

.field-toggle {
  flex-direction: row !important;
  align-items: center;
  justify-content: space-between;
}

.switch {
  height: 28px;
  cursor: pointer;
  position: relative;
  transition: all 0.25s ease;
}

/* ===== Options (Step 3) ===== */
.options-wrapper {
  height: 100%;
  overflow: hidden;
}

/* ===== Render Card (Step 4) ===== */
.render-card {
  display: flex;
  flex-direction: row;
  max-width: 900px;
  margin: 24px auto;
  border-radius: 28px;
  overflow: hidden;
  min-height: 360px;
}

.render-cover {
  width: 40%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.cover-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  min-height: 280px;
}

.render-info {
  width: 60%;
  padding: 28px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.render-header {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}
.render-header h3 {
  font-size: 18px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
  margin: 0;
}

.info-rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
}
.info-row {
  display: flex;
  justify-content: space-between;
  padding: 7px 0;
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
}
.info-row:last-child { border-bottom: none; }
.info-label { color: rgba(255, 255, 255, 0.5); font-size: 13px; }
.info-value { color: rgba(255, 255, 255, 0.85); font-size: 13px; text-align: right; }

.render-progress {
  margin-top: 8px;
}
.progress-text {
  text-align: center;
  margin-top: 6px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.5);
}

.render-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: auto;
  padding-top: 12px;
}

/* ===== Overlays ===== */
.drop-overlay {
  z-index: 200;
}
.drop-zone {
  padding: 48px;
  display: flex;
  flex-direction: column;
  align-items: center;
  border-radius: 16px;
  border: 2px dashed rgba(var(--bg-color-rgb), 0.4);
}
.drop-zone p {
  margin: 0;
  font-size: 16px;
  font-weight: 500;
}

.parse-overlay {
  background-color: rgba(0, 0, 0, 0.85);
  z-index: 200;
}
.parse-card {
  display: flex;
  align-items: center;
  gap: 16px;
  background: rgba(28, 28, 28, 0.95);
  border-radius: 28px;
  padding: 24px 32px;
  color: rgba(255, 255, 255, 0.7);
  font-size: 14px;
}

/* ===== Responsive ===== */
@media (max-width: 768px) {
  .config-grid {
    grid-template-columns: 1fr;
  }
  .shelf-row {
    gap: 16px;
  }
  .book-card {
    width: 160px;
    height: 260px;
  }
  .md3-top-bar {
    padding: 0 12px;
    height: 48px;
  }
  .bar-btn span {
    display: none;
  }
  .config-card {
    padding: 20px;
    border-radius: 20px;
  }
  .render-card {
    flex-direction: column;
    margin: 16px;
  }
  .render-cover {
    width: 100%;
    min-height: 200px;
  }
  .render-info {
    width: 100%;
    padding: 20px;
  }
}
</style>
