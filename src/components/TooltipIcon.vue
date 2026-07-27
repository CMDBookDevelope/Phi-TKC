<script setup lang="ts">
import { ref } from 'vue';
import { bgColor, mainColor, subColor } from '@/colorStore';

defineProps<{ tooltip: string; btnClass?: string[] | string }>();

const show = ref(false);
</script>

<template>
  <fv-callout
  v-model="show"
  effect="hover"
  position="bottomLeft"
  max-width="250px"
  :callout-bg="bgColor"
  border-radius="16px"
  >
    <fv-animated-icon icon="Info" model-value="scaleDown" />
    <template v-slot:main>
      <fv-animated-icon icon="Info" model-value="scaleDown" />
      <a>{{ tooltip }}</a>
    </template>
  </fv-callout>
</template>