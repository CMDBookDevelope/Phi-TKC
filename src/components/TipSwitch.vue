<script setup lang="ts">
import TooltipIcon from './TooltipIcon.vue';
import { mainColor, bgColor, subColor } from '@/colorStore';

defineProps<{ label: string; tooltip?: string }>();
</script>

<template>
  <a style="font-size: 12px; font-weight: 400;">{{ label }}</a>
  <fv-toggle-switch
    :borderColor="mainColor"
    :ringBackground="mainColor"
    :switchOnBackground="subColor"
    :onForeground="subColor"
    :offForeground="mainColor"
    class="switch"
  />
  <TooltipIcon v-if="tooltip" :tooltip="tooltip"></TooltipIcon>
</template>

<style scoped>
.switch {
  height: 28px;
  cursor: pointer;
  position: relative;
  transition: all 0.25s ease;
}
</style>