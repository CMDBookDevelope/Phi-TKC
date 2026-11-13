/**
 * Pick a representative colour instead of averaging unrelated hues to grey.
 * Alpha weights ignore invisible pixels. A 4-bit histogram groups nearby shades;
 * meaningful coloured areas take priority over black/white wallpaper margins.
 * This module is also embedded by the Rust server for the standalone WebUI.
 * @param {ArrayLike<number>} pixels RGBA pixels
 * @returns {string}
 */
export function dominantColor(pixels) {
  const bins = new Map();
  let total = 0;
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    const weight = pixels[i + 3] / 255;
    if (weight < 0.1) continue;
    const r = pixels[i], g = pixels[i + 1], b = pixels[i + 2];
    const key = (r >> 4) << 8 | (g >> 4) << 4 | (b >> 4);
    let bin = bins.get(key);
    if (!bin) {
      bin = { weight: 0, r: 0, g: 0, b: 0 };
      bins.set(key, bin);
    }
    bin.weight += weight;
    bin.r += r * weight;
    bin.g += g * weight;
    bin.b += b * weight;
    total += weight;
  }
  if (!total) throw new Error('The wallpaper has no visible pixels');

  const palette = Array.from(bins.values(), bin => {
    const rgb = [bin.r / bin.weight, bin.g / bin.weight, bin.b / bin.weight];
    const max = Math.max(...rgb), min = Math.min(...rgb);
    const saturation = max ? (max - min) / max : 0;
    return { rgb, weight: bin.weight, saturation,
      colourful: max - min >= 24 && max >= 48 && min <= 232 };
  });
  const colourful = palette.filter(bin => bin.colourful);
  // A stray bright pixel must not outweigh an otherwise monochrome wallpaper.
  const candidates = colourful.reduce((sum, bin) => sum + bin.weight, 0) >= total * 0.01
    ? colourful : palette;
  const winner = candidates.reduce((best, bin) =>
    bin.weight * (0.7 + 0.3 * bin.saturation) > best.weight * (0.7 + 0.3 * best.saturation)
      ? bin : best);
  return '#' + winner.rgb.map(value => Math.round(value).toString(16).padStart(2, '0')).join('');
}

/** Turn the sampled hue into a legible accent for the selected surface colour.
 * @param {string} color
 * @param {string} background
 * @returns {string}
 */
export function readableMainColor(color, background) {
  const rgb = hex => {
    const match = /^#([0-9a-f]{6})(?:[0-9a-f]{2})?$/i.exec(hex);
    const value = parseInt(match ? match[1] : '222222', 16);
    return [value >> 16 & 255, value >> 8 & 255, value & 255];
  };
  const luminance = channels => channels.map(value => {
    const c = value / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, value, i) => sum + value * [0.2126, 0.7152, 0.0722][i], 0);
  const original = rgb(color), surface = luminance(rgb(background));
  const contrast = value => (Math.max(value, surface) + 0.05) / (Math.min(value, surface) + 0.05);
  const lighten = contrast(1) >= contrast(0);
  const target = lighten ? 255 : 0;
  const required = Math.min(7, contrast(lighten ? 1 : 0));
  for (let step = 0; step <= 100; step++) {
    const result = original.map(value => Math.round(value + (target - value) * step / 100));
    const value = luminance(result);
    // Dark wallpapers need a light accent over the glass as well as the base
    // surface. White/black mixing retains the source hue; grey stays neutral.
    if (contrast(value) >= required && (!lighten || value >= 0.55)) {
      return '#' + result.map(channel => channel.toString(16).padStart(2, '0')).join('');
    }
  }
  return lighten ? '#ffffff' : '#000000';
}

/** Sample the supplied, already displayed wallpaper; never request a new random image.
 * @param {string} source
 * @param {string} [background]
 * @returns {Promise<string>}
 */
export async function extractMainColor(source, background = '#222222') {
  if (!source) throw new Error('No wallpaper is loaded');
  const image = new Image();
  await new Promise((resolve, reject) => {
    const finish = (error) => {
      clearTimeout(timer);
      image.onload = null;
      image.onerror = null;
      if (error) reject(error);
      else resolve(undefined);
    };
    const timer = setTimeout(() => finish(new Error('Wallpaper loading timed out')), 10000);
    image.onload = () => finish(null);
    image.onerror = () => finish(new Error('Could not read the wallpaper'));
    // Remote images need CORS permission for canvas readback. Cached data/blob
    // URLs need no HTTP request, including Android/iOS app-private backgrounds.
    if (/^https?:/i.test(source)) image.crossOrigin = 'anonymous';
    image.src = source;
  });
  if (!image.naturalWidth || !image.naturalHeight) throw new Error('Empty wallpaper');
  const scale = Math.min(1, 96 / Math.max(image.naturalWidth, image.naturalHeight));
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
  canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context) throw new Error('Canvas is unavailable');
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  return readableMainColor(dominantColor(context.getImageData(0, 0, canvas.width, canvas.height).data), background);
}
