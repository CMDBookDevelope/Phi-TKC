<i18n>
  en:
    settings:
      outputPath:
        warning:
          empty: "Path must not be empty"
        label: "Output Path"
        placeholder: "Click to select folder"
        hint: "Prefer Browser FS API; use Electron for absolute path"
        aria: "Output path"
      selectFolder: "Browse"
      save: "Save"
      saved: "Saved"
      selected:
        picked: "Selected folder: {name}"
        fallback: "Fallback selection: {count} files (root: {root})"
      background:
        label: "Custom Background"
        placeholder: "Click to select image"
        hint: "Supports JPG, PNG, WEBP format"
        selectFile: "Select"
        clear: "Clear background"
        saved: "Background Refreshed"
        fix: "Fix"
        refresh: "Refresh"
        fixed: "Background successfully fixed"
        noCustom: "No custom background to clear"
      colors:
        submit: "Apply"
        main: "Main Color"
        bg: "Background Color"
        sub: "Sub Color"
        custom: "Customize"
        saved: "Color saved!"
        label: "Theme Color"
  zh-CN:
    settings:
      outputPath:
        warning:
          empty: "不能选择棍母"
        label: "自定义输出路径"
        placeholder: "点击选择文件夹"
        hint: "请输入正确路径"
        aria: "自定义输出路径"
      selectFolder: "选择"
      save: "保存"
      copy: "复制路径"
      selected:
        picked: "已选择文件夹：{name}"
        fallback: "回退选择：{count} 个文件（根：{root})"
      background:
        label: "自定义背景"
        placeholder: "点击选择图片文件"
        hint: "支持 JPG、PNG、WEBP 格式"
        selectFile: "选择图片"
        clear: "清除背景"
        saved: "背景已刷新!"
        fix: "固定背景"
        refresh: "刷新背景"
        fixed: "背景成功固定!"
        noCustom: "当前没有自定义背景"
      colors:
        submit: "应用"
        main: "主色"
        bg: "背景色"
        sub: "辅色"
        custom: "自定义"
        saved: "颜色已保存！"
        label: "主题色"
  </i18n>
  
  <template>
    <div class="settings-container">
      <div class="settings-scroll">
        <!-- 输出路径卡片 -->
        <fv-card class="md3-card glass fluent-glass" v-glass>
          <div class="card-label">{{ t('settings.outputPath.label') }}</div>
          <div v-if="outputPath"></div>
          <div v-else class="path-empty">{{ t('settings.outputPath.placeholder') }}</div>
          <div v-if="selectedInfo" class="hint-text">{{ selectedInfo }}</div>
          <div class="card-actions">
            <fv-button icon="FolderOpen" :background="mainColor" @click="selectFolder">
              <span>{{ t('settings.selectFolder') }}</span>
            </fv-button>
            <fv-button icon="Save" :background="mainColor" @click="saveOutputPath">
              <span>{{ t('settings.save') }}</span>
            </fv-button>
          </div>
        </fv-card>
  
        <!-- 自定义背景卡片 -->
        <fv-card class="glass md3-card fluent-glass" v-glass>
          <div class="card-label">{{ t('settings.background.label') }}</div>
          <div class="bg-preview" v-if="backgroundPreviewUrl">
            <img :src="backgroundPreviewUrl" alt="Background preview" class="preview-img" />
          </div>
          <div v-else class="blur-glass bg-preview empty">未选择背景</div>
          <div class="card-actions">
            <fv-button icon="CalculatorAddition" :background="mainColor" @click="selectBackground">
              <span>{{ t('settings.background.selectFile') }}</span>
            </fv-button>
            <fv-button icon="Pin" :background="mainColor" @click="fixCurrentBackground">
              <span>{{ t('settings.background.fix') }}</span>
            </fv-button>
            <fv-button icon="Refresh" :background="mainColor" @click="refreshBackground">
              <span>{{ t('settings.background.refresh') }}</span>
            </fv-button>
          </div>
        </fv-card>
  
        <!-- 自定义主题色卡片 -->
        <fv-card class="md3-card fluent-glass glass" v-glass>
          <div class="card-label">{{ t('settings.colors.label') }}</div>
          <div class="color-row">
            <!-- 主色 -->
            <div class="color-item">
              <div class="color-prom"><a>{{ t('settings.colors.main') }}</a></div>
              <fv-callout
                border-radius="16px"
                :callout-bg="tempMainColor"
                position="rightTop"
              >
              <ColorSwatch :color="tempMainColor" :size="28" />
                <template v-slot:main>
                  <div class="picker-content">
                    <fv-color-picker hideFields v-model="tempMainColor"/>
                  </div>
                </template>
              </fv-callout>
              <fv-button icon="CheckMark" :background="mainColor" @click="confirmColor('main')">{{ t('settings.colors.submit') }}</fv-button>
            </div>
  
            <!-- 背景色 -->
            <div class="color-item">
              <div class="color-prom"><a>{{ t('settings.colors.bg') }}</a></div>
              <fv-callout
                border-radius="16px"
                :callout-bg="tempBgColor"
                position="rightTop"
              >
              <ColorSwatch :color="tempBgColor":size="28" />
                <template v-slot:main>
                  <div class="picker-content">
                    <fv-color-picker hideFields v-model="tempBgColor" />
                  </div>
                </template>
              </fv-callout>
              <fv-button icon="CheckMark" :background="mainColor" @click="confirmColor('bg')">{{ t('settings.colors.submit') }}</fv-button>
            </div>
  
            <!-- 辅色 -->
            <div class="color-item">
              <div class="color-prom"><a>{{ t('settings.colors.sub') }}</a></div>
              <fv-callout
                position="rightTop"
                border-radius="16px"
                :callout-bg="tempSubColor"
              >
              <ColorSwatch :color="tempSubColor" :size="28" />
                <template v-slot:main>
                  <div class="picker-content">
                    <fv-color-picker hideFields v-model="tempSubColor" />
                  </div>
                </template>
              </fv-callout>
              <fv-button icon="CheckMark" :background="mainColor" @click="confirmColor('sub')">{{ t('settings.colors.submit') }}</fv-button>
            </div>
          </div>
        </fv-card>
      </div>
    </div>
  </template>
  
  <script setup lang="ts">
  import { ref, computed, inject, watch, onMounted, nextTick } from 'vue';
  import { useI18n } from 'vue-i18n';
  import { open } from '@tauri-apps/plugin-dialog';
  import { appConfigDir, appDataDir } from '@tauri-apps/api/path';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { mainColor, bgColor, subColor } from './colorStore'

  defineOptions({ name: 'SettingsPanel' });
  
  const { t } = useI18n();

  // ---- 颜色选择器弹出状态 ----
  const tempMainColor = ref(mainColor.value);
  const tempBgColor = ref(bgColor.value);
  const tempSubColor = ref(subColor.value);

  function confirmColor(type: 'main' | 'bg' | 'sub') {
      if (type === 'main') {
        saveColor('mainColor', tempMainColor.value);
      } else if (type === 'bg') {
        saveColor('bgColor', tempBgColor.value);
      } else {
        saveColor('subColor', tempSubColor.value);
      }
    }
  function saveColor(colorKey: string, value: string) {
    localStorage.setItem(colorKey, value);
    if (colorKey === 'mainColor') mainColor.value = value;
    else if (colorKey === 'bgColor') bgColor.value = value;
    else if (colorKey === 'subColor') subColor.value = value;
    window.dispatchEvent(new CustomEvent('themeColorChanged', { detail: { key: colorKey, value } }));
    showToast(t('settings.colors.saved'), 'success');
  }

  // ---- 输出路径 ----
  const outputPath = ref<string>(localStorage.getItem('outputPath') || '');
  const selectedInfo = ref<string | null>(null);
  
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
      showToast(t('settings.outputPath.warning.select_error', { msg: err.message || String(err) }), 'error');
    }
  }
  
  function saveOutputPath() {
    if (!outputPath.value) {
      showToast(t('settings.outputPath.warning.empty'), 'warning');
      return;
    }
    localStorage.setItem('outputPath', outputPath.value);
    showToast(t('settings.saved'), 'success');
  }
  
  // ---- 背景操作 ----
  const backgroundPath = ref<string>(localStorage.getItem('customBackground') || '');
  
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
      showToast(t('settings.background.saved'), 'success');
    } catch (err: any) {
      console.error('Failed to select background:', err);
      showToast('Failed to select background', 'error');
    }
  }
  
  async function fixCurrentBackground() {
    if (window.fixCurrentBackground) {
      await window.fixCurrentBackground();
      const stored = localStorage.getItem('customBackground');
      if (stored) {
        backgroundPath.value = stored;
      }
      showToast(t('settings.background.fixed'), 'success');
    } else {
      showToast('fixCurrentBackground not available', 'warning');
    }
  }

  async function refreshBackground() {
    if (window.refreshApiBackground) {
      await window.refreshApiBackground();
      backgroundPath.value = '';
      showToast(t('settings.background.saved'), 'success');
    } else {
      showToast('refreshApiBackground not available', 'warning');
    }
  }
  
  // ---- 监听输出路径变化 ----
  watch(outputPath, (newPath) => {
    if (newPath && !selectedInfo.value) {
      const name = newPath.split(/[\\/]/).pop() || newPath;
      selectedInfo.value = t('settings.selected.picked', { name });
    }
  });
  </script>
  
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
    position: relative;
  }
  
  .settings-scroll {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }
  
  .md3-card {
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
  }
  
  .card-label {
    color: var(--main-color);
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 1px;
  }
  
  .card-actions {
    color: var(--bg-color);
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  
  .hint-text {
    font-weight: 600;
    color: var(--sub-color);
  }
  
  .path-empty {
    color: var(--sub-color);
    font-style: italic;
  }
  
  .bg-preview {
    width: 100%;
    max-width: 480px;
    height: 160px;
    border-radius: 16px;
    overflow: hidden;
  }
  .bg-preview.empty {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: large;
    color: rgba(var(--main-color-rgb), 0.3);
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
    color: var(--bg-color);
    font-size: 13.3px;
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }
  .color-prom {
    color: var(--main-color)
  }
  .color-label {
    min-width: 100px;
    color: var(--main-color);
    font-weight: 500;
  }
  .color-picker-callout {
    backdrop-filter: blur(20px);
    border-radius: 16px;
    padding: 16px;
    z-index: 1000;
  }
  .picker-content {
    display: flex;
    flex-direction: column;
  }
  .picker-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  
  @keyframes slideIn {
    from {
      opacity: 0;
      transform: translateX(20px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }
  
  @media (max-width: 600px) {
    .settings-container { padding: 16px; }
    .md3-card { padding: 16px; }
    .color-item { flex-direction: column; align-items: stretch; gap: 8px; }
  }

</style>