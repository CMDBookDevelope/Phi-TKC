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

  <!-- 自定义背景 -->
  <div class="custom-bg-layer" :style="backgroundStyle"></div>
  <div class="custom-bg-overlay"></div>
  
  <div class="app-root">
	  <aside class="sidebar fluent-glass">
	  	<div class="poslogo">
			<img src="/phi-tklogo.png" alt="Phi TK" class="tklogo" />
		</div>
			<div class="nav-list">
				<div class="nav-spacer"></div>
				<div class="nav-items-wrap">
					<div
					  v-for="item in navItems"
					  :key="item.key"
					  class="nav-item"
					  :class="{ selected: route.name === item.key }"
					  @click="navigateTo(item.key)"
					>
					  <fv-animated-icon 
						:modelValue="item.anim"
						fontSize="20" 
						theme=global
						:icon="route.name === item.key ? item.activeIcon : item.icon"
					  />
				  <span class="nav-label" style="margin-left: 6px;">{{ t(item.key) }}</span>
				</div>
				</div>
				<div class="nav-spacer"></div>
			</div>
	  </aside>

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
import { ref, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { convertFileSrc } from '@tauri-apps/api/core'
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const component = ref()

// 删掉 export，改为挂载window全局
function useOnLoaded() {
  return component
}
window.useOnLoaded = useOnLoaded

// 导航菜单图标使用VFluent内置图标名
const navItems = [
  { key: 'render', icon: 'Play', activeIcon: 'PlaySolid', anim: 'scaleDown' },
  { key: 'rpe', icon: 'Page', activeIcon: 'PageSolid', anim: 'scaleYDown' },
  { key: 'tasks', icon: 'Work', activeIcon: 'WorkSolid', anim: 'scaleXDown' },
  { key: 'batch-render', icon: 'Set', activeIcon: 'SetSolid', anim: 'scaleDown' },
  { key: 'setting', icon: 'Settings', activeIcon: 'SettingsSolid', anim: 'bounceRotate' },
  { key: 'about', icon: 'Info', activeIcon: 'InfoSolid', anim: 'bounceRotate' },
]
const navigateTo = (name: string) => router.push({ name })
window.goto = navigateTo

// 自定义背景路径（null 表示未设置）
const customBackground = ref<string | null>(null)
window.addEventListener('customBackgroundChanged', ((e: CustomEvent) => {
  customBackground.value = e.detail
}) as EventListener)

// 背景样式计算
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
      // 转换失败则回退至默认 API
      return defaultBgStyle()
    }
  } else {
    return defaultBgStyle()
  }
})

// 默认背景样式（使用 API）
function defaultBgStyle() {
  return {
    backgroundImage: `url("https://api.yppp.net/api.php")`,
    backgroundSize: 'cover',
    backgroundPosition: 'center',
    backgroundRepeat: 'no-repeat',
    backgroundAttachment: 'fixed',
  }
}

// 手动封装Toast状态（VFluent无useToastController）
type ToastType = 'success' | 'info' | 'warning'
interface ToastItem {
  id: string
  msg: string
  type: ToastType
  timer?: ReturnType<typeof setTimeout>
}
const toastList = ref<ToastItem[]>([])
const toastOpen = computed({
  get() {
    return toastList.value.length > 0
  },
  set(val) {
    // 关闭弹窗时清空所有toast
    if (!val) toastList.value = []
  }
})
function genId() {
  return Math.random().toString(36).slice(2)
}
// 全局挂载toast方法供common.ts调用
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
  if (target) {
    clearTimeout(target.timer)
    toastList.value = toastList.value.filter(i => i.id !== id)
  }
}
</script>

<style scoped>
/* 整体布局 */
.app-root {
  display: flex;
  width: 100%;
  height: 100vh;
  overflow: hidden;
  background: transparent !important; /* 确保根容器透明 */
}
.sidebar {
  width: 235px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 12px 0;
}
.brand-text {
  padding: 6px;
  font-size: 18px;
  font-weight: 700;
  color: #39c8bbff;
  text-align: center;
  margin-bottom: 16px;
}
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

/* ===== 侧边栏导航项（纯 div 实现） ===== */
.nav-list {
  display: flex;
  flex-direction: column;
  height: 100%;
  flex: 1;
  padding: 0 8px;
}

.nav-spacer {
  flex: 1; /* 上下各占 1/6，使中间区域占 2/3 */
}

.nav-items-wrap {
  flex: 2; /* 占 2/3 高度 */
  display: flex;
  flex-direction: column;
  justify-content: space-evenly;
  gap: 4px;
}

.nav-item {
  padding: 8px 16px;
  border-radius: 6px;
  cursor: pointer;
  position: relative;
  display: flex;
  align-items: center;
  transition: background 0.2s ease-in-out; /* 只对背景做过渡，与 .fluent-glass 保持一致 */
}

.nav-item:hover {
  background: rgba(255, 255, 255, 0.08);
}

.nav-item.selected {
  background: rgba(255, 255, 255, 0.12);
}

.nav-item.selected::before {
  content: '';
  position: absolute;
  left: 0;
  top: 6px;
  bottom: 6px;
  width: 3px;
  border-radius: 0 3px 3px 0;
  background: #39c8bb;
  opacity: 1;
  transition: opacity 0.2s ease-in-out; /* 与 .fluent-glass 一致 */
}

.nav-item::before {
  opacity: 0;
  transition: opacity 0.2s ease-in-out; /* 保持统一 */
}

/* PhiTKCLogo */
.tklogo{
	position: relative;
}
.poslogo{
	position: absolute;
	left: 50%;
	top: 14%;
	transform: translate(-50%, -50%);
}

/* ===== 自定义背景 ===== */
.custom-bg-layer {
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: 0;
}
.custom-bg-overlay {
  position: fixed;
  inset: 0;
  background: radial-gradient(ellipse at center, transparent 0%, rgba(13,13,13,0.4) 40%, rgba(13,13,13,0.92) 100%);
  pointer-events: none;
  z-index: 0;
}

/* ===== Toast 弹窗 ===== */
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
  background: rgba(30,30,30,0.95);
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
  .sidebar { width: 64px; }
}
</style>
