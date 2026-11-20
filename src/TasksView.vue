<i18n>
en:
  empty: Nothing here

  status:
    pending: Pending…
    loading: Loading…
    mixing: Mixing…
    mixing-progress: 'Mixing audio: {progress}%'
    finalizing: Finalizing output…
    rendering: Rendering {fps} FPS, estimated to end {estimate}
    done: 'Done, took {duration}, average frame rate: {fps} fps'
    canceled: Canceled
    failed: Failed

  chip:
    pending: Ready
    loading: Loading
    mixing: Mixing
    finalizing: Finalizing
    rendering: Rendering
    done: Done
    canceled: Canceled
    failed: Failed

  detail:
    name: Name
    path: Path
    charter: Charter
    composer: Composer
    illustrator: Illustrator
    level: Difficulty
    status: Status
    progress: Progress
    fps: Frame rate
    duration: Duration
    average-fps: Average frame rate
    output: Output
    error: Error

  cancel: Cancel
  confirm: Confirm

  details: Details
  error: Error
  output: Output

  show-output: Show Output
  show-in-folder: Open Folder
  save-log: Save Log
  log-saved: Log saved
  remove-record: Remove record
  task-actions: Task actions

  source-local: This device
  source-remote: Server
  source-local-server: This PC's server
  download: Download
  remote-offline: 'Render server unreachable: {server}'
  remote-job: Remote job

zh-CN:
  empty: 空空如也

  status:
    pending: 等待中…
    loading: 加载中…
    mixing: 混音中…
    mixing-progress: '正在混音：{progress}%'
    finalizing: 正在封装或保存…
    rendering: 渲染中 {fps} FPS  预计 {estimate} 结束
    done: '已完成，耗时 {duration}，平均帧率：{fps} fps'
    canceled: 已取消
    failed: 失败

  chip:
    pending: 就绪
    loading: 加载中
    mixing: 混音中
    finalizing: 封装中
    rendering: 渲染中
    done: 已完成
    canceled: 已取消
    failed: 失败

  detail:
    name: 名称
    path: 路径
    charter: 谱师
    composer: 曲师
    illustrator: 画师
    level: 难度
    status: 状态
    progress: 进度
    fps: 帧率
    duration: 耗时
    average-fps: 平均帧率
    output: 输出
    error: 错误

  cancel: 取消
  confirm: 确定

  details: 详情
  error: 错误
  output: 输出

  show-output: 查看输出
  show-in-folder: 打开文件夹
  save-log: 保存日志
  log-saved: 日志已保存
  remove-record: 删除记录
  task-actions: 任务操作

  source-local: 本机
  source-remote: 服务器
  source-local-server: 本机服务
  download: 下载
  remote-offline: '渲染服务器连不上：{server}'
  remote-job: 远程任务

</i18n>

<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import type { Task, TaskStatus } from './model';
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { open as openUrl } from '@tauri-apps/plugin-shell';
import { toast } from './common';
import moment from 'moment';
import { toastError } from './common';
import { isMobile } from './platform';
import RenderProgressBar from './components/RenderProgressBar.vue';
import RenderProgressCircular from './components/RenderProgressCircular.vue';

const { t } = useI18n();
const mobile = isMobile();

const tasks = ref<Task[]>();

/// A job as returned by `GET /api/jobs` on a remote `phi-tkc server`.
interface RemoteJob {
  id: string;
  name: string;
  charter?: string;
  composer?: string;
  illustrator?: string;
  level?: string;
  status: string;
  progress?: number;
  fps?: number;
  eta?: number;
  error?: string | null;
  download?: string | null;
  createdAt?: number;
  duration?: number;
  averageFps?: number;
}

interface ServerSource {
  base: string;
  kind: 'local-server' | 'remote';
  serverId?: string;
  jobs: RemoteJob[];
}
interface ManagedServerStatus {
  running: boolean;
  host: string;
  port: number;
  serverId?: string | null;
}
const serverSources = ref<ServerSource[]>([]);
const serverErrors = ref<{ base: string; error: string }[]>([]);
const canceling = ref(new Set<string>());
let stopped = false;
let sourceGeneration = 0;
let localListGeneration = 0;
const identityCache = new Map<string, string | null>();

const ACTIVE_STATUSES = ['pending', 'loading', 'mixing', 'rendering', 'finalizing'];
const POLL_ACTIVE_MS = 300;
const POLL_IDLE_MS = 2000;

async function updateList() {
  const generation = ++localListGeneration;
  try {
    const list = await invoke<Task[]>('get_tasks');
    if (!stopped && generation === localListGeneration) tasks.value = list;
  } catch (e) {
    console.error('Failed to refresh tasks:', e);
    tasks.value ??= [];
  }
}

