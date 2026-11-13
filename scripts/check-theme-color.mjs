// Run with: node --test scripts/check-theme-color.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const source = await readFile(new URL('../src/themeColor.js', import.meta.url), 'utf8');
const { dominantColor, readableMainColor } = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'));
const pixels = (...areas) => new Uint8ClampedArray(areas.flatMap(([count, rgba]) =>
  Array.from({ length: count }, () => rgba).flat()));

test('picks the prevalent hue without mixing red and blue to grey/purple', () => {
  assert.equal(dominantColor(pixels([70, [224, 48, 48, 255]], [30, [32, 64, 208, 255]])), '#e03030');
});
test('black and white margins do not replace a meaningful coloured subject', () => {
  assert.equal(dominantColor(pixels([45, [0, 0, 0, 255]], [45, [255, 255, 255, 255]],
    [10, [48, 176, 96, 255]])), '#30b060');
});
test('ignores invisible pixels and weights translucent areas by their alpha', () => {
  assert.equal(dominantColor(pixels([1000, [255, 0, 0, 0]], [100, [0, 255, 0, 30]],
    [50, [48, 80, 224, 255]])), '#3050e0');
});
test('retains monochrome wallpapers, including all black/all white', () => {
  for (const [value, expected] of [[0, '#000000'], [128, '#808080'], [255, '#ffffff']]) {
    assert.equal(dominantColor(pixels([40, [value, value, value, 255]])), expected);
  }
});
test('a lone bright pixel does not dominate an otherwise monochrome wallpaper', () => {
  assert.equal(dominantColor(pixels([1000, [112, 112, 112, 255]], [1, [0, 255, 0, 255]])), '#707070');
});
test('nearby shades form one representative colour and keep the dominant hue', () => {
  const result = dominantColor(pixels([50, [224, 48, 48, 255]], [50, [232, 56, 56, 255]],
    [70, [32, 64, 208, 255]]));
  assert.equal(result, '#e43434');
});
test('empty and fully transparent wallpapers fail instead of overwriting the theme', () => {
  assert.throws(() => dominantColor(new Uint8ClampedArray()), /visible pixels/);
  assert.throws(() => dominantColor(pixels([10, [255, 255, 255, 0]])), /visible pixels/);
});

test('dark-wallpaper accents become lighter without changing the dominant hue', () => {
  const result = readableMainColor('#30b060', '#222222');
  const [r, g, b] = result.match(/[a-f0-9]{2}/g).map(value => parseInt(value, 16));
  assert(g > b && b > r);
  assert(r > 48 && g > 176 && b > 96);
  const neutral = readableMainColor('#000000', '#222222').match(/[a-f0-9]{2}/g).map(value => parseInt(value, 16));
  assert(neutral[0] >= 190);
  assert.equal(neutral[0], neutral[1]);
  assert.equal(neutral[1], neutral[2]);
});

test('light surfaces get a dark accent and already legible light hues remain unchanged', () => {
  const dark = readableMainColor('#ffffff', '#ffffff');
  assert(parseInt(dark.slice(1,3), 16) < 100);
  assert.equal(readableMainColor('#aaffcc', '#222222'), '#aaffcc');
});
