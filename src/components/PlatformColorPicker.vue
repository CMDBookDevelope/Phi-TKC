<script setup lang="ts">
import { isApple } from '@/platform';
import ColorDot from './ColorDot.vue';

defineProps<{ label: string; active?: boolean }>();
const color = defineModel<string>({ required: true });
const emit = defineEmits<{ activate: [] }>();
const native = isApple();

function updateColor(value: unknown) {
  if (typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value) && value !== color.value) color.value = value;
}

function updateNativeColor(event: Event) {
  updateColor((event.target as HTMLInputElement).value);
}
</script>

<template>
  <div class="platform-color-picker">
    <input v-if="native" type="color" class="color-input" :class="{ 'is-active': active }"
      :value="color" :aria-label="label" @click="emit('activate')"
      @input="updateNativeColor" @change="updateNativeColor" />
    <v-menu v-else :close-on-content-click="false" location="bottom">
      <template #activator="{ props }">
        <button v-bind="props" type="button" class="swatch-btn" :aria-label="label" @click="emit('activate')">
          <ColorDot :color="color" :size="28" :active="active" />
        </button>
      </template>
      <div class="color-picker-panel fluent-glass glass" v-glass>
        <v-color-picker :model-value="color" mode="hex" :modes="['hex']" hide-inputs @update:model-value="updateColor" />
      </div>
    </v-menu>
  </div>
</template>

<style scoped>
.platform-color-picker {
  display: inline-flex;
  flex: none;
  align-items: center;
}
.color-input {
  width: 52px;
  height: 36px;
  border: 1px solid rgba(var(--main-color-rgb), 0.3);
  border-radius: 6px;
  background: transparent;
  padding: 2px;
  cursor: pointer;
}
.color-input.is-active { border-color: var(--main-color); }
.swatch-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 36px;
  min-height: 36px;
  border: 0;
  padding: 0;
  background: transparent;
  border-radius: 50%;
  cursor: pointer;
  transition: transform 0.12s ease;
}
.swatch-btn:hover { transform: scale(1.08); }
.color-input:focus-visible,
.swatch-btn:focus-visible {
  outline: 2px solid var(--main-color);
  outline-offset: 3px;
}
@media (pointer: coarse) {
  .color-input, .swatch-btn { min-height: 44px; }
  .swatch-btn { min-width: 44px; }
}
</style>
