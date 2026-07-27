<i18n>
en:
  resolution: Resolution
  ffmpeg-preset: Preset
  video-codec: Encoder
  encoder-select: Hardware Encoder
  encoder-auto: Auto
  encoder-nvenc: NVIDIA NVENC
  encoder-qsv: Intel QSV
  encoder-amf: AMD AMF
  encoder-vulkan: Vulkan
  encoder-cpu: CPU Software
  fps: FPS
  hw-accel: Hardware Acceleration
  hw-accel-tips: Improve speed, slightly reduce quality
  fxaa: FXAA
  fxaa-tips: Causes blur, not recommended
  sample-count: Sample Count
  sample-count-tips: Power of 2 for MSAA
  bitrate-control: Bitrate Control
  bitrate: Bitrate/CRF
  ffmpeg-thread: Thread Optimization
  video-format: Video Format
  video-format-flac-error: Not available with FLAC audio
  player-avatar: Avatar
  player-name: Name
  player-rks: RKS
  challenge-color: Challenge Color
  challenge-colors: White,Green,Blue,Red,Golden,Rainbow
  challenge-rank: Challenge Rank
  respack: Resource Pack
  respack-default: '[Default]'
  note-scale: Note Scale
  render: UI Display
  double-hint: Double Hint
  aggressive: Aggressive Optimization
  aggressive-tips: May cause notes to disappear
  disable-particle: No Particles
  disable-effect: No Effects
  volume-music: Music Volume
  volume-sfx: SFX Volume
  audio-format: Audio Format
  audio-bit-depth: Bit Depth
  ending-length: Ending Duration
  disable-loading: Skip Loading
  disable-loading-tips: May have issues
  chart_debug: Debug Mode
  chart_ratio: Chart Scale
  buffer_size: Buffer Size
  target_audio: Sample Rate
  combo: COMBO Text
  watermark: Watermark
  flid_x: Mirror
  background: BG Only
  hand-split: Hand Split
  note-speed-factor: Note Speed
  bar: Judge Bar
  render-list: Judge Line,Score,Combo,Level,Name,Progress,Percent,Time,Pause
  presets: Presets
  preset-create-title: Preset name
  preset-created: Preset created
  preset-deleted: Preset deleted
  preset-replaced: Preset replaced
  preset-cannot-use-default: Cannot use 'default'
  default-preset: Default
  back: Back
  head-categories:
    output: Output
    record: Record
    game: Game
    effect: Effect
    player: Player
    debug: Debug

zh-CN:
  resolution: 分辨率
  ffmpeg-preset: 编码预设
  video-codec: 编码器
  encoder-select: 硬件编码器
  encoder-auto: 自动选择
  encoder-nvenc: NVIDIA NVENC
  encoder-qsv: Intel QSV
  encoder-amf: AMD AMF
  encoder-vulkan: Vulkan
  encoder-cpu: CPU软编码
  fps: 帧率
  hw-accel: 硬件加速
  hw-accel-tips: 提升速度，略微降低质量
  fxaa: FXAA抗锯齿
  fxaa-tips: 会导致画面模糊
  sample-count: 采样数
  sample-count-tips: 非1启用MSAA
  bitrate-control: 码率控制
  bitrate: 码率/CRF
  ffmpeg-thread: 线程优化
  video-format: 视频格式
  video-format-flac-error: FLAC音频下不可用
  player-avatar: 头像
  player-name: 名称
  player-rks: RKS
  challenge-color: 课题颜色
  challenge-colors: 白,绿,蓝,红,金,彩
  challenge-rank: 课题等级
  respack: 资源包
  respack-default: '[默认]'
  note-scale: 音符缩放
  render: UI显示
  double-hint: 双押提示
  aggressive: 激进优化
  aggressive-tips: 可能导致音符消失
  disable-particle: 禁用粒子
  disable-effect: 禁用特效
  volume-music: 音乐音量
  volume-sfx: 音效音量
  audio-format: 音频格式
  audio-bit-depth: 位深度
  ending-length: 结算时长
  disable-loading: 渲染时跳过加载
  disable-loading-tips: 可能有问题
  chart_debug: 调试模式
  chart_ratio: 谱面缩放
  buffer_size: 缓冲区
  target_audio: 采样率
  combo: COMBO文本
  watermark: 水印
  flid_x: 镜像
  background: 仅背景显示
  hand-split: 手序拆解
  note-speed-factor: 流速
  bar: 判定条显示
  render-list: 判定线,分数,连击,等级,名字,进度,百分比,时间,暂停
  presets: 预设
  preset-create-title: 预设名称
  preset-created: 预设已创建
  preset-deleted: 预设已删除
  preset-replaced: 预设已替换
  preset-cannot-use-default: 不能使用default
  default-preset: 默认
  back: 返回
  head-categories: 
    output: 输出
    record: 录制
    game: 游戏
    effect: 效果
    player: 玩家
    debug: 调试
</i18n>

<script setup lang="ts">
import { mainColor, bgColor, subColor } from '../colorStore'
import { ref, watch, computed } from 'vue';
import { useI18n } from 'vue-i18n';
const { t } = useI18n();

import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { anyFilter } from '../common';
import type { RenderConfig } from '../model';
import TipSwitch from './TipSwitch.vue';
import { ElMessage } from 'element-plus'
import type { FormInstance } from 'element-plus'
const form = ref<FormInstance>()

const props = defineProps<{ initAspectRatio?: number }>();

const volumePercent = computed({
  get() {
    // 滑块值转百分比
    return Math.round(volumeMusic.value * 100)
  },
  set(val: number) {
    // 百分比转回滑块真实值
    volumeMusic.value = val / 100
  }
})

const volumeSfxPercent = computed({
  get() {
    // 滑块值转百分比
    return Math.round(volumeSfx.value * 100)
  },
  set(val: number) {
    // 百分比转回滑块真实值
    volumeSfx.value = val / 100
  }
})

const RESOLUTIONS = [
  { key: '1280x960', text: '1280x960' },
  { key: '1920x1440', text: '1920x1440' },
  { key: '2560x1920', text: '2560x1920' },
  { key: '3840x2560', text: '3840x2560' },
  { key: '2844x2133', text: '2844x2133' },
  { key: '2388x1668', text: '2388x1668' },
  { key: '1600x1080', text: '1600x1080' },
  { key: '7680x5760', text: '7680x5760' }
]
;
const FFMPEG_PRESETS = ['veryfast', 'faster', 'fast', 'medium', 'slow', 'slower', 'veryslow'];
const VIDEO_CODECS = [
  { key: 'h264', text: 'H.264' },
  { key: 'hevc', text: 'HEVC' },
  { key: 'av1', text: 'AV1' },
];
const ENCODERS = [
  { key: 'auto', text: '自动选择' },
  { key: 'nvenc', text: 'NVIDIA NVENC' },
  { key: 'qsv', text: 'Intel QSV' },
  { key: 'amf', text: 'AMD AMF' },
  { key: 'vulkan', text: 'Vulkan' },
  { key: 'cpu', text: 'CPU 软编码' },
];
const AUDIO_FORMATS = [
  {text: 'flac', key: 'flac'},
  {text: 'mp3',key: 'mp3'},
  {text: 'aac',key: 'aac'},
  {text: 'opus',key: 'opus'},
  {text: 'wav', key: 'wav'}
];
const AUDIO_BITS = [
  {text: '16', key: 16},
  {text: '24',key: 24},
  {text: '32',key: 32},
  {text: '48',key: 48},
];
const SAMPLE_RATES = [
  {text: '44.1kHz', key: 44100},
  {text: '48kHz',key: 48000},
  {text: '96kHz',key: 96000},
  {text: '192kHz',key: 192000},
  {text: '384kHz',key: 384000},
  {text: '768kHz',key: 768000},
];
const STD_CHALLENGE_COLORS = ['white', 'green', 'blue', 'red', 'golden', 'rainbow'];

const activeCategory = ref('output');

const categories = [
  { key: 'output', icon: 'Video', label: t('head-categories.output') },
  { key: 'recording', icon: 'Record', label: t('head-categories.record') },
  { key: 'game', icon: 'Game', label: t('head-categories.game') },
  { key: 'graphics', icon: 'Color', label: t('head-categories.effect') },
  { key: 'player', icon: 'Contact', label: t('head-categories.player') },
  { key: 'debug', icon: 'Bug', label: t('head-categories.debug') },
];

const resolution = ref('1920x1080');
const fps = ref('60');
const sampleCount = ref('1');
const videoCodec = ref('h264');
const encoder = ref('auto');
const ffmpegPreset = ref('medium');
const videoFormat = ref('mp4');
const bitrateControl = ref('CRF');
const bitrate = ref('28');
const hwAccel = ref(true);
const fxaa = ref(false);
const ffmpegThread = ref(false);
const audioFormat = ref('flac');
const audioBit = ref<number | undefined>(undefined);
const targetAudio = ref(48000);
const volumeMusic = ref(1);
const volumeSfx = ref(1);
const bufferSize = ref(256);
const combo = ref('AUTOPLAY');
const watermark = ref('');
const endingLength = ref('-2.0');
const chartRatio = ref(1.0);
const noteSpeedFactor = ref(1.0);
const background = ref(false);
const bar = ref(false);
const noteScale = ref(1);
const doubleHint = ref(true);
const aggressive = ref(false);
const disableParticle = ref(false);
const disableEffect = ref(false);
const render = ref<string[]>([]);
const DEFAULT_RENDER_LIST = ['Judge Line', 'Score', 'Combo', 'Level', 'Name', 'Progress', 'Percent', 'Time', 'Pause'];
const renderList = ref(t('render-list').split(',').length === 9 ? t('render-list').split(',') : DEFAULT_RENDER_LIST);
function toggleRender(item: string) {
  const index = render.value.indexOf(item);
  if (index > -1) render.value.splice(index, 1);
  else render.value.push(item);
}

const playerAvatar = ref<string>();
const playerName = ref('');
const playerRks = ref('15.0');
const challengeRank = ref('45');
const challengeColor = ref(t('challenge-colors').split(',')[4]);
const chartDebug = ref(false);
const flidX = ref(false);
const handSplit = ref(false);
const disableLoading = ref(false);

const video = ref(false);
watch([audioFormat, video], ([newAudio, newVideo]) => {
  if (newAudio === 'flac' && newVideo) {
    video.value = false;
    videoFormat.value = 'mp4';
    showToast(t('video-format-flac-error'), 'error');
  }
});
watch(videoFormat, (val) => {
  video.value = val === 'mov';
});

interface Respack {
  text: string;
  key: string;
  path: string | null;
  index: number;
}
const DEFAULT_RESPACK: Respack = { text: t('respack-default'), key:t('respack-default'), path: null, index: 0 };
const respacks = ref([DEFAULT_RESPACK]);
const respack = ref(DEFAULT_RESPACK);
async function updateRespacks() {
  const list = (await invoke('get_respacks')) as { name: string; path: string }[];
  respacks.value = [DEFAULT_RESPACK, ...list.map((obj, i) => ({ text: obj.name, key: obj.name, path: obj.path, index: i + 1 }))];
  respack.value = respacks.value.find((x) => x.text === respack.value.text) || respacks.value[0];
}
updateRespacks();

interface Preset {
  name: string;
  key: string;
  config: RenderConfig;
}
const DEFAULT_PRESET: Preset = {
  name: t('default-preset'),
  key: 'default',
  config: {
    resolution: [1920, 1080],
    ffmpegPreset: 'medium',
    endingLength: -2.0,
    disableLoading: true,
    chartDebug: false,
    flidX: false,
    chartRatio: 1,
    bufferSize: 256,
    fps: 60,
    hardwareAccel: true,
    videoCodec: 'h264',
    encoder: 'auto',
    bitrateControl: 'CRF',
    bitrate: '28',
    targetAudio: 48000,
    video: false,
    audioBit: undefined,
    audioFormat: 'flac',
    background: false,
    aggressive: false,
    challengeColor: 'golden',
    challengeRank: 45,
    disableEffect: false,
    doubleHint: true,
    fxaa: false,
    noteScale: 1,
    particle: true,
    playerAvatar: null,
    playerName: '',
    playerRks: 15,
    sampleCount: 1,
    resPackPath: null,
    speed: 1,
    volumeMusic: 1,
    volumeSfx: 1,
    combo: 'AUTOPLAY',
    watermark: '',
    ffmpegThread: false,
    showProgressText: false,
    showTimeText: false,
    handSplit: false,
    noteSpeedFactor: 1.0,
    uiLine: true,
    uiScore: true,
    uiCombo: true,
    uiLevel: true,
    uiName: true,
    uiPb: true,
    uiPause: true,
    bar: false,
  },
};

const presetOptions = computed(() => {
  return presets.value.map(p => ({
    text: p.name,
    key: p.key,
    config: p.config
  }))
})
const presets = ref([DEFAULT_PRESET]);
const preset = ref(DEFAULT_PRESET);

async function updatePresets() {
  const currentKey = preset.value.key;
  const pairs = (await invoke('get_presets')) as Record<string, RenderConfig>;
  presets.value = [
    DEFAULT_PRESET,
    ...Object.keys(pairs)
      .sort()
      .map((key) => ({ name: key, key, config: pairs[key] })),
  ];
  const found = presets.value.find((x) => x.key === currentKey);
  preset.value = found || presets.value[0];
  applyConfig(preset.value.config);
}
updatePresets();

