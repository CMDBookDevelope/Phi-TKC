// Run the production store against picker, disk and network boundary doubles.
// node scripts/check-background.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const { test } = require('node:test');
const ts = require('typescript');
const vue = require('vue');

const source = fs.readFileSync(path.join(__dirname, '../src/background.ts'), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
const png = (n) => Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 13, 10, 26, 10, n]);
const jpg = (n) => Uint8Array.from([0xff, 0xd8, 0xff, n]);
const url = (bytes) => `data:image/${bytes[0] === 0xff ? 'jpeg' : 'png'};base64,${Buffer.from(bytes).toString('base64')}`;

function harness({ mobile = true, storage = new Map(), files = new Map() } = {}) {
  let network = async () => new Response(png(1));
  let extraction = async () => '#99ccaa';
  let quota = false;
  const calls = [];
  const colors = {
    mainColor: vue.ref(storage.get('mainColor') || '#82b1ff'),
    bgColor: vue.ref(storage.get('bgColor') || '#1e1e2e'),
  };
  const key = (file, options) => options?.baseDir === 14 ? `app/${file}` : file;
  class FileReader {
    readAsDataURL(blob) {
      blob.arrayBuffer().then((buffer) => {
        this.result = `data:${blob.type};base64,${Buffer.from(buffer).toString('base64')}`;
        this.onload();
      });
    }
  }
  const exports = {};
  const modules = {
    vue,
    './platform': { isMobile: () => mobile },
    './colorStore': colors,
    './themeColor.js': { extractMainColor: (source, background) => {
      calls.push(['extract', source, background]);
      return extraction(source, background);
    } },
    '@tauri-apps/api/path': { appDataDir: async () => '/container/new/com.cnmdbook.phi.tkc/' },
    '@tauri-apps/plugin-http': { fetch: (...args) => network(...args) },
    '@tauri-apps/plugin-fs': {
      BaseDirectory: { AppData: 14 },
      mkdir: async () => {},
      readFile: async (file, options) => {
        calls.push(['read', file, options]);
        const bytes = files.get(key(file, options));
        if (!bytes) throw new Error(`No picker permission or missing file: ${file}`);
        return Uint8Array.from(bytes);
      },
      writeFile: async (file, bytes, options) => files.set(key(file, options), Uint8Array.from(bytes)),
      exists: async (file, options) => files.has(key(file, options)),
      remove: async (file, options) => {
        calls.push(['remove', key(file, options)]);
        files.delete(key(file, options));
      },
    },
  };
  vm.runInNewContext(compiled, {
    exports,
    require: (name) => { assert.ok(modules[name], `Unexpected import ${name}`); return modules[name]; },
    Blob, FileReader, Uint8Array, atob, Date,
    console: { error() {}, warn() {} },
    localStorage: {
      getItem: (name) => storage.get(name) ?? null,
      setItem: (name, value) => {
        if (quota && name === 'BgBase64') throw new Error('Quota exceeded');
        storage.set(name, value);
      },
      removeItem: (name) => storage.delete(name),
    },
  });
  return { store: exports, storage, files, calls, colors,
    network: (handler) => { network = handler; },
    extraction: (handler) => { extraction = handler; },
    quota: () => { quota = true; } };
}

for (const pickerPath of ['content://photos/document/27', 'file:///private/cache/photo.png']) {
  test(`mobile picker survives lost URI permission and app container relocation: ${pickerPath}`, async () => {
    const h = harness();
    h.files.set(pickerPath, png(7));
    assert.equal(await h.store.selectCustomBackground(pickerPath), true);
    assert.equal(h.storage.get('customBackground'), 'custom_background.png');
    assert.equal(h.store.backgroundUrl.value, url(png(7)));
    h.files.delete(pickerPath);
    const restarted = harness(h);
    assert.equal(await restarted.store.initializeBackground(), true);
    assert.equal(restarted.store.backgroundUrl.value, url(png(7)));
  });
}

test('refresh and refix saves visible B, deletes A, and survives restart', async () => {
  const h = harness({ storage: new Map([['BgBase64', url(jpg(1))]]) });
  assert.equal(await h.store.initializeBackground(), true);
  assert.equal(await h.store.fixCurrentBackground(), true);
  assert.ok(h.files.has('app/fixed_background.jpg'));
  h.network(async () => new Response(png(2)));
  assert.equal(await h.store.refreshApiBackground(), true);
  assert.equal(h.store.backgroundUrl.value, url(png(2)));
  assert.equal(h.storage.has('customBackground'), false);
  assert.equal(h.files.has('app/fixed_background.jpg'), false);
  assert.equal(await h.store.fixCurrentBackground(), true);
  assert.deepEqual(h.files.get('app/fixed_background.png'), png(2));
  assert.equal(h.store.customBackgroundUrl.value, url(png(2)));
  const restarted = harness(h);
  assert.equal(await restarted.store.initializeBackground(), true);
  assert.equal(restarted.store.backgroundUrl.value, url(png(2)));
});

test('cancel fixed A reveals cached B and the next fix saves B', async () => {
  const h = harness({
    storage: new Map([['customBackground', 'fixed_background.png'], ['BgBase64', url(png(2))]]),
    files: new Map([['app/fixed_background.png', png(1)]]),
  });
  await h.store.initializeBackground();
  assert.equal(h.store.backgroundUrl.value, url(png(1)));
  assert.equal(await h.store.clearCustomBackground(), true);
  assert.equal(h.files.has('app/fixed_background.png'), false);
  assert.equal(h.store.backgroundUrl.value, url(png(2)));
  assert.equal(await h.store.fixCurrentBackground(), true);
  assert.deepEqual(h.files.get('app/fixed_background.png'), png(2));
});

test('same-format refresh replaces displayed bytes even when the saved name is unchanged', async () => {
  const h = harness({ storage: new Map([['BgBase64', url(png(1))]]) });
  await h.store.initializeBackground();
  await h.store.fixCurrentBackground();
  h.network(async () => new Response(png(2)));
  await h.store.refreshApiBackground();
  await h.store.fixCurrentBackground();
  assert.equal(h.store.backgroundUrl.value, url(png(2)));
  assert.deepEqual(h.files.get('app/fixed_background.png'), png(2));
});

test('failed refresh and unreadable picker retain the working background', async () => {
  const h = harness({ storage: new Map([['BgBase64', url(png(1))]]) });
  await h.store.initializeBackground();
  await h.store.fixCurrentBackground();
  h.network(async () => new Response('offline', { status: 503 }));
  assert.equal(await h.store.refreshApiBackground(), false);
  assert.equal(await h.store.selectCustomBackground('content://expired'), false);
  assert.equal(h.store.backgroundUrl.value, url(png(1)));
  assert.ok(h.files.has('app/fixed_background.png'));
});

test('legacy owned absolute paths migrate, external same-named images remain external', async () => {
  const h = harness({
    storage: new Map([['customBackground', '/container/old/com.cnmdbook.phi.tkc/fixed_background.png']]),
    files: new Map([['app/fixed_background.png', png(1)]]),
  });
  await h.store.initializeBackground();
  assert.equal(h.storage.get('customBackground'), 'fixed_background.png');
  const file = '/pictures/fixed_background.png';
  const desktop = harness({ mobile: false, files: new Map([[file, png(3)]]) });
  await desktop.store.selectCustomBackground(file);
  const restarted = harness({ ...desktop, mobile: false });
  await restarted.store.initializeBackground();
  assert.equal(restarted.store.backgroundUrl.value, url(png(3)));
  await restarted.store.clearCustomBackground();
  assert.ok(desktop.files.has(file), 'Clear must not delete the original user image');
});

test('background controls cannot fix an old image while a refresh is pending', async () => {
  const h = harness({ storage: new Map([['BgBase64', url(png(1))]]) });
  await h.store.initializeBackground();
  let resolve;
  h.network(() => new Promise((done) => { resolve = done; }));
  const pending = h.store.refreshApiBackground();
  assert.equal(h.store.backgroundBusy.value, true);
  assert.equal(await h.store.fixCurrentBackground(), false);
  resolve(new Response(png(2)));
  assert.equal(await pending, true);
  assert.equal(await h.store.fixCurrentBackground(), true);
  assert.deepEqual(h.files.get('app/fixed_background.png'), png(2));
});

test('cache quota failure removes the stale cached A while B remains fixable', async () => {
  const h = harness({ storage: new Map([['BgBase64', url(png(1))]]) });
  await h.store.initializeBackground();
  h.quota();
  h.network(async () => new Response(png(2)));
  assert.equal(await h.store.refreshApiBackground(), true);
  assert.equal(h.storage.has('BgBase64'), false);
  assert.equal(await h.store.fixCurrentBackground(), true);
  assert.deepEqual(h.files.get('app/fixed_background.png'), png(2));
});

test('explicit refresh picks only displayed B once and retains the background colour', async () => {
  const h = harness({
    storage: new Map([['BgBase64', url(png(1))], ['bgColor', '#224466'], ['subColor', '#aa66cc']]),
  });
  await h.store.initializeBackground();
  await h.store.fixCurrentBackground();
  assert.equal(h.calls.filter(([action]) => action === 'extract').length, 0,
    'Startup and pinning must not automatically change the accent');
  h.network(async () => new Response(png(2)));
  h.extraction(async (source) => {
    assert.equal(h.store.backgroundUrl.value, url(png(2)));
    assert.equal(source, url(png(2)));
    assert.equal(h.store.customBackgroundUrl.value, null);
    assert.equal(h.store.backgroundBusy.value, true);
    return '#cceeaa';
  });
  assert.equal(await h.store.refreshApiBackground(), true);
  assert.deepEqual(h.calls.filter(([action]) => action === 'extract'), [['extract', url(png(2)), '#224466']]);
  assert.equal(h.colors.mainColor.value, '#cceeaa');
  assert.equal(h.storage.get('mainColor'), '#cceeaa');
  assert.equal(h.storage.get('bgColor'), '#224466');
  assert.equal(h.storage.get('subColor'), '#aa66cc');
});

test('disabled automatic colour selection is persisted across restart and does not sample', async () => {
  const h = harness({ storage: new Map([['BgBase64', url(png(1))]]) });
  assert.equal(h.store.autoColorOnRefresh.value, true);
  h.store.autoColorOnRefresh.value = false;
  assert.equal(h.storage.get('autoColorOnRefresh'), 'false');
  const restarted = harness(h);
  await restarted.store.initializeBackground();
  assert.equal(restarted.store.autoColorOnRefresh.value, false);
  restarted.network(async () => new Response(png(2)));
  await restarted.store.refreshApiBackground();
  assert.equal(restarted.store.backgroundUrl.value, url(png(2)));
  assert.equal(restarted.colors.mainColor.value, '#82b1ff');
  assert.equal(restarted.calls.filter(([action]) => action === 'extract').length, 0);
  restarted.store.autoColorOnRefresh.value = true;
  assert.equal(restarted.storage.get('autoColorOnRefresh'), 'true');
});

test('failed refresh never samples and a colour-read failure preserves the prior accent', async () => {
  const h = harness({ storage: new Map([['BgBase64', url(png(1))], ['mainColor', '#abcdef']]) });
  await h.store.initializeBackground();
  h.network(async () => new Response('offline', { status: 503 }));
  assert.equal(await h.store.refreshApiBackground(), false);
  assert.equal(h.calls.filter(([action]) => action === 'extract').length, 0);
  assert.equal(h.colors.mainColor.value, '#abcdef');
  h.network(async () => new Response(png(2)));
  h.extraction(async () => { throw new Error('Canvas readback unavailable'); });
  assert.equal(await h.store.refreshApiBackground(), true, 'The wallpaper itself was refreshed');
  assert.equal(h.store.backgroundUrl.value, url(png(2)));
  assert.equal(h.store.backgroundColorError.value, true);
  assert.equal(h.colors.mainColor.value, '#abcdef');
  assert.equal(h.storage.get('mainColor'), '#abcdef');
  assert.equal(h.store.backgroundBusy.value, false);
  h.extraction(async () => '#cceeaa');
  assert.equal(await h.store.refreshApiBackground(), true);
  assert.equal(h.store.backgroundColorError.value, false);
  assert.equal(h.colors.mainColor.value, '#cceeaa');
});

test('controls remain busy through asynchronous colour read and do not overlap a second refresh', { timeout: 2000 }, async () => {
  const h = harness({ storage: new Map([['BgBase64', url(png(1))]]) });
  await h.store.initializeBackground();
  let resolve;
  h.extraction(() => new Promise((done) => { resolve = done; }));
  const pending = h.store.refreshApiBackground();
  while (!resolve) await new Promise((done) => setImmediate(done));
  assert.equal(h.store.backgroundBusy.value, true);
  assert.equal(await h.store.refreshApiBackground(), false);
  assert.equal(await h.store.fixCurrentBackground(), false);
  assert.equal(h.calls.filter(([action]) => action === 'extract').length, 1);
  resolve('#cceeaa');
  assert.equal(await pending, true);
  assert.equal(h.store.backgroundBusy.value, false);
  assert.equal(h.colors.mainColor.value, '#cceeaa');
});
