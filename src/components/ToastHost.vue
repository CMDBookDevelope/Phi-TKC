<i18n>
en:
  close: Dismiss notification
zh-CN:
  close: 关闭提示
</i18n>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { dismissToast, toastMessages, type ToastKind } from '../toastStore';

withDefaults(defineProps<{ offset?: number }>(), { offset: 24 });
const { t } = useI18n();
const visibleToasts = computed(() => toastMessages.value.slice(0, 3));
// TransitionGroup keeps outgoing cards in the layout until their fade ends.
// Include those cards when sharing the screen height with new notifications.
const leavingCount = ref(0);
const layoutCount = computed(() => visibleToasts.value.length + leavingCount.value);
const READ_TIME = 4000;
const icons: Record<ToastKind, string> = {
  success: 'mdi-check-circle-outline',
  info: 'mdi-information-outline',
  warning: 'mdi-alert-outline',
  error: 'mdi-alert-circle-outline',
};

interface ToastTimer {
  remaining: number;
  started: number;
  timeout?: ReturnType<typeof setTimeout>;
  entered: boolean;
  hovered: boolean;
  focused: boolean;
}

const timers = new Map<number, ToastTimer>();
const toastId = (el: Element) => Number((el as HTMLElement).dataset.toastId);

function prepareToast(el: Element) {
  timers.set(toastId(el), {
    remaining: READ_TIME, started: 0, entered: false, hovered: false, focused: false,
  });
}

function pauseTimer(id: number) {
  const timer = timers.get(id);
  if (timer?.timeout !== undefined) {
    clearTimeout(timer.timeout);
    timer.timeout = undefined;
    timer.remaining = Math.max(0, timer.remaining - (performance.now() - timer.started));
  }
}

function resumeTimer(id: number) {
  const timer = timers.get(id);
  if (!timer || !timer.entered || timer.hovered || timer.focused || document.hidden || timer.timeout !== undefined) return;
  timer.started = performance.now();
  timer.timeout = setTimeout(() => {
    timer.timeout = undefined;
    dismissToast(id);
  }, timer.remaining);
}

function startReadTime(el: Element) {
  const id = toastId(el);
  const timer = timers.get(id);
  if (timer) timer.entered = true;
  resumeTimer(id);
}

function setInteraction(id: number, interaction: 'hovered' | 'focused', active: boolean) {
  const timer = timers.get(id);
  if (!timer) return;
  timer[interaction] = active;
  if (active) pauseTimer(id);
  else resumeTimer(id);
}

function handleFocusOut(id: number, event: FocusEvent) {
  if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) {
    setInteraction(id, 'focused', false);
  }
}

function removeToast(id: number) {
  pauseTimer(id);
  dismissToast(id);
}

function updateVisibility() {
  for (const id of timers.keys()) {
    if (document.hidden) pauseTimer(id);
    else resumeTimer(id);
  }
}

watch(visibleToasts, (entries) => {
  const visibleIds = new Set(entries.map((entry) => entry.id));
  for (const id of timers.keys()) {
    if (!visibleIds.has(id)) {
      pauseTimer(id);
      timers.delete(id);
    }
  }
});

onMounted(() => document.addEventListener('visibilitychange', updateVisibility));
onUnmounted(() => {
  document.removeEventListener('visibilitychange', updateVisibility);
  for (const id of timers.keys()) pauseTimer(id);
  timers.clear();
});
</script>

<template>
  <TransitionGroup
    tag="div"
    name="tk-toast"
    appear
    class="tk-toast-host"
    :style="{
      '--toast-offset': `${offset}px`,
      '--toast-count': Math.max(1, layoutCount),
      '--toast-gaps': `${Math.max(0, layoutCount - 1) * 10}px`,
    }"
    @before-enter="prepareToast"
    @after-enter="startReadTime"
    @before-leave="leavingCount++"
    @after-leave="leavingCount--"
  >
    <div
      v-for="entry in visibleToasts"
      :key="entry.id"
      :data-toast-id="entry.id"
      :data-kind="entry.kind"
      class="tk-toast blur-glass"
      :role="entry.kind === 'error' ? 'alert' : 'status'"
      :aria-live="entry.kind === 'error' ? 'assertive' : 'polite'"
      aria-atomic="true"
      @pointerenter="$event.pointerType === 'mouse' && setInteraction(entry.id, 'hovered', true)"
      @pointerleave="$event.pointerType === 'mouse' && setInteraction(entry.id, 'hovered', false)"
      @focusin="setInteraction(entry.id, 'focused', true)"
      @focusout="handleFocusOut(entry.id, $event)"
    >
      <i :class="['mdi', icons[entry.kind], 'tk-toast-icon']" aria-hidden="true"></i>
      <span class="tk-toast-message copyable">{{ entry.message }}</span>
      <button class="tk-toast-close" type="button" :aria-label="t('close')" @click="removeToast(entry.id)">×</button>
    </div>
  </TransitionGroup>
</template>

<style scoped>
.tk-toast-host {
  --toast-top: max(var(--toast-offset), env(safe-area-inset-top, 0px));
  position: fixed;
  top: var(--toast-top);
  right: max(16px, env(safe-area-inset-right, 0px));
  width: min(356px, calc(100vw - 32px - env(safe-area-inset-left, 0px) - env(safe-area-inset-right, 0px)));
  z-index: 10000;
  display: grid;
  gap: 10px;
  pointer-events: none;
}
.tk-toast {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 12px 14px 16px;
  min-width: 0;
  background: rgba(var(--bg-color-rgb), 0.72);
  color: var(--main-color);
  border: 1px solid rgba(var(--main-color-rgb), 0.25);
  border-radius: 16px;
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.25);
  backdrop-filter: blur(28px);
  -webkit-backdrop-filter: blur(28px);
  pointer-events: auto;
}
.tk-toast-icon {
  font-size: 22px;
  flex: none;
}
.tk-toast-message {
  flex: 1;
  min-width: 0;
  /* Share the available height so all three messages remain on a small screen. */
  max-height: min(240px, calc((100dvh - var(--toast-top) - max(16px, env(safe-area-inset-bottom, 0px)) - var(--toast-gaps)) / var(--toast-count) - 30px));
  overflow: auto;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
  font-size: 14px;
  line-height: 1.5;
}
.tk-toast-close {
  flex: none;
  width: 30px;
  height: 30px;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: inherit;
  font-size: 24px;
  line-height: 1;
  cursor: pointer;
}
.tk-toast-close:hover,
.tk-toast-close:focus-visible {
  background: rgba(var(--main-color-rgb), 0.14);
}
.tk-toast-enter-active,
.tk-toast-leave-active {
  transition: opacity 180ms ease, transform 180ms ease;
}
.tk-toast-enter-from,
.tk-toast-leave-to {
  opacity: 0;
  transform: translateY(-12px);
}
@media (pointer: coarse) {
  .tk-toast {
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
  }
}
@media (prefers-reduced-motion: reduce) {
  .tk-toast-enter-active,
  .tk-toast-leave-active {
    transition: none;
  }
}
</style>