async function chooseAvatar() {
  const file = await open({ filters: [{ name: 'Image', extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp'] }, anyFilter()] });
  if (file) playerAvatar.value = file as string;
}
async function openRespackFolder() {
  try {
    await invoke('open_respack_folder');
  } catch (e) {
    showToast(e, 'error');
  }
}

// Helper: combobox may return {key,text} object instead of plain string
function getResolutionString(): string {
  const val = resolution.value as any;
  return typeof val === 'string' ? val : (val?.key || val?.text || '1920x1080');
}

// Preview computations
const aspectPreview = computed(() => {
  const [w, h] = getResolutionString().split('x').map(Number);
  if (!w || !h || isNaN(w) || isNaN(h)) return { w: 16, h: 9, label: '16:9' };
  const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));
  const g = gcd(w, h);
  return { w: w / g, h: h / g, label: `${w / g}:${h / g}` };
});

const pixelGrid = computed(() => {
  const [w, h] = getResolutionString().split('x').map(Number);
  if (!w || !h || isNaN(w) || isNaN(h)) return { cols: 32, rows: 18, dots: 576 };
  const scale = Math.min(48 / w, 36 / h, 0.025);
  const cols = Math.max(8, Math.round(w * scale));
  const rows = Math.max(6, Math.round(h * scale));
  return { cols, rows, dots: cols * rows };
});

const presetSpeed = computed(() => {
  const speeds: Record<string, number> = { veryfast: 95, faster: 85, fast: 70, medium: 50, slow: 35, slower: 20, veryslow: 8 };
  return speeds[ffmpegPreset.value] ?? 50;
});

const presetQuality = computed(() => 100 - presetSpeed.value);

const challengeColorHex = computed(() => {
  const colors: Record<string, string> = {
    white: '#e0e0e0',
    green: '#4caf50',
    blue: '#2196f3',
    red: '#f44336',
    golden: '#ffc107',
    rainbow: 'linear-gradient(90deg, #f44336, #ff9800, #ffeb3b, #4caf50, #2196f3, #9c27b0)',
  };
  const idx = t('challenge-colors').split(',').indexOf(challengeColor.value);
  return colors[STD_CHALLENGE_COLORS[idx]] || colors.golden;
});

// Combobox may return {key,text} object — extract the key
function getKey(val: any): string {
  return typeof val === 'string' ? val : (val?.key || String(val || ''));
}
function getNumKey(val: any): number | undefined {
  if (val === undefined || val === null) return undefined;
  return typeof val === 'number' ? val : (val?.key != null ? Number(val.key) : Number(val));
}

async function buildConfig(): Promise<RenderConfig | null> {
  // Element Plus 表单校验，返回布尔值
  const valid = true
  if (!valid) {
    showToast(t('has-error'), 'error')
    return null
  }

  const [w, h] = getResolutionString().split('x').map(Number);
  return {
    resolution: [w, h],
    ffmpegPreset: getKey(ffmpegPreset.value),
    endingLength: parseFloat(endingLength.value),
    disableLoading: disableLoading.value,
    chartDebug: chartDebug.value,
    flidX: flidX.value,
    chartRatio: chartRatio.value,
    bufferSize: bufferSize.value,
    fps: parseInt(fps.value),
    hardwareAccel: hwAccel.value,
    videoCodec: getKey(videoCodec.value),
    encoder: getKey(encoder.value),
    bitrateControl: getKey(bitrateControl.value),
    bitrate: bitrate.value,
    targetAudio: getNumKey(targetAudio.value) || 48000,
    background: background.value,
    handSplit: handSplit.value,
    noteSpeedFactor: noteSpeedFactor.value,
    video: video.value,
    audioBit: getKey(audioFormat.value) === 'wav' ? getNumKey(audioBit.value) : undefined,
    audioFormat: getKey(audioFormat.value),
    bar: bar.value,
    aggressive: aggressive.value,
    challengeColor: STD_CHALLENGE_COLORS[t('challenge-colors').split(',').indexOf(challengeColor.value)],
    challengeRank: parseInt(challengeRank.value),
    disableEffect: disableEffect.value,
    doubleHint: doubleHint.value,
    fxaa: fxaa.value,
    noteScale: noteScale.value,
    particle: !disableParticle.value,
    playerAvatar: playerAvatar.value?.length ? playerAvatar.value : null,
    playerName: playerName.value,
    playerRks: parseFloat(playerRks.value),
    sampleCount: parseInt(sampleCount.value),
    resPackPath: respack.value.path,
    speed: 1,
    volumeMusic: volumeMusic.value,
    volumeSfx: volumeSfx.value,
    combo: combo.value,
    watermark: watermark.value,
    ffmpegThread: ffmpegThread.value,
    uiLine: render.value.includes(renderList.value[0]),
    uiScore: render.value.includes(renderList.value[1]),
    uiCombo: render.value.includes(renderList.value[2]),
    uiLevel: render.value.includes(renderList.value[3]),
    uiName: render.value.includes(renderList.value[4]),
    uiPb: render.value.includes(renderList.value[5]),
    showProgressText: render.value.includes(renderList.value[6]),
    showTimeText: render.value.includes(renderList.value[7]),
    uiPause: render.value.includes(renderList.value[8]),
  };
}


function applyConfig(c: RenderConfig) {
  resolution.value = c.resolution.join('x');
  ffmpegPreset.value = c.ffmpegPreset;
  endingLength.value = String(c.endingLength);
  disableLoading.value = c.disableLoading;
  chartDebug.value = c.chartDebug;
  flidX.value = c.flidX;
  chartRatio.value = c.chartRatio;
  bufferSize.value = c.bufferSize;
  fps.value = String(c.fps);
  hwAccel.value = c.hardwareAccel;
  videoCodec.value = c.videoCodec;
  encoder.value = c.encoder || 'auto';
  bitrateControl.value = c.bitrateControl;
  bitrate.value = c.bitrate;
  targetAudio.value = c.targetAudio;
  background.value = c.background ?? false;
  handSplit.value = c.handSplit ?? false;
  noteSpeedFactor.value = c.noteSpeedFactor;
  video.value = c.video;
  audioFormat.value = c.audioFormat || 'flac';
  audioBit.value = c.audioBit;
  aggressive.value = c.aggressive;
  challengeColor.value = t('challenge-colors').split(',')[STD_CHALLENGE_COLORS.indexOf(c.challengeColor)];
  challengeRank.value = String(c.challengeRank);
  disableEffect.value = c.disableEffect;
  doubleHint.value = c.doubleHint;
  fxaa.value = c.fxaa;
  noteScale.value = c.noteScale;
  disableParticle.value = !(c.particle ?? true);
  playerAvatar.value = c.playerAvatar || undefined;
  playerName.value = c.playerName;
  playerRks.value = String(c.playerRks);
  sampleCount.value = String(c.sampleCount);
  respack.value = respacks.value.find((x) => x.path === c.resPackPath) || respacks.value[0];
  volumeMusic.value = c.volumeMusic;
  volumeSfx.value = c.volumeSfx;
  combo.value = c.combo;
  watermark.value = c.watermark;
  bar.value = c.bar ?? false;
  ffmpegThread.value = c.ffmpegThread ?? false;
  render.value = [];
  const list = renderList.value;
  if ((c.uiLine ?? true) && list[0]) render.value.push(list[0]);
  if ((c.uiScore ?? true) && list[1]) render.value.push(list[1]);
  if ((c.uiCombo ?? true) && list[2]) render.value.push(list[2]);
  if ((c.uiLevel ?? true) && list[3]) render.value.push(list[3]);
  if ((c.uiName ?? true) && list[4]) render.value.push(list[4]);
  if ((c.uiPb ?? true) && list[5]) render.value.push(list[5]);
  if ((c.showProgressText ?? false) && list[6]) render.value.push(list[6]);
  if ((c.showTimeText ?? false) && list[7]) render.value.push(list[7]);
  if ((c.uiPause ?? true) && list[8]) render.value.push(list[8]);
}
defineExpose({ buildConfig, applyConfig });

async function createPreset() {
  const config = await buildConfig();
  if (!config) return;
  const name = prompt(t('preset-create-title'));
  if (!name) return;
  if (name === 'default') {
    showToast(t('preset-cannot-use-default'), 'error');
    return;
  }
  try {
    await invoke('add_preset', { name, config });
    await updatePresets();
    preset.value = presets.value.find((x) => x.key === name) || presets.value[0];
    showToast(t('preset-created'), 'success');
  } catch (e) {
    showToast(e, 'error');
  }
}
async function deletePreset() {
  try {
    await invoke('remove_preset', { name: preset.value.key });
    await updatePresets();
    showToast(t('preset-deleted'), 'success');
  } catch (e) {
    showToast(e, 'error');
  }
}
async function replacePreset() {
  const config = await buildConfig();
  if (!config) return;
  try {
    await invoke('remove_preset', { name: preset.value.key });
    await invoke('add_preset', { name: preset.value.key, config });
    await updatePresets();
    showToast(t('preset-replaced'), 'success');
  } catch (e) {
    showToast(e, 'error');
  }
}
</script>

<template>
  <form id="form" class="config-root" @submit.prevent method="post">
    <!-- Category chips -->
    <div class="clear-glass cat-bar">
      <fv-button
      v-for="cat in categories"
      :key="cat.key"
      type="button"
      @click="activeCategory = cat.key"
      :icon="cat.icon"
      :foreground="activeCategory === cat.key ? bgColor : mainColor"
      :background="activeCategory === cat.key ? mainColor : bgColor"
      :border-radius="16"
      >
        <span>{{ cat.label }}</span>
      </fv-button>
    </div>

    <!-- Two-column body: settings + preview -->
    <div class="config-body">
      <!-- LEFT: Settings scroll -->
      <div class="settings-col">
        <div class="settings-scroll">
          <!-- OUTPUT -->
          <div v-show="activeCategory === 'output'" class="settings-stack">
            <div class="fluent-glass md3-card">
              <div class="field-row">
                <div class="card-label">{{ t('resolution') }}</div>
                <fv-combobox
                v-model="resolution" 
                :options="(RESOLUTIONS as any)" 
                :placeholder="t('resolution')"
                class="combo-boxes"
                :border-radius="7"
                :border-width="2"
                />
                <div class="card-label">{{ t('fps') }}</div>
                <input v-model="fps"
                type="number"
                class="blur-glass input-field"
                />
                <div class="card-label">{{ t('sample-count') }}</div>
                <input v-model="sampleCount"
                type="number"
                class="blur-glass input-field"
                />
                </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="field-row">
                <div class="card-label">{{t('video-codec')}}</div>
                <fv-combobox
                  v-model="videoCodec" 
                  :options="(VIDEO_CODECS as any)" 
                  :placeholder="t('video-codec')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
                <div class="card-label">{{t('encoder-select')}}</div>
                <fv-combobox
                  v-model="encoder" 
                  :options="(ENCODERS as any)" 
                  :placeholder="t('encoder-select')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
              </div>
              <div class="field-row">
                <div class="card-label">{{t('ffmpeg-preset')}}</div>
                <fv-combobox
                  v-model="ffmpegPreset"
                  :options="(FFMPEG_PRESETS.map(p => ({text: p, key: p})) as any)"
                  :placeholder="t('ffmpeg-preset')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
                <div class="card-label">{{t('video-format')}}</div>
                <fv-combobox
                  v-model="videoFormat"
                  :options="([{text: 'mp4', key: 'mp4'}, {text: 'mov', key: 'mov'}] as any)"
                  :placeholder="t('video-format')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="field-row">
                <div class="card-label">{{t('bitrate-control')}}</div>
                <fv-combobox
                  v-model="bitrateControl" 
                  :options="([{text: 'CRF', key: 'CRF'}, {text: 'CBR', key: 'CBR'}] as any)"
                  :placeholder="t('bitrate-control')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
                <input v-model="bitrate" :placeholder="t('bitrate')" class="blur-glass input-field" />
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="switch-grid">
                <div class="card-label">{{t('hw-accel')}}</div>
                <TipSwitch v-model="hwAccel" :label="t('hw-accel')" :tooltip="t('hw-accel-tips')" density="compact" color="primary" />
                <TipSwitch v-model="fxaa" :label="t('fxaa')" :tooltip="t('fxaa-tips')" density="compact" color="primary" />
                <TipSwitch v-model="ffmpegThread" :label="t('ffmpeg-thread')" density="compact" color="primary" />
              </div>
            </div>
          </div>

          <!-- RECORDING -->
          <div v-show="activeCategory === 'recording'" class="settings-stack-s">
            <div class="fluent-glass md3-card-s">
              <div class="slider-row">
                <fv-AnimatedIcon icon="MusicNote"/>
                <div class="card-label" style="margin: 0px !important; margin-left: 5px !important;">{{ t('volume-music') }}</div>
                <fv-slider
                  v-model="volumePercent"
                  :mininum="0"
                  :maxinum="200"
                  :unit="1"
                  :color="mainColor"
                  :icon-wrapper-background="bgColor"
                  class="slider"
                />
                <span class="slider-val">{{ volumePercent }}%</span>
              </div>
              <div class="slider-row">
                <fv-AnimatedIcon icon="Volume"/>
                <div class="card-label" style="margin: 0px !important; margin-left: 5px !important;">{{ t('volume-sfx') }}</div>
                <fv-slider
                  v-model="volumeSfxPercent"
                  :mininum="0"
                  :maxinum="200"
                  :unit="1"
                  :color="mainColor"
                  :icon-wrapper-background="bgColor"
                  class="slider"
                />
                <span class="slider-val">{{ volumeSfxPercent }}%</span>
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="field-row">
                <div class="card-label">{{ t('audio-format') }}</div>
                <fv-combobox
                  v-model="audioFormat" 
                  :options="(AUDIO_FORMATS as any)" 
                  :placeholder="t('audio-format')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
                <div class="card-label">{{ t('audio-bit-depth') }}</div>
                <fv-combobox
                  v-model="audioBit"
                  :options="(AUDIO_BITS as any)" 
                  :placeholder="t('audio-bit-depth')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
                <div class="card-label">{{ t('buffer_size') }}</div>
                <fv-combobox
                  v-model="targetAudio"
                  :options="(SAMPLE_RATES as any)"
                  :placeholder="t('buffer_size')"
                  class="combo-boxes"
                  :border-radius="7"
                  :border-width="2"
                />
              </div>
            </div>
          </div>

          <!-- GAME -->
          <div v-show="activeCategory === 'game'" class="settings-stack">
            <div class="fluent-glass md3-card-s" style="min-height: 100%;">
              <div class="slider-header">
                  <span>{{ t('chart_ratio') }}</span
                  ><span class="slider-val">{{ chartRatio.toFixed(2) }}</span>
                </div>
              <div class="slider-row">
                <fv-slider
                  v-model="chartRatio"
                  :mininum="0.05"
                  :maxinum="1"
                  :unit="0.01"
                  :color="mainColor"
                  :icon-wrapper-background="bgColor"
                  class="slider"
                />
              </div>
              <div class="slider-header">
                  <span>{{ t('note-speed-factor') }}</span
                  ><span class="slider-val">{{ noteSpeedFactor.toFixed(2) }}</span>
              </div>
              <div class="slider-row">
                <fv-slider
                  v-model="noteSpeedFactor"
                  :mininum="0.1"
                  :maxinum="2"
                  :unit="0.01"
                  :color="mainColor"
                  :icon-wrapper-background="bgColor"
                  class="slider"
                />
              </div>
              <div class="switch-grid">
                <div class="card-label">{{t('bar')}}</div>
                <fv-toggle-switch
                  v-model="bar"
                  :borderColor="mainColor"
                  :ringBackground="mainColor"
                  :switchOnBackground="subColor"
                  :onForeground="subColor"
                  :offForeground="mainColor"
                  class="switch"
                />
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="field-row two-col">
                <div class="card-label">{{ t('combo') }}</div>
                <input v-model="combo" :placeholder="t('combo')" class="blur-glass input-field" />
                <div class="card-label">{{ t('watermark') }}</div>
                <input v-model="watermark" :placeholder="t('watermark')" class="blur-glass input-field" />
                <div class="card-label">{{ t('ending-length') }}</div>
                <input v-model="endingLength" :placeholder="t('ending-length')" class="blur-glass input-field" />
              </div>
            </div>
          </div>

          <!-- GRAPHICS -->
          <div v-show="activeCategory === 'graphics'" class="settings-stack">
            <div class="fluent-glass md3-card">
              <div class="respack-row" style="align-items: baseline; margin-top: 15px;">
                <div class="card-label">{{ t('respack') }}</div>
                <fv-combobox
                v-model="respack" 
                :options="(respacks as any)" 
                :placeholder="t('respack')"
                class="combo-boxes"
                :border-radius="7"
                :border-width="2"
                />
                <fv-button
                  v-model="respack"
                  type="button"
                  @click="updateRespacks"
                  icon="Refresh"
                  :foreground="mainColor"
                  :border-radius="16"
                  :background="bgColor"
                  class="blur-glass"
                  style="border-radius: 16px; margin-left: 10px;"
                >刷新</fv-button>
                <button type="button" class="icon-btn-respack" @click="openRespackFolder"><fv-animated-icon icon="FolderHorizontal" :font-size="18" /></button>
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="slider-row">
                <div class="card-label" style="min-width: 60px;">{{ t('note-scale') }}</div>
                <fv-slider
                  v-model="noteScale"
                  :mininum="0"
                  :maxinum="5"
                  :unit="0.05"
                  :color="mainColor"
                  :icon-wrapper-background="bgColor"
                  class="slider"
                />
                <span class="slider-val">{{ noteScale.toFixed(2) }}</span>
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="card-label">效果开关</div>
              <div class="switch-grid">
                <TipSwitch v-model="doubleHint" :label="t('double-hint')" density="compact" color="primary" />
                <TipSwitch v-model="aggressive" :label="t('aggressive')" :tooltip="t('aggressive-tips')" density="compact" color="primary" />
                <TipSwitch v-model="disableParticle" :label="t('disable-particle')" density="compact" color="primary" />
                <TipSwitch v-model="disableEffect" :label="t('disable-effect')" density="compact" color="primary" />
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="card-label">UI 显示</div>
              <div class="ui-grid">
                <div v-for="(item, index) in renderList" :key="index" class="ui-chip" :class="{ 'is-on': render.includes(item) }" @click="toggleRender(item)">
                  <fv-animated-icon icon="CheckMark" :font-size="16" />
                  <span>{{ item }}</span>
                </div>
              </div>
            </div>
          </div>

          <!-- PLAYER -->
          <div v-show="activeCategory === 'player'" class="settings-stack">
            <div class="fluent-glass md3-card" style="flex-direction: column;">
              <div class="card-label">个人信息</div>
              <div class="avatar-row">
                <fv-Persona :src="playerAvatar ? playerAvatar : ''" :size="56" :name="playerName ? playerName : ''" class="blur-glass persona">
                  <template v-slot:name>
                      <span>{{ playerName }}</span>
                  </template>
                </fv-Persona>
                <fv-button
                  type="button"
                  @click="chooseAvatar"
                  icon="Add"
                  :foreground="mainColor"
                  :background="bgColor"
                  :border-radius="16"
                  >
                    <span>{{ t('player-avatar') }}</span>
                  </fv-button>
              </div>
              <div class="field-row">
                <input v-model="playerName" :placeholder="t('player-name')" class="blur-glass input-field" />
                <input v-model="playerRks" type="number" :placeholder="t('player-rks')" class="blur-glass input-field" />
              </div>
            </div>
            <div class="fluent-glass md3-card">
              <div class="card-label">课题模式</div>
              <div class="field-row">
                <input v-model="challengeRank" type="number" :placeholder="t('challenge-rank')" class="blur-glass input-field" />
                <fv-combobox
                v-model="challengeColor" 
                :options="(t('challenge-colors').split(',').map(s => ({text: s, key: s})) as any)"
                :placeholder="t('challenge-colors')"
                class="combo-boxes"
                :border-radius="7"
                :border-width="2"
                />
              </div>
            </div>
          </div>

          <!-- DEBUG -->
          <div v-show="activeCategory === 'debug'" class="settings-stack">
            <div class="fluent-glass md3-card">
              <div class="card-label">调试选项</div>
              <div class="switch-grid">
                <TipSwitch v-model="chartDebug" :label="t('chart_debug')" density="compact" color="primary" hide-details />
                <TipSwitch v-model="flidX" :label="t('flid_x')" density="compact" color="primary" hide-details />
                <TipSwitch v-model="handSplit" :label="t('hand-split')" density="compact" color="primary" hide-details />
                <TipSwitch v-model="disableLoading" :label="t('disable-loading')" :tooltip="t('disable-loading-tips')" density="compact" color="warning" />
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Preview panel disabled
      <div class="preview-col">
        <div class="preview-panel">
          <Transition name="preview-fade" mode="out-in">
            <div key="default" class="preview-content preview-default">
              <div style="font-size: 48px; opacity: 0.15;">👁️</div>
              <p class="preview-hint">悬停选项查看预览</p>
            </div>
          </Transition>
        </div>
      </div>
      -->
    </div>

    <!-- Preset bar -->
    <div class="preset-bar">
        <fv-combobox
          v-model="preset" 
          :options="(presetOptions as any)" 
          :placeholder="t('presets')"
          class="combo-boxes"
          :border-radius="7"
          :border-width="2"
          return-object
          @update:model-value="
            (val: Preset) => {
              preset = val;
              applyConfig(val.config);
            }"
        />
      <div class="preset-actions">
        <fv-button
          type="button"
          @click="createPreset"
          icon="Add"
          :foreground="mainColor"
          :background="bgColor"
          :border-radius="16"
        >新建</fv-button>
        <fv-button
          type="button"
          :disabled="preset.key === 'default'"
          @click="replacePreset"
          icon="Add"
          :foreground="mainColor"
          :background="bgColor"
          :border-radius="16"
        >保存</fv-button>
        <fv-button
          type="button"
          :disabled="preset.key === 'default'"
          @click="deletePreset"
          icon="Add"
          :foreground="mainColor"
          :background="bgColor"
          :border-radius="16"
        >删除</fv-button>
      </div>
    </div>
  </form>
</template>

<style scoped>
.config-root {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
}

.cat-bar {
  border-radius: 16px;
  display: flex;
  gap: 8px;
  padding: 10px 16px;
  flex-shrink: 0;
}

.config-body {
  flex: 1;
  display: flex;
  gap: 12px;
  min-height: 0;
  padding: 0 16px;
  overflow: hidden;
}

/* LEFT column */
.settings-col {
  vertical-align: center;
  align-items: center;
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: row;
}
.settings-scroll {
  height: 90%;
  flex: 1;
  overflow-y: auto;
  padding-right: 4px;
}
.settings-stack {
  height: 100%;
  display: flex;
  flex-direction: row;
  gap: 10px;
}
.settings-stack-s {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* RIGHT preview column */
.preview-col {
  width: 320px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
}
.preview-panel {
  flex: 1;
  background: transparent;
  backdrop-filter: blur(16px);
  border: 1px solid rgba(var(--main-color-rgb), 0.08);
  border-radius: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  position: relative;
}

.preview-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 24px;
  width: 100%;
}
.preview-title {
  font-size: 13px;
  font-weight: 600;
  color: rgba(var(--main-color-rgb),0.8);
  text-transform: uppercase;
  letter-spacing: 1px;
  align-self: flex-start;
}
.preview-default {
  opacity: 0.6;
}
.preview-hint {
  color: rgba(255, 255, 255, 0.35);
  font-size: 14px;
  margin: 8px 0 0;
}

/* Preview transition */
.preview-fade-enter-active {
  transition:
    opacity 0.25s ease,
    transform 0.25s ease;
}
.preview-fade-leave-active {
  transition:
    opacity 0.15s ease,
    transform 0.15s ease;
}
.preview-fade-enter-from {
  opacity: 0;
  transform: translateY(8px);
}
.preview-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

/* ===== Cards ===== */
.md3-card {
  align-items: baseline;
  padding: 18px;
  min-height: 350px;
  height: 100%;
  max-width: 100%;
  display: flex;
  flex-direction: row;
  gap: 12px;
  cursor: default;
  background: transparent;
  backdrop-filter: blur(16px);
  border: 1px solid rgba(var(--main-color-rgb), 0.12);
  border-radius: 16px;
  transition: border-color 0.2s ease, box-shadow 0.2s ease, background 0.2s ease;
  color: var(--main-color) !important;
}
.md3-card:hover {
  background: rgba(var(--main-color-rgb), 0.04);
  border-color: rgba(var(--main-color-rgb), 0.25);
}
.md3-card-s {
  padding: 18px;
  height: 120px;
  max-width: 50%;
  display: flex;
  flex-direction: column;
  gap: 24px;
  cursor: default;
  background: transparent;
  backdrop-filter: blur(12px);
  border: 1px solid rgba(var(--main-color-rgb), 0.08);
  border-radius: 16px;
  transition: border-color 0.2s ease, box-shadow 0.2s ease;
  color: var(--main-color) !important;
}
.md3-card-s:hover {
  background: rgba(var(--main-color-rgb), 0.04);
  border-color: rgba(var(--main-color-rgb), 0.2);
}
.card-label {
  min-width: 40px;
  margin: 5px;
  margin-top: 10px;
  font-size: 12px;
  font-weight: 700;
  color: var(--main-color);
  text-transform: uppercase;
  letter-spacing: 1px;
}

.field-row {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.input-field {
  height: 36px;
  width: 300px;
  border-top: 1px solid rgba(var(--main-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--main-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--main-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--main-color-rgb), 0.05);
  padding: 12px 16px;
  border-radius: 7px;
  font-size: 14px;
  transition: border-color 0.2s ease, box-shadow 0.2s ease;
  color: var(--main-color) !important;
}
.input-field:focus {
  outline: none;
  border-top: 1px solid rgba(var(--sub-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--sub-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--sub-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--sub-color-rgb), 0.05);
  box-shadow: 0 0 0 3px rgba(var(--sub-color-rgb), 0.15);
}
.input-field::placeholder {
  color: rgba(var(--main-color-rgb), 0.3);
}
.switch-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 8px;
}

.slider-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.slider-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.respack-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.avatar-row {
  display: flex;
  align-items: center;
  gap: 16px;
}

.ui-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 6px;
}
.ui-chip {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 10px;
  background: rgba(50, 50, 50, 0.5);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 12px;
  cursor: pointer;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.5);
  transition: all 0.2s;
  user-select: none;
  white-space: nowrap;
}
.ui-chip:hover {
  background: rgba(60, 60, 60, 0.6);
}
.ui-chip.is-on {
  background: rgba(var(--main-color-rgb),0.12);
  border-color: rgba(var(--main-color-rgb),0.3);
  color: var(--main-color);
}