// The connected server's jobs are merged into the same list. `localStorage` is
// re-read on every tick so connecting/disconnecting in Settings is picked up
// without a restart (the same window does not get a `storage` event).
// `localStorage` stores what the user typed ("192.168.1.10:6787"); reqwest
// needs an absolute URL, so the scheme is added here like Settings does.
function normalizeBase(raw: string): string {
  let value = raw.trim().replace(/\/+$/, '');
  if (!value) return '';
  if (!/^https?:\/\//i.test(value)) value = `http://${value}`;
  try { return new URL(value).toString().replace(/\/+$/, ''); } catch { return value; }
}

function configuredRemote(): string {
  return localStorage.getItem('remoteConnected') === '1'
    ? normalizeBase(localStorage.getItem('remoteRenderer') || '') : '';
}

function managedBase(server: ManagedServerStatus): string {
  let host = server.host.replace(/^\[|\]$/g, '');
  if (host === '0.0.0.0' || host === '*') host = '127.0.0.1';
  if (host === '::') host = '::1';
  return normalizeBase(`http://${host.includes(':') ? `[${host}]` : host}:${server.port}`);
}

async function updateServers() {
  const generation = ++sourceGeneration;
  const remote = configuredRemote();
  const sources: ServerSource[] = [];
  const errors: { base: string; error: string }[] = [];
  try {
    const local = await invoke<ManagedServerStatus | null>('render_server_status');
    if (local?.running) sources.push({
      base: managedBase(local), kind: 'local-server', serverId: local.serverId || undefined, jobs: [],
    });
  } catch (e) {
    errors.push({ base: t('source-local-server'), error: String(e) });
  }
  const local = sources[0];
  if (remote && remote !== local?.base) {
    let serverId: string | null = null;
    // Resolve identity when an endpoint or local server instance changes. LAN
    // and loopback addresses can point to the same server; job IDs alone are
    // not unique across distinct servers.
    if (local?.serverId) {
      const cacheKey = `${local.serverId} ${remote}`;
      if (!identityCache.has(cacheKey)) {
        try {
          const status = await invoke<{ serverId?: string }>('remote_status', { base: remote });
          identityCache.set(cacheKey, status?.serverId || null);
        } catch { /* older/offline servers can still be queried for jobs below */ }
      }
      serverId = identityCache.get(cacheKey) || null;
    }
    if (!serverId || serverId !== local?.serverId) {
      sources.push({ base: remote, kind: 'remote', serverId: serverId || undefined, jobs: [] });
    }
  }
  await Promise.all(sources.map(async (source) => {
    try {
      const list = await invoke<RemoteJob[]>('remote_jobs', { base: source.base });
      source.jobs = Array.isArray(list) ? list : [];
    } catch (e) {
      errors.push({ base: source.base, error: String((e as Error)?.message || e) });
    }
  }));
  if (stopped || generation !== sourceGeneration || remote !== configuredRemote()) return;
  serverSources.value = sources;
  serverErrors.value = errors;
}

await Promise.all([updateList(), updateServers()]);

// Adaptive polling: a fixed interval can stack calls when IPC is slower than the tick,
// so chain the next tick only after the previous one settles.
let pollTimer: ReturnType<typeof setTimeout> | undefined;

function remoteBusy() {
  return serverSources.value.some((source) => source.jobs.some((job) =>
    ['queued', 'mixing', 'running'].includes(job.status)));
}

function scheduleNextPoll() {
  if (stopped) return;
  const busy =
    (tasks.value?.some((task) => ACTIVE_STATUSES.includes(task.status.type)) ?? false) || remoteBusy();
  pollTimer = setTimeout(poll, busy ? POLL_ACTIVE_MS : POLL_IDLE_MS);
}

async function poll() {
  try {
    await Promise.all([updateList(), updateServers()]);
  } catch (e) {
    console.error('Failed to refresh tasks:', e);
  }
  scheduleNextPoll();
}

scheduleNextPoll();

onUnmounted(() => {
  stopped = true;
  sourceGeneration++;
  localListGeneration++;
  if (pollTimer !== undefined) clearTimeout(pollTimer);
});

function formatDuration(seconds: number) {
  const duration = moment.duration(Math.ceil(seconds), 'seconds');
  const hours = Math.floor(duration.asHours());
  const minutes = duration.minutes();
  const secs = duration.seconds();
  if (hours > 0) return `${hours}h ${minutes}m ${secs}s`;
  if (minutes > 0) return `${minutes}m ${secs}s`;
  if (secs > 0) return `${secs}s`;
  return '';
}

function remoteStatus(job: RemoteJob): TaskStatus {
  switch (job.status) {
    case 'queued': return { type: 'pending' };
    case 'mixing': return { type: 'mixing' };
    case 'running':
      return {
        type: 'rendering',
        progress: job.progress ?? 0,
        fps: Math.round(job.fps ?? 0),
        estimate: job.eta ?? 0,
      };
    case 'done': return { type: 'done', duration: job.duration ?? Number.NaN, average_fps: job.averageFps, output: job.download ?? '' };
    case 'canceled': return { type: 'canceled' };
    default: return { type: 'failed', error: job.error || 'unknown error' };
  }
}

interface Row {
  key: string;
  source: 'local' | 'local-server' | 'remote';
  name: string;
  charter: string;
  composer: string;
  illustrator: string;
  level: string;
  path: string;
  cover: string;
  status: TaskStatus;
  task?: Task;
  job?: RemoteJob;
  jobBase?: string;
}

// Union of this device's queue and the connected server's jobs. Local tasks
// keep their order; remote ones are newest-first, exactly as the server lists
// them.
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  for (const task of tasks.value ?? []) {
    out.push({
      key: `local-${task.id}`,
      source: 'local',
      name: task.name,
      charter: task.charter || '—',
      composer: task.composer || '—',
      illustrator: task.illustrator || '—',
      level: task.level || '—',
      path: task.path,
      cover: task.cover,
      status: task.status,
      task,
    });
  }
  for (const server of serverSources.value) for (const job of server.jobs) {
    out.push({
      key: JSON.stringify([server.serverId || server.base, job.id]),
      source: server.kind,
      name: job.name || t('remote-job'),
      charter: job.charter || '—',
      composer: job.composer || '—',
      illustrator: job.illustrator || '—',
      level: job.level || '—',
      path: `${server.base} · ${job.id}`,
      cover: '',
      status: remoteStatus(job),
      job,
      jobBase: server.base,
    });
  }
  return out;
});

