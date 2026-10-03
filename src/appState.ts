import { ref, watch } from 'vue';

/**
 * Shared between App.vue and the router.
 *
 * The router's `scrollBehavior` returns a promise that only settles once the
 * routed component has actually mounted, so it needs a handle on the component
 * that App.vue renders. Keeping the ref here (rather than in App.vue's plain
 * `<script>` block) guarantees the template's `ref="routeComponent"` binds to
 * it — an imported ref is resolvable from `<script setup>`'s render scope.
 */
export const onLoaded = ref<() => void>();

/** Template ref bound to the currently rendered route component. */
export const routeComponent = ref();

watch(routeComponent, (comp) => {
  if (comp && onLoaded.value) onLoaded.value();
});

export function useOnLoaded() {
  return onLoaded;
}
