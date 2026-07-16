<template>
  <div class="custom-bg-layer" :style="backgroundStyle"></div>
  <div class="custom-bg-overlay"></div>

  <div class="app-root">
    <fv-navigation-view
      :options="navOptions"
      :expand="true"
      expand-mode="relative"
      :expand-width="250"
      :compact-width="64"
      :show-search="false"
      :show-setting="true"
      :show-back="false"
      :full-size-display="500"
      theme="dark"
      :foreground="subColor"
      title="Phi-TKC"
      background="#00000000"
      :setting-title="t('setting')"
      class="nav-view"
      @item-click="onNavItemClick"
      @setting-click="goToSetting"
    >
      <template v-slot:navIcon>
        <div class="poslogo">
          <fv-img src="/phi-tklogo.png" alt="Phi TKC" class="tklogo" />
        </div>
      </template>
    </fv-navigation-view>

    <main class="main-content">
      <router-view v-slot="{ Component }">
        <Suspense>
          <template #default>
            <component :is="Component" ref="component" class="page-view" />
          </template>
          <template #fallback>
            <div class="loading-box">
              <fv-spinner size="large" />
            </div>
          </template>
        </Suspense>
      </router-view>
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, provide, onBeforeUnmount, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { convertFileSrc } from '@tauri-apps/api/core'
import { appDataDir } from '@tauri-apps/api/path'
import { readFile, writeFile, BaseDirectory } from '@tauri-apps/plugin-fs'
import { fetch } from '@tauri-apps/plugin-http'
import { Vibrant } from '@vibrant/core'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const selectedKey = ref(route.name as string)
const sliderIndex = ref(-1)

// ===== 主题颜色（提供全局） =====
const mainColor = ref('#ffffff')
const bgColor = ref('#222222')
const subColor = ref('#888888')
provide('themeColors', { mainColor, bgColor, subColor })

// ===== 背景相关 =====
const customBackground = ref<string | null>(null)
const currentBgBase64 = ref<string | null>(null)

// ===== NavigationView 导航项 =====
const navOptions = [
  { key: 'render', name: t('render'), icon: 'Play' },
  { key: 'rpe', name: t('rpe'), icon: 'Page' },
  { key: 'tasks', name: t('tasks'), icon: 'Work' },
  { key: 'batch-render', name: t('batch-render'), icon: 'Set' },
  { key: 'about', name: t('about'), icon: 'Info' },
]

watch(
  () => route.name,
  (newName) => {
    if (newName && typeof newName === 'string' && newName !== 'setting') {
      selectedKey.value = newName
    }
  },
  { immediate: true }
)

function updateNavigation(routeName: string) {
  if (routeName && typeof routeName === 'string' && routeName !== 'setting') {
    selectedKey.value = routeName
    const index = navOptions.findIndex(item => item.key === routeName)
    if (index !== -1) {
      sliderIndex.value = index
    }
  }
}

watch(
  () => route.name,
  (newName) => {
    updateNavigation(newName as string)
  },
  { immediate: true }
)

watch(
  [mainColor, bgColor, subColor],
  () => {
    updateCssVariables();
  },
  { immediate: true }
);

function onNavItemClick(item: { key: string }) {
  if (item.key === route.name) return
  router.push({ name: item.key })
}

function goToSetting() {
  if (route.name === 'setting') return
  router.push({ name: 'setting' })
}

// ---- 监听外部事件（来自 Setting） ----
window.addEventListener('customBackgroundChanged', ((e: CustomEvent) => {
  const detail = e.detail
  if (detail === null || detail === '') {
    customBackground.value = null
    localStorage.removeItem('customBackground')
  } else {
    customBackground.value = detail
    localStorage.setItem('customBackground', detail)
  }
  // 背景变化后重新提取颜色（但保留用户自定义颜色）
  nextTick(() => extractColorsFromBackground())
}) as EventListener)

// ---- 背景样式 ----
const backgroundImageUrl = ref<string>('') // 用于强制刷新

const backgroundStyle = computed(() => {
  if (customBackground.value) {
    try {
      const imgUrl = convertFileSrc(customBackground.value)
      return {
        backgroundImage: `url(${imgUrl})`,
        backgroundSize: 'cover',
        backgroundPosition: 'center',
        backgroundRepeat: 'no-repeat',
        backgroundAttachment: 'fixed',
      }
    } catch {
      return defaultBgStyle()
    }
  } else if (currentBgBase64.value) {
    return {
      backgroundImage: `url(${currentBgBase64.value})`,
      backgroundSize: 'cover',
      backgroundPosition: 'center',
      backgroundRepeat: 'no-repeat',
      backgroundAttachment: 'fixed',
    }
  } else {
    return defaultBgStyle()
  }
})

function defaultBgStyle() {
  return {
    backgroundImage: `url("https://api.yppp.net/api.php?t=${Date.now()}")`,
    backgroundSize: 'cover',
    backgroundPosition: 'center',
    backgroundRepeat: 'no-repeat',
    backgroundAttachment: 'fixed',
  }
}

// ---- 加载 API 背景 ----
async function loadApiBackground() {
  try {
    const res = await fetch('https://api.yppp.net/api.php')
    const blob = await res.blob()
    const reader = new FileReader()
    const base64 = await new Promise<string>((resolve) => {
      reader.onload = () => resolve(reader.result as string)
      reader.readAsDataURL(blob)
    })
    currentBgBase64.value = base64
    customBackground.value = null
    await extractColorsFromBase64(base64)
  } catch (err) {
    console.error('加载 API 背景失败', err)
  }
}

// ---- 加载自定义背景 ----
async function loadCustomBackground(path: string) {
  try {
    const data = await readFile(path)
    const base64 = `data:image/png;base64,${btoa(String.fromCharCode(...new Uint8Array(data)))}`
    currentBgBase64.value = base64
    customBackground.value = path
    await extractColorsFromBase64(base64)
  } catch (err) {
    console.error('加载自定义背景失败', err)
  }
}

// ---- 辅助：ArrayBuffer to base64 ----
function arrayBufferToBase64(buffer: ArrayBuffer): string {
  let binary = ''
  const bytes = new Uint8Array(buffer)
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i])
  }
  return window.btoa(binary)
}

// ---- 强制刷新 API 背景（清除自定义） ----
async function refreshApiBackground() {
  await loadApiBackground()
  localStorage.removeItem('customBackground')
  customBackground.value = null
  window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: null }))
}
window.refreshApiBackground = refreshApiBackground

// ---- 从当前背景重新提取颜色（供 Setting 重置使用） ----
async function extractColorsFromCurrentBackground() {
  if (!currentBgBase64.value) return;
  try {
    const v = new Vibrant(currentBgBase64.value, {});
    const palette = await v.getPalette();
    const colors = [
      palette.Vibrant?.hex,
      palette.Muted?.hex,
      palette.DarkVibrant?.hex
    ].filter(Boolean);
    if (colors.length >= 3) {
      // 强制提取后，只保存那些没有自定义 localStorage 项的颜色
      const mainStored = localStorage.getItem('mainColor');
      const bgStored = localStorage.getItem('bgColor');
      const subStored = localStorage.getItem('subColor');
      if (!mainStored) {
        mainColor.value = colors[0];
        localStorage.setItem('mainColor', colors[0]);
      }
      if (!bgStored) {
        bgColor.value = colors[1];
        localStorage.setItem('bgColor', colors[1]);
      }
      if (!subStored) {
        subColor.value = colors[2];
        localStorage.setItem('subColor', colors[2]);
      }
    }
  } catch (err) {
    console.warn('颜色提取失败', err);
  }
}
window.extractColorsFromCurrentBackground = extractColorsFromCurrentBackground;

// ---- 读取用户自定义颜色 ----
function loadCustomColors() {
  const m = localStorage.getItem('mainColor')
  const b = localStorage.getItem('bgColor')
  const s = localStorage.getItem('subColor')
  if (m) mainColor.value = m
  if (b) bgColor.value = b
  if (s) subColor.value = s
}

// ---- 颜色提取（使用 @vibrant/core），并保存到 localStorage（若未自定义） ----
async function extractColorsFromBase64(base64: string) {
  // 如果用户已经自定义了所有颜色，则不自动提取
  if (localStorage.getItem('mainColor') && localStorage.getItem('bgColor') && localStorage.getItem('subColor')) {
    return
  }
  try {
    const v = new Vibrant(base64, {})
    const palette = await v.getPalette()
    const colors = [
      palette.Vibrant?.hex,
      palette.Muted?.hex,
      palette.DarkVibrant?.hex
    ].filter(Boolean)
    if (colors.length >= 3) {
      // 仅对未自定义的颜色进行赋值并保存
      if (!localStorage.getItem('mainColor')) {
        mainColor.value = colors[0]
        localStorage.setItem('mainColor', colors[0])
      }
      if (!localStorage.getItem('bgColor')) {
        bgColor.value = colors[1]
        localStorage.setItem('bgColor', colors[1])
      }
      if (!localStorage.getItem('subColor')) {
        subColor.value = colors[2]
        localStorage.setItem('subColor', colors[2])
      }
    }
  } catch (err) {
    console.warn('颜色提取失败', err)
  }
}

// ---- 将颜色注入到全局 ----
provide('themeColors', {
  mainColor,
  bgColor,
  subColor,
})

window.themeColors = { mainColor, bgColor, subColor }

// ---- 固定背景（保存到本地文件） ----
window.fixCurrentBackground = async () => {
  if (!currentBgBase64.value || customBackground.value) return
  try {
    const base64Data = currentBgBase64.value.split(',')[1]
    const binary = atob(base64Data)
    const bytes = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
    const filePath = await appDataDir() + '/fixed_background.png'
    await writeFile(filePath, bytes, { dir: BaseDirectory.AppData })
    customBackground.value = filePath
    localStorage.setItem('customBackground', filePath)
    // 触发事件通知 Setting
    window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: filePath }))
  } catch (err) {
    console.error('固定背景失败', err)
  }
}

// ---- 新增：保存自定义背景（由 Setting 调用） ----
window.saveCustomBackground = async (path: string) => {
  try {
    // 直接加载自定义背景
    await loadCustomBackground(path)
    localStorage.setItem('customBackground', path)
    window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: path }))
  } catch (err) {
    console.error('保存自定义背景失败', err)
  }
}

// ---- CSS 变量更新 ----
function hexToRgb(hex: string): string {
  let clean = hex.replace(/^#/, '').trim();
  if (!clean) return '255, 255, 255';
  if (clean.length === 3) {
    clean = clean.split('').map(c => c + c).join('');
  }
  if (clean.length >= 6) {
    clean = clean.slice(0, 6);
  }
  const r = parseInt(clean.substring(0, 2), 16);
  const g = parseInt(clean.substring(2, 4), 16);
  const b = parseInt(clean.substring(4, 6), 16);
  if ([r, g, b].some(v => isNaN(v))) {
    return '255, 255, 255';
  }
  return `${r}, ${g}, ${b}`;
}

function updateCssVariables() {
  const root = document.documentElement;
  root.style.setProperty('--main-color', mainColor.value);
  root.style.setProperty('--bg-color', bgColor.value);
  root.style.setProperty('--sub-color', subColor.value);
  root.style.setProperty('--main-color-rgb', hexToRgb(mainColor.value));
  root.style.setProperty('--bg-color-rgb', hexToRgb(bgColor.value));
  root.style.setProperty('--sub-color-rgb', hexToRgb(subColor.value));
}

// ---- 暴露方法给全局 ----
window.loadApiBackground = loadApiBackground
window.loadCustomBackground = loadCustomBackground
// 注意：refreshApiBackground 和 fixCurrentBackground 已在上方赋值

// ===== 生命周期 =====
onMounted(async () => {
  document.addEventListener('contextmenu', (e) => e.preventDefault())
  const stored = localStorage.getItem('customBackground')
  if (stored && stored !== 'null') {
    await loadCustomBackground(stored)
  } else {
    await loadApiBackground()
  }

  // 恢复用户自定义颜色（若存在则覆盖提取的颜色）
  loadCustomColors()
  // 同步导航索引
  const currentKey = route.name as string
  if (currentKey) {
    const index = navOptions.findIndex(item => item.key === currentKey)
    if (index !== -1) sliderIndex.value = index
  }
})

onBeforeUnmount(() => {
  document.removeEventListener('contextmenu', (e) => e.preventDefault())
})
</script>


<style scoped>
/* ===== 全局布局 ===== */
.app-root {
  display: flex;
  width: 100%;
  height: 100vh;
  overflow: hidden;
}

/* ===== Logo 标题区域 ===== */
.poslogo {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  padding: 8px 16px;
  width: 100%;
}
.tklogo {
  transition: all 0.2s ease-in-out;
  width: 28px;
  height: auto;
}
/* ===== 主内容区 ===== */
.main-content {
  flex: 1;
  min-height: 100vh;
  padding: 0;
  overflow: hidden;
}
/* ===== 背景过渡动画 ===== */
.custom-bg-layer {
  transition: background-image 0.3s ease;
}
.page-view {
  width: 100%;
  min-height: 100vh;
  animation: pageEnter 0.35s ease both;
}
@keyframes pageEnter {
  from { opacity: 0; transform: translateY(12px); }
  to { opacity: 1; transform: translateY(0); }
}
.loading-box {
  width: 100%;
  height: 100vh;
  display: grid;
  place-items: center;
}

/* ===== 背景层 ===== */
.custom-bg-layer {
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: 0;
}
.custom-bg-overlay {
  position: fixed;
  inset: 0;
  background: radial-gradient(ellipse at center, transparent 0%, rgba(13, 13, 13, 0.4) 40%, rgba(13, 13, 13, 0.92) 100%);
  pointer-events: none;
  z-index: 0;
}

/* ===== 响应式 ===== */
@media (max-width: 600px) {
  .tklogo { width: 28px; }
  .poslogo { padding: -5px -5px; }
}
</style>