function describeStatus(status: TaskStatus): string {
  switch (status.type) {
    case 'pending': return t('status.pending');
    case 'loading': return t('status.loading');
    case 'mixing': return status.progress === undefined
      ? t('status.mixing')
      : t('status.mixing-progress', { progress: Math.round(status.progress * 100) });
    case 'finalizing': return t('status.finalizing');
    case 'rendering':
      return t('status.rendering', {
        fps: status.fps,
        estimate: status.estimate ? formatDuration(status.estimate) : '',
      });
    case 'done': return t('status.done', {
      duration: `${formatMetric(status.duration)} s`,
      fps: formatMetric(status.average_fps),
    });
    case 'canceled': return t('status.canceled');
    case 'failed': return t('status.failed');
  }
}

function formatMetric(value: number | undefined): string {
  return value !== undefined && Number.isFinite(value) && value >= 0 ? value.toFixed(2) : '—';
}

function statusColor(statusType: string): string {
  const colors: Record<string, string> = {
    pending: 'info', loading: 'info', mixing: 'info', finalizing: 'info',
    rendering: 'primary', done: 'success', canceled: 'warning', failed: 'error',
  };
  return colors[statusType] || 'info';
}

const errorDialog = ref(false), errorDialogMessage = ref('');
const outputDialog = ref(false), outputDialogMessage = ref('');
const detailDialog = ref(false), selectedRow = ref<Row | null>(null);
const contextMenu = ref(false);
const contextMenuX = ref(0);
const contextMenuY = ref(0);
const contextMenuRow = ref<Row | null>(null);
const SWIPE_DELETE_THRESHOLD = 0.7;
const swipe = ref<{ key: string; offset: number; dragging: boolean } | null>(null);
const removingRows = new Set<string>();
let touchStart: { key: string; id: number; x: number; y: number; width: number; axis: 'pending' | 'horizontal' | 'vertical' } | null = null;
let suppressSwipeClick: { key: string; x: number; y: number; until: number } | null = null;

function canRemoveRow(row: Row): row is Row & { task: Task } {
  return row.source === 'local' && row.status.type === 'done' && !!row.task;
}

async function showInFolder(path: string) {
  try { await invoke('show_in_folder', { path }); } catch (e) { toastError(e); }
}

async function showFolder() {
  try { await invoke('show_folder'); } catch (e) { toastError(e); }
}

async function cancelRow(row: Row) {
  if (canceling.value.has(row.key)) return;
  canceling.value.add(row.key);
  try {
    if (row.source === 'local' && row.task) {
      await invoke('cancel_task', { id: row.task.id });
    } else if (row.job && row.jobBase) {
      await invoke('remote_cancel', { base: row.jobBase, id: row.job.id });
    }
    await Promise.all([updateList(), updateServers()]);
  } catch (e) {
    toastError(e);
  } finally {
    canceling.value.delete(row.key);
  }
}

async function removeRow(row: Row) {
  if (!canRemoveRow(row) || removingRows.has(row.key)) return;
  removingRows.add(row.key);
  try {
    await invoke('remove_task', { id: row.task.id });
    closeContextMenu();
    await updateList();
  } catch (e) {
    toastError(e);
  } finally {
    removingRows.delete(row.key);
  }
}

