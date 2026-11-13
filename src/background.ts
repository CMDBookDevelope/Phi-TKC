import { computed, ref, watch } from 'vue';
import { appDataDir } from '@tauri-apps/api/path';
import { BaseDirectory, exists, mkdir, readFile, remove, writeFile } from '@tauri-apps/plugin-fs';
import { fetch } from '@tauri-apps/plugin-http';
import { isMobile } from './platform';
import { mainColor, bgColor } from './colorStore';
import { extractMainColor } from './themeColor.js';

export const API_BACKGROUND_URL = 'https://api.yppp.net/api.php';
export const backgroundPath = ref<string | null>(null);
export const customBackgroundUrl = ref<string | null>(null);
export const apiBackgroundUrl = ref<string | null>(null);
export const backgroundBusy = ref(false);
export const backgroundUrl = computed(() => customBackgroundUrl.value ?? apiBackgroundUrl.value);
export const autoColorOnRefresh = ref(localStorage.getItem('autoColorOnRefresh') !== 'false');
export const backgroundColorError = ref(false);

watch(autoColorOnRefresh, (enabled) => {
  localStorage.setItem('autoColorOnRefresh', String(enabled));
}, { flush: 'sync' });

const formats = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'];
const managedName = /^(?:custom|fixed)_background\.(png|jpe?g|gif|webp|bmp)$/;

function imageType(bytes: Uint8Array): string {
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return 'jpg';
  if (bytes[0] === 0x47 && bytes[1] === 0x49 && bytes[2] === 0x46) return 'gif';
  if (bytes[0] === 0x52 && bytes[1] === 0x49 && bytes[2] === 0x46 && bytes[3] === 0x46 &&
      bytes[8] === 0x57 && bytes[9] === 0x45 && bytes[10] === 0x42 && bytes[11] === 0x50) return 'webp';
  if (bytes[0] === 0x42 && bytes[1] === 0x4d) return 'bmp';
  return 'png';
}

async function dataUrl(bytes: Uint8Array): Promise<string> {
  if (!bytes.length) throw new Error('Empty background image');
  const type = imageType(bytes);
  const blob = new Blob([bytes], { type: `image/${type === 'jpg' ? 'jpeg' : type}` });
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error ?? new Error('Failed to read background image'));
    reader.readAsDataURL(blob);
  });
}

function managedFile(path: string): string | null {
  const name = path.split(/[\\/]/).pop() ?? '';
  // Legacy app-owned absolute paths contain our bundle ID, even if an iOS
  // update changed the container prefix. A same-named user file is external.
  const owned = path === name || path.replace(/\\/g, '/').includes('/com.cnmdbook.phi.tkc/');
  return owned && managedName.test(name) ? name : null;
}

async function readBackground(path: string): Promise<Uint8Array> {
  const name = managedFile(path);
  // Store app-owned filenames relative to AppData: iOS's container path can
  // change after an update. Also migrate the old absolute fixed-background path.
  return name
    ? readFile(name, { baseDir: BaseDirectory.AppData })
    : readFile(path);
}

async function cleanManagedFiles(keep?: string) {
  for (const prefix of ['fixed', 'custom']) {
    for (const format of formats) {
      const name = `${prefix}_background.${format}`;
      if (name !== keep && await exists(name, { baseDir: BaseDirectory.AppData })) {
        await remove(name, { baseDir: BaseDirectory.AppData });
      }
    }
  }
}

async function saveBackground(bytes: Uint8Array, prefix: 'custom' | 'fixed'): Promise<string> {
  const name = `${prefix}_background.${imageType(bytes)}`;
  await mkdir(await appDataDir(), { recursive: true });
  await writeFile(name, bytes, { baseDir: BaseDirectory.AppData });
  await cleanManagedFiles(name);
  return name;
}

function setCustom(path: string | null, url: string | null) {
  if (path) localStorage.setItem('customBackground', path);
  else localStorage.removeItem('customBackground');
  backgroundPath.value = path;
  customBackgroundUrl.value = url;
}

async function action(work: () => Promise<void>): Promise<boolean> {
  if (backgroundBusy.value) return false;
  backgroundBusy.value = true;
  try {
    await work();
    return true;
  } catch (error) {
    console.error('Background operation failed', error);
    return false;
  } finally {
    backgroundBusy.value = false;
  }
}

async function loadApiBackground() {
  const response = await fetch(`${API_BACKGROUND_URL}?t=${Date.now()}`, { cache: 'no-store' });
  if (!response.ok) throw new Error(`Background request failed: ${response.status}`);
  const url = await dataUrl(new Uint8Array(await response.arrayBuffer()));
  apiBackgroundUrl.value = url;
  try {
    localStorage.setItem('BgBase64', url);
  } catch (error) {
    localStorage.removeItem('BgBase64');
    console.warn('Background cache exceeds localStorage quota', error);
  }
}

export async function initializeBackground(): Promise<boolean> {
  return action(async () => {
    apiBackgroundUrl.value = localStorage.getItem('BgBase64');
    const stored = localStorage.getItem('customBackground');
    if (stored && stored !== 'null') {
      try {
        const bytes = await readBackground(stored);
        const url = await dataUrl(bytes);
        const name = managedFile(stored);
        const path = name ?? (isMobile() ? await saveBackground(bytes, 'custom') : stored);
        setCustom(path, url);
      } catch (error) {
        console.warn('Stored background is no longer readable', error);
        setCustom(null, null);
      }
    }
    if (!backgroundUrl.value) await loadApiBackground();
  });
}

export async function selectCustomBackground(path: string): Promise<boolean> {
  return action(async () => {
    // readFile uses Android's ContentResolver for content:// and handles iOS
    // file URLs. Read while the picker grant is live, then retain our own copy.
    const bytes = await readFile(path);
    const url = await dataUrl(bytes);
    const saved = isMobile() ? await saveBackground(bytes, 'custom') : path;
    if (!isMobile()) await cleanManagedFiles();
    setCustom(saved, url);
  });
}

export async function clearCustomBackground(): Promise<boolean> {
  return action(async () => {
    await cleanManagedFiles();
    setCustom(null, null);
    if (!apiBackgroundUrl.value) await loadApiBackground();
  });
}

export async function refreshApiBackground(): Promise<boolean> {
  return action(async () => {
    backgroundColorError.value = false;
    // Keep the displayed image if fetching fails. Once B has loaded, discard
    // A's persisted snapshot before switching back to the API layer.
    await loadApiBackground();
    await cleanManagedFiles();
    setCustom(null, null);
    if (autoColorOnRefresh.value) {
      try {
        // Sample B after the refresh has switched away from any fixed/custom A.
        // extractMainColor waits for these exact data-URL bytes to decode; it
        // never fetches another random wallpaper. Keep the controls busy until
        // the colour is applied, so another action cannot change the source.
        const color = await extractMainColor(backgroundUrl.value!, bgColor.value);
        localStorage.setItem('mainColor', color);
        mainColor.value = color;
      } catch (error) {
        // The wallpaper refresh succeeded. A colour-read failure must retain
        // the old accent and must not be reported as a failed image download.
        backgroundColorError.value = true;
        console.error('Wallpaper color extraction failed', error);
      }
    }
  });
}

export async function fixCurrentBackground(): Promise<boolean> {
  return action(async () => {
    const url = backgroundUrl.value;
    if (!url) throw new Error('No loaded background to fix');
    const encoded = url.slice(url.indexOf(',') + 1);
    const bytes = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0));
    const saved = await saveBackground(bytes, 'fixed');
    // Display the exact bytes being saved, rather than an asset URL whose
    // same-name file may still be cached by the WebView as the previous image.
    setCustom(saved, url);
  });
}