.preset-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  flex-shrink: 0;
}
.preset-select {
  flex: 1;
  max-width: 240px;
}
.preset-actions {
  display: flex;
  gap: 8px;
}

/* ===== PREVIEW VISUALIZATIONS ===== */

/* Pixel grid demo */
.pixel-grid-wrap {
  width: 100%;
  display: flex;
  justify-content: center;
  padding: 12px 0;
}
.pixel-grid {
  display: grid;
  gap: 2px;
  width: fit-content;
  padding: 8px;
  background: rgba(0, 0, 0, 0.3);
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.06);
}
.pixel-dot {
  width: 5px;
  height: 5px;
  border-radius: 1px;
  background: var(--main-color);
  opacity: 0;
  animation: pixelFadeIn 0.6s ease forwards;
}
@keyframes pixelFadeIn {
  0% {
    opacity: 0;
    transform: scale(0);
  }
  100% {
    opacity: 0.7;
    transform: scale(1);
  }
}
.pixel-dot:nth-child(odd) {
  animation-delay: 0.1s;
}
.pixel-dot:nth-child(3n) {
  animation-delay: 0.2s;
}
.pixel-dot:nth-child(5n) {
  animation-delay: 0.3s;
}

/* Performance bars */
.perf-bars {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.perf-bar-group {
  display: flex;
  align-items: center;
  gap: 10px;
}
.perf-label {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.5);
  min-width: 36px;
}
.perf-bar {
  flex: 1;
  height: 8px;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 4px;
  overflow: hidden;
}
.perf-fill {
  height: 100%;
  border-radius: 4px;
  transition: width 0.4s ease;
}
.perf-speed {
  background: linear-gradient(90deg, #4caf50, #8bc34a);
}
.perf-quality {
  background: linear-gradient(90deg, #2196f3, var(--main-color));
}
.perf-val {
  font-size: 12px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.7);
  min-width: 32px;
  text-align: right;
  font-family: 'Consolas', monospace;
}

/* Bitrate gauge */
.bitrate-demo {
  width: 100%;
  display: flex;
  justify-content: center;
}
.bitrate-gauge {
  display: flex;
  flex-direction: column;
  align-items: center;
}
.gauge-svg {
  width: 160px;
  height: 80px;
}
.gauge-value {
  font-size: 28px;
  font-weight: 700;
  color: var(--main-color);
  font-family: 'Consolas', monospace;
  margin-top: -8px;
}
.gauge-unit {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.4);
}

