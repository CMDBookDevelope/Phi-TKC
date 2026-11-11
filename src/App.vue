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
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { useTheme } from 'vuetify';
import { VSonner } from 'vuetify-sonner';

import { mainColor, bgColor, subColor, loadCustomColors } from './colorStore';
import { routeComponent } from './appState';
import { isAndroid, isMobile } from './platform';
import { API_BACKGROUND_URL, backgroundUrl, initializeBackground, refreshApiBackground, fixCurrentBackground } from './background';

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

const navItems = [
  { key: 'render', icon: 'mdi-play-circle-outline', activeIcon: 'mdi-play-circle' },
  { key: 'rpe', icon: 'mdi-book-open-page-variant-outline', activeIcon: 'mdi-book-open-page-variant' },
  { key: 'tasks', icon: 'mdi-server-network-outline', activeIcon: 'mdi-server-network' },
  { key: 'batch-render', icon: 'mdi-timeline-clock-outline', activeIcon: 'mdi-timeline-clock' },
  { key: 'about', icon: 'mdi-information-outline', activeIcon: 'mdi-information' },
];

// Bottom bar destinations. Batch rendering is desktop-only, so it does not earn
// one of the five thumb slots; settings takes the last one.
const mobileNavItems = [
  { key: 'render', icon: 'mdi-play-circle-outline', activeIcon: 'mdi-play-circle' },
  { key: 'rpe', icon: 'mdi-book-open-page-variant-outline', activeIcon: 'mdi-book-open-page-variant' },
  { key: 'tasks', icon: 'mdi-server-network-outline', activeIcon: 'mdi-server-network' },
  { key: 'about', icon: 'mdi-information-outline', activeIcon: 'mdi-information' },
  { key: 'setting', icon: 'mdi-cog-outline', activeIcon: 'mdi-cog' },
];

const navigateTo = (name: string) => {
  router.push({ name });
  // The permanent desktop rail must stay open; only the mobile overlay closes.
  if (mobile) navOpen.value = false;
};

// ===== Custom background =====
const backgroundStyle = computed(() => ({
  backgroundImage: `url("${backgroundUrl.value ?? API_BACKGROUND_URL}")`,
  backgroundSize: 'cover',
  backgroundPosition: 'center',
  backgroundRepeat: 'no-repeat',
  backgroundAttachment: 'fixed',
}));

window.refreshApiBackground = refreshApiBackground;
window.fixCurrentBackground = fixCurrentBackground;

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

  void initializeBackground();
});

// `immediate` so the stored colours are applied on first paint.
watch([mainColor, bgColor, subColor], applyThemeColors, { immediate: true });
</script>

<template>
  <v-app id="phi-tkc">
    <div class="custom-bg-layer" :style="backgroundStyle"></div>
    <div class="custom-bg-overlay"></div>

    <v-sonner position="top-center" />

    <v-app-bar
      v-if="mobile"
      class="glass fluent-glass tk-appbar"
      :style="android ? { top: '26px' } : undefined"
      density="compact"
      flat
    >
      <v-app-bar-title>Phi-TKC</v-app-bar-title>
    </v-app-bar>

    <v-navigation-drawer
      v-if="!mobile"
      v-model="navOpen"
      permanent
      class="glass fluent-glass tk-nav"
      width="250"
      :rail-width="64"
      rail
      expand-on-hover
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

    <v-main class="tk-main" :style="android ? { paddingTop: '74px' } : undefined">
      <router-view v-slot="{ Component }">
        <Suspense timeout="0">
          <template #default>
            <component :is="Component" ref="routeComponent" class="page-view" />
          </template>
          <template #fallback>
            <div class="loading-container">
              <v-progress-circular indeterminate size="48" color="primary" />
            </div>
          </template>
        </Suspense>
      </router-view>
    </v-main>

    <!-- Phones get a bottom bar: all primary destinations live in the thumb
         zone instead of a hamburger drawer at the top of the screen. -->
    <v-bottom-navigation
      v-if="mobile"
      :model-value="String(route.name)"
      class="glass fluent-glass tk-bottomnav"
      grow
      flat
    >
      <v-btn
        v-for="item in mobileNavItems"
        :key="item.key"
        :value="item.key"
        class="tk-bottomnav-btn"
        @click="navigateTo(item.key)"
      >
        <v-icon :icon="route.name === item.key ? item.activeIcon : item.icon" size="22" />
        <span>{{ t(item.key) }}</span>
      </v-btn>
    </v-bottom-navigation>
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

/* Bottom navigation: thumb-reachable primary destinations on phones. */
.tk-bottomnav {
  border-top: 1px solid rgba(var(--main-color-rgb), 0.14) !important;
  background: rgba(var(--bg-color-rgb), 0.55) !important;
  backdrop-filter: blur(24px);
  padding-bottom: env(safe-area-inset-bottom);
  z-index: 10;
}
.tk-bottomnav .v-btn {
  min-height: 58px;
  opacity: 0.66;
  transition: opacity 0.2s ease, color 0.2s ease;
}
.tk-bottomnav .v-btn.v-btn--active {
  opacity: 1;
}
/* The active pill is filled with the primary colour, so the icon and label must
   invert — otherwise they are primary-on-primary and invisible. */
.tk-bottomnav .v-btn--active .v-icon,
.tk-bottomnav .v-btn--active span {
  color: #0d0d0f !important;
}
.tk-bottomnav .v-btn span {
  font-size: 11px;
  font-weight: 600;
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
