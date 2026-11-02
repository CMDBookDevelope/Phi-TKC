/// <reference types="vite/client" />

declare global {
  interface Window {
    // Background controls, installed by App.vue and driven from Setting.vue.
    refreshApiBackground: () => Promise<boolean>
    fixCurrentBackground: () => Promise<boolean>
  }
}

export {}
