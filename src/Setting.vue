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
      label: "Custom background"
      placeholder: "Click to select image file"
      hint: "Supports JPG, PNG, WEBP format"
      selectFile: "Select image"
      clear: "Clear background"
      saved: "Background saved!"
      fix: "Fix"
      refresh: "Refresh"
      fixed: "Background successfully fixed"
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
      fix: "固定背景"
      refresh: "刷新背景"
      fixed: "背景成功固定!"
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
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { open } from '@tauri-apps/plugin-dialog';
import { appConfigDir } from '@tauri-apps/api/path';
import { convertFileSrc } from '@tauri-apps/api/core';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

defineOptions({ name: 'SettingsPanel' });
import { RULES as rules, toast } from './common';
import { mainColor, bgColor, subColor } from './colorStore';
import ColorDot from './components/ColorDot.vue';

const { t } = useI18n();

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
    toast(t('settings.saved'), 'success');
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
  await window.fixCurrentBackground?.();
  const stored = localStorage.getItem('customBackground');
  if (stored) backgroundPath.value = stored;
  toast(t('settings.background.fixed'), 'success');
}

async function refreshBackground() {
  await window.refreshApiBackground?.();
  backgroundPath.value = '';
  toast(t('settings.background.saved'), 'success');
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
</script>

<template>
  <div class="settings-container">
    <div class="settings-scroll">
      <!-- Output Path -->
      <div class="md3-card glass fluent-glass" v-glass>
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
</style>
