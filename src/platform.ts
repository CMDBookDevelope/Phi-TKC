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
  if (cached === null) {
    try {
      cached = platform();
    } catch {
      // Not running inside Tauri (e.g. plain `vite dev` in a browser): treat as desktop.
      cached = 'unknown';
    }
  }
  return cached;
}

export function isMobile(): boolean {
  const p = currentPlatform();
  return p === 'ios' || p === 'android';
}
