<i18n>
en:
  render: Render
  rpe: RPE
  tasks: Tasks
  about: About
  batch-render: Batch Render
  setting: Settings
  more: More

zh-CN:
  render: 渲染
  rpe: RPE
  tasks: 任务列表
  about: 关于
  batch-render: 批量渲染
  setting: 设置
  more: 更多
</i18n>

<script lang="ts">
declare global {
  interface Window {
    goto: (name: string) => void;
  }
}
</script>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, provide, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { convertFileSrc } from '@tauri-apps/api/core';
import { appDataDir, join } from '@tauri-apps/api/path';
import { readFile, writeFile, BaseDirectory } from '@tauri-apps/plugin-fs';
import { fetch } from '@tauri-apps/plugin-http';
import { useTheme } from 'vuetify';
import { VSonner } from 'vuetify-sonner';

import { mainColor, bgColor, subColor, loadCustomColors } from './colorStore';
import { routeComponent } from './appState';
import { isMobile } from './platform';
import MobileUnsupported from './components/MobileUnsupported.vue';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const vuetifyTheme = useTheme();

// Mobile (iOS/iPadOS/Android): the nav rail becomes a temporary overlay drawer
// opened from the app bar.
const mobile = isMobile();
const navOpen = ref(false);

// Routes whose features cannot work on mobile (see MOBILE.md) — the page is
// replaced with an explanation instead of failing at runtime.
const mobileUnsupportedRoutes = ['render', 'rpe', 'batch-render'];

const navItems = [
  { key: 'render', icon: 'mdi-play-circle-outline', activeIcon: 'mdi-play-circle' },
  { key: 'rpe', icon: 'mdi-book-open-page-variant-outline', activeIcon: 'mdi-book-open-page-variant' },
  { key: 'tasks', icon: 'mdi-server-network-outline', activeIcon: 'mdi-server-network' },
  { key: 'batch-render', icon: 'mdi-timeline-clock-outline', activeIcon: 'mdi-timeline-clock' },
  { key: 'about', icon: 'mdi-information-outline', activeIcon: 'mdi-information' },
];

const navigateTo = (name: string) => {
  router.push({ name });
  navOpen.value = false;
};

window.goto = navigateTo;

// ===== Custom background =====
// Three layers, in priority order: a user-picked image (`customBackground`), a
// snapshot written to AppData by "fix background", and the remote random API image.
const customBackground = ref<string | null>(null);
const currentBgBase64 = ref<string | null>(null);

function defaultBgStyle() {
  return {
    backgroundImage: `url("https://api.yppp.net/api.php?t=${Date.now()}")`,
    backgroundSize: 'cover',
    backgroundPosition: 'center',
    backgroundRepeat: 'no-repeat',
    backgroundAttachment: 'fixed',
  };
}

const backgroundStyle = computed(() => {
  if (customBackground.value) {
    try {
      const imgUrl = convertFileSrc(customBackground.value);
      return {
        backgroundImage: `url(${imgUrl})`,
        backgroundSize: 'cover',
        backgroundPosition: 'center',
        backgroundRepeat: 'no-repeat',
        backgroundAttachment: 'fixed',
      };
    } catch {
      return defaultBgStyle();
    }
  } else if (currentBgBase64.value) {
    return {
      backgroundImage: `url(${currentBgBase64.value})`,
      backgroundSize: 'cover',
      backgroundPosition: 'center',
      backgroundRepeat: 'no-repeat',
      backgroundAttachment: 'fixed',
    };
  } else {
    return defaultBgStyle();
  }
});

async function loadApiBackground() {
  try {
    const res = await fetch('https://api.yppp.net/api.php');
    const blob = await res.blob();
    const reader = new FileReader();
    const base64 = await new Promise<string>((resolve) => {
      reader.onload = () => resolve(reader.result as string);
      reader.readAsDataURL(blob);
    });
    currentBgBase64.value = base64;
    localStorage.setItem('BgBase64', base64);
    customBackground.value = null;
  } catch (err) {
    console.error('加载 API 背景失败', err);
  }
}

async function loadCustomBackground(path: string) {
  try {
    const data = await readFile(path);
    const base64 = `data:image/png;base64,${btoa(String.fromCharCode(...new Uint8Array(data)))}`;
    currentBgBase64.value = base64;
    localStorage.setItem('BgBase64', base64);
    customBackground.value = path;
  } catch (err) {
    console.error('加载自定义背景失败', err);
  }
}

async function refreshApiBackground() {
  customBackground.value = null;
  currentBgBase64.value = null;
  localStorage.removeItem('customBackground');

  await loadApiBackground();

  window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: null }));
  console.log('API 背景已刷新');
}

async function fixCurrentBackground() {
  if (!currentBgBase64.value) {
    console.warn('暂无可固定的背景图片');
    return;
  }

  try {
    const [, pureBase64] = currentBgBase64.value.split(',');
    if (!pureBase64) throw new Error('当前背景不是有效的 base64 图片');

    const binaryStr = atob(pureBase64);
    const byteArr = new Uint8Array(binaryStr.length);
    for (let i = 0; i < binaryStr.length; i++) {
      byteArr[i] = binaryStr.charCodeAt(i);
    }

    const fileName = 'fixed_background.png';
    await writeFile(fileName, byteArr, { baseDir: BaseDirectory.AppData });

    const savePath = await join(await appDataDir(), fileName);
    customBackground.value = savePath;
    localStorage.setItem('customBackground', savePath);
    window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: savePath }));

    console.log('背景固定成功，已覆盖旧文件：', savePath);
  } catch (err) {
    console.error('固定背景失败：', err);
  }
}

// Exposed for Setting.vue and the in-page background controls.
window.loadApiBackground = loadApiBackground;
window.loadCustomBackground = loadCustomBackground;
window.refreshApiBackground = refreshApiBackground;
window.fixCurrentBackground = fixCurrentBackground;

function onBackgroundChanged(e: Event) {
  const detail = (e as CustomEvent).detail;
  if (detail === null || detail === '') {
    customBackground.value = null;
    localStorage.removeItem('customBackground');
  } else {
    customBackground.value = detail;
    localStorage.setItem('customBackground', detail);
  }
}

// ===== Theme colours =====
provide('themeColors', { mainColor, bgColor, subColor });

function hexToRgb(hex: string): string {
  let clean = hex.replace(/^#/, '').trim();
  if (!clean) return '255, 255, 255';
  if (clean.length === 3) {
    clean = clean
      .split('')
      .map((c) => c + c)
      .join('');
  }
  if (clean.length >= 6) {
    clean = clean.slice(0, 6);
  }
  const r = parseInt(clean.substring(0, 2), 16);
  const g = parseInt(clean.substring(2, 4), 16);
  const b = parseInt(clean.substring(4, 6), 16);
  if ([r, g, b].some((v) => isNaN(v))) {
    return '255, 255, 255';
  }
  return `${r}, ${g}, ${b}`;
}

// Shifts a #rrggbb colour towards white (amount > 0) or black (amount < 0).
function shade(hex: string, amount: number): string {
  const [r, g, b] = hexToRgb(hex)
    .split(',')
    .map((v) => parseInt(v.trim(), 10));
  const mix = (c: number) => Math.round(amount >= 0 ? c + (255 - c) * amount : c * (1 + amount));
  return `#${[mix(r), mix(g), mix(b)].map((c) => Math.max(0, Math.min(255, c)).toString(16).padStart(2, '0')).join('')}`;
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

// Keep Vuetify's palette in step with the CSS variables so Vuetify components
// (buttons, switches, sliders, …) follow the user's custom colours too.
function updateVuetifyTheme() {
  const colors = vuetifyTheme.themes.value.customTheme.colors;
  colors.primary = mainColor.value;
  colors.secondary = subColor.value;
  colors.tertiary = subColor.value;
  colors.info = mainColor.value;
  colors.background = bgColor.value;
  colors.surface = shade(bgColor.value, 0.04);
  colors['surface-variant'] = shade(bgColor.value, 0.1);
  colors['surface-bright'] = shade(bgColor.value, 0.16);
  colors['surface-container'] = shade(bgColor.value, 0.07);
  colors['surface-container-lowest'] = shade(bgColor.value, -0.06);
  colors['surface-container-low'] = shade(bgColor.value, 0.03);
  colors['surface-container-high'] = shade(bgColor.value, 0.14);
  colors['surface-container-highest'] = shade(bgColor.value, 0.2);
}

function applyThemeColors() {
  updateCssVariables();
  updateVuetifyTheme();
}

// ===== Lifecycle =====
function blockContextMenu(e: Event) {
  e.preventDefault();
}

onMounted(async () => {
  document.addEventListener('contextmenu', blockContextMenu);

  loadCustomColors();

  const stored = localStorage.getItem('customBackground');
  if (stored && stored !== 'null') {
    await loadCustomBackground(stored);
  }

  const cachedBase64 = localStorage.getItem('BgBase64');
  if (cachedBase64 && !currentBgBase64.value) {
    currentBgBase64.value = cachedBase64;
  }

  window.addEventListener('customBackgroundChanged', onBackgroundChanged as EventListener);
});

onUnmounted(() => {
  document.removeEventListener('contextmenu', blockContextMenu);
  window.removeEventListener('customBackgroundChanged', onBackgroundChanged as EventListener);
});

// `immediate` so the stored colours are applied on first paint.
watch([mainColor, bgColor, subColor], applyThemeColors, { immediate: true });
</script>

<template>
  <v-app id="phi-tkc">
    <div class="custom-bg-layer" :style="backgroundStyle"></div>
    <div class="custom-bg-overlay"></div>

    <v-sonner position="top-center" />

    <v-app-bar v-if="mobile" class="glass fluent-glass tk-appbar" density="compact" flat>
      <v-app-bar-nav-icon @click="navOpen = !navOpen" />
      <v-app-bar-title>Phi-TKC</v-app-bar-title>
    </v-app-bar>

    <v-navigation-drawer
      v-model="navOpen"
      :permanent="!mobile"
      :temporary="mobile"
      class="glass fluent-glass tk-nav"
      width="250"
      :rail-width="64"
      :rail="!mobile"
      :expand-on-hover="!mobile"
    >
      <div class="nav-logo">
        <img src="/phi-tklogo.png" alt="Phi-TKC" class="tklogo" />
      </div>

      <v-list nav density="compact" class="nav-list">
        <v-list-item
          v-for="item in navItems"
          :key="item.key"
          :prepend-icon="route.name === item.key ? item.activeIcon : item.icon"
          :title="t(item.key)"
          :active="route.name === item.key"
          class="nav-item"
          @click="navigateTo(item.key)"
        />
      </v-list>

      <template #append>
        <v-list nav density="compact" class="nav-list">
          <v-list-item
            prepend-icon="mdi-cog-outline"
            :title="t('setting')"
            :active="route.name === 'setting'"
            class="nav-item"
            @click="navigateTo('setting')"
          />
        </v-list>
      </template>
    </v-navigation-drawer>

    <v-main class="tk-main">
      <router-view v-slot="{ Component }">
        <Suspense timeout="0">
          <template #default>
            <MobileUnsupported
              v-if="mobile && mobileUnsupportedRoutes.includes(String(route.name))"
              ref="routeComponent"
              class="page-view"
              :feature="t(String(route.name))"
            />
            <component v-else :is="Component" ref="routeComponent" class="page-view" />
          </template>
          <template #fallback>
            <div class="loading-container">
              <v-progress-circular indeterminate size="48" color="primary" />
            </div>
          </template>
        </Suspense>
      </router-view>
    </v-main>
  </v-app>
</template>

<style>
* {
  box-sizing: border-box;
}
</style>

<style scoped>
.custom-bg-layer {
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: 0;
  transition: background-image 0.3s ease;
}
.custom-bg-overlay {
  position: fixed;
  inset: 0;
  background: radial-gradient(ellipse at center, transparent 0%, rgba(13, 13, 13, 0.4) 40%, rgba(13, 13, 13, 0.92) 100%);
  pointer-events: none;
  z-index: 0;
}

/* ===== Navigation ===== */
.tk-nav {
  border-right: 1px solid rgba(var(--main-color-rgb), 0.12) !important;
  z-index: 10;
}

.nav-logo {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px 8px;
  min-height: 56px;
}
.tklogo {
  width: 28px;
  height: auto;
  transition: all 0.2s ease-in-out;
}

.nav-list {
  padding: 4px 8px;
}
.nav-item {
  border-radius: 12px;
  margin-bottom: 2px;
  color: rgba(var(--main-color-rgb), 0.65);
}
.nav-item:hover {
  background: rgba(var(--main-color-rgb), 0.1);
}
.nav-item.v-list-item--active {
  color: var(--main-color);
  background: rgba(var(--main-color-rgb), 0.16);
}

/* ===== Main content ===== */
.tk-main {
  background: transparent !important;
  position: relative;
  z-index: 1;
}

.page-view {
  width: 100%;
  min-height: 100vh;
  animation: pageEnter 0.35s cubic-bezier(0.2, 0, 0, 1) both;
}

@keyframes pageEnter {
  from {
    opacity: 0;
    transform: translateY(12px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.loading-container {
  display: flex;
  justify-content: center;
  align-items: center;
  min-height: 100vh;
}

/* ===== Mobile (iOS/iPadOS/Android) ===== */
.tk-appbar {
  border-bottom: 1px solid rgba(var(--main-color-rgb), 0.12) !important;
}

@media (pointer: coarse) {
  /* Keep content clear of the notch / home indicator. */
  .tk-main {
    padding-bottom: env(safe-area-inset-bottom);
  }
  .nav-logo {
    padding-top: max(12px, env(safe-area-inset-top));
  }
  .nav-item {
    min-height: 48px; /* 44px+ touch target */
  }
}
</style>
