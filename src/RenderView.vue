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
      filter-name: Chart file

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
    render-waiting: Waiting for the render queue...
    render-loading: Loading chart...
    render-mixing: Mixing audio...
    render-done: 'Render finished in {duration}'
    render-canceled: Render canceled
    render-failed: Render failed — see the task list for details
    duration: Duration

    ffmpeg-not-found: You haven't installed ffmpeg yet. Please download FFmpeg.exe and put it in the specific folder.
    remote-submitted: 'Sent to the render server (job {id})'

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
      filter-name: 谱面文件

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

    file:
      title: 文件
      chart: 谱面文件（留空则使用默认）
      music: 音乐（留空则使用默认）
      illustration: 插画（留空则使用默认）

    preview: 演示
    render: 渲染
    play: 游玩

    render-started: 视频开始渲染了！
    see-tasks: 查看任务列表
    next-chart: 渲染下一个谱面
    render-status: '进度：{progress}% | FPS: {fps} | 预计：{estimate}'
    render-waiting: 等待渲染队列…
    render-loading: 正在加载谱面…
    render-mixing: 正在混音…
    render-done: '渲染完成，耗时 {duration}'
    render-canceled: 渲染已取消
    render-failed: 渲染失败，详情请查看任务列表
    duration: 耗时

    ffmpeg-not-found: 笨蛋怎么没安装 FFmpeg。请下载 FFmpeg.exe 并放置在指定文件夹内。
    remote-submitted: 已提交到渲染服务器（任务 {id}）