function beginSwipe(event: TouchEvent, row: Row) {
  if (event.touches.length !== 1) return;
  cancelSwipe();
  suppressSwipeClick = null;
  if (!mobile || !canRemoveRow(row) || removingRows.has(row.key)) return;
  const touch = event.changedTouches[0];
  const shell = event.currentTarget as HTMLElement | null;
  if (!touch || !shell) return;
  touchStart = {
    key: row.key,
    id: touch.identifier,
    x: touch.clientX,
    y: touch.clientY,
    width: shell.getBoundingClientRect().width,
    axis: 'pending',
  };
}

function findTouch(touches: TouchList, id: number) {
  return Array.from(touches).find((touch) => touch.identifier === id);
}

function moveSwipe(event: TouchEvent, row: Row) {
  const start = touchStart;
  if (!start || start.key !== row.key) return;
  if (event.touches.length !== 1) {
    cancelSwipe();
    return;
  }
  const touch = findTouch(event.changedTouches, start.id);
  if (!touch) return;
  const dx = touch.clientX - start.x;
  const dy = touch.clientY - start.y;
  if (start.axis === 'pending') {
    if (Math.abs(dx) > 10 && Math.abs(dx) > Math.abs(dy) * 1.25) start.axis = 'horizontal';
    else if (Math.abs(dy) > 10 && Math.abs(dy) >= Math.abs(dx)) start.axis = 'vertical';
  }
  if (start.axis !== 'horizontal') return;
  if (event.cancelable) event.preventDefault();
  swipe.value = {
    key: row.key,
    offset: Math.max(-start.width, Math.min(start.width, dx)),
    dragging: true,
  };
}

function finishSwipe(event: TouchEvent, row: Row) {
  const start = touchStart;
  if (!start || start.key !== row.key) return;
  const touch = findTouch(event.changedTouches, start.id);
  cancelSwipe();
  if (!touch || event.touches.length !== 0 || event.changedTouches.length !== 1 || start.axis !== 'horizontal') return;
  if (event.cancelable) event.preventDefault();
  suppressSwipeClick = { key: row.key, x: touch.clientX, y: touch.clientY, until: Date.now() + 500 };
  if (start.x - touch.clientX > start.width * SWIPE_DELETE_THRESHOLD) void removeRow(row);
}

function cancelSwipe() {
  touchStart = null;
  if (swipe.value?.dragging) swipe.value = { ...swipe.value, offset: 0, dragging: false };
}

// The additional finger can land outside the active row and be lifted before
// that row receives another move/end. Invalidate the gesture at document capture.
function cancelSwipeOnMultitouch(event: TouchEvent) {
  if (event.touches.length > 1) cancelSwipe();
}

function suppressClickAfterSwipe(event: MouseEvent, row: Row) {
  const swipe = suppressSwipeClick;
  if (!swipe || swipe.key !== row.key || Date.now() >= swipe.until || event.detail === 0) return;
  if (Math.hypot(event.clientX - swipe.x, event.clientY - swipe.y) > 48) return;
  suppressSwipeClick = null;
  event.preventDefault();
  event.stopPropagation();
}

async function downloadRemote(row: Row) {
  if (!row.job?.download || !row.jobBase) return;
  try {
    await openUrl(`${row.jobBase}${row.job.download}`);
  } catch (e) {
    toastError(e);
  }
}

async function saveLog(text: string, name: string) {
  try {
    const path = await save({
      defaultPath: name,
      filters: [{ name: 'Log', extensions: ['log', 'txt'] }],
    });
    if (!path) return;
    await invoke('save_text_file', { path, content: text });
    toast(t('log-saved'), 'success');
  } catch (e) {
    toastError(e);
  }
}

function showOutput(row: Row) {
  if (row.status.type !== 'done') return;
  if (row.source !== 'local' && row.job) {
    void downloadRemote(row);
    return;
  }
  outputDialogMessage.value = row.status.output;
  outputDialog.value = true;
}

const showContextMenu = (event: MouseEvent, row: Row) => {
  event.preventDefault();
  contextMenu.value = true;
  contextMenuX.value = event.clientX;
  contextMenuY.value = event.clientY;
  contextMenuRow.value = row;
};

// Touch-friendly replacement for the right-click menu: anchor it to the button.
const showContextMenuFromButton = (event: MouseEvent, row: Row) => {
  const target = event.currentTarget as HTMLElement;
  const rect = target.getBoundingClientRect();
  contextMenuRow.value = row;
  contextMenuX.value = rect.left;
  contextMenuY.value = rect.bottom;
  contextMenu.value = true;
};

const closeContextMenu = () => {
  contextMenu.value = false;
  contextMenuRow.value = null;
  cancelSwipe();
};

