<i18n>
en:
  settings:
    outputPath:
      label: "Custom output path"
      placeholder: "Click to choose folder or type manually"
      hint: "Prefer Browser FS API; use Electron for absolute path"
      aria: "Custom output path"
    selectFolder: "Select folder"
    save: "Save"
    copy: "Copy path"
    copied: "Path copied"
    clear: "Clear"
    saved: "Saved"
    language:
      label: "Language"
      hint: "The app reloads after switching languages"
    server:
      label: "Render server"
      hint: "Lets phones on the same network submit charts to this machine"
      host: "Bind address"
      hostHint: "0.0.0.0 accepts phones; 127.0.0.1 is local only"
      port: "Port"
      start: "Start server"
      stop: "Stop server"
      running: "Running on {url}"
      stopped: "Not running"
      open: "Open WebUI"
      copy: "Copy link"
      copied: "Link copied"
      failed: "Could not start: {error}"
    remote:
      label: "Remote renderer"
      hint: "Start the server on a PC first, then open its WebUI here"
      address: "Renderer address"
      placeholder: "192.168.1.10:6787"
      test: "Test connection"
      open: "Open WebUI"
      connected: "Connected · v{version}"
      offline: "Cannot connect: {error}"
      live: "Live (WebSocket)"
      fallback: "Polling"
      noJobs: "No tasks yet"
      download: "Download"
      statusQueued: "Queued"
      statusMixing: "Mixing"
      statusRunning: "Rendering"
      statusDone: "Done"
      statusFailed: "Failed"
      statusCanceled: "Canceled"
    selected:
      picked: "Selected folder: {name}"
      fallback: "Fallback selection: {count} files (root: {root})"
    warning:
      empty: "Path must not be empty"
      select_error: "Failed to select folder: {msg}"
      copy_error: "Copy failed: please copy manually"
    background:
      label: "Custom background"
      placeholder: "Click to select image file"
      hint: "Supports JPG, PNG, WEBP format"
      selectFile: "Select image"
      clear: "Clear background"
      saved: "Background saved!"
      fix: "Fix"
      refresh: "Refresh"
      fixed: "Background successfully fixed"
      fixFailed: "Failed to fix the background"
      refreshFailed: "Failed to refresh the background"
      noCustom: "No custom background to clear"
    colors:
      label: "Theme Color"
      main: "Main Color"
      bg: "Background Color"
      sub: "Sub Color"
      submit: "Apply"
      saved: "Color saved!"
zh-CN:
  settings:
    outputPath:
      label: "自定义输出路径"
      placeholder: "点击选择文件夹或手动输入"
      hint: "请输入正确路径"
      aria: "自定义输出路径"
    selectFolder: "选择文件夹"
    save: "保存路径"
    copy: "复制路径"
    copied: "路径已复制"
    clear: "清除"
    saved: "保存成功！"
    language:
      label: "语言"
      hint: "切换语言后应用会自动重新加载"
    server:
      label: "渲染服务器"
      hint: "让同一局域网里的手机把谱面提交到这台电脑渲染"
      host: "监听地址"
      hostHint: "0.0.0.0 允许手机连接；127.0.0.1 仅本机"
      port: "端口"
      start: "启动服务器"
      stop: "停止服务器"
      running: "运行中：{url}"
      stopped: "未启动"
      open: "打开 WebUI"
      copy: "复制链接"
      copied: "链接已复制"
      failed: "启动失败：{error}"
    remote:
      label: "远程渲染器"
      hint: "先在电脑上启动服务器，再在这里打开它的 WebUI"
      address: "渲染器地址"
      placeholder: "192.168.1.10:6787"
      test: "测试连接"
      open: "打开 WebUI"
      connected: "已连接 · v{version}"
      offline: "无法连接：{error}"
      live: "实时连接"
      fallback: "轮询模式"
      noJobs: "暂无任务"
      download: "下载"
      statusQueued: "排队中"
      statusMixing: "混音中"
      statusRunning: "渲染中"
      statusDone: "已完成"
      statusFailed: "失败"
      statusCanceled: "已取消"
    selected:
      picked: "已选择文件夹：{name}"
      fallback: "回退选择：{count} 个文件（根：{root})"
    warning:
      empty: "路径不能为空"
      select_error: "选择文件夹时出错：{msg}"
      copy_error: "复制失败：请手动复制"
    background:
      label: "自定义背景"
      placeholder: "点击选择图片文件"
      hint: "支持 JPG、PNG、WEBP 格式"
      selectFile: "选择图片"
      clear: "清除背景"
      saved: "背景已保存！"
      fix: "固定背景"
      refresh: "刷新背景"
      fixed: "背景成功固定!"
      fixFailed: "固定背景失败"
      refreshFailed: "刷新背景失败"
      noCustom: "当前没有自定义背景"
    colors:
      label: "主题色"
      main: "主色"
      bg: "背景色"
      sub: "辅色"
      submit: "应用"
      saved: "颜色已保存！"
