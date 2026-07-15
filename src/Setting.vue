<i18n>
en:
  settings:
    outputPath:
      label: "Output Path"
      placeholder: "Click to select folder"
      hint: "Prefer Browser FS API; use Electron for absolute path"
      aria: "Output path"
    selectFolder: "Select folder"
    save: "Save"
    copy: "Copy path"
    clear: "Clear"
    saved: "Saved"
    selected:
      picked: "Selected folder: {name}"
      fallback: "Fallback selection: {count} files (root: {root})"
    warning:
      empty: "Path must not be empty"
      select_error: "Failed to select folder: {msg}"
      copy_error: "Copy failed: please copy manually"
    background:
      label: "Custom Background"
      placeholder: "Click to select image"
      hint: "Supports JPG, PNG, WEBP format"
      selectFile: "Select image"
      clear: "Clear background"
      saved: "Background saved!"
      fixed: "Fix current background"
      refresh: "Refresh background"
      clearAndRefresh: "Clear custom & use API"
      noCustom: "No custom background to clear"
    colors:
      main: "Main Color"
      bg: "Background Color"
      sub: "Sub Color"
      custom: "Customize"
      auto: "Auto (from image)"
      reset: "Reset to auto"
      saved: "Color saved!"
zh-CN:
  settings:
    outputPath:
      label: "自定义输出路径"
      placeholder: "点击选择文件夹"
      hint: "请输入正确路径"
      aria: "自定义输出路径"
    selectFolder: "选择文件夹"
    save: "保存路径"
    copy: "复制路径"
    clear: "清除"
    saved: "保存成功！"
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
      fixed: "固定当前背景"
      refresh: "刷新背景"
      clearAndRefresh: "清除自定义并使用API"
      noCustom: "当前没有自定义背景"
    colors:
      main: "主色"
      bg: "背景色"
      sub: "辅色"
      custom: "自定义"
      auto: "自动 (来自图片)"
      reset: "重置为自动"
      saved: "颜色已保存！"
</i18n>

<script setup lang="ts">
import { ref, computed, inject, watch, onMounted, nextTick } from 'vue';
import { useI18n } from 'vue-i18n';
import { open } from '@tauri-apps/plugin-dialog';
import { appConfigDir, appDataDir, homeDir } from '@tauri-apps/api/path';
import { convertFileSrc } from '@tauri-apps/api/core';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { writeFile, BaseDirectory, readDir } from "@tauri-apps/plugin-fs";
import { fetch } from "@tauri-apps/plugin-http";

defineOptions({ name: 'SettingsPanel' });

const { t } = useI18n();

// ---- 主题颜色注入 ----
const themeColors = inject<{ mainColor: Ref<string>; bgColor: Ref<string>; subColor: Ref<string> }>('themeColors');
if (!themeColors) throw new Error('themeColors not provided');
const { mainColor, bgColor, subColor } = themeColors;

// 本地显示颜色
const localMain = ref(mainColor.value);
const localBg = ref(bgColor.value);
const localSub = ref(subColor.value);

watch([mainColor, bgColor, subColor], ([m, b, s]) => {
  localMain.value = m;
  localBg.value = b;
  localSub.value = s;
});

// 是否有自定义颜色（用于显示重置按钮）
const hasMainColor = computed(() => !!localStorage.getItem('mainColor'));
const hasBgColor = computed(() => !!localStorage.getItem('bgColor'));
const hasSubColor = computed(() => !!localStorage.getItem('subColor'));
const useCustomColors = computed(() => hasMainColor.value || hasBgColor.value || hasSubColor.value);

// ---- 颜色选择器弹出状态 ----
const mainColorPickerOpen = ref(false);
const bgColorPickerOpen = ref(false);
const subColorPickerOpen = ref(false);
const tempMainColor = ref(localMain.value);
const tempBgColor = ref(localBg.value);
const tempSubColor = ref(localSub.value);

// 颜色按钮的 ref
const mainColorBtn = ref<HTMLElement | null>(null);
const bgColorBtn = ref<HTMLElement | null>(null);
const subColorBtn = ref<HTMLElement | null>(null);

// ---- 颜色操作方法 ----
function openColorPicker(type: 'main' | 'bg' | 'sub') {
  if (type === 'main') {
    tempMainColor.value = localMain.value;
    mainColorPickerOpen.value = true;
  } else if (type === 'bg') {
    tempBgColor.value = localBg.value;
    bgColorPickerOpen.value = true;
  } else {
    tempSubColor.value = localSub.value;
    subColorPickerOpen.value = true;
  }
}

