/// <reference types="vite/client" />

declare global {
  interface Window {
    // Background controls, installed by App.vue and driven from Setting.vue.
    refreshApiBackground: () => Promise<void>
    fixCurrentBackground: () => Promise<void>
    loadApiBackground: () => Promise<void>
    loadCustomBackground: (path: string) => Promise<void>
  }
}

export {}