/* Toggle demo */
.toggles-demo {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
}
.toggle-demo-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  background: rgba(40, 40, 40, 0.6);
  border-radius: 12px;
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
  transition: all 0.2s;
}
.toggle-demo-item.active {
  background: rgba(var(--main-color-rgb),0.1);
  color: var(--main-color);
}
.toggle-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.2);
  transition: background 0.2s;
}
.toggle-demo-item.active .toggle-dot {
  background: var(--main-color);
  box-shadow: 0 0 8px rgba(var(--main-color-rgb),0.5);
}

/* Wave demo */
.wave-demo {
  width: 100%;
}
.wave-bars {
  display: flex;
  align-items: flex-end;
  gap: 3px;
  height: 80px;
  padding: 4px 0;
}
.wave-bar {
  flex: 1;
  background: linear-gradient(180deg, var(--main-color), #4a7adf);
  border-radius: 2px;
  min-height: 4px;
  transition: height 0.3s ease;
  animation: wavePulse 1.5s ease-in-out infinite alternate;
}
.wave-bar-sfx {
  background: linear-gradient(180deg, #e91e63, #ad1457);
}
@keyframes wavePulse {
  0% {
    opacity: 0.7;
  }
  100% {
    opacity: 1;
  }
}
.wave-label {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  margin-top: 4px;
  display: block;
}

/* Audio quality bars */
.audio-quality-bars {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.aq-bar-group {
  display: flex;
  align-items: center;
  gap: 8px;
}
.aq-label {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.35);
  min-width: 36px;
  font-weight: 600;
  text-transform: uppercase;
}
.aq-label.is-active {
  color: var(--main-color);
}
.aq-bar {
  flex: 1;
  height: 6px;
  background: rgba(255, 255, 255, 0.06);
  border-radius: 3px;
  overflow: hidden;
}
.aq-fill {
  height: 100%;
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.15);
  transition: width 0.4s ease;
}
.aq-fill.is-active {
  background: var(--main-color);
}

/* Chart preview */
.chart-preview-box {
  width: 100%;
  height: 140px;
  background: linear-gradient(180deg, rgba(20, 20, 30, 0.9), rgba(10, 10, 20, 0.95));
  border-radius: 12px;
  position: relative;
  overflow: hidden;
  transition: transform 0.3s ease;
  transform-origin: center center;
}
.chart-judge-line {
  position: absolute;
  bottom: 20px;
  left: 10%;
  right: 10%;
  height: 2px;
  background: linear-gradient(90deg, transparent, rgba(var(--main-color-rgb),0.6), rgba(233, 30, 99, 0.6), rgba(var(--main-color-rgb),0.6), transparent);
  box-shadow: 0 0 8px rgba(var(--main-color-rgb),0.3);
}
.chart-note {
  position: absolute;
  width: 36px;
  height: 36px;
  object-fit: contain;
  filter: drop-shadow(0 0 8px rgba(99, 102, 241, 0.4));
  animation: noteFall linear infinite;
}
.chart-note-1 {
  left: 30%;
  animation-delay: 0s;
}
.chart-note-2 {
  left: 55%;
  animation-delay: 0.6s;
}
.chart-note-3 {
  left: 75%;
  animation-delay: 1.2s;
}
@keyframes noteFall {
  0% {
    top: -30px;
    opacity: 0;
  }
  10% {
    opacity: 1;
  }
  85% {
    opacity: 1;
  }
  100% {
    top: calc(100% - 30px);
    opacity: 0;
  }
}

/* Text preview */
.text-demo {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  width: 100%;
}
.combo-preview {
  font-size: 32px;
  font-weight: 800;
  color: rgba(255, 255, 255, 0.9);
  letter-spacing: 2px;
  text-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
}
.watermark-preview {
  font-size: 14px;
  color: rgba(255, 255, 255, 0.3);
  font-style: italic;
}
.ending-preview {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.4);
  background: rgba(255, 255, 255, 0.05);
  padding: 6px 14px;
  border-radius: 8px;
}

