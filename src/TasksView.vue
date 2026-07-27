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

  show-output: Output
  show-in-folder: Open

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

  show-output: 输出
  show-in-folder: 打开

</i18n>

<script setup lang="ts">
import { mainColor, bgColor } from '@/colorStore'
import { ref, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import type { Task, TaskStatus } from './model';
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import moment from 'moment';

const { t } = useI18n();

const tasks = ref<Task[]>();

async function updateList() {
  tasks.value = await invoke<Task[]>('get_tasks');
}

await updateList();

const updateTask = setInterval(updateList, 700);
onUnmounted(() => clearInterval(updateTask));

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
      const progressDisplay = status.progress >= 0.999 ? '100.00' : (status.progress * 100).toFixed(2);
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
const contextMenu = ref(false);
const contextMenuX = ref(0);
const contextMenuY = ref(0);
const contextMenuTask = ref<Task | null>(null);

async function showInFolder(path: string) {
  try { await invoke('show_in_folder', { path }); } catch (e) { showToast(e, 'error'); }
}

async function showFolder() {
  try { await invoke('show_folder'); } catch (e) { showToast(e, 'error'); }
}

const showContextMenu = (event: MouseEvent, task: Task) => {
  event.preventDefault();
  contextMenu.value = true;
  contextMenuX.value = event.clientX;
  contextMenuY.value = event.clientY;
  contextMenuTask.value = task;
};

const closeContextMenu = () => {
  contextMenu.value = false;
  contextMenuTask.value = null;
};

document.addEventListener('click', closeContextMenu);
</script>

<template>
  <div class="tasks-container">
    <!-- Header -->
    <div class="tasks-header">
      <h2 class="tasks-title">{{ t('output') }}</h2>
      <fv-button :background="bgColor" :foreground="mainColor" :border-radius="20" @click="showFolder()">
        <fv-animated-icon icon="FolderOpen" :font-size="16" />
        <span>{{ t('show-in-folder') }}</span>
      </fv-button>
    </div>

    <!-- Empty state -->
    <div v-if="!tasks || !tasks.length" class="empty-state">
      <fv-animated-icon icon="Mail" :font-size="64" style="opacity: 0.2;" />
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
          </div>

          <p class="task-path" :title="task.path">{{ task.path }}</p>

          <div class="task-status">
            <fv-spinner
              v-if="['loading', 'mixing', 'pending'].includes(task.status.type)"
              size="small"
            />
            <div class="mini-progress-ring" v-else-if="task.status.type === 'rendering'"
              :style="{ '--progress': (task.status.progress * 100) + '%' }">
            </div>
            <span class="status-text">{{ describeStatus(task.status) }}</span>
          </div>

          <div class="css-progress-bar" v-if="task.status.type === 'rendering'">
            <div class="css-progress-fill" :style="{ width: (task.status.progress * 100) + '%' }"></div>
          </div>

          <div class="task-actions">
            <template v-if="['loading', 'mixing', 'rendering', 'pending'].includes(task.status.type)">
              <fv-button :background="bgColor" :foreground="mainColor" :border-radius="20" @click="invoke('cancel_task', { id: task.id })">
                <fv-animated-icon icon="ChromeClose" :font-size="16" />
                <span>{{ t('cancel') }}</span>
              </fv-button>
            </template>
            <template v-else-if="task.status.type === 'failed'">
              <fv-button :background="bgColor" :foreground="mainColor" :border-radius="20" @click="errorDialogMessage = task.status.error; errorDialog = true;">
                <fv-animated-icon icon="Warning" :font-size="16" />
                <span>{{ t('details') }}</span>
              </fv-button>
            </template>
            <template v-else-if="task.status.type === 'done'">
              <fv-button :background="bgColor" :foreground="mainColor" :border-radius="20" @click="showInFolder(task.output)">
                <fv-animated-icon icon="FolderOpen" :font-size="16" />
                <span>{{ t('show-in-folder') }}</span>
              </fv-button>
            </template>
          </div>
        </div>
      </div>
    </div>

    <!-- Error/Detail dialog -->
    <div v-if="errorDialog" class="ctx-backdrop" @click="errorDialog = false">
      <div class="ctx-menu" style="left: 50%; top: 50%; transform: translate(-50%, -50%); max-width: 500px; width: 90%;">
        <div style="padding: 12px 16px; font-size: 14px; color: var(--sub-color); white-space: pre-wrap; max-height: 60vh; overflow: auto;">{{ errorDialogMessage }}</div>
        <div style="padding: 8px 16px; text-align: right; border-top: 1px solid rgba(255,255,255,0.06);">
          <fv-button :background="bgColor" :foreground="mainColor" :border-radius="20" @click="errorDialog = false">{{ t('confirm') }}</fv-button>
        </div>
      </div>
    </div>

    <!-- Context menu -->
    <div v-if="contextMenu" class="ctx-backdrop" @click="closeContextMenu">
      <div class="ctx-menu" :style="{ left: contextMenuX + 'px', top: contextMenuY + 'px' }">
        <div class="ctx-item" @click="errorDialogMessage = describeStatus(contextMenuTask!.status); errorDialog = true; closeContextMenu();">
          <fv-animated-icon icon="Info" :font-size="16" />
          <span>{{ t('details') }}</span>
        </div>
      </div>
    </div>

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
  color: rgba(255, 255, 255, 0.9);
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
  color: rgba(255, 255, 255, 0.4);
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
  border-radius: 16px;
  overflow: hidden;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
  height: 180px;
}