function confirmColor(type: 'main' | 'bg' | 'sub') {
  if (type === 'main') {
    saveColor('mainColor', tempMainColor.value);
    mainColorPickerOpen.value = false;
  } else if (type === 'bg') {
    saveColor('bgColor', tempBgColor.value);
    bgColorPickerOpen.value = false;
  } else {
    saveColor('subColor', tempSubColor.value);
    subColorPickerOpen.value = false;
  }
}

function saveColor(colorKey: string, value: string) {
  localStorage.setItem(colorKey, value);
  if (colorKey === 'mainColor') mainColor.value = value;
  else if (colorKey === 'bgColor') bgColor.value = value;
  else if (colorKey === 'subColor') subColor.value = value;
  window.dispatchEvent(new CustomEvent('themeColorChanged', { detail: { key: colorKey, value } }));
  colorSaved.value = true;
  setTimeout(() => (colorSaved.value = false), 1500);
}

function resetColor(colorKey: string) {
  localStorage.removeItem(colorKey);
  // 重新从当前背景提取颜色（需要 App 提供方法）
  if (window.extractColorsFromCurrentBackground) {
    window.extractColorsFromCurrentBackground();
  }
  colorSaved.value = true;
  setTimeout(() => (colorSaved.value = false), 1500);
}

function resetAllColors() {
  localStorage.removeItem('mainColor');
  localStorage.removeItem('bgColor');
  localStorage.removeItem('subColor');
  if (window.extractColorsFromCurrentBackground) {
    window.extractColorsFromCurrentBackground();
  }
  colorSaved.value = true;
  setTimeout(() => (colorSaved.value = false), 1500);
}

const colorSaved = ref(false);

// ---- 输出路径 ----
const outputPath = ref<string>(localStorage.getItem('outputPath') || '');
const saved = ref(false);
const warning = ref('');
const selectedInfo = ref<string | null>(null);

async function selectFolder() {
  warning.value = '';
  selectedInfo.value = null;
  try {
    const selected = await open({ directory: true, multiple: false, defaultPath: await appConfigDir() });
    if (selected === null) return;
    const path = Array.isArray(selected) ? selected[0] : selected;
    outputPath.value = path;
    const rootName = path.split(/[\\/]/).pop() || path;
    selectedInfo.value = t('settings.selected.picked', { name: rootName });
  } catch (err: any) {
    warning.value = t('settings.outputPath.warning.select_error', { msg: err.message || String(err) });
  }
}

function saveOutputPath() {
  if (!outputPath.value) { warning.value = t('settings.outputPath.warning.empty'); return; }
  localStorage.setItem('outputPath', outputPath.value);
  saved.value = true;
  setTimeout(() => (saved.value = false), 1500);
}

async function copyPath() {
  if (!outputPath.value) return;
  try { await writeText(outputPath.value); saved.value = true; setTimeout(() => (saved.value = false), 1500); }
  catch { warning.value = t('settings.outputPath.warning.copy_error'); }
}

function clearPath() {
  outputPath.value = '';
  selectedInfo.value = null;
  localStorage.removeItem('outputPath');
}

// ---- 背景操作 ----
const backgroundPath = ref<string>(localStorage.getItem('customBackground') || '');
const backgroundSaved = ref(false);

const backgroundPreviewUrl = computed(() => {
  if (backgroundPath.value) {
    try { return convertFileSrc(backgroundPath.value); } catch { return backgroundPath.value; }
  }
  return '';
});