/* Note scale demo */
.note-scale-demo {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}
.note-img {
  object-fit: contain;
  filter: drop-shadow(0 0 12px rgba(99, 102, 241, 0.4));
  transition: all 0.3s ease;
}
.note-scale-label {
  font-size: 14px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.6);
  font-family: 'Consolas', monospace;
}

/* Effects preview */
.effects-grid {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
}
.fx-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  background: rgba(40, 40, 40, 0.6);
  border-radius: 12px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
  position: relative;
  overflow: hidden;
  min-height: 44px;
}
.fx-item.off {
  opacity: 0.4;
}
.fx-particles {
  position: absolute;
  inset: 0;
  pointer-events: none;
}
.particle {
  position: absolute;
  width: 4px;
  height: 4px;
  background: rgba(var(--main-color-rgb),0.6);
  border-radius: 50%;
  animation: particleFloat 2s ease-in-out infinite;
}
@keyframes particleFloat {
  0% {
    transform: translateY(0) scale(1);
    opacity: 0.8;
  }
  50% {
    transform: translateY(-12px) scale(1.3);
    opacity: 0.4;
  }
  100% {
    transform: translateY(0) scale(1);
    opacity: 0.8;
  }
}
.fx-glow {
  position: absolute;
  inset: 0;
  background: radial-gradient(circle at 30% 50%, rgba(var(--main-color-rgb),0.15), transparent 60%);
  pointer-events: none;
}
.fx-double {
  display: flex;
  gap: 6px;
}
.hint-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: #e91e63;
  box-shadow: 0 0 6px rgba(233, 30, 99, 0.5);
}