</i18n>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { open } from '@tauri-apps/plugin-dialog';
import { appConfigDir } from '@tauri-apps/api/path';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { open as openUrl } from '@tauri-apps/plugin-shell';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

defineOptions({ name: 'SettingsPanel' });
import { RULES as rules, toast, toastError, changeLocale } from './common';
import { mainColor, bgColor, subColor } from './colorStore';
import { isMobile } from './platform';
import ColorDot from './components/ColorDot.vue';

const { t } = useI18n();
const mobile = isMobile();

// Only these two message sets exist; `changeLocale` normalises zh-TW to zh-CN.
const LANGUAGES = [
  { value: 'en', title: 'English' },
  { value: 'zh-CN', title: '简体中文' },
];
const storedLocale = localStorage.getItem('locale');
const currentLocale = ref(storedLocale === 'en' || storedLocale === 'zh-CN' ? storedLocale : storedLocale === 'zh-TW' ? 'zh-CN' : 'en');

function applyLocale(value: string) {
  changeLocale(value);
  // Several labels are resolved once during setup (render list, preset names, moment
  // locale); a reload guarantees every view picks up the new language.
  window.location.reload();
}

const outputPath = ref<string>(localStorage.getItem('outputPath') || '');
const selectedInfo = ref<string | null>(null);

const backgroundPath = ref<string>(localStorage.getItem('customBackground') || '');

const backgroundPreviewUrl = computed(() => {
  if (backgroundPath.value) {
    try { return convertFileSrc(backgroundPath.value); } catch { return backgroundPath.value; }
  }
  return '';
});

async function selectFolder() {
  selectedInfo.value = null;
  try {
    const selected = await open({ directory: true, multiple: false, defaultPath: await appConfigDir() });
    if (selected === null) return;
    const path = Array.isArray(selected) ? selected[0] : selected;
    outputPath.value = path;
    const rootName = path.split(/[\\/]/).pop() || path;
    selectedInfo.value = t('settings.selected.picked', { name: rootName });
  } catch (err: any) {
    toast(t('settings.warning.select_error', { msg: err.message || String(err) }), 'error');
  }
}

function saveOutputPath() {
  // non_empty returns `true` when valid and an error *string* otherwise — both truthy, so compare explicitly.
  if (rules.non_empty(outputPath.value) !== true) {
    toast(t('settings.warning.empty'), 'warning');
    return;
  }
  localStorage.setItem('outputPath', outputPath.value);
  toast(t('settings.saved'), 'success');
}

async function copyPath() {
  if (!outputPath.value) return;
  try {
    await writeText(outputPath.value);
    toast(t('settings.copied'), 'success');
  } catch {
    toast(t('settings.warning.copy_error'), 'error');
  }
}

function clearPath() {
  outputPath.value = '';
  selectedInfo.value = null;
  localStorage.removeItem('outputPath');
}

async function selectBackground() {
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Image', extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp'] }],
      defaultPath: await appConfigDir(),
    });
    if (selected === null) return;
    const path = Array.isArray(selected) ? selected[0] : selected;
    backgroundPath.value = path as string;
    localStorage.setItem('customBackground', backgroundPath.value);
    window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: backgroundPath.value }));
    toast(t('settings.background.saved'), 'success');
  } catch (err: any) {
    console.error('Failed to select background:', err);
    toast(String(err), 'error');
  }
}

function clearBackground() {
  if (!backgroundPath.value) {
    toast(t('settings.background.noCustom'), 'warning');
    return;
  }
  backgroundPath.value = '';
  localStorage.removeItem('customBackground');
  window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: null }));
  toast(t('settings.background.saved'), 'success');
}

