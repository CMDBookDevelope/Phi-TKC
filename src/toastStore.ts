import { ref } from 'vue';

export type ToastKind = 'success' | 'info' | 'warning' | 'error';

export interface ToastMessage {
  id: number;
  message: string;
  kind: ToastKind;
}

let nextToastId = 0;

// Keep notifications until their host renders them. An event-only publisher
// drops anything emitted before the host has subscribed or while it remounts.
export const toastMessages = ref<ToastMessage[]>([]);

export function showToast(message: string, kind: ToastKind = 'info') {
  const id = nextToastId++;
  toastMessages.value.push({ id, message, kind });
  return id;
}

export function dismissToast(id: number) {
  toastMessages.value = toastMessages.value.filter((entry) => entry.id !== id);
}
