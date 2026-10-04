<i18n>
en:
  empty: Nothing here

  status:
    pending: Pending…
    loading: Loading…
    mixing: Mixing…
    rendering: Rendering { fps } FPS, estimated to end { estimate }
    done: Done, took { duration }
    canceled: Canceled
    failed: Failed

  cancel: Cancel
  confirm: Confirm

  details: Details
  error: Error
  output: Output

  show-output: Show Output
  show-in-folder: Open Folder

zh-CN:
  empty: 空空如也

  status:
    pending: 等待中…
    loading: 加载中…
    mixing: 混音中…
    rendering: 渲染中 { fps } FPS  预计 { estimate } 结束
    done: 已完成，耗时 { duration }
    canceled: 已取消
    failed: 失败

  cancel: 取消
  confirm: 确定

  details: 详情
  error: 错误
  output: 输出

  show-output: 查看输出
  show-in-folder: 打开文件夹

</i18n>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import type { Task, TaskStatus } from './model';
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import moment from 'moment';
import { toastError } from './common';
import { isMobile } from './platform';

const { t } = useI18n();
const mobile = isMobile();

const tasks = ref<Task[]>();

const ACTIVE_STATUSES = ['pending', 'loading', 'mixing', 'rendering'];
const POLL_ACTIVE_MS = 300;
const POLL_IDLE_MS = 2000;

async function updateList() {
  tasks.value = await invoke<Task[]>('get_tasks');
}

await updateList();

// Adaptive polling: a fixed interval can stack calls when IPC is slower than the tick,
// so chain the next tick only after the previous one settles.
let stopped = false;
let pollTimer: ReturnType<typeof setTimeout> | undefined;

function scheduleNextPoll() {
  if (stopped) return;
  const busy = tasks.value?.some((task) => ACTIVE_STATUSES.includes(task.status.type)) ?? false;
  pollTimer = setTimeout(poll, busy ? POLL_ACTIVE_MS : POLL_IDLE_MS);
}

async function poll() {
  try {
    await updateList();
  } catch (e) {
    console.error('Failed to refresh tasks:', e);
  }
  scheduleNextPoll();
}

scheduleNextPoll();