async function selectBackground() {
  try {
    const selected = await open({ multiple: false, filters: [{ name: 'Image', extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp'] }], defaultPath: await appConfigDir() });
    if (selected === null) return;
    const path = Array.isArray(selected) ? selected[0] : selected;
    backgroundPath.value = path as string;
    if (window.saveCustomBackground) {
      window.saveCustomBackground(path as string);
    } else {
      localStorage.setItem('customBackground', path as string);
      window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: path }));
    }
    backgroundSaved.value = true;
    setTimeout(() => (backgroundSaved.value = false), 1500);
  } catch (err: any) { console.error('Failed to select background:', err); }
}

// 固定当前背景（调用 App 的方法）
async function fixCurrentBackground() {
  if (window.fixCurrentBackground) {
    await window.fixCurrentBackground();
    // 更新本地显示
    const stored = localStorage.getItem('customBackground');
    if (stored) {
      backgroundPath.value = stored;
    }
    backgroundSaved.value = true;
    setTimeout(() => (backgroundSaved.value = false), 1500);
  } else {
    console.warn('fixCurrentBackground not available');
  }
}

// 刷新背景（从API获取新图片）
function refreshBackground() {
  if (window.refreshApiBackground) {
    window.refreshApiBackground();
    backgroundPath.value = '';
    // 提示刷新成功（可通过状态显示）
    backgroundSaved.value = true;
    setTimeout(() => (backgroundSaved.value = false), 1500);
  } else {
    console.warn('refreshApiBackground not available');
  }
}

// 清除自定义背景并刷新API
function clearAndRefreshBackground() {
  if (backgroundPath.value) {
    // 清除自定义
    if (window.saveCustomBackground) {
      window.saveCustomBackground(null);
    } else {
      localStorage.removeItem('customBackground');
    }
    backgroundPath.value = '';
  }
  refreshBackground();
}

// ---- TreeView 文件选择（保留但简化，未改动） ----
const treeDialogOpen = ref(false);
const treeData = ref<any[]>([]);
const treeLoading = ref(false);
const treeError = ref('');

async function buildTree(path: string): Promise<any[]> {
  try {
    const entries = await readDir(path, { recursive: false });
    const children = [];
    for (const entry of entries) {
      let isDirectory = false;
      try {
        await readDir(path + '/' + entry.name, { recursive: false });
        isDirectory = true;
      } catch { /* ignore */ }
      if (isDirectory) {
        children.push({
          label: entry.name,
          path: path + '/' + entry.name,
          children: [],
          isLeaf: false,
        });
      }
    }
    return children;
  } catch (err: any) {
    throw new Error(`Failed to read ${path}: ${err.message}`);
  }
}

async function openTreeDialog() {
  treeDialogOpen.value = true;
  treeLoading.value = true;
  treeError.value = '';
  try {
    const root = await homeDir();
    const children = await buildTree(root);
    treeData.value = [{ label: 'Home', path: root, children, isLeaf: false }];
  } catch (err: any) {
    treeError.value = err.message;
  } finally {
    treeLoading.value = false;
  }
}

async function loadChildren(node: any) {
  if (node.children && node.children.length > 0) return;
  try {
    const children = await buildTree(node.path);
    node.children = children;
  } catch (err: any) {
    console.error(err);
  }
}

function selectTreeNode(node: any) {
  if (node.isLeaf) return;
  outputPath.value = node.path;
  const rootName = node.path.split(/[\\/]/).pop() || node.path;
  selectedInfo.value = t('settings.selected.picked', { name: rootName });
  treeDialogOpen.value = false;
}

function onTreeItemClick(item: any) {
  if (!item.isLeaf && (!item.children || item.children.length === 0)) {
    loadChildren(item);
  }
  selectTreeNode(item);
}

function onTreeExpand(item: any) {
  if (!item.isLeaf && (!item.children || item.children.length === 0)) {
    loadChildren(item);
  }
}

// ---- 监听输出路径变化 ----
watch(outputPath, (newPath) => {
  if (newPath && !selectedInfo.value) {
    const name = newPath.split(/[\\/]/).pop() || newPath;
    selectedInfo.value = t('settings.selected.picked', { name });
  }
});

// ---- 监听外部主题颜色变化 ----
onMounted(() => {
  window.addEventListener('themeColorChanged', ((e: CustomEvent) => {
    const { key, value } = e.detail;
    if (key === 'mainColor') localMain.value = value;
    else if (key === 'bgColor') localBg.value = value;
    else if (key === 'subColor') localSub.value = value;
  }) as EventListener);
});

// ---- 暴露全局方法（供App调用） ----
// 这些方法已在 App.vue 中实现，这里不需要额外暴露
</script>

<template>
  <div class="settings-container">
    <div class="settings-scroll">
      <!-- 输出路径卡片 -->
      <fv-card class="md3-card fluent-glass">
        <div class="card-label">{{ t('settings.outputPath.label') }}</div>
        <fv-breadcrumb
          v-if="outputPath"
          :items="outputPath.split(/[\\/]/).filter(Boolean).map((seg) => ({ text: seg, to: '#' }))"
          separator="/"
          class="path-breadcrumb"
        />
        <div v-else class="path-empty">{{ t('settings.outputPath.placeholder') }}</div>
        <div v-if="selectedInfo" class="hint-text">{{ selectedInfo }}</div>
        <div class="card-actions">
          <fv-button appearance="tonal" @click="selectFolder">
            <fv-icon name="folder-open" size="18" />
            <span>{{ t('settings.selectFolder') }}</span>
          </fv-button>
          <fv-button appearance="tonal" @click="openTreeDialog">
            <fv-icon name="tree-view" size="18" />
            <span>Tree</span>
          </fv-button>
          <fv-button appearance="filled" @click="saveOutputPath">
            <fv-icon name="save" size="18" />
            <span>{{ t('settings.save') }}</span>
          </fv-button>
          <fv-button appearance="text" @click="copyPath" :disabled="!outputPath" :title="t('settings.copy')">
            <fv-icon name="copy" size="18" />
          </fv-button>
          <fv-button appearance="text" @click="clearPath" :disabled="!outputPath" :title="t('settings.clear')">
            <fv-icon name="close" size="18" />
          </fv-button>
        </div>
        <fv-message-bar v-if="warning" type="warning" class="mt-2">
          {{ warning }}
        </fv-message-bar>
        <fv-message-bar v-if="saved" type="success" class="mt-2">
          {{ t('settings.saved') }}
        </fv-message-bar>
      </fv-card>

      <!-- 自定义背景卡片 -->
      <fv-card class="md3-card fluent-glass">
        <div class="card-label">{{ t('settings.background.label') }}</div>
        <div class="bg-preview" v-if="backgroundPreviewUrl">
          <img :src="backgroundPreviewUrl" alt="Background preview" class="preview-img" />
        </div>
        <div v-else class="bg-preview empty">No background</div>
        <div class="card-actions">
          <fv-button appearance="tonal" @click="selectBackground">
            <fv-icon name="image" size="18" />
            <span>{{ t('settings.background.selectFile') }}</span>
          </fv-button>
          <fv-button appearance="filled" @click="fixCurrentBackground">
            <fv-icon name="pin" size="18" />
            <span>{{ t('settings.background.fixed') }}</span>
          </fv-button>
          <fv-button appearance="tonal" @click="refreshBackground">
            <fv-icon name="refresh" size="18" />
            <span>{{ t('settings.background.refresh') }}</span>
          </fv-button>
          <fv-button appearance="text" @click="clearAndRefreshBackground" :disabled="!backgroundPath" :title="t('settings.background.clearAndRefresh')">
            <fv-icon name="delete" size="18" />
          </fv-button>
        </div>
        <fv-message-bar v-if="backgroundSaved" type="success" class="mt-2">
          {{ t('settings.background.saved') }}
        </fv-message-bar>
      </fv-card>

      <!-- 自定义主题色卡片 -->
      <fv-card class="md3-card fluent-glass">
        <div class="card-label">Theme Colors</div>
        <div class="color-row">
          <!-- 主色 -->
          <div class="color-item">
            <span class="color-label">{{ t('settings.colors.main') }}</span>
            <fv-button
              ref="mainColorBtn"
              class="color-swatch"
              :style="{ backgroundColor: localMain }"
              @click="openColorPicker('main')"
            >
              {{ localMain }}
            </fv-button>
            <fv-callout
              v-model:open="mainColorPickerOpen"
              :target="mainColorBtn"
              position="below"
              class="color-picker-callout"
            >
              <div class="picker-content">
                <fv-color-picker v-model="tempMainColor" />
                <div class="picker-actions">
                  <fv-button appearance="filled" @click="confirmColor('main')">确定</fv-button>
                  <fv-button appearance="text" @click="mainColorPickerOpen = false">取消</fv-button>
                </div>
              </div>
            </fv-callout>
            <fv-button
              appearance="text"
              @click="resetColor('mainColor')"
              :disabled="!hasMainColor"
            >
              {{ t('settings.colors.reset') }}
            </fv-button>
          </div>

          <!-- 背景色 -->
          <div class="color-item">
            <span class="color-label">{{ t('settings.colors.bg') }}</span>
            <fv-button
              ref="bgColorBtn"
              class="color-swatch"
              :style="{ backgroundColor: localBg }"
              @click="openColorPicker('bg')"
            >
              {{ localBg }}
            </fv-button>
            <fv-callout
              v-model:open="bgColorPickerOpen"
              :target="bgColorBtn"
              position="below"
              class="color-picker-callout"
            >
              <div class="picker-content">
                <fv-color-picker v-model="tempBgColor" />
                <div class="picker-actions">
                  <fv-button appearance="filled" @click="confirmColor('bg')">确定</fv-button>
                  <fv-button appearance="text" @click="bgColorPickerOpen = false">取消</fv-button>
                </div>
              </div>
            </fv-callout>
            <fv-button
              appearance="text"
              @click="resetColor('bgColor')"
              :disabled="!hasBgColor"
            >
              {{ t('settings.colors.reset') }}
            </fv-button>
          </div>

          <!-- 辅色 -->
          <div class="color-item">
            <span class="color-label">{{ t('settings.colors.sub') }}</span>
            <fv-button
              ref="subColorBtn"
              class="color-swatch"
              :style="{ backgroundColor: localSub }"
              @click="openColorPicker('sub')"
            >
              {{ localSub }}
            </fv-button>
            <fv-callout
              v-model:open="subColorPickerOpen"
              :target="subColorBtn"
              position="below"
              class="color-picker-callout"
            >
              <div class="picker-content">
                <fv-color-picker v-model="tempSubColor" />
                <div class="picker-actions">
                  <fv-button appearance="filled" @click="confirmColor('sub')">确定</fv-button>
                  <fv-button appearance="text" @click="subColorPickerOpen = false">取消</fv-button>
                </div>
              </div>
            </fv-callout>
            <fv-button
              appearance="text"
              @click="resetColor('subColor')"
              :disabled="!hasSubColor"
            >
              {{ t('settings.colors.reset') }}
            </fv-button>
          </div>
        </div>

        <div class="card-actions">
          <fv-button appearance="tonal" @click="resetAllColors" :disabled="!useCustomColors">
            {{ t('settings.colors.reset') }} All
          </fv-button>
        </div>
        <fv-message-bar v-if="colorSaved" type="success" class="mt-2">
          {{ t('settings.colors.saved') }}
        </fv-message-bar>
      </fv-card>
    </div>

    <!-- TreeView 对话框（保持不变） -->
    <fv-dialog v-model:open="treeDialogOpen" modal class="tree-dialog">
      <fv-card class="tree-card">
        <h3>Select Folder</h3>
        <fv-tree-view
          v-if="!treeLoading && !treeError"
          :items="treeData"
          @item-click="onTreeItemClick"
          @item-expand="onTreeExpand"
          expand-on-click
          class="tree-view"
        />
        <div v-if="treeLoading" class="loading-box"><fv-spinner size="large" /></div>
        <div v-if="treeError" class="error-box">
          <fv-message-bar type="warning">{{ treeError }}</fv-message-bar>
        </div>
        <div class="dialog-actions">
          <fv-button appearance="text" @click="treeDialogOpen = false">Cancel</fv-button>
        </div>
      </fv-card>
    </fv-dialog>
  </div>
</template>

<style scoped>
.settings-container {
  width: 100%;
  max-width: 900px;
  margin: 0 auto;
  padding: 24px;
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}

.settings-scroll {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.md3-card {
  background: rgba(30, 30, 30, 0.6);
  backdrop-filter: blur(16px);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 20px;
  padding: 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
}

.card-label {
  font-size: 14px;
  font-weight: 700;
  color: #fff;
  text-transform: uppercase;
  letter-spacing: 1px;
}

.card-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.hint-text {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.5);
}

.path-breadcrumb {
  background: rgba(255, 255, 255, 0.05);
  padding: 8px 12px;
  border-radius: 8px;
  color: #fff;
  font-size: 13px;
  word-break: break-all;
}

.path-empty {
  color: rgba(255, 255, 255, 0.3);
  font-style: italic;
}

.bg-preview {
  width: 100%;
  max-width: 480px;
  height: 160px;
  border-radius: 16px;
  overflow: hidden;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(0, 0, 0, 0.3);
}
.bg-preview.empty {
  display: flex;
  align-items: center;
  justify-content: center;
  color: rgba(255, 255, 255, 0.3);
}
.preview-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.color-row {
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.color-item {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}
.color-label {
  min-width: 100px;
  color: rgba(255, 255, 255, 0.7);
  font-weight: 500;
}
.color-swatch {
  min-width: 80px;
  padding: 4px 12px;
  border-radius: 8px;
  font-weight: 600;
  font-size: 14px;
  color: #fff;
  text-shadow: 0 1px 2px rgba(0,0,0,0.5);
}
.color-picker-callout {
  --fv-callout-background: rgba(30, 30, 30, 0.95);
  backdrop-filter: blur(20px);
  border-radius: 16px;
  padding: 16px;
}
.picker-content {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.picker-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.tree-dialog {
  --fv-dialog-surface: transparent;
}
.tree-card {
  background: rgba(30, 30, 30, 0.95);
  backdrop-filter: blur(20px);
  border-radius: 20px;
  padding: 24px;
  min-width: 400px;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.tree-view {
  flex: 1;
  overflow-y: auto;
  max-height: 50vh;
  color: #fff;
}
.loading-box, .error-box {
  display: flex;
  justify-content: center;
  padding: 20px;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

@media (max-width: 600px) {
  .settings-container { padding: 16px; }
  .md3-card { padding: 16px; }
  .color-item { flex-direction: column; align-items: stretch; gap: 8px; }
  .color-swatch { width: 100%; }
}
</style>
