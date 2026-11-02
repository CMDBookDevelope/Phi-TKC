import './assets/main.css';
import '@mdi/font/css/materialdesignicons.css';

import { createApp } from 'vue';

import { changeLocale } from './common';
import { i18n, SUPPORTED_LOCALES } from './i18n';
import { isMobile } from './platform';

import App from './App.vue';
import router from './router';

import { createVuetify } from 'vuetify';
import * as components from 'vuetify/components';
import * as directives from 'vuetify/directives';
import { aliases, mdi } from 'vuetify/iconsets/mdi';
import 'vuetify/styles';
// `vuetify-sonner` is a wrapper around vue-sonner and relies on its own stylesheet;
// without it toasts render unstyled / invisible.
import 'vuetify-sonner/style.css';

import theme from './theme';

function detectLocale(): string {
  const stored = localStorage.getItem('locale');
  if (stored) return stored;

  const languages = navigator.languages?.length ? navigator.languages : [navigator.language];
  for (const alt of languages) {
    if (!alt) continue;
    // zh-HK / zh-Hant etc. fall back to the Simplified Chinese message set.
    if (alt.toLowerCase().startsWith('zh')) return 'zh-CN';
    if (SUPPORTED_LOCALES.includes(alt)) return alt;
  }
  return 'en';
}

changeLocale(detectLocale());

const vuetify = createVuetify({
  components: { ...components },
  directives,
  theme: {
    defaultTheme: 'customTheme',
    themes: {
      customTheme: theme,
    },
  },
  icons: {
    defaultSet: 'mdi',
    aliases,
    sets: {
      mdi,
    },
  },
  defaults: {
    VCard: { rounded: 'lg', flat: true },
    VBtn: { rounded: 'xl' },
    VTextField: { variant: 'filled', rounded: 'lg', density: 'comfortable' },
    VSelect: { variant: 'filled', rounded: 'lg', density: 'comfortable' },
    VCombobox: { variant: 'filled', rounded: 'lg', density: 'comfortable' },
    VTextarea: { variant: 'filled', rounded: 'lg', density: 'comfortable' },
    VAutocomplete: { variant: 'filled', rounded: 'lg', density: 'comfortable' },
    VChip: { rounded: 'lg' },
    VDialog: { scrollable: true },
    VList: { rounded: 'lg' },
    VSlider: { color: 'primary' },
    VSwitch: { color: 'primary', inset: true },
    VCheckboxBtn: { color: 'primary' },
    VProgressLinear: { rounded: true },
    VProgressCircular: { color: 'primary' },
  },
});

const app = createApp(App);
app.use(i18n).use(router).use(vuetify);

// Registers the global v-glass directive: a mouse-following specular highlight.
// `.glass::after` in assets/main.css consumes the custom properties it writes.
app.directive('glass', {
  mounted(el: HTMLElement) {
    const handleMove = (e: MouseEvent) => {
      const rect = el.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const y = e.clientY - rect.top;
      el.style.setProperty('--highlight-x', `${x}px`);
      el.style.setProperty('--highlight-y', `${y}px`);
      el.style.setProperty('--highlight-opacity', '1');
    };
    const handleLeave = () => {
      el.style.setProperty('--highlight-opacity', '0');
    };

    el.addEventListener('mousemove', handleMove);
    el.addEventListener('mouseleave', handleLeave);

    // Stored on the element so `unmounted` can detach the exact handlers.
    (el as any).__glassHandlers = { handleMove, handleLeave };
  },
  unmounted(el: HTMLElement) {
    const handlers = (el as any).__glassHandlers;
    if (handlers) {
      el.removeEventListener('mousemove', handlers.handleMove);
      el.removeEventListener('mouseleave', handlers.handleLeave);
    }
  },
});

app.mount('#app');

// Phi-TKC keeps the browser's default context menu and devtools shortcuts disabled.
// The context-menu block doubles as long-press-callout suppression on touch devices.
document.addEventListener('contextmenu', (e) => e.preventDefault());

// Devtools shortcuts only exist on desktop; skipping this on mobile avoids fighting
// the on-screen keyboard for Ctrl/Meta chords.
if (!isMobile()) {
  document.addEventListener('keydown', (e) => {
    const ctrl = e.ctrlKey || e.metaKey;
    const shift = e.shiftKey;
    const key = e.key.toUpperCase();

    if (e.key === 'F12') e.preventDefault();
    if (ctrl && shift && (key === 'I' || key === 'J')) e.preventDefault();
    if (ctrl && (key === 'U' || key === 'R')) e.preventDefault();
  });
}