const showDetail = (row: Row) => {
  selectedRow.value = row;
  detailDialog.value = true;
  closeContextMenu();
};

onMounted(() => {
  document.addEventListener('click', closeContextMenu);
  if (mobile) document.addEventListener('touchstart', cancelSwipeOnMultitouch, { capture: true, passive: true });
});
onUnmounted(() => {
  document.removeEventListener('click', closeContextMenu);
  if (mobile) document.removeEventListener('touchstart', cancelSwipeOnMultitouch, true);
});
</script>

<template>
  <div class="tasks-container">
    <!-- Header -->
    <div class="tasks-header">
      <h2 class="tasks-title">{{ t('output') }}</h2>
      <v-btn v-if="!mobile" color="primary" variant="tonal" @click="showFolder()">
        <v-icon icon="mdi-folder-open-outline" size="18" class="mr-1" />
        <span>{{ t('show-in-folder') }}</span>
      </v-btn>
    </div>

    <!-- Task list -->
    <div class="task-list">
      <div v-for="error in serverErrors" :key="error.base" class="remote-offline glass" :title="error.error">
        <v-icon icon="mdi-cloud-off-outline" size="18" class="mr-2" />
        <span>{{ t('remote-offline', { server: error.base }) }}</span>
      </div>

      <div
        v-for="row in rows"
        :key="row.key"
        class="task-row-shell"
        :class="{ 'is-swipeable': mobile && canRemoveRow(row), 'is-swipe-dragging': swipe?.key === row.key && swipe.dragging }"
         :style="swipe?.key === row.key ? { '--swipe-offset': `${swipe.offset}px`, '--delete-width': `${Math.max(0, -swipe.offset - 8)}px` } : undefined"
        @touchstart.passive="beginSwipe($event, row)"
        @touchmove="moveSwipe($event, row)"
        @touchend="finishSwipe($event, row)"
        @touchcancel="cancelSwipe"
        @click.capture="suppressClickAfterSwipe($event, row)"
      >
        <div v-if="mobile && canRemoveRow(row)" class="swipe-delete clear-glass" aria-hidden="true">
          <span>删除-Delete</span>
        </div>
        <div
          class="task-card glass fluent-glass"
          v-glass
          @contextmenu="showContextMenu($event, row)"
                  >
        <!-- Cover (left 35%) -->
        <div class="task-cover">
          <div
            class="cover-image"
            :style="row.cover ? { backgroundImage: 'url(' + convertFileSrc(row.cover) + ')' } : {}"
          ></div>
          <div v-if="!row.cover" class="cover-placeholder">
            <v-icon icon="mdi-cloud-render-outline" size="40" />
          </div>
        </div>

        <!-- Info (right 65%) -->
        <div class="task-info">
          <div class="task-header" :class="{ 'has-source': row.source !== 'local' }">
            <h3 class="task-name" :title="row.name">{{ row.name }}</h3>
            <span v-if="row.source !== 'local'" class="source-chip">
              <v-icon icon="mdi-cloud-outline" size="12" class="mr-1" />{{ t(row.source === 'local-server' ? 'source-local-server' : 'source-remote') }}
            </span>
            <span class="status-chip" :class="'chip-' + statusColor(row.status.type)">
              {{ t('chip.' + row.status.type) }}
            </span>
            <v-btn
              v-if="row.status.type === 'done'"
              icon="mdi-dots-vertical"
              variant="text"
              size="x-small"
              class="task-menu-btn"
              :title="t('task-actions')"
              :aria-label="t('task-actions')"
              @click.stop="showContextMenuFromButton($event, row)"
            />
          </div>

          <div class="task-metadata">
            <span v-for="field in (['charter', 'composer', 'illustrator', 'level'] as const)" :key="field" :title="row[field]">
              {{ t('detail.' + field) }}：{{ row[field] }}
            </span>
          </div>

          <div class="task-status">
            <RenderProgressCircular
              v-if="['loading', 'mixing', 'pending', 'rendering', 'finalizing', 'done'].includes(row.status.type)"
              :complete="row.status.type === 'done'"
              color="primary" :size="20" :width="2" :aria-label="describeStatus(row.status)"
            />
            <span class="status-text">{{ describeStatus(row.status) }}</span>
          </div>

          <RenderProgressBar
            v-if="row.status.type === 'rendering'"
            :model-value="row.status.progress * 100"
            :color="statusColor(row.status.type)" :height="3" :aria-label="t('detail.progress')"
          />

          <div class="task-actions">
            <template v-if="['loading', 'mixing', 'rendering', 'pending', 'finalizing'].includes(row.status.type)">
              <v-btn variant="text" size="small" color="primary" :disabled="canceling.has(row.key)" :loading="canceling.has(row.key)" @click="cancelRow(row)">
                <v-icon icon="mdi-cancel" size="16" class="mr-1" />
                <span>{{ t('cancel') }}</span>
              </v-btn>
            </template>
            <template v-else-if="row.status.type === 'failed'">
              <v-btn variant="text" size="small" color="primary" @click="errorDialogMessage = row.status.error; errorDialog = true;">
                <v-icon icon="mdi-alert-circle-outline" size="16" class="mr-1" />
                <span>{{ t('details') }}</span>
              </v-btn>
            </template>
            <template v-else-if="row.status.type === 'done'">
              <v-btn variant="text" size="small" color="primary" @click="showOutput(row)">
                <v-icon :icon="row.source !== 'local' ? 'mdi-download' : 'mdi-text-box-outline'" size="16" class="mr-1" />
                <span>{{ row.source !== 'local' ? t('download') : t('show-output') }}</span>
              </v-btn>
              <!-- Desktop shell integration only (explorer / gdbus / open). -->
              <v-btn
                v-if="!mobile && row.source === 'local' && row.task"
                variant="text" size="small" color="primary"
                @click="showInFolder(row.task.output)"
              >
                <v-icon icon="mdi-folder-open-outline" size="16" class="mr-1" />
                <span>{{ t('show-in-folder') }}</span>
              </v-btn>
            </template>
          </div>
        </div>
        </div>
      </div>

      <!-- Empty state -->
      <div v-if="!rows.length" class="empty-state">
        <v-icon icon="mdi-inbox-outline" size="64" />
        <p>{{ t('empty') }}</p>
      </div>
    </div>

    <!-- Error dialog -->
    <v-dialog v-model="errorDialog" width="auto" min-width="400px" persistent>
      <v-card class="error-dialog">
        <v-card-title class="d-flex align-center">
          <v-icon color="error" class="mr-2">mdi-alert</v-icon>
          <span>{{ t('error') }}</span>
        </v-card-title>
        <v-divider />
        <v-card-text>
          <pre class="error-message copyable">{{ errorDialogMessage }}</pre>
        </v-card-text>
        <v-card-actions class="justify-end">
          <v-btn variant="text" prepend-icon="mdi-content-save" @click="saveLog(errorDialogMessage, 'phi-tkc-error.log')">
            {{ t('save-log') }}
          </v-btn>
          <v-btn variant="text" color="primary" @click="errorDialog = false">{{ t('confirm') }}</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- Output dialog -->
    <v-dialog v-model="outputDialog" width="auto" min-width="400px">
      <v-card>
        <v-card-title>{{ t('output') }}</v-card-title>
        <v-card-text>
          <pre class="output-pre copyable">{{ outputDialogMessage }}</pre>
        </v-card-text>
        <v-card-actions class="justify-end">
          <v-btn variant="text" prepend-icon="mdi-content-save" @click="saveLog(outputDialogMessage, 'phi-tkc-output.log')">
            {{ t('save-log') }}
          </v-btn>
          <v-btn variant="tonal" color="primary" @click="outputDialog = false">{{ t('confirm') }}</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- Context menu -->
    <v-menu v-model="contextMenu" :target="[contextMenuX, contextMenuY]">
      <v-list class="ctx-menu">
        <v-list-item @click="showDetail(contextMenuRow!)">
          <template v-slot:prepend>
            <v-icon>mdi-information</v-icon>
          </template>
          <v-list-item-title>{{ t('details') }}</v-list-item-title>
        </v-list-item>
        <v-list-item
          v-if="contextMenuRow?.source === 'local' && contextMenuRow.status.type === 'done'"
          class="text-error"
          @click="removeRow(contextMenuRow!)"
        >
          <template v-slot:prepend><v-icon>mdi-delete-outline</v-icon></template>
          <v-list-item-title>{{ t('remove-record') }}</v-list-item-title>
        </v-list-item>
      </v-list>
    </v-menu>

    <!-- Detail dialog -->
    <v-dialog v-model="detailDialog" width="min(560px, calc(100vw - 32px))">
      <v-card v-if="selectedRow">
        <v-card-title>{{ t('details') }}</v-card-title>
        <v-divider />
        <v-card-text>
          <div class="detail-list">
            <div class="detail-row"><span class="detail-label">{{ t('detail.name') }}</span><span>{{ selectedRow.name }}</span></div>
            <div v-for="field in (['charter', 'composer', 'illustrator', 'level'] as const)" :key="field" class="detail-row"><span class="detail-label">{{ t('detail.' + field) }}</span><span class="break-all">{{ selectedRow[field] }}</span></div>
            <div v-if="!mobile" class="detail-row"><span class="detail-label">{{ t('detail.path') }}</span><span class="break-all">{{ selectedRow.path }}</span></div>
            <div class="detail-row"><span class="detail-label">{{ t('detail.status') }}</span><span>{{ describeStatus(selectedRow.status) }}</span></div>
            <div class="detail-row" v-if="selectedRow.status.type === 'rendering'">
              <span class="detail-label">{{ t('detail.progress') }}</span><span>{{ Math.round(selectedRow.status.progress * 100) }}%</span>
            </div>
            <div class="detail-row" v-if="selectedRow.status.type === 'rendering'">
              <span class="detail-label">{{ t('detail.fps') }}</span><span>{{ selectedRow.status.fps }} FPS</span>
            </div>
            <div class="detail-row" v-if="selectedRow.status.type === 'done' && selectedRow.status.duration">
              <span class="detail-label">{{ t('detail.duration') }}</span><span>{{ formatMetric(selectedRow.status.duration) }} s</span>
            </div>
            <div class="detail-row" v-if="selectedRow.status.type === 'done'">
              <span class="detail-label">{{ t('detail.average-fps') }}</span><span>{{ formatMetric(selectedRow.status.average_fps) }} fps</span>
            </div>
            <div class="detail-row" v-if="selectedRow.status.type === 'done' && selectedRow.status.output">
              <span class="detail-label">{{ t('detail.output') }}</span><span class="break-all">{{ selectedRow.status.type === 'done' ? selectedRow.status.output : '' }}</span>
            </div>
            <div class="detail-row" v-if="selectedRow.status.type === 'failed' && selectedRow.status.error">
              <span class="detail-label">{{ t('detail.error') }}</span><span class="error-text">{{ selectedRow.status.error }}</span>
            </div>
          </div>
        </v-card-text>
        <v-card-actions class="justify-end">
          <v-btn variant="tonal" color="primary" @click="detailDialog = false">{{ t('confirm') }}</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
