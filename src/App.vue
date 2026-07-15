<template>
  <!-- 全局Toast -->
  <fv-dialog v-model:open="toastOpen" modal persistent class="toast-wrap">
    <div class="toast-container" v-for="item in toastList" :key="item">
      <div class="toast-item" :class="item.type">
        <span class="toast-icon">
          <template v-if="item.type === 'success'">✅</template>
          <template v-if="item.type === 'error'">❌</template>
          <template v-if="item.type === 'warning'">⚠️</template>
          <template v-if="item.type === 'info'">ℹ️</template>
        </span>
        <span class="toast-text">{{ item.msg }}</span>
        <fv-button appearance="text" @click="dismissToast(item.id)">×</fv-button>
      </div>
    </div>
  </fv-dialog>

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
    :fontSize=18
    title=Phi-TKC
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
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { convertFileSrc } from '@tauri-apps/api/core'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const component = ref()
const selectedKey = ref(route.name as string)
const sliderIndex = ref(-1)
window.useOnLoaded = () => component

// ===== NavigationView 导航项（符合 VFluent3 文档规范） =====
// 数据格式参考 ListView 的 items 结构：
// { key: string, name: string, icon: string, type?: string, disabled?: boolean }
const navOptions = [
  { key: 'render', name: t('render'), icon: 'Play'},
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

// 统一更新函数
function updateNavigation(routeName: string) {
  if (routeName && typeof routeName === 'string' && routeName !== 'setting') {
    selectedKey.value = routeName
    const index = navOptions.findIndex(item => item.key === routeName)
    if (index !== -1) {
      sliderIndex.value = index
    }
  }
}

// 路由变化时同步
watch(
  () => route.name,
  (newName) => {
    updateNavigation(newName as string)
  },
  { immediate: true } // 立即执行一次，保证初始状态正确
)

// 点击导航项只跳转路由
function onNavItemClick(item: { key: string }) {
  if (item.key === route.name) return
  router.push({ name: item.key })
}

// 设置页按钮
function goToSetting() {
  if (route.name === 'setting') return
  router.push({ name: 'setting' })
}

// 路由变化时同步索引（保证浏览器前进后退也生效）
watch(
  () => route.name,
  (newName) => {
    if (newName && typeof newName === 'string' && newName !== 'setting') {
      selectedKey.value = newName
      const index = navOptions.findIndex(item => item.key === newName)
      if (index !== -1) sliderIndex.value = index
    }
  },
  { immediate: true }
)
// ===== 自定义背景 =====
const customBackground = ref<string | null>(null)
window.addEventListener('customBackgroundChanged', ((e: CustomEvent) => {
  customBackground.value = e.detail
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
  } else {
    return defaultBgStyle()
  }
})

function defaultBgStyle() {
  return {
    backgroundImage: `url("https://api.yppp.net/api.php")`,
    backgroundSize: 'cover',
    backgroundPosition: 'center',
    backgroundRepeat: 'no-repeat',
    backgroundAttachment: 'fixed',
  }
}

// ===== Toast =====
type ToastType = 'success' | 'info' | 'warning'
interface ToastItem {
  id: string
  msg: string
  type: ToastType
  timer?: ReturnType<typeof setTimeout>
}
const toastList = ref<ToastItem[]>([])
const toastOpen = computed({
  get() { return toastList.value.length > 0 },
  set(val) { if (!val) toastList.value = [] },
})
function genId() { return Math.random().toString(36).slice(2) }
window.$toast = (msg: string, type: ToastType = 'info') => {
  const id = genId()
  const item: ToastItem = { id, msg, type }
  item.timer = setTimeout(() => {
    toastList.value = toastList.value.filter(i => i.id !== id)
  }, 2000)
  toastList.value.push(item)
}
function dismissToast(id: string) {
  const target = toastList.value.find(i => i.id === id)
  if (target) { clearTimeout(target.timer); toastList.value = toastList.value.filter(i => i.id !== id) }
}

onMounted(() => {
  document.addEventListener('contextmenu', (e) => e.preventDefault())
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

/* ===== Toast ===== */
.toast-wrap {
  --fv-dialog-surface: transparent !important;
  position: fixed;
  top: 24px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 9999;
  pointer-events: none;
}
.toast-container {
  display: flex;
  flex-direction: column;
  gap: 8px;
  pointer-events: auto;
}
.toast-item {
  min-width: 320px;
  padding: 12px 16px;
  display: flex;
  align-items: center;
  gap: 10px;
  background: rgba(30, 30, 30, 0.95);
  backdrop-filter: blur(16px);
  border-radius: 12px;
  border-left: 4px #82b1ff;
}
.toast-item.success { border-left-color: #7dd87d; }
.toast-item.warning { border-left-color: #ffb86b; }
.toast-item.error { border-left-color: #ffb4ab; }
.toast-icon { font-size: 18px; }
.toast-text { flex: 1; font-size: 14px; }

/* ===== 响应式 ===== */
@media (max-width: 600px) {
  .tklogo { width: 28px; }
  .poslogo { padding: -5px -5px; }
}
</style>
