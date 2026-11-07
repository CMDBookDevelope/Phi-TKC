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

// Registers the global v-glass directive: a pointer-following specular
// highlight. `.glass::after` in assets/main.css consumes the custom properties
// it writes.
//
// The highlight appears as soon as the pointer enters and, once the pointer
// leaves (mouse out / finger lifted), fades out over 1.5 s instead of snapping
// off. The fade is driven here rather than by a CSS transition so touch and
// mouse behave the same and re-entering can cancel it mid-flight.
const GLASS_FADE_IN = 200;
const GLASS_FADE_OUT = 1500;

app.directive('glass', {
  mounted(el: HTMLElement) {
    let raf = 0;
    let from = 0;
    let to = 0;
    let started = 0;
    let duration = 0;

    const read = () => parseFloat(el.style.getPropertyValue('--highlight-opacity')) || 0;
    const write = (v: number) => el.style.setProperty('--highlight-opacity', v.toFixed(3));

    const animate = (target: number, ms: number) => {
      if (raf) {
        cancelAnimationFrame(raf);
        raf = 0;
      }
      from = read();
      to = target;
      duration = ms;
      started = performance.now();
      if (Math.abs(to - from) < 0.001) {
        write(to);
        return;
      }
      const step = (now: number) => {
        const t = duration <= 0 ? 1 : Math.min((now - started) / duration, 1);
        const k = t * t * (3 - 2 * t); // smoothstep
        write(from + (to - from) * k);
        if (t < 1) {
          raf = requestAnimationFrame(step);
        } else {
          raf = 0;
          write(to);
        }
      };
      raf = requestAnimationFrame(step);
    };

    const handleMove = (e: PointerEvent) => {
      const rect = el.getBoundingClientRect();
      el.style.setProperty('--highlight-x', `${e.clientX - rect.left}px`);
      el.style.setProperty('--highlight-y', `${e.clientY - rect.top}px`);
      animate(1, GLASS_FADE_IN);
    };
    const handleLeave = () => animate(0, GLASS_FADE_OUT);

    // Pointer events cover the mouse and touch alike: on a touchscreen the
    // finger is implicitly captured, `pointerleave` arrives when it lifts.
    el.addEventListener('pointermove', handleMove);
    el.addEventListener('pointerdown', handleMove);
    el.addEventListener('pointerup', handleLeave);
    el.addEventListener('pointerleave', handleLeave);
    el.addEventListener('pointercancel', handleLeave);

    // Stored on the element so `unmounted` can detach the exact handlers.
    (el as any).__glassHandlers = { handleMove, handleLeave, cancel: () => raf && cancelAnimationFrame(raf) };
  },
  unmounted(el: HTMLElement) {
    const handlers = (el as any).__glassHandlers;
    if (handlers) {
      el.removeEventListener('pointermove', handlers.handleMove);
      el.removeEventListener('pointerdown', handlers.handleMove);
      el.removeEventListener('pointerup', handlers.handleLeave);
      el.removeEventListener('pointerleave', handlers.handleLeave);
      el.removeEventListener('pointercancel', handlers.handleLeave);
      handlers.cancel?.();
      delete (el as any).__glassHandlers;
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