</template>

<style scoped>
.tasks-container {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  width: 100%;
  max-width: 960px;
  margin: 0 auto;
  padding: 24px;
  box-sizing: border-box;
}

/* ===== Header ===== */
.tasks-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
  flex-shrink: 0;
}
.tasks-title {
  font-size: 24px;
  font-weight: 700;
  color: var(--main-color);
  margin: 0;
}

/* ===== Empty State ===== */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 16px;
  color: rgba(var(--main-color-rgb), 0.4);
  font-size: 16px;
}

/* ===== Task List ===== */
.task-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  touch-action: pan-y;
  overscroll-behavior: none;
}

.task-row-shell {
  --task-metadata-size: 12px;
  position: relative;
  flex: 0 0 auto;
  min-width: 0;
}

.swipe-delete {
  display: none;
}

/* ===== Task Card ===== */
.task-card {
  display: flex;
  flex-direction: row;
  overflow: hidden;
  position: relative;
  transition: transform 0.24s cubic-bezier(0.65, 0, 0.35, 1), box-shadow 0.2s cubic-bezier(0.65, 0, 0.35, 1);
  height: 180px;
  /* The list is a scrolling flex column: keep cards at their intended height
     instead of shrinking them all to fit one screen. */
  flex-shrink: 0;
}



