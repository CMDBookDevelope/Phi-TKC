// Vue's standard SFC shim uses `{}` for the props/raw-bindings generics.
/* eslint-disable @typescript-eslint/no-empty-object-type */
declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