</i18n>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue';
import { useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { toastError, toast, anyFilter, isString, RULES } from './common';
import type { ChartInfo, Task } from './model';
import { VForm } from 'vuetify/components';
import ConfigView from './components/ConfigView.vue';
import moment from 'moment';
import * as dialog from '@tauri-apps/plugin-dialog';
import * as shell from '@tauri-apps/plugin-shell';
import gsap from 'gsap';
import { isAndroid, isMobile } from './platform';

const android = isAndroid();
const mobile = isMobile();

const { t } = useI18n();

// Polling `get_tasks` is the only reliable progress source: the Rust backend never
// emitted the `render-msg` / `render-progress` / `render-done` events these listeners
// used to wait for, so live progress was always empty.
let disposed = false;
let unlistenDrag: (() => void) | null = null;

if (!(await invoke('is_the_only_instance'))) {
  await dialog.message(t('already-running'));
  await invoke('exit_program');
}

const router = useRouter();

const steps = ['choose', 'config', 'options', 'render'];
const stepIndex = ref(1),
  step = computed(() => steps[stepIndex.value - 1]);

const chartInfo = ref<ChartInfo>();
const pstBackgroundDim = ref(0);
let chartPath = '';
let renderTaskId: number | null = null;

const choosingChart = ref(false),
  parsingChart = ref(false);

const flipCardRef = ref<HTMLElement>();

function handleDimChange(val: number) {
  pstBackgroundDim.value = val;
  if (chartInfo.value) {
    chartInfo.value.backgroundDim = val / 100;
  }
}

async function chooseChart(folder?: boolean) {
  if (choosingChart.value) return;
  choosingChart.value = true;
  try {
    // Android charts use ACTION_OPEN_DOCUMENT and are imported before the
    // temporary URI permission expires; GET_CONTENT may open a photo picker.
    let file = folder
      ? await dialog.open({ directory: true })
      : android
        ? await invoke<string | null>('pick_chart')
        : await dialog.open({
                filters: [
                  {
                    name: t('choose.filter-name'),
                    extensions: ['zip', 'pez', 'json'],
                  },
                  anyFilter(),
                ],
              });
    if (!file) return;
    await loadChart(file as string);
  } catch (e) {
    // Cancelling the system picker is a normal outcome, not an error.
    if (!/cancel/i.test(String(e))) toastError(e);
  } finally {
    choosingChart.value = false;
  }
}

async function loadChart(file: string) {
  if (!file) return;
  try {
    parsingChart.value = true;
    chartPath = file;
    renderTaskId = null;
    renderCover.value = null;
    renderProgress.value = undefined;
    renderDuration.value = undefined;
    chartInfo.value = (await invoke('parse_chart', { path: file })) as ChartInfo;
    // Show the illustration immediately instead of waiting for the first task
    // update: covers live inside the chart archive and must be extracted first.
    try {
      const cover = await invoke<string>('load_chart_illustration', {
        path: file,
        illustration: chartInfo.value.illustration,
      });
      if (cover) renderCover.value = cover;
    } catch {
      /* charts without an illustration keep the placeholder */
    }
    // Keep the Phi-TKC dim slider in sync with the freshly parsed chart.
    pstBackgroundDim.value = Math.round(chartInfo.value.backgroundDim * 100);

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

const form = ref<VForm>();

const configView = ref<typeof ConfigView>();
async function buildParams() {
  let config = await configView.value!.buildConfig();
  if (!config) return null;
  return {
    path: chartPath,
    info: chartInfo.value,
    config,
  };
}

// Set by the Settings page: when a render server is connected, renders are
// submitted there instead of running on this device.
function remoteServer(): string {
  if (localStorage.getItem('remoteConnected') !== '1') return '';
  let value = (localStorage.getItem('remoteRenderer') || '').trim().replace(/\/+$/, '');
  if (!value) return '';
  if (!/^https?:\/\//i.test(value)) value = `http://${value}`;
  return value;
}

async function postRender(): Promise<boolean | 'remote'> {
  try {
    let params = await buildParams();
    if (!params) return false;

    const base = remoteServer();
    if (base) {
      const result = await invoke<{ id?: string }>('remote_post_render', { base, params });
      toast(t('remote-submitted', { id: result?.id ?? '' }), 'success');
      // The remote job shows up in the union task list; this device has no local
      // task to attach the progress screen to.
      return 'remote';
    }

    // Android encodes in-process with MediaCodec, so there is no ffmpeg to find.
    if (!mobile && !(await invoke('test_ffmpeg'))) {
      await dialog.message(t('ffmpeg-not-found'));
      await invoke('open_app_folder');
      await shell.open('https://github.com/BtbN/FFmpeg-Builds/releases');
      return false;
    }
    const outputPath = localStorage.getItem('outputPath');
    renderTaskId = await invoke<number>('post_render', { params, outputPath: outputPath || null });
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
let taskPollTimer: ReturnType<typeof setTimeout> | null = null;
let taskPolling = false;
let taskPollGeneration = 0;

function formatDuration(seconds: number) {
  const duration = moment.duration(Math.round(seconds), 'seconds');
  const hours = Math.floor(duration.asHours());
  const minutes = duration.minutes();
  const secs = duration.seconds();
  if (hours > 0) return `${hours}h ${minutes}m ${secs}s`;
  if (minutes > 0) return `${minutes}m ${secs}s`;
  return `${secs}s`;
}

async function pollRenderTask() {
  if (disposed || !taskPolling || renderTaskId === null) return;
  const generation = taskPollGeneration;
  const taskId = renderTaskId;
  try {
    const tasks = await invoke<Task[]>('get_tasks');
    if (disposed || !taskPolling || generation !== taskPollGeneration || taskId !== renderTaskId) return;
    // Android may import a document URI into a different path. The returned ID
    // also keeps another render of the same chart from replacing this progress.
    const task = tasks.find((item) => item.id === taskId);
    if (!task) return;

    if (!renderCover.value && task.cover) renderCover.value = task.cover;

    const status = task.status;
    switch (status.type) {
      case 'pending':
        renderProgress.value = 0;
        renderMsg.value = t('render-waiting');
        break;
      case 'loading':
        renderProgress.value = 0;
        renderMsg.value = t('render-loading');
        break;
      case 'mixing':
        renderProgress.value = 0;
        renderMsg.value = t('render-mixing');
        break;
      case 'rendering':
        renderProgress.value = status.progress * 100;
        renderMsg.value = t('render-status', {
          progress: (status.progress * 100).toFixed(2),
          fps: status.fps,
          estimate: moment.duration(status.estimate, 'seconds').humanize(true, { ss: 1 }),
        });
        break;
      case 'done':
        renderProgress.value = 100;
        renderDuration.value = Math.round(status.duration);
        renderMsg.value = t('render-done', { duration: formatDuration(status.duration) });
        stopTaskPolling();
        break;
      case 'canceled':
        renderProgress.value = 0;
        renderMsg.value = t('render-canceled');
        stopTaskPolling();
        break;
      case 'failed':
        renderProgress.value = 0;
        // The raw error can include the renderer's whole stderr; keep the card readable
        // and point the user at the task list for details.
        console.error('Render failed:', status.error);
        renderMsg.value = t('render-failed');
        toast(t('render-failed'), 'error');
        stopTaskPolling();
        break;
    }
  } catch (e) {
    console.error('Failed to poll task status:', e);
  } finally {
    if (!disposed && taskPolling && generation === taskPollGeneration) {
      taskPollTimer = setTimeout(pollRenderTask, 300);
    }
  }
}

function startTaskPolling() {
  stopTaskPolling();
  taskPolling = true;
  renderProgress.value = 0;
  renderDuration.value = undefined;
  renderMsg.value = t('render-waiting');
  void pollRenderTask();
}

function stopTaskPolling() {
  taskPolling = false;
  taskPollGeneration++;
  if (taskPollTimer !== null) {
    clearTimeout(taskPollTimer);
    taskPollTimer = null;
  }
}

watch(step, (s) => {
  if (s === 'render') startTaskPolling();
  else stopTaskPolling();
});

onUnmounted(() => {
  disposed = true;
  stopTaskPolling();
  if (unlistenDrag) {
    unlistenDrag();
    unlistenDrag = null;
  }
});

async function moveNext() {
  if (step.value === 'config') {
    // The chart fields below are Vuetify inputs inside this <v-form>, so `validate()`
    // now sees their rules (name / level / charter are required).
    const valid = form.value ? (await form.value.validate()).valid : true;
    if (valid) {
      stepIndex.value++;
    } else {
      toast(t('has-error'), 'error');
    }
    return;
  }
  if (step.value === 'options') {
    const result = await postRender();
    if (result === 'remote') {
      router.push({ name: 'tasks' });
      return;
    }
    if (result) {
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

const fileHovering = ref(false);

getCurrentWebview()
  .onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === 'enter' || payload.type === 'over') {
      fileHovering.value = step.value === 'choose';
      return;
    }
    if (payload.type === 'leave') {
      fileHovering.value = false;
      return;
    }
    fileHovering.value = false;
    const path = payload.paths[0];
    if (!path) return;
    if (step.value !== 'choose') stepIndex.value = 1;
    void loadChart(path);
  })
  .then((unlisten) => {
    if (disposed) unlisten();
    else unlistenDrag = unlisten;
  })
  .catch((e) => console.error('Failed to register drag-drop listener:', e));

function resetAndGoChoose() {
  stepIndex.value = 1;
  chartInfo.value = undefined;
  chartPath = '';
  renderTaskId = null;
  renderCover.value = null;
  renderProgress.value = undefined;
  renderDuration.value = undefined;
  renderMsg.value = '';
}
</script>

<template>
  <div class="render-container">
    <!-- MD3 Top Action Bar -->
    <header class="clear-glass glass md3-top-bar" v-glass v-if="step !== 'choose' && step !== 'render'">
      <div class="bar-left">
        <button class="bar-btn bar-btn-text" v-if="step === 'config' || step === 'options'" @click="goBack">
          <v-icon icon="mdi-arrow-left" size="20" />
          <span>{{ t('prev-step') }}</span>
        </button>
      </div>

      <div class="bar-center">
        <div class="step-chips">
          <span v-for="(s, i) in steps" :key="s" class="step-chip" :class="{ 'is-active': step === s, 'is-done': stepIndex > i + 1 }">
            <span class="chip-dot"></span>
            <span class="chip-label" v-if="step === s">{{ t('steps.' + s) }}</span>
          </span>
        </div>
      </div>

      <div class="bar-right">
        <button class="bar-btn bar-btn-tonal" v-if="step === 'options' && !mobile" @click="playChart">
          <v-icon icon="mdi-gamepad-variant" size="18" />
          <span>{{ t('play') }}</span>
        </button>
        <button class="bar-btn bar-btn-tonal" v-if="step === 'options' && !mobile" @click="previewChart">
          <v-icon icon="mdi-eye" size="18" />
          <span>{{ t('preview') }}</span>
        </button>
        <button class="bar-btn bar-btn-filled" v-if="!mobile && (step === 'config' || step === 'options')" @click="moveNext">
          <span>{{ step === 'options' ? t('render') : t('next-step') }}</span>
          <v-icon icon="mdi-arrow-right" size="18" />
        </button>
      </div>
    </header>

    <!-- Content area -->
    <main class="render-content">
      <!-- Step 1: Choose (Bookshelf) -->
      <div class="step-panel" :class="{ 'is-active': step === 'choose' }" :inert="step === 'choose' ? undefined : true">
        <div class="clear-glass bookshelf glass" v-glass>
          <div class="glass fluent-glass shelf-label" v-glass>{{ t('steps.choose') }}</div>
          <div class="shelf-row">
            <!-- Archive card (book-style) -->
            <div class="glass book-card" v-glass @click="chooseChart(false)">
              <div class="fluent-glass book-face">
                <span class="book-title">{{ t('choose.archive') }}</span>
                <span class="book-desc">.zip / .pez</span>
              </div>
            </div>
            <!-- Folder card (book-style) -->
            <div class="book-card glass" v-glass @click="chooseChart(true)">
              <div class="fluent-glass book-face">
                <span class="book-title">{{ t('choose.folder') }}</span>
                <span class="book-desc">{{ t('choose.folder') }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Step 2: Config -->
      <div class="step-panel" :class="{ 'is-active': step === 'config' }" :inert="step === 'config' ? undefined : true">
        <div class="config-card-wrapper" ref="flipCardRef">
          <div class="glass fluent-glass config-card" v-glass v-if="chartInfo">
            <v-form ref="form" class="config-form" @submit.prevent>
              <h2 class="config-title">{{ t('steps.config') }}</h2>
              <div class="config-grid">
                <div class="config-section">
                  <div class="field-group">
                    <v-text-field
                      v-model="chartInfo.name"
                      :label="t('chart-name')"
                      :rules="[RULES.non_empty]"
                      variant="outlined"
                      density="compact"
                      hide-details />
                  </div>
                  <div class="field-group">
                    <v-text-field
                      v-model="chartInfo.level"
                      :label="t('level')"
                      :rules="[RULES.non_empty]"
                      variant="outlined"
                      density="compact"
                      hide-details />
                  </div>
                  <div class="field-group">
                    <v-text-field
                      v-model="chartInfo.charter"
                      :label="t('charter')"
                      :rules="[RULES.non_empty]"
                      variant="outlined"
                      density="compact"
                      hide-details />
                  </div>
                  <div class="field-group">
                    <v-text-field v-model="chartInfo.composer" :label="t('composer')" variant="outlined" density="compact" hide-details />
                  </div>
                  <div class="field-group">
                    <v-text-field v-model="chartInfo.illustrator" :label="t('illustrator')" variant="outlined" density="compact" hide-details />
                  </div>
                </div>
                <div class="config-section">
                  <div class="field-group">
                    <label class="field-label">{{ t('dim') }} — {{ Math.round(pstBackgroundDim) }}%</label>
                    <v-slider
                      class="slider"
                      color="primary"
                      :min="0"
                      :max="100"
                      :step="1"
                      hide-details
                      v-model="pstBackgroundDim"
                      @update:model-value="handleDimChange"
                    />
                  </div>
                  <div class="field-group field-toggle">
                    <label class="field-label">{{ t('hold_cover') }}</label>
                    <v-switch class="switch" color="primary" hide-details v-model="chartInfo.HoldPartialCover" />
                  </div>
                  <div class="field-group">
                    <v-text-field
                      v-model="chartInfo.tip"
                      :label="t('tip')"
                      :placeholder="t('tip-placeholder')"
                      variant="outlined"
                      density="compact"
                      hide-details />
                  </div>
                </div>
              </div>
            </v-form>
          </div>
        </div>
      </div>

      <!-- Step 3: Options -->
      <div class="clear-glass glass step-panel" v-glass :class="{ 'is-active': step === 'options' }" :inert="step === 'options' ? undefined : true">
        <div class="options-wrapper">
          <ConfigView ref="configView" />
        </div>
      </div>

      <!-- Step 4: Render -->
      <div class="step-panel" :class="{ 'is-active': step === 'render' }" :inert="step === 'render' ? undefined : true">
        <div class="render-card fluent-glass glass" v-if="chartInfo" v-glass>
          <!-- Left: Illustration -->
          <div class="render-cover">
            <img v-if="renderCover" :src="convertFileSrc(renderCover)" class="cover-img" alt="" />
            <div v-else class="cover-placeholder">
              <v-icon icon="mdi-music-note-outline" size="32" color="rgba(var(--main-color-rgb), 0.4)" />
            </div>
          </div>
          <!-- Right: Info -->
          <div class="render-info">
            <div class="render-header">
              <v-icon icon="mdi-check-circle-outline" size="24" color="primary" />
              <h3>{{ t('render-started') }}</h3>
            </div>
            <div class="info-rows">
              <div class="info-row">
                <span class="info-label">{{ t('chart-name') }}</span
                ><span class="info-value">{{ chartInfo.name }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">{{ t('level') }}</span
                ><span class="info-value">{{ chartInfo.level }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">{{ t('charter') }}</span
                ><span class="info-value">{{ chartInfo.charter }}</span>
              </div>
              <div class="info-row" v-if="chartInfo.composer">
                <span class="info-label">{{ t('composer') }}</span
                ><span class="info-value">{{ chartInfo.composer }}</span>
              </div>
              <div class="info-row" v-if="chartInfo.illustrator">
                <span class="info-label">{{ t('illustrator') }}</span
                ><span class="info-value">{{ chartInfo.illustrator }}</span>
              </div>
              <div class="info-row" v-if="renderDuration !== undefined">
                <span class="info-label">{{ t('duration') }}</span
                ><span class="info-value">{{ formatDuration(renderDuration) }}</span>
              </div>
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

    <Teleport to="body">
      <button v-if="mobile && (step === 'config' || step === 'options')"
        class="bar-btn bar-btn-filled mobile-render-action" @click="moveNext">
        <span>{{ step === 'options' ? t('render') : t('next-step') }}</span>
        <v-icon icon="mdi-arrow-right" size="18" />
      </button>
    </Teleport>

    <!-- File drag overlay -->
    <v-overlay v-model="fileHovering" contained class="align-center justify-center drop-overlay" persistent :close-on-content-click="false">
      <div class="glass drop-zone" v-glass>
        <v-icon size="48" class="mb-2">mdi-download</v-icon>
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
  color: var(--main-color);
}

/* ===== MD3 Top Action Bar ===== */
.md3-top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 56px;
  padding: 0 20px;
  border-bottom-left-radius: 16px;
  border-bottom-right-radius: 16px;
  flex-shrink: 0;
  z-index: 10;
}

.bar-left,
.bar-right {
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
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: rgba(var(--main-color-rgb), 0.25);
  transition: all 0.3s ease;
}

.step-chip.is-active {
  background: rgba(var(--main-color-rgb), 0.12);
}
.step-chip.is-active .chip-dot {
  background: var(--sub-color);
  box-shadow: 0 0 8px rgba(var(--sub-color-rgb), 0.5);
}
.step-chip.is-done .chip-dot {
  background: var(--main-color);
}

.chip-label {
  font-size: 13px;
  font-weight: 500;
  color: rgba(var(--main-color-rgb), 0.8);
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
}

.bar-btn-text {
  background: transparent;
  color: rgba(var(--main-color-rgb), 0.7);
}
.bar-btn-text:hover {
  background: rgba(var(--main-color-rgb), 0.08);
  color: var(--main-color);
}

.bar-btn-tonal {
  background: rgba(var(--main-color-rgb), 0.12);
  color: var(--main-color);
}
.bar-btn-tonal:hover {
  background: rgba(var(--main-color-rgb), 0.2);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
}

.bar-btn-filled {
  background: var(--main-color);
  color: var(--bg-color);
  font-weight: 600;
}
.bar-btn-filled:hover {
  box-shadow: 0 2px 8px rgba(var(--main-color-rgb), 0.3);
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
  transition:
    opacity 0.35s cubic-bezier(0.2, 0, 0, 1),
    transform 0.35s cubic-bezier(0.2, 0, 0, 1);
  overflow-y: auto;
}
.step-panel.is-active {
  opacity: 1;
  transform: translateY(0);
  pointer-events: auto;
}

/* ===== Bookshelf (Step 1) ===== */
.bookshelf {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  padding: 20px;
  border-radius: 16px;
}

.shelf-label {
  padding: 12px 20px;
  font-size: 22px;
  font-weight: 600;
  color: var(--main-color);
  margin-bottom: 32px;
  letter-spacing: 0.5px;
  border-radius: 16px;
}

.shelf-row {
  display: flex;
  flex-direction: column; /* 竖直排列 */
  gap: 16px;
  width: 100%;
}

.book-card {
  min-width: 100%;
  border-radius: 16px;
  cursor: pointer;
  position: relative;
  transition:
    transform 0.3s ease,
    box-shadow 0.3s ease;
  display: flex;
  flex-direction: column;
}

.book-card:hover {
  transform: translateY(-4px);
}

.book-face {
  min-width: 100%;
  display: flex;
  flex-direction: column;
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
  margin: 4px 0 0;
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

.slider {
  width: 100%;
  color: var(--main-color);
}

.field-toggle {
  flex-direction: row !important;
  align-items: center;
  justify-content: space-between;
}

.switch {
  flex: none;
  cursor: pointer;
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
  color: rgba(var(--main-color-rgb), 0.9);
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
  border-bottom: 1px solid rgba(var(--main-color-rgb), 0.05);
}
.info-row:last-child {
  border-bottom: none;
}
.info-label {
  color: rgba(var(--main-color-rgb), 0.5);
  font-size: 13px;
}
.info-value {
  color: rgba(var(--main-color-rgb), 0.85);
  font-size: 13px;
  text-align: right;
}

.render-progress {
  margin-top: 8px;
}
.progress-text {
  text-align: center;
  margin-top: 6px;
  font-size: 12px;
  color: rgba(var(--main-color-rgb), 0.5);
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
  background-color: rgba(0, 0, 0, 0.85);
  z-index: 200;
}
.drop-zone {
  padding: 48px;
  display: flex;
  flex-direction: column;
  align-items: center;
  border-radius: 16px;
  border: 2px dashed rgba(var(--main-color-rgb), 0.4);
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
  background: rgba(var(--bg-color-rgb), 0.95);
  border-radius: 28px;
  padding: 24px 32px;
  color: rgba(var(--main-color-rgb), 0.7);
  font-size: 14px;
}

  .mobile-render-action {
    position: fixed;
    left: 14px;
    right: 14px;
    bottom: calc(84px + env(safe-area-inset-bottom));
    height: 52px;
    justify-content: center;
    border-radius: 16px;
    font-size: 16px;
    font-weight: 700;
    z-index: 1005;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
  }
  .mobile-render-action span { display: inline; }
/* ===== Responsive ===== */
@media (max-width: 768px) {
  .config-grid {
    grid-template-columns: 1fr;
  }
  .shelf-row {
    gap: 16px;
  }
  .md3-top-bar {
    padding: 0 12px;
    height: 48px;
  }
  .bar-btn span {
    display: none;
  }
  .render-container {
    padding-bottom: 84px;
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
