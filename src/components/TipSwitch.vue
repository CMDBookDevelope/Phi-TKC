<script setup lang="ts">
import TooltipIcon from './TooltipIcon.vue';
import { isAndroid } from '@/platform';
import { OptionSwitch } from './optionControls';

const android = isAndroid();

defineOptions({ inheritAttrs: false });
defineProps<{ label: string; tooltip?: string }>();
</script>

<template>
  <!-- Keep the Android switch and its tooltip; desktop uses the external-label wrapper. -->
  <v-switch v-if="android" v-bind="$attrs" color="primary" class="mx-2 d-flex justify-center tk-switch">
    <template v-slot:label>
      {{ label }}
      <TooltipIcon v-if="tooltip" :tooltip="tooltip"></TooltipIcon>
    </template>
  </v-switch>
  <OptionSwitch v-else v-bind="$attrs" :label="label" color="primary" hide-details />
</template>

<style scoped>
.tk-switch :deep(.v-label) {
  color: rgba(var(--main-color-rgb), 0.85);
}
</style>
