/// <reference types="vite/client" />

declare global {
  function showToast(text: unknown, type?: 'success' | 'warning' | 'error'): void
  function toast(text: unknown, type?: 'success' | 'warning' | 'info' | 'error'): void
  function toastError(error: unknown): void

  interface Window {
    showToast: (text: unknown, type?: 'success' | 'warning' | 'error') => void
    refreshApiBackground: () => Promise<void>
    fixCurrentBackground: () => Promise<void>
    loadApiBackground: () => Promise<void>
    loadCustomBackground: (path: string) => Promise<void>
    saveCustomBackground: (path: string) => Promise<void>
  }
}

export {}