/* UI phone mock */
.ui-phone-mock {
  width: 120px;
  height: 200px;
  background: rgba(10, 10, 10, 0.9);
  border: 2px solid rgba(255, 255, 255, 0.1);
  border-radius: 16px;
  position: relative;
  overflow: hidden;
}
.phone-screen {
  width: 100%;
  height: 100%;
  position: relative;
}
.phone-ui-line {
  position: absolute;
  bottom: 30px;
  left: 10%;
  right: 10%;
  height: 2px;
  background: linear-gradient(90deg, transparent, rgba(var(--main-color-rgb),0.5), transparent);
}
.phone-ui-score {
  position: absolute;
  top: 12px;
  right: 8px;
  font-size: 8px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.7);
  font-family: 'Consolas', monospace;
}
.phone-ui-combo {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  font-size: 10px;
  font-weight: 800;
  color: rgba(255, 255, 255, 0.5);
}
.phone-ui-name {
  position: absolute;
  top: 12px;
  left: 8px;
  font-size: 7px;
  color: rgba(255, 255, 255, 0.5);
}
.phone-ui-pb {
  position: absolute;
  bottom: 8px;
  left: 8px;
  right: 8px;
  height: 3px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 2px;
}
.pb-fill {
  height: 100%;
  background: var(--main-color);
  border-radius: 2px;
}