async function fixBackground() {
  const ok = await window.fixCurrentBackground?.();
  const stored = localStorage.getItem('customBackground');
  if (stored) backgroundPath.value = stored;
  toast(t(ok ? 'settings.background.fixed' : 'settings.background.fixFailed'), ok ? 'success' : 'error');
}

async function refreshBackground() {
  const ok = await window.refreshApiBackground?.();
  backgroundPath.value = '';
  toast(t(ok ? 'settings.background.saved' : 'settings.background.refreshFailed'), ok ? 'success' : 'error');
}

// ===== Theme colours =====
// Drafts are edited in the picker and only committed to the store on "Apply",
// so dragging the picker doesn't repaint the whole app on every frame.
const tempMainColor = ref(mainColor.value);
const tempBgColor = ref(bgColor.value);
const tempSubColor = ref(subColor.value);

function saveColor(colorKey: 'mainColor' | 'bgColor' | 'subColor', value: string) {
  localStorage.setItem(colorKey, value);
  if (colorKey === 'mainColor') mainColor.value = value;
  else if (colorKey === 'bgColor') bgColor.value = value;
  else subColor.value = value;
  toast(t('settings.colors.saved'), 'success');
}

function confirmColor(type: 'main' | 'bg' | 'sub') {
  if (type === 'main') saveColor('mainColor', tempMainColor.value);
  else if (type === 'bg') saveColor('bgColor', tempBgColor.value);
  else saveColor('subColor', tempSubColor.value);
}

// ===== Render server (desktop) =====
// Started in-process by Tauri: the HTTP handler only queues jobs and spawns the
// same `render` child the GUI task queue uses.
const serverHost = ref(localStorage.getItem('serverHost') || '0.0.0.0');
const serverPort = ref(Number(localStorage.getItem('serverPort')) || 6787);
const serverRunning = ref(false);
const serverBusy = ref(false);
const serverError = ref('');

const serverDisplayHost = computed(() =>
  serverHost.value === '0.0.0.0' || serverHost.value === '::' ? '127.0.0.1' : serverHost.value,
);
const serverUrl = computed(() => `http://${serverDisplayHost.value}:${serverPort.value}/`);

async function refreshServer() {
  try {
    const status = (await invoke('render_server_status')) as { running: boolean; host: string; port: number };
    serverRunning.value = status.running;
    if (status.running) {
      serverHost.value = status.host;
      serverPort.value = status.port;
    }
  } catch {
    /* command unavailable on older builds */
  }
}

async function startServer() {
  serverBusy.value = true;
  serverError.value = '';
  try {
    localStorage.setItem('serverHost', serverHost.value);
    localStorage.setItem('serverPort', String(serverPort.value));
    const status = (await invoke('start_render_server', {
      host: serverHost.value,
      port: Number(serverPort.value),
    })) as { running: boolean; port: number };
    serverRunning.value = status.running;
    serverPort.value = status.port;
    toast(t('settings.server.running', { url: serverUrl.value }), 'success');
  } catch (e) {
    serverError.value = String((e as Error)?.message || e);
    toastError(serverError.value);
  } finally {
    serverBusy.value = false;
  }
}

async function stopServer() {
  serverBusy.value = true;
  try {
    await invoke('stop_render_server');
    serverRunning.value = false;
  } catch (e) {
    toastError(e);
  } finally {
    serverBusy.value = false;
  }
}

async function openServerWebUI() {
  try {
    await openUrl(serverUrl.value);
  } catch (e) {
    toastError(e);
  }
}

async function copyServerUrl() {
  try {
    await navigator.clipboard.writeText(serverUrl.value);
    toast(t('settings.server.copied'), 'success');
  } catch {
    toastError(t('settings.server.copy'));
  }
}

// ===== Remote renderer (mobile) =====
const remoteAddress = ref(localStorage.getItem('remoteRenderer') || '192.168.1.10:6787');
const remoteBusy = ref(false);
const remoteVersion = ref('');
const remoteError = ref('');

