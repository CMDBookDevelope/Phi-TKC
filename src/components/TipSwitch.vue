<script setup lang="ts">
import TooltipIcon from './TooltipIcon.vue';

defineProps<{ label: string; tooltip?: string }>();
</script>

<template>
  <!-- Single root, so the parent's `v-model` falls through to `v-switch` via $attrs.
       (A multi-root template silently drops it, which left every switch in
       ConfigView non-functional.) -->
  <v-switch color="primary" class="mx-2 d-flex justify-center tk-switch">
    <template v-slot:label>
      {{ label }}
      <TooltipIcon v-if="tooltip" :tooltip="tooltip"></TooltipIcon>
    </template>
  </v-switch>
</template>

<style scoped>
.tk-switch :deep(.v-label) {
  color: rgba(var(--main-color-rgb), 0.85);
}
</style>
