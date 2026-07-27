import { ref, watch } from 'vue'

// ---- Reactive color state with localStorage persistence ----
export const mainColor = ref(localStorage.getItem('mainColor') || '#82b1ff')
export const bgColor = ref(localStorage.getItem('bgColor') || '#1e1e2e')
export const subColor = ref(localStorage.getItem('subColor') || '#cba6f7')

// ---- Auto-persist on change with debounce ----
watch(mainColor, (val) => localStorage.setItem('mainColor', val))
watch(bgColor, (val) => localStorage.setItem('bgColor', val))
watch(subColor, (val) => localStorage.setItem('subColor', val))

// ---- Restore from localStorage on app mount ----
export function loadCustomColors() {
  const mc = localStorage.getItem('mainColor')
  const bc = localStorage.getItem('bgColor')
  const sc = localStorage.getItem('subColor')
  if (mc) mainColor.value = mc
  if (bc) bgColor.value = bc
  if (sc) subColor.value = sc
}
