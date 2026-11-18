import { ref } from 'vue';

/** Template ref bound to the currently rendered route component. */
export const routeComponent = ref();

/** Current render wizard step, shown in the app bar. */
export const renderStepIndex = ref<number | null>(null);
