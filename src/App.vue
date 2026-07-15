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
      mobileControlAcrylic="true"
      :mobile-control-acrylic="true"
      theme="dark"
      :fontSize="18"
      title="Phi-TKC"
      :background="'transparent'"
      :setting-title="t('setting')"
      class="fluent-glass nav-view"
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

function onNavItemClick(item: { key: string }) {
  if (item.key === route.name) return
  router.push({ name: item.key })
}

function goToSetting() {
  if (route.name === 'setting') return
  router.push({ name: 'setting' })
}

// 保存背景到 localStorage 并触发事件
function saveCustomBackground(path: string | null) {
  if (path) {
    localStorage.setItem('customBackground', path)
  } else {
    localStorage.removeItem('customBackground')
  }
  customBackground.value = path
  window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: path }))
}

// 监听外部事件（来自 Setting）
window.addEventListener('customBackgroundChanged', ((e: CustomEvent) => {
  const detail = e.detail
  if (detail === null || detail === '') {
    customBackground.value = null
    localStorage.removeItem('customBackground')
  } else {
    customBackground.value = detail
    localStorage.setItem('customBackground', detail)
  }
  // 背景变化后重新提取颜色
  nextTick(() => extractColorsFromBackground())
}) as EventListener)

// 背景样式
const backgroundImageUrl = ref<string>('') // 用于强制刷新

const backgroundStyle = computed(() => {
  // 优先使用自定义背景（本地文件）
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
    // 使用缓存的 base64（API 图片）
    return {
      backgroundImage: `url(${currentBgBase64.value})`,
      backgroundSize: 'cover',
      backgroundPosition: 'center',
      backgroundRepeat: 'no-repeat',
      backgroundAttachment: 'fixed',
    }
  } else {
    // fallback: 直接请求 API（但不会缓存，仅当缓存为空时）
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

// 加载 API 背景
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

// 加载自定义背景
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

// 辅助函数
function arrayBufferToBase64(buffer: ArrayBuffer): string {
  let binary = ''
  const bytes = new Uint8Array(buffer)
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i])
  }
  return window.btoa(binary)
}

// 强制刷新 API 背景（清除自定义）
async function refreshApiBackground() {
  await loadApiBackground()
  // 同时清除自定义背景存储
  localStorage.removeItem('customBackground')
  window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: null }))
}
// 暴露刷新方法给全局
window.refreshApiBackground = refreshApiBackground

// 读取用户自定义颜色
function loadCustomColors() {
  const m = localStorage.getItem('mainColor')
  const b = localStorage.getItem('bgColor')
  const s = localStorage.getItem('subColor')
  if (m) mainColor.value = m
  if (b) bgColor.value = b
  if (s) subColor.value = s
}

// 颜色提取（使用 @vibrant/core）
async function extractColorsFromBase64(base64: string) {
  // 如果用户已自定义颜色则跳过
  if (localStorage.getItem('mainColor') || localStorage.getItem('bgColor') || localStorage.getItem('subColor')) {
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
      mainColor.value = colors[0]
      bgColor.value = colors[1]
      subColor.value = colors[2]
    }
  } catch (err) {
    console.warn('颜色提取失败', err)
  }
}

// 将颜色注入到全局
provide('themeColors', {
  mainColor,
  bgColor,
  subColor,
})

// 也暴露到 window 方便其他组件
window.themeColors = { mainColor, bgColor, subColor }

// 固定背景（保存到本地）
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
  } catch (err) {
    console.error('固定背景失败', err)
  }
}

// 更新 CSS 变量的函数
function updateCssVariables() {
  const root = document.documentElement
  root.style.setProperty('--main-color', mainColor.value)
  root.style.setProperty('--bg-color', bgColor.value)
  root.style.setProperty('--sub-color', subColor.value)
}

// 暴露一些方法给全局（供 Setting 使用）
window.loadApiBackground = loadApiBackground
window.refreshApiBackground = refreshApiBackground
window.fixCurrentBackground = fixCurrentBackground
window.loadCustomBackground = loadCustomBackground

// ===== 生命周期 =====
onMounted(async () => {
  document.addEventListener('contextmenu', (e) => e.preventDefault())
	const stored = localStorage.getItem('customBackground')
	if (stored && stored !== 'null') {
	await loadCustomBackground(stored)
	} else {
	// 无自定义，加载 API 背景
	await loadApiBackground()
	}

	// 恢复用户自定义颜色
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
  background: transparent !important;
}

/* ===== NavigationView 容器 ===== */
.nav-view {
  flex-shrink: 0;
  --fv-navigation-background: transparent !important;
  color: #fff;
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