const remoteBase = computed(() => {
  let raw = remoteAddress.value.trim().replace(/\/+$/, '');
  if (!raw) return '';
  if (!/^https?:\/\//i.test(raw)) raw = `http://${raw}`;
  return raw;
});
const remoteUrl = computed(() => `${remoteBase.value}/`);

type RemoteJob = {
  id: string;
  name: string;
  status: string;
  progress: number;
  fps: number;
  eta: number;
  error?: string;
  download?: string;
};
const remoteJobs = ref<RemoteJob[]>([]);
const remoteLive = ref(false);
const remotePolling = ref(false);
let socket: WebSocket | null = null;
let pollTimer: number | null = null;
let watching = false;

const JOB_STATUS_KEYS: Record<string, string> = {
  queued: 'statusQueued',
  mixing: 'statusMixing',
  running: 'statusRunning',
  done: 'statusDone',
  failed: 'statusFailed',
  canceled: 'statusCanceled',
};
const jobStatusText = (status: string) =>
  t(`settings.remote.${JOB_STATUS_KEYS[status] || 'statusQueued'}`);
const isActiveJob = (job: RemoteJob) => ['queued', 'mixing', 'running'].includes(job.status);

function stopWatching() {
  watching = false;
  socket?.close();
  socket = null;
  remoteLive.value = false;
  remotePolling.value = false;
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function startPolling() {
  if (pollTimer !== null) {
    remotePolling.value = true;
    return;
  }
  remotePolling.value = true;
  const tick = async () => {
    try {
      remoteJobs.value = (await invoke('remote_jobs', { base: remoteBase.value })) as RemoteJob[];
    } catch {
      /* server gone; keep the last snapshot */
    }
  };
  void tick();
  pollTimer = window.setInterval(tick, 1500);
}

function startWatching() {
  stopWatching();
  watching = true;
  // `/render` is the WebSocket endpoint; `/` is the WebUI.
  const wsUrl = `${remoteBase.value.replace(/^http/i, 'ws')}/render`;
  try {
    const ws = new WebSocket(wsUrl);
    socket = ws;
    ws.onopen = () => {
      remoteLive.value = true;
      remotePolling.value = false;
      if (pollTimer !== null) {
        clearInterval(pollTimer);
        pollTimer = null;
      }
    };
    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as { type?: string; jobs?: RemoteJob[] };
        if (data.type === 'jobs' && Array.isArray(data.jobs)) remoteJobs.value = data.jobs;
      } catch {
        /* ignore malformed frames */
      }
    };
    ws.onerror = () => {
      // Android WebView may refuse cleartext ws://; fall back to Rust polling.
      if (watching && !remoteLive.value) startPolling();
    };
    ws.onclose = () => {
      remoteLive.value = false;
      if (watching) startPolling();
    };
  } catch {
    startPolling();
  }
}

async function openRemoteDownload(download?: string) {
  if (!download) return;
  try {
    await openUrl(`${remoteBase.value}${download}`);
  } catch (e) {
    toastError(e);
  }
}

async function testRemote(): Promise<boolean> {
  if (!remoteBase.value) return false;
  remoteBusy.value = true;
  remoteError.value = '';
  remoteVersion.value = '';
  localStorage.setItem('remoteRenderer', remoteAddress.value.trim());
  try {
    const status = (await invoke('remote_status', { base: remoteBase.value })) as { version?: string };
    remoteVersion.value = status.version || '?';
    startWatching();
    return true;
  } catch (e) {
    remoteError.value = String((e as Error)?.message || e);
    stopWatching();
    return false;
  } finally {
    remoteBusy.value = false;
  }
}

async function openRemoteWebUI() {
  if (!remoteBase.value) return;
  localStorage.setItem('remoteRenderer', remoteAddress.value.trim());
  try {
    await openUrl(remoteUrl.value);
  } catch (e) {
    toastError(e);
  }
}

onMounted(() => {
  if (mobile) {
    if (remoteAddress.value) void testRemote();
  } else {
    refreshServer();
  }
});

onUnmounted(stopWatching);

</script>

<template>
  <div class="settings-container">
    <div class="settings-scroll">
      <!-- Language (available on mobile too; MOBILE.md lists it as supported) -->
      <div class="md3-card glass fluent-glass" v-glass>
        <div class="card-label">{{ t('settings.language.label') }}</div>
        <v-select
          v-model="currentLocale"
          :items="LANGUAGES"
          item-title="title"
          item-value="value"
          :label="t('settings.language.label')"
          :hint="t('settings.language.hint')"
          persistent-hint
          density="compact"
          variant="outlined"
          @update:model-value="applyLocale" />
      </div>

      <!-- Render server: desktop only (phones cannot host renders) -->
      <div v-if="!mobile" class="md3-card glass fluent-glass" v-glass>
        <div class="card-label">{{ t('settings.server.label') }}</div>
        <p class="server-hint">{{ t('settings.server.hint') }}</p>
        <div class="server-row">
          <v-text-field
            v-model="serverHost"
            :label="t('settings.server.host')"
            :hint="t('settings.server.hostHint')"
            persistent-hint
            density="compact"
            variant="outlined"
            :disabled="serverRunning" />
          <v-text-field
            v-model.number="serverPort"
            :label="t('settings.server.port')"
            type="number"
            density="compact"
            variant="outlined"
            hide-details
            class="port-field"
            :disabled="serverRunning" />
        </div>
        <div class="server-actions">
          <v-btn
            v-if="!serverRunning"
            color="primary"
            prepend-icon="mdi-play"
            :loading="serverBusy"
            @click="startServer">
            {{ t('settings.server.start') }}
          </v-btn>
          <v-btn
            v-else
            color="error"
            variant="tonal"
            prepend-icon="mdi-stop"
            :loading="serverBusy"
            @click="stopServer">
            {{ t('settings.server.stop') }}
          </v-btn>
          <v-btn
            variant="tonal"
            prepend-icon="mdi-open-in-new"
            :disabled="!serverRunning"
            @click="openServerWebUI">
            {{ t('settings.server.open') }}
          </v-btn>
          <v-btn
            variant="text"
            prepend-icon="mdi-content-copy"
            :disabled="!serverRunning"
            @click="copyServerUrl">
            {{ t('settings.server.copy') }}
          </v-btn>
        </div>
        <p v-if="serverRunning" class="server-status">{{ t('settings.server.running', { url: serverUrl }) }}</p>
        <p v-else class="server-status">{{ t('settings.server.stopped') }}</p>
      </div>

      <!-- Remote renderer: mobile only, replaces the local render pages -->
      <div v-if="mobile" class="md3-card glass fluent-glass" v-glass>
        <div class="card-label">{{ t('settings.remote.label') }}</div>
        <p class="server-hint">{{ t('settings.remote.hint') }}</p>
        <v-text-field
          v-model="remoteAddress"
          :label="t('settings.remote.address')"
          :placeholder="t('settings.remote.placeholder')"
          density="compact"
          variant="outlined"
          hide-details
          class="mt-2"
          @keyup.enter="testRemote" />
        <div class="server-actions">
          <v-btn color="primary" prepend-icon="mdi-lan-connect" :loading="remoteBusy" @click="testRemote">
            {{ t('settings.remote.test') }}
          </v-btn>
          <v-btn variant="tonal" prepend-icon="mdi-open-in-new" @click="openRemoteWebUI">
            {{ t('settings.remote.open') }}
          </v-btn>
        </div>
        <p v-if="remoteVersion" class="server-status ok">{{ t('settings.remote.connected', { version: remoteVersion }) }}</p>
        <p v-else-if="remoteError" class="server-status bad">{{ t('settings.remote.offline', { error: remoteError }) }}</p>
        <p v-if="remoteLive || remotePolling" class="live-pill">
          <span class="dot" :class="{ live: remoteLive }"></span>
          {{ remoteLive ? t('settings.remote.live') : t('settings.remote.fallback') }}
        </p>

        <div v-if="remoteJobs.length" class="remote-jobs">
          <div v-for="job in remoteJobs.slice(0, 6)" :key="job.id" class="remote-job">
            <div class="remote-job-head">
              <span class="remote-job-name">{{ job.name || job.id }}</span>
              <span class="remote-job-state" :class="job.status">{{ jobStatusText(job.status) }}</span>
            </div>
            <v-progress-linear
              v-if="isActiveJob(job)"
              :model-value="Math.round((job.progress || 0) * 100)"
              height="6"
              rounded
              color="primary"
              class="mt-1" />
            <div v-if="isActiveJob(job)" class="remote-job-meta">
              <span>{{ job.fps || 0 }} FPS</span>
              <span>{{ Math.round((job.progress || 0) * 100) }}%</span>
            </div>
            <div v-if="job.error" class="remote-job-error">{{ job.error }}</div>
            <a
              v-if="job.status === 'done' && job.download"
              class="remote-job-link"
              href="#"
              @click.prevent="openRemoteDownload(job.download)">
              {{ t('settings.remote.download') }}
            </a>
          </div>
        </div>
        <p v-else class="server-status">{{ t('settings.remote.noJobs') }}</p>
      </div>

      <!-- Output Path (desktop only: mobile render output is fixed to the app data dir) -->
      <div v-if="!mobile" class="md3-card glass fluent-glass" v-glass>
        <div class="card-label">{{ t('settings.outputPath.label') }}</div>
        <v-text-field
          v-model="outputPath"
          :label="t('settings.outputPath.label')"
          :placeholder="t('settings.outputPath.placeholder')"
          :rules="[rules.non_empty]"
          clearable
          density="compact"
          :hint="t('settings.outputPath.hint')"
          persistent-hint
          variant="outlined"
          append-icon="mdi-folder-open"
          @click:append="selectFolder"
        />
        <div v-if="selectedInfo" class="hint-text">{{ selectedInfo }}</div>
        <div class="card-actions">
          <v-btn variant="tonal" color="primary" @click="selectFolder">
            <v-icon icon="mdi-folder-outline" size="18" class="mr-1" />
            <span>{{ t('settings.selectFolder') }}</span>
          </v-btn>
          <v-btn color="primary" @click="saveOutputPath">
            <v-icon icon="mdi-content-save-outline" size="18" class="mr-1" />
            <span>{{ t('settings.save') }}</span>
          </v-btn>
          <v-btn variant="text" :disabled="!outputPath" :title="t('settings.copy')" @click="copyPath">
            <v-icon icon="mdi-content-copy" size="18" />
          </v-btn>
          <v-btn variant="text" :disabled="!outputPath" :title="t('settings.clear')" @click="clearPath">
            <v-icon icon="mdi-close" size="18" />
          </v-btn>
        </div>
      </div>

      <!-- Background -->
      <div class="md3-card glass fluent-glass" v-glass>
        <div class="card-label">{{ t('settings.background.label') }}</div>
        <div v-if="backgroundPreviewUrl" class="bg-preview">
          <img :src="backgroundPreviewUrl" alt="Background preview" class="preview-img" />
        </div>
        <div v-else class="blur-glass bg-preview empty">{{ t('settings.background.noCustom') }}</div>

        <div class="card-actions">
          <v-btn variant="tonal" color="primary" @click="selectBackground">
            <v-icon icon="mdi-image-outline" size="18" class="mr-1" />
            <span>{{ t('settings.background.selectFile') }}</span>
          </v-btn>
          <v-btn variant="tonal" color="primary" @click="fixBackground">
            <v-icon icon="mdi-pin-outline" size="18" class="mr-1" />
            <span>{{ t('settings.background.fix') }}</span>
          </v-btn>
          <v-btn variant="tonal" color="primary" @click="refreshBackground">
            <v-icon icon="mdi-refresh" size="18" class="mr-1" />
            <span>{{ t('settings.background.refresh') }}</span>
          </v-btn>
          <v-btn variant="text" :disabled="!backgroundPath" :title="t('settings.background.clear')" @click="clearBackground">
            <v-icon icon="mdi-close" size="18" />
          </v-btn>
        </div>
      </div>

      <!-- Theme colours -->
      <div class="md3-card glass fluent-glass" v-glass>
        <div class="card-label">{{ t('settings.colors.label') }}</div>
        <div class="color-row">
          <div class="color-item">
            <span class="color-label">{{ t('settings.colors.main') }}</span>
            <v-menu :close-on-content-click="false" location="bottom">
              <template v-slot:activator="{ props }">
                <div v-bind="props" class="swatch-btn">
                  <ColorDot :color="tempMainColor" :size="28" />
                </div>
              </template>
              <v-color-picker v-model="tempMainColor" hide-inputs mode="hex" />
            </v-menu>
            <v-btn variant="tonal" color="primary" size="small" @click="confirmColor('main')">
              <v-icon icon="mdi-check" size="16" class="mr-1" />
              <span>{{ t('settings.colors.submit') }}</span>
            </v-btn>
          </div>

          <div class="color-item">
            <span class="color-label">{{ t('settings.colors.bg') }}</span>
            <v-menu :close-on-content-click="false" location="bottom">
              <template v-slot:activator="{ props }">
                <div v-bind="props" class="swatch-btn">
                  <ColorDot :color="tempBgColor" :size="28" />
                </div>
              </template>
              <v-color-picker v-model="tempBgColor" hide-inputs mode="hex" />
            </v-menu>
            <v-btn variant="tonal" color="primary" size="small" @click="confirmColor('bg')">
              <v-icon icon="mdi-check" size="16" class="mr-1" />
              <span>{{ t('settings.colors.submit') }}</span>
            </v-btn>
          </div>

          <div class="color-item">
            <span class="color-label">{{ t('settings.colors.sub') }}</span>
            <v-menu :close-on-content-click="false" location="bottom">
              <template v-slot:activator="{ props }">
                <div v-bind="props" class="swatch-btn">
                  <ColorDot :color="tempSubColor" :size="28" />
                </div>
              </template>
              <v-color-picker v-model="tempSubColor" hide-inputs mode="hex" />
            </v-menu>
            <v-btn variant="tonal" color="primary" size="small" @click="confirmColor('sub')">
              <v-icon icon="mdi-check" size="16" class="mr-1" />
              <span>{{ t('settings.colors.submit') }}</span>
            </v-btn>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-container {
  width: 100%;
  max-width: 720px;
  margin: 0 auto;
  padding: 24px;
  height: 100vh;
  display: flex;
  flex-direction: column;
}

