<script setup lang="ts">
import TooltipIcon from './TooltipIcon.vue';

defineProps<{ tooltip: string; modelValue?: string }>();
defineEmits<{ 'update:modelValue': [value: string] }>();
</script>

<template>
  <div class="tip-text-field">
    <input
      class="blur-glass md3-field"
      :value="modelValue"
      @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
    <TooltipIcon :tooltip="tooltip" btn-class="ml-n2"></TooltipIcon>
  </div>
</template>

<style scoped>
.tip-text-field {
  display: flex;
  align-items: center;
  gap: 4px;
}
.md3-field {
  flex: 1;
  border-top: 1px solid rgba(var(--main-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--main-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--main-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--main-color-rgb), 0.05);
  padding: 12px 16px;
  border-radius: 16px;
  font-size: 14px;
  font-family: inherit;
  transition: border-color 0.2s ease, box-shadow 0.2s ease;
  color: var(--main-color) !important;
  background: transparent;
}
.md3-field:focus {
  outline: none;
  border-top: 1px solid rgba(var(--sub-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--sub-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--sub-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--sub-color-rgb), 0.05);
  box-shadow: 0 0 0 3px rgba(var(--sub-color-rgb), 0.15);
}
</style>
