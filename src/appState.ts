import { ref } from 'vue';

/** Template ref bound to the currently rendered route component. */
export const routeComponent = ref();

/** Chart configuration / parameters / rendering (0–2); hidden while choosing. */
export const renderStepIndex = ref<number | null>(null);
