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

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
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
import { isAndroid, isMobile } from './platform';
import MobileUnsupported from './components/MobileUnsupported.vue';

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const vuetifyTheme = useTheme();

// Mobile (iOS/iPadOS/Android): the nav rail becomes a temporary overlay drawer
// opened from the app bar.
const mobile = isMobile();
// Android ships the NDK MediaCodec renderer, so its render routes stay enabled.
const android = isAndroid();
// The desktop rail is permanent, but Vuetify still honours `v-model`; starting closed
// left the desktop app with no navigation at all (the app bar is mobile-only).
const navOpen = ref(!mobile);

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
  // The permanent desktop rail must stay open; only the mobile overlay closes.
  if (mobile) navOpen.value = false;
};

// ===== Custom background =====
// Three layers, in priority order: a user-picked image (`customBackground`), a
// snapshot written to AppData by "fix background", and the remote random API image.
const customBackground = ref<string | null>(null);
const currentBgBase64 = ref<string | null>(null);

const API_BACKGROUND_URL = 'https://api.yppp.net/api.php';

function defaultBgStyle() {
  return {
    backgroundImage: `url("${API_BACKGROUND_URL}?t=${Date.now()}")`,
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

async function loadApiBackground(): Promise<boolean> {
  try {
    const res = await fetch(API_BACKGROUND_URL);
    const blob = await res.blob();
    const reader = new FileReader();
    const base64 = await new Promise<string>((resolve, reject) => {
      reader.onload = () => resolve(reader.result as string);
      reader.onerror = () => reject(reader.error ?? new Error('Failed to read background image'));
      reader.readAsDataURL(blob);
    });
    currentBgBase64.value = base64;
    try {
      localStorage.setItem('BgBase64', base64);
    } catch (err) {
      // A random API image can be several MB; localStorage has a ~5 MB quota.
      console.warn('背景缓存写入失败（图片过大）', err);
    }
    customBackground.value = null;
    return true;
  } catch (err) {
    console.error('加载 API 背景失败', err);
    return false;
  }
}

function loadCustomBackground(path: string) {
  // The image is displayed through convertFileSrc(); no need to read it into memory.
  customBackground.value = path;
}

function decodeBase64(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function detectImageExtension(bytes: Uint8Array): string {
  if (bytes.length >= 8 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47) return 'png';
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return 'jpg';
  if (bytes.length >= 3 && bytes[0] === 0x47 && bytes[1] === 0x49 && bytes[2] === 0x46) return 'gif';
  if (bytes.length >= 12 && bytes[0] === 0x52 && bytes[1] === 0x49 && bytes[2] === 0x46 && bytes[3] === 0x46 && bytes[8] === 0x57 && bytes[9] === 0x45 && bytes[10] === 0x42 && bytes[11] === 0x50)
    return 'webp';
  if (bytes.length >= 2 && bytes[0] === 0x42 && bytes[1] === 0x4d) return 'bmp';
  // Tauri's asset protocol derives the MIME type from the extension; PNG is the
  // safest fallback for unknown data.
  return 'png';
}

async function refreshApiBackground(): Promise<boolean> {
  customBackground.value = null;
  currentBgBase64.value = null;
  localStorage.removeItem('customBackground');
  localStorage.removeItem('BgBase64');

  const ok = await loadApiBackground();

  window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: null }));
  return ok;
}

async function fixCurrentBackground(): Promise<boolean> {
  try {
    let bytes: Uint8Array | null = null;
    if (customBackground.value) {
      bytes = new Uint8Array(await readFile(customBackground.value));
    } else if (currentBgBase64.value) {
      const [, pureBase64] = currentBgBase64.value.split(',');
      if (pureBase64) bytes = decodeBase64(pureBase64);
    }
    if (!bytes?.length) {
      console.warn('暂无可固定的背景图片');
      return false;
    }

    // Keep the real format: the asset protocol serves the file by extension, and the
    // random API image is often a JPEG even though the old code always wrote `.png`.
    const fileName = `fixed_background.${detectImageExtension(bytes)}`;
    await writeFile(fileName, bytes, { baseDir: BaseDirectory.AppData });

    const savePath = await join(await appDataDir(), fileName);
    customBackground.value = savePath;
    localStorage.setItem('customBackground', savePath);
    window.dispatchEvent(new CustomEvent('customBackgroundChanged', { detail: savePath }));

    console.log('背景固定成功：', savePath);
    return true;
  } catch (err) {
    console.error('固定背景失败：', err);
    return false;
  }
}

// Exposed for Setting.vue's background controls.
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
onMounted(() => {
  loadCustomColors();

  const stored = localStorage.getItem('customBackground');
  if (stored && stored !== 'null') {
    loadCustomBackground(stored);
  }

  const cachedBase64 = localStorage.getItem('BgBase64');
  if (cachedBase64) {
    currentBgBase64.value = cachedBase64;
  }

  // First run (or after clearing the cache): fetch the random API background through
  // the HTTP plugin and cache it, instead of relying on a remote CSS URL that the CSP
  // has to allow. `defaultBgStyle()` remains the fallback when the request fails.
  if (!customBackground.value && !currentBgBase64.value) {
    void loadApiBackground();
  }

  window.addEventListener('customBackgroundChanged', onBackgroundChanged as EventListener);
});

onUnmounted(() => {
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
              v-if="mobile && !android && mobileUnsupportedRoutes.includes(String(route.name))"
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