.task-cover {
  width: 35%;
  flex-shrink: 0;
  background: rgba(var(--bg-color-rgb), 0.2);
}
.cover-image {
  width: 100%;
  height: 100%;
  background-position: center;
  background-repeat: no-repeat;
  background-size: cover;
  min-height: 120px;
}

.task-info {
  width: 65%;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.task-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.task-name {
  font-size: 16px;
  font-weight: 600;
  color: var(--main-color);
  margin: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}
.task-metadata {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 4px 12px;
  font-size: var(--task-metadata-size);
  color: rgba(var(--main-color-rgb), 0.45);
}
.task-metadata span {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ===== Status ===== */
.task-status {
  display: flex;
  align-items: center;
  gap: 8px;
}
.status-text {
  font-size: 13px;
  color: rgba(var(--main-color-rgb), 0.7);
  min-width: 0;
  overflow-wrap: anywhere;
}

/* ===== Status Chip ===== */
.status-chip {
  display: inline-block;
  padding: 2px 10px;
  border-radius: 12px;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.chip-info { background: rgba(var(--main-color-rgb), 0.15); color: var(--main-color); }
.chip-primary { background: rgba(var(--main-color-rgb), 0.2); color: var(--main-color); }
.chip-warning { background: rgba(var(--sub-color-rgb), 0.15); color: var(--sub-color); }
.chip-error { background: rgba(var(--sub-color-rgb), 0.15); color: var(--sub-color); }

/* Done Badge (version-badge style) */
.chip-success {
  background: transparent;
  border: 1px solid var(--main-color);
  color: var(--main-color);
  padding: 4px 12px;
  border-radius: 12px;
}

/* Marks jobs that live on the connected render server. */
.source-chip {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.4px;
  color: rgba(var(--main-color-rgb), 0.8);
  border: 1px solid rgba(var(--main-color-rgb), 0.25);
  white-space: nowrap;
}

.task-cover {
  position: relative;
}
.cover-placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: rgba(var(--main-color-rgb), 0.35);
}

.remote-offline {
  display: flex;
  align-items: center;
  padding: 10px 14px;
  border-radius: 12px;
  font-size: 13px;
  color: var(--sub-color);
  background: rgba(var(--bg-color-rgb), 0.35);
}

/* ===== Actions ===== */
.task-actions {
  display: flex;
  gap: 8px;
  margin-top: auto;
}

/* ===== Error dialog ===== */
.error-dialog {
  border-color: rgba(var(--sub-color-rgb), 0.3) !important;
}

.error-message {
  color: var(--sub-color);
  font-family: monospace;
  background: rgba(var(--bg-color-rgb), 0.4);
  padding: 12px;
  border-radius: 12px;
  max-height: 60vh;
  overflow: auto;
  line-height: 1.5;
  white-space: pre-wrap;
}

/* ===== Output dialog ===== */
.output-pre {
  background: rgba(var(--bg-color-rgb), 0.3);
  padding: 12px;
  border-radius: 12px;
  font-family: monospace;
  font-size: 13px;
  max-height: 60vh;
  overflow: auto;
  white-space: pre-wrap;
}

/* ===== Detail list ===== */
.detail-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.detail-row {
  display: flex;
  padding: 8px 0;
  border-bottom: 1px solid rgba(var(--main-color-rgb), 0.08);
  font-size: 14px;
  color: var(--main-color);
}

.detail-row:last-child { border-bottom: none; }

.detail-label {
  width: 72px;
  flex-shrink: 0;
  color: rgba(var(--main-color-rgb), 0.5);
  font-weight: 500;
}

.break-all { word-break: break-all; }

.error-text {
  color: var(--sub-color);
  font-family: monospace;
  font-size: 13px;
  background: rgba(var(--sub-color-rgb), 0.1);
  padding: 6px 10px;
  border-radius: 8px;
}

.task-menu-btn {
  margin-left: auto;
}

/* ===== Responsive ===== */
.task-row-shell.is-swipeable { overflow: hidden; border-radius: 16px; }
@media (max-width: 600px) {
  .tasks-container {
    padding: 12px 12px 16px;
  }
  /* Keep the cover as a small thumbnail instead of stacking it on top: the
     information (name/status/actions) is what people come here for, and a
     phone screen only fits a couple of cards otherwise. */
  .task-card { flex-direction: row; height: auto; min-height: 124px; }
  .task-cover { width: 92px; min-height: 92px; align-self: stretch; }
  .cover-image { min-height: 92px; }
  .task-info { width: auto; flex: 1; padding: 10px 12px; gap: 4px; }
  .task-name { font-size: 15px; }
  /* Keep a visible title and the menu on the first line. Server/status badges
     share the next line instead of pushing the menu beyond a narrow card. */
  .task-header.has-source { flex-wrap: wrap; gap: 4px 8px; }
  .task-header.has-source .task-name { flex-basis: calc(100% - 52px); }
  .task-header.has-source :is(.source-chip, .status-chip) { order: 1; }
  .task-path { font-size: 11px; }
  .task-actions { gap: 2px; }
  .tasks-title { font-size: 20px; }
}

/* ===== Mobile (touch) ===== */
@media (pointer: coarse) {
  .task-menu-btn {
    min-width: 44px;
    min-height: 44px;
  }
}

.task-row-shell.is-swipeable { touch-action: pan-y; }
.is-swipeable .task-card {
  transform: translateX(var(--swipe-offset, 0px));
  border-radius: 16px;
  transition: transform 0.24s cubic-bezier(0.65, 0, 0.35, 1),
    box-shadow 0.2s ease-in-out, backdrop-filter 0.2s ease-in-out !important;
}
.is-swipeable .swipe-delete {
  position: absolute;
  inset: 0 0 0 auto;
  display: grid;
  place-items: center;
  width: var(--delete-width, 0px);
  border-radius: 16px;
  overflow: hidden;
  pointer-events: none;
  transition: width 0.24s cubic-bezier(0.65, 0, 0.35, 1);
  border: 0;
  box-shadow: inset 0 0 0 1px rgba(var(--bg-color-rgb), 0.2);
}
.swipe-delete span {
  color: rgb(255 255 255 / 80%);
  font-size: var(--task-metadata-size);
  font-weight: normal;
  white-space: nowrap;
}
.is-swipeable.is-swipe-dragging :is(.task-card, .swipe-delete) { transition: none !important; }
@media (prefers-reduced-motion: reduce) {
  .is-swipeable :is(.task-card, .swipe-delete) { transition: none !important; }
}
</style>