.task-cover {
  width: 35%;
  flex-shrink: 0;
  background: rgba(0, 0, 0, 0.2);
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
  color: rgba(255, 255, 255, 0.9);
  margin: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}
.task-path {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.45);
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
  color: rgba(255, 255, 255, 0.7);
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
.chip-success { background: rgba(var(--main-color-rgb), 0.15); color: var(--main-color); }
.chip-warning { background: rgba(var(--main-color-rgb), 0.15); color: var(--main-color); }
.chip-error { background: rgba(var(--sub-color-rgb), 0.15); color: var(--sub-color); }

/* ===== Done Badge (version-badge style) ===== */
.chip-success {
  background: transparent;
  border: 1px solid var(--main-color);
  color: var(--main-color);
  padding: 4px 12px;
  border-radius: 12px;
}

/* ===== Progress ===== */
.css-progress-bar {
  width: 100%;
  height: 3px;
  background: rgba(var(--bg-color-rgb), 0.08);
  border-radius: 2px;
  overflow: hidden;
}
.css-progress-fill {
  height: 100%;
  background: var(--main-color);
  border-radius: 2px;
  transition: width 0.3s linear;
}
.mini-progress-ring {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 2px solid rgba(255, 255, 255, 0.1);
  border-top-color: var(--main-color);
  animation: spin 1s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }

/* ===== Actions ===== */
.task-actions {
  display: flex;
  gap: 8px;
  margin-top: auto;
}

/* ===== Context Menu ===== */
.ctx-backdrop {
  position: fixed;
  inset: 0;
  z-index: 2000;
}
.ctx-menu {
  position: fixed;
  background: rgba(28, 28, 28, 0.95);
  backdrop-filter: blur(12px);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 16px;
  padding: 4px;
  min-width: 160px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
}
.ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-radius: 12px;
  cursor: pointer;
  color: rgba(255, 255, 255, 0.7);
  font-size: 14px;
  transition: background 0.15s;
}
.ctx-item:hover {
  background: rgba(255, 255, 255, 0.06);
}

/* ===== Responsive ===== */
@media (max-width: 600px) {
  .tasks-container { padding: 16px; }
  .task-card { flex-direction: column; }
  .task-cover { width: 100%; min-height: 140px; }
  .task-info { width: 100%; }
}
</style>