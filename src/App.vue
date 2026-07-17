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
      class="glass fluent-glass nav-view"
      v-glass
      showSlider="true"
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
import { appDataDir, join} from '@tauri-apps/api/path'
import { readFile, writeFile, BaseDirectory } from '@tauri-apps/plugin-fs'
import { fetch } from '@tauri-apps/plugin-http'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const selectedKey = ref(route.name as string)
const sliderIndex = ref(-1)

// ===== 主题颜色（提供全局） =====
import { mainColor, bgColor, subColor } from './stores/colorStore'

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

  // ---- 强制刷新 API 背景 ----
  async function refreshApiBackground() {
    // 先清空状态，避免缓存残留
    customBackground.value = null
    currentBgBase64.value = null
    localStorage.removeItem('customBackground')
    
    // 加载新的 API 背景
    await loadApiBackground()
    
    window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: null }))
    console.log('API 背景已刷新')
  }

  window.refreshApiBackground = refreshApiBackground

  // ---- 固定背景（保存到本地AppData目录，支持覆盖旧文件） ----
  async function fixCurrentBackground() {
    // 仅校验是否有当前背景，不再拦截已存在的自定义背景
    if (!currentBgBase64.value) {
      console.warn('暂无可固定的背景图片')
      return
    }

    try {
      // 1. 提取纯 base64 编码
      const [, pureBase64] = currentBgBase64.value.split(',')
      if (!pureBase64) throw new Error('当前背景不是有效的 base64 图片')

      // 2. base64 转 Uint8Array 二进制
      const binaryStr = atob(pureBase64)
      const byteArr = new Uint8Array(binaryStr.length)
      for (let i = 0; i < binaryStr.length; i++) {
        byteArr[i] = binaryStr.charCodeAt(i)
      }

      // 3. 跨平台拼接正确保存路径
      const fileName = 'fixed_background.png'
      const savePath = await join(await appDataDir(), fileName)

      // 4. 写入文件（默认覆盖同路径文件）
      await writeFile(savePath, byteArr)

      // 5. 更新全局状态、缓存、派发事件
      customBackground.value = savePath
      localStorage.setItem('customBackground', savePath)
      window.dispatchEvent(
        new CustomEvent('customBackgroundChanged', { detail: savePath })
      )

      console.log('背景固定成功，已覆盖旧文件：', savePath)
    } catch (err) {
      console.error('固定背景失败：', err)
    }
  }

  window.fixCurrentBackground = fixCurrentBackground

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
}) as EventListener)

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
    localStorage.setItem("BgBase64", base64)
    customBackground.value = null
  } catch (err) {
    console.error('加载 API 背景失败', err)
  }
}

// ---- 加载自定义背景（同步修复路径逻辑） ----
async function loadCustomBackground(path: string) {
  try {
    // path 是完整绝对路径，直接读取即可
    const data = await readFile(path)
    const base64 = `data:image/png;base64,${btoa(String.fromCharCode(...new Uint8Array(data)))}`
    currentBgBase64.value = base64
    localStorage.setItem("BgBase64", base64)
    customBackground.value = path
  } catch (err) {
    console.error('加载自定义背景失败', err)
  }
}

// ---- 读取用户自定义颜色 ----
function loadCustomColors() {
  const m = localStorage.getItem('mainColor')
  const b = localStorage.getItem('bgColor')
  const s = localStorage.getItem('subColor')
  if (m) mainColor.value = m
  if (b) bgColor.value = b
  if (s) subColor.value = s
}
provide('themeColors', { mainColor, bgColor, subColor })

// ---- 将颜色注入到全局 ----
provide('themeColors', {
  mainColor,
  bgColor,
  subColor,
})

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
  } else

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

.nav-view:deep(*) {
  backdrop-filter: none !important;
  box-shadow: none !important;
  border: none !important;
  color: var(--main-color) !important;
  font-size: 18px !important;
  font-weight: 600 !important;
}

/* ===== 响应式 ===== */
@media (max-width: 600px) {
  .tklogo { width: 28px; }
  .poslogo { padding: -5px -5px; }
}
</style>
