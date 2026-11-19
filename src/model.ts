export interface ChartInfo {
  name: string;
  level: string;
  charter: string;
  composer: string;
  illustrator: string;
  illustration?: string;

  tip: string | null;

  backgroundDim: number;
  HoldPartialCover: boolean;
}

export type TaskStatus =
  | {
      type: 'pending';
    }
  | {
      type: 'loading';
    }
  | {
      type: 'mixing';
    }
  | {
      type: 'rendering';
      progress: number;
      fps: number;
      estimate: number;
    }
  | {
      type: 'done';
      duration: number;
      average_fps?: number;
      output: string;
    }
  | {
      type: 'canceled';
    }
  | {
      type: 'failed';
      error: string;
    };

export interface Task {
  id: number;
  name: string;
  charter?: string;
  composer?: string;
  illustrator?: string;
  level?: string;
  output: string;
  path: string;
  cover: string;
  status: TaskStatus;
}

export interface RenderConfig {
  resolution: number[];
  ffmpegPreset: string;
  endingLength: number;
  disableLoading: boolean;
  chartDebug: boolean;
  blockAreaUi: boolean;
  rhythmAnalysis?: boolean;
  rhythmAutoBpm?: boolean;
  rhythmTap?: boolean;
  rhythmDrag?: boolean;
  rhythmHold?: boolean;
  rhythmFlick?: boolean;
  rhythmSpeed?: number;
  rhythmColors?: [string, string, string] | null;
  flidX: boolean;
  showProgressText: boolean;
  showTimeText: boolean;
  chartRatio: number;
  bufferSize: number;
  combo: string;
  watermark: string;
  background: boolean;
  backgroundMode: 'chart' | 'solid' | 'video' | 'none';
  backgroundColor: string;
  backgroundVideo: string | null;
  backgroundVideoPlayback: 'normal' | 'repeat' | 'stretch';
  colorSpace: 'rec709' | 'srgb' | 'rec2100_pq' | 'rec2100_hlg';
  hdrSettings: {
    blockArea: HdrLayerSettings;
    note: HdrLayerSettings;
    background: HdrLayerSettings;
    ui: Record<HdrUiKey, HdrLayerSettings>;
  };
  stereoEnabled: boolean;
  stereoStrength: number;
  fps: number;
  hardwareAccel: boolean;
  videoCodec: string;
  bitrateControl: string;
  bitrate: string;
  targetAudio: number;
  video: boolean;
  audioBit?: number;
  audioFormat: string;
  // Only used by the preview/play commands; ignored by the render backend.
  autoplay?: boolean;
  // Encoder selection: 'auto', 'nvenc', 'qsv', 'amf', 'vulkan', 'cpu'
  encoder?: string;

  aggressive: boolean;
  challengeColor: string;
  challengeRank: number;
  disableEffect: boolean;
  doubleHint: boolean;
  fxaa: boolean;
  noteScale: number;
  particle: boolean;
  playerAvatar: string | null;
  playerName: string;
  playerRks: number;
  sampleCount: number;
  resPackPath: string | null;
  speed: number;
  volumeMusic: number;
  volumeSfx: number;
  handSplit: boolean;
  noteSpeedFactor: number;

  uiScore: boolean;
  uiName: boolean;
  uiLine: boolean;
  uiLevel: boolean;
  uiCombo: boolean;
  uiPb: boolean;
  uiPause: boolean;
  bar: boolean;
  ffmpegThread: boolean;
}

export interface HdrLayerSettings {
  mode: 'sdr' | 'mapped' | 'custom';
  peakNits: number;
  knee: number;
  power: number;
  shadowGain: number;
  chromaScale: number;
  gamutClip: 'hard' | 'soft';
}

export type HdrUiKey =
  | 'score' | 'combo' | 'name' | 'level' | 'line' | 'pb' | 'pause' | 'progressText'
  | 'timeText' | 'bar' | 'scoreAim' | 'watermark';

export interface RPEChart {
  name: string;
  id: string;
  path: string;
  illustration: string;
  charter: string;
}

export interface BatchChart {
  path: string;
  name: string;
  level: string;
  charter: string;
  preset: string;
  status: 'pending' | 'rendering' | 'done' | 'failed';
  error?: string;
}

export interface FileDropEvent {
  paths: string[];
  position: { x: number; y: number };
}