onUnmounted(() => {
  stopped = true;
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

function describeStatus(status: TaskStatus): string {
  switch (status.type) {
    case 'pending': return t('status.pending');
    case 'loading': return t('status.loading');
    case 'mixing': return t('status.mixing');
    case 'rendering': {
      const progressDisplay = status.progress;
      return t('status.rendering', {
        progress: progressDisplay,
        fps: status.fps,
        estimate: status.estimate ? formatDuration(status.estimate) : '',
      });
    }
    case 'done': return t('status.done', { duration: status.duration ? formatDuration(status.duration) : '' });
    case 'canceled': return t('status.canceled');
    case 'failed': return t('status.failed');
  }
}

function statusColor(statusType: string): string {
  const colors: Record<string, string> = {
    pending: 'info', loading: 'info', mixing: 'info',
    rendering: 'primary', done: 'success', canceled: 'warning', failed: 'error',
  };
  return colors[statusType] || 'info';
}

const errorDialog = ref(false), errorDialogMessage = ref('');
const outputDialog = ref(false), outputDialogMessage = ref('');
const detailDialog = ref(false), selectedTask = ref<Task | null>(null);
const contextMenu = ref(false);
const contextMenuX = ref(0);
const contextMenuY = ref(0);
const contextMenuTask = ref<Task | null>(null);

async function showInFolder(path: string) {
  try { await invoke('show_in_folder', { path }); } catch (e) { toastError(e); }
}

async function showFolder() {
  try { await invoke('show_folder'); } catch (e) { toastError(e); }
}

function showOutput(task: Task) {
  if (task.status.type === 'done') {
    outputDialogMessage.value = task.status.output;
    outputDialog.value = true;
  }
}

const showContextMenu = (event: MouseEvent, task: Task) => {
  event.preventDefault();
  contextMenu.value = true;
  contextMenuX.value = event.clientX;
  contextMenuY.value = event.clientY;
  contextMenuTask.value = task;
};

// Touch-friendly replacement for the right-click menu: anchor it to the button.
const showContextMenuFromButton = (event: MouseEvent, task: Task) => {
  const target = event.currentTarget as HTMLElement;
  const rect = target.getBoundingClientRect();
  contextMenuTask.value = task;
  contextMenuX.value = rect.left;
  contextMenuY.value = rect.bottom;
  contextMenu.value = true;
};

const closeContextMenu = () => {
  contextMenu.value = false;
  contextMenuTask.value = null;
};

const showDetail = (task: Task) => {
  selectedTask.value = task;
  detailDialog.value = true;
  closeContextMenu();
};

onMounted(() => document.addEventListener('click', closeContextMenu));
onUnmounted(() => document.removeEventListener('click', closeContextMenu));
</script>

<template>
  <div class="tasks-container">
    <!-- Header -->
    <div class="tasks-header">
      <h2 class="tasks-title">{{ t('output') }}</h2>
      <v-btn color="primary" variant="tonal" @click="showFolder()">
        <v-icon icon="mdi-folder-open-outline" size="18" class="mr-1" />
        <span>{{ t('show-in-folder') }}</span>
      </v-btn>
    </div>

    <!-- Empty state -->
    <div v-if="!tasks || !tasks.length" class="empty-state">
      <v-icon icon="mdi-inbox-outline" size="64" />
      <p>{{ t('empty') }}</p>
    </div>

    <!-- Task list -->
    <div v-else class="task-list">
      <div
        v-for="task in tasks"
        :key="task.id"
        class="task-card glass fluent-glass"
        v-glass
        @contextmenu="showContextMenu($event, task)"
      >
        <!-- Cover (left 35%) -->
        <div class="task-cover">
          <div
            class="cover-image"
            :style="{ backgroundImage: 'url(' + convertFileSrc(task.cover) + ')' }"
          ></div>
        </div>

        <!-- Info (right 65%) -->
        <div class="task-info">
          <div class="task-header">
            <h3 class="task-name" :title="task.name">{{ task.name }}</h3>
            <span class="status-chip" :class="'chip-' + statusColor(task.status.type)">
              {{ task.status.type.toUpperCase() }}
            </span>
            <v-btn
              icon="mdi-dots-vertical"
              variant="text"
              size="x-small"
              class="task-menu-btn"
              :title="t('details')"
              @click.stop="showContextMenuFromButton($event, task)"
            />
          </div>

          <p class="task-path" :title="task.path">{{ task.path }}</p>

          <div class="task-status">
            <v-progress-circular
              v-if="['loading', 'mixing', 'pending'].includes(task.status.type)"
              indeterminate :color="statusColor(task.status.type)" size="20" width="2"
            />
            <v-progress-circular
              v-else-if="task.status.type === 'rendering'"
              :model-value="task.status.progress * 100"
              :color="statusColor(task.status.type)" size="20" width="2"
            />
            <span class="status-text">{{ describeStatus(task.status) }}</span>
          </div>

          <v-progress-linear
            v-if="task.status.type === 'rendering'"
            :model-value="task.status.progress * 100"
            :color="statusColor(task.status.type)" height="3" rounded
          />

          <div class="task-actions">
            <template v-if="['loading', 'mixing', 'rendering', 'pending'].includes(task.status.type)">
              <v-btn variant="text" size="small" color="primary" @click="invoke('cancel_task', { id: task.id })">
                <v-icon icon="mdi-cancel" size="16" class="mr-1" />
                <span>{{ t('cancel') }}</span>
              </v-btn>
            </template>
            <template v-else-if="task.status.type === 'failed'">
              <v-btn variant="text" size="small" color="primary" @click="errorDialogMessage = task.status.error; errorDialog = true;">
                <v-icon icon="mdi-alert-circle-outline" size="16" class="mr-1" />
                <span>{{ t('details') }}</span>
              </v-btn>
            </template>
            <template v-else-if="task.status.type === 'done'">
              <v-btn variant="text" size="small" color="primary" @click="showOutput(task)">
                <v-icon icon="mdi-text-box-outline" size="16" class="mr-1" />
                <span>{{ t('show-output') }}</span>
              </v-btn>
              <!-- Desktop shell integration only (explorer / gdbus / open). -->
              <v-btn v-if="!mobile" variant="text" size="small" color="primary" @click="showInFolder(task.output)">
                <v-icon icon="mdi-folder-open-outline" size="16" class="mr-1" />
                <span>{{ t('show-in-folder') }}</span>
              </v-btn>
            </template>
          </div>
        </div>
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
          <v-btn variant="tonal" color="primary" @click="outputDialog = false">{{ t('confirm') }}</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- Context menu -->
    <v-menu v-model="contextMenu" :target="[contextMenuX, contextMenuY]">
      <v-list class="ctx-menu">
        <v-list-item @click="showDetail(contextMenuTask!)">
          <template v-slot:prepend>
            <v-icon>mdi-information</v-icon>
          </template>
          <v-list-item-title>{{ t('details') }}</v-list-item-title>
        </v-list-item>
      </v-list>
    </v-menu>

    <!-- Detail dialog -->
    <v-dialog v-model="detailDialog" width="auto" min-width="500px">
      <v-card v-if="selectedTask">
        <v-card-title>{{ t('details') }}</v-card-title>
        <v-divider />
        <v-card-text>
          <div class="detail-list">
            <div class="detail-row"><span class="detail-label">名称</span><span>{{ selectedTask.name }}</span></div>
            <div class="detail-row"><span class="detail-label">路径</span><span>{{ selectedTask.path }}</span></div>
            <div class="detail-row"><span class="detail-label">状态</span><span>{{ describeStatus(selectedTask.status) }}</span></div>
            <div class="detail-row" v-if="selectedTask.status.type === 'rendering'">
              <span class="detail-label">进度</span><span>{{ Math.round(selectedTask.status.progress * 100) }}%</span>
            </div>
            <div class="detail-row" v-if="selectedTask.status.type === 'rendering'">
              <span class="detail-label">帧率</span><span>{{ selectedTask.status.fps }} FPS</span>
            </div>
            <div class="detail-row" v-if="selectedTask.status.type === 'done' && selectedTask.status.duration">
              <span class="detail-label">耗时</span><span>{{ formatDuration(selectedTask.status.duration) }}</span>
            </div>
            <div class="detail-row" v-if="selectedTask.status.type === 'done' && selectedTask.output">
              <span class="detail-label">输出</span><span class="break-all">{{ selectedTask.output }}</span>
            </div>
            <div class="detail-row" v-if="selectedTask.status.type === 'failed' && selectedTask.status.error">
              <span class="detail-label">错误</span><span class="error-text">{{ selectedTask.status.error }}</span>
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
  height: 100vh;
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
  overflow-y: auto;
}

/* ===== Task Card ===== */
.task-card {
  display: flex;
  flex-direction: row;
  overflow: hidden;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
  height: 180px;
}

.task-card:hover {
  transform: translateY(-2px);
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
.task-path {
  font-size: 12px;
  color: rgba(var(--main-color-rgb), 0.45);
  margin: 0;
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
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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
@media (max-width: 600px) {
  .tasks-container { padding: 16px; }
  .task-card { flex-direction: column; height: auto; }
  .task-cover { width: 100%; min-height: 140px; }
  .task-info { width: 100%; }
}

/* ===== Mobile (touch) ===== */
@media (pointer: coarse) {
  .task-menu-btn {
    min-width: 44px;
    min-height: 44px;
  }
}
</style>