.settings-scroll {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.md3-card {
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.card-label {
  font-size: 11px;
  font-weight: 700;
  color: rgba(var(--main-color-rgb), 0.8);
  text-transform: uppercase;
  letter-spacing: 1px;
}

.card-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
}

.hint-text {
  font-size: 12px;
  font-weight: 600;
  color: var(--sub-color);
}

/* ===== Background Preview ===== */
.bg-preview {
  width: 100%;
  max-width: 360px;
  height: 140px;
  border-radius: 16px;
  overflow: hidden;
}
.bg-preview.empty {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  color: rgba(var(--main-color-rgb), 0.4);
}
.preview-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

/* ===== Theme colours ===== */
.color-row {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.color-item {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}
.color-label {
  min-width: 100px;
  color: var(--main-color);
  font-weight: 500;
}
.swatch-btn {
  cursor: pointer;
  transition: transform 0.12s ease;
}
.swatch-btn:hover {
  transform: scale(1.08);
}

/* ===== Responsive ===== */
@media (max-width: 600px) {
  .settings-container {
    padding: 16px;
  }
  .color-item {
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }
}
.server-hint {
  margin: -2px 0 10px;
  font-size: 0.78rem;
  opacity: 0.65;
  line-height: 1.5;
}
.server-row {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}
.server-row .v-input:first-child {
  flex: 1;
}
.port-field {
  max-width: 120px;
}
.server-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 12px;
}
.server-status {
  margin: 12px 0 0;
  font-size: 0.78rem;
  opacity: 0.75;
  word-break: break-all;
}
.server-status.ok { color: #6ee7a8; }
.live-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin: 10px 0 0;
  font-size: 0.74rem;
  opacity: 0.75;
}
.live-pill .dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #ffcf6e;
}
.live-pill .dot.live {
  background: #6ee7a8;
  box-shadow: 0 0 8px rgba(110, 231, 168, 0.8);
}
.remote-jobs {
  margin-top: 10px;
  display: flex;
  flex-direction: column;
}
.remote-job {
  padding: 9px 0;
}
.remote-job + .remote-job {
  border-top: 1px solid rgba(255, 255, 255, 0.07);
}
.remote-job-head {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  align-items: baseline;
}
.remote-job-name {
  font-size: 0.86rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.remote-job-state {
  flex: none;
  font-size: 0.68rem;
  padding: 1px 7px;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.16);
  opacity: 0.8;
}
.remote-job-state.done { color: #6ee7a8; }
.remote-job-state.failed,
.remote-job-state.canceled { color: #ff8f8f; }
.remote-job-meta {
  display: flex;
  justify-content: space-between;
  font-size: 0.7rem;
  opacity: 0.6;
  margin-top: 3px;
}
.remote-job-error {
  font-size: 0.72rem;
  color: #ff8f8f;
  word-break: break-word;
  margin-top: 4px;
}
.remote-job-link {
  display: inline-block;
  margin-top: 5px;
  font-size: 0.76rem;
  color: var(--main-color, #8ab4ff);
  text-decoration: none;
}
.server-status.bad { color: #ff8f8f; }
</style>
