// Runtime platform detection.
//
// `isMobile()` means the Tauri mobile targets (iOS/iPadOS, Android). On those the
// desktop render pipeline cannot run at all — it spawns a copy of the app binary
// and an external FFmpeg executable, which iOS forbids — so the UI hides those
// features behind an explanation instead of letting them fail at runtime.
// See MOBILE.md for the full support matrix.
import { platform } from '@tauri-apps/plugin-os';

let cached: string | null = null;

function currentPlatform(): string {
  if (cached !== null) return cached;
  try {
    cached = platform();
    return cached;
  } catch {
    // Not running inside Tauri (e.g. plain `vite dev` in a browser) or the OS plugin
    // is not ready yet. Don't cache the fallback, so a later call can retry instead of
    // permanently classifying a phone as a desktop.
    return 'unknown';
  }
}

export function isMobile(): boolean {
  const p = currentPlatform();
  if (p === 'ios' || p === 'android') return true;
  // Last-resort fallback when the OS plugin is unavailable; iPadOS may report a
  // desktop UA, in which case the plugin path above is what makes it work.
  if (p === 'unknown') return /android|iphone|ipad|ipod/i.test(navigator.userAgent);
  return false;
}