/* Player card */
.player-card-preview {
  display: flex;
  align-items: center;
  gap: 14px;
  background: rgba(40, 40, 40, 0.6);
  padding: 14px 18px;
  border-radius: 16px;
  width: 100%;
}
.player-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.player-name-pv {
  font-size: 16px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
}
.player-rks-pv {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.5);
  font-family: 'Consolas', monospace;
}

/* Challenge badge */
.challenge-badge {
  width: 80px;
  height: 80px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 3px solid var(--badge-color, #ffc107);
  box-shadow: 0 0 20px color-mix(in srgb, var(--badge-color, #ffc107) 40%, transparent);
  transition: all 0.3s ease;
}
.badge-rank {
  font-size: 28px;
  font-weight: 800;
  color: var(--badge-color, #ffc107);
}

/* Debug demo */
.debug-demo {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  width: 100%;
}
.debug-grid {
  width: 100%;
  height: 80px;
  background-image: linear-gradient(rgba(255, 255, 255, 0.05) 1px, transparent 1px), linear-gradient(90deg, rgba(255, 255, 255, 0.05) 1px, transparent 1px);
  background-size: 20px 20px;
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.08);
}
.debug-mirror {
  width: 60px;
  height: 60px;
  background: rgba(40, 40, 40, 0.6);
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.3s ease;
}
.debug-mirror.flipped {
  transform: scaleX(-1);
}
.mirror-arrow {
  font-size: 24px;
  color: rgba(255, 255, 255, 0.5);
}
.debug-split {
  display: flex;
  gap: 4px;
}
.split-left {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: rgba(33, 150, 243, 0.4);
}
.split-right {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: rgba(233, 30, 99, 0.4);
}
.debug-skip {
  font-size: 12px;
  font-weight: 700;
  color: rgba(var(--main-color-rgb),0.7);
  background: rgba(var(--main-color-rgb),0.1);
  padding: 6px 14px;
  border-radius: 8px;
}

/* Meta info */
.preview-meta {
  display: flex;
  gap: 16px;
  width: 100%;
  justify-content: center;
  flex-wrap: wrap;
}
.meta-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}
.meta-label {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.35);
  text-transform: uppercase;
}
.meta-value {
  font-size: 14px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.8);
  font-family: 'Consolas', monospace;
}
.combo-boxes:deep(*) {
  background: transparent !important;
  color: var(--main-color) !important;
}
.combo-boxes {
  border-top: 1px solid rgba(var(--main-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--main-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--main-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--main-color-rgb), 0.05);
  transition: all .2s ease;
}
.combo-boxes:hover {
  border-top: 1px solid rgba(var(--sub-color-rgb), 0.3);
  border-left: 1px solid rgba(var(--sub-color-rgb), 0.25);
  border-right: 1px solid rgba(var(--sub-color-rgb), 0.1);
  border-bottom: 1px solid rgba(var(--sub-color-rgb), 0.05);
}

.slider {
  cursor: pointer;
}

.persona:deep(*) {
  background: transparent !important;
  color: var(--main-color) !important;
}
.persona {
  border-radius: 300px;
  border: 1px solid rgba(var(--main-color-rgb), 0.2);
}

/* ===== Responsive ===== */
@media (max-width: 900px) {
  .config-body {
    flex-direction: column;
  }
  .preview-col {
    width: 100%;
    height: 200px;
    flex-shrink: 0;
  }
  .preview-panel {
    border-radius: 16px;
  }
}
@media (max-width: 600px) {
  .preview-col {
    display: none;
  }
  .ui-grid {
    grid-template-columns: repeat(2, 1fr);
  }
}
</style>
