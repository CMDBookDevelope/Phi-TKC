import type { HdrLayerSettings, RenderConfig, RenderHdrSettings } from './model';

type LegacyHdrSettings = {
  [K in keyof RenderHdrSettings]?: RenderHdrSettings[K] extends HdrLayerSettings
    ? Partial<HdrLayerSettings>
    : Partial<Record<keyof RenderHdrSettings[K], Partial<HdrLayerSettings>>>;
};

function hdrLayer(saved?: Partial<HdrLayerSettings>): HdrLayerSettings {
  return {
    mode: 'sdr', peakNits: 1000, knee: 0.7, power: 1.7, shadowGain: 1, chromaScale: 1, gamutClip: 'hard',
    ...saved,
  };
}

/** Fill absent layers in older presets and give each layer its own editable state. */
export function withHdrDefaults(saved?: LegacyHdrSettings): RenderHdrSettings {
  return {
    blockArea: hdrLayer(saved?.blockArea),
    note: hdrLayer(saved?.note),
    background: hdrLayer(saved?.background),
    ui: {
      score: hdrLayer(saved?.ui?.score),
      combo: hdrLayer(saved?.ui?.combo),
      name: hdrLayer(saved?.ui?.name),
      level: hdrLayer(saved?.ui?.level),
      line: hdrLayer(saved?.ui?.line),
      pb: hdrLayer(saved?.ui?.pb),
      pause: hdrLayer(saved?.ui?.pause),
      progressText: hdrLayer(saved?.ui?.progressText),
      timeText: hdrLayer(saved?.ui?.timeText),
      bar: hdrLayer(saved?.ui?.bar),
      scoreAim: hdrLayer(saved?.ui?.scoreAim),
      watermark: hdrLayer(saved?.ui?.watermark),
    },
    rhythm: {
      metronome: hdrLayer(saved?.rhythm?.metronome),
      note: hdrLayer(saved?.rhythm?.note),
      value: hdrLayer(saved?.rhythm?.value),
    },
    handIndicator: {
      left: hdrLayer(saved?.handIndicator?.left),
      right: hdrLayer(saved?.handIndicator?.right),
    },
    hitEffect: hdrLayer(saved?.hitEffect),
  };
}

export const HAND_APPEARANCE_DEFAULTS = {
  handLeftColor: '#3380FF',
  handRightColor: '#FF99B3',
  handLeftOpacity: 1,
  handRightOpacity: 1,
};

export const HIT_EFFECT_DEFAULT = true;
export const STEREO_STRENGTH_DEFAULT = 0.5;
export const STEREO_STRENGTH_MAX = 2;

type RenderAppearance = typeof HAND_APPEARANCE_DEFAULTS & Pick<RenderConfig, 'hdrSettings' | 'stereoStrength' | 'hitEffectEnabled'>;

function hexColor(value: string | undefined, fallback: string): string {
  return typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
}

function boundedNumber(value: number | undefined, max: number, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.min(Math.max(value, 0), max) : fallback;
}

export function renderAppearance(saved?: Partial<Omit<RenderAppearance, 'hdrSettings'>> & { hdrSettings?: LegacyHdrSettings }): RenderAppearance {
  return {
    hdrSettings: withHdrDefaults(saved?.hdrSettings),
    stereoStrength: boundedNumber(saved?.stereoStrength, STEREO_STRENGTH_MAX, STEREO_STRENGTH_DEFAULT),
    handLeftColor: hexColor(saved?.handLeftColor, HAND_APPEARANCE_DEFAULTS.handLeftColor),
    handRightColor: hexColor(saved?.handRightColor, HAND_APPEARANCE_DEFAULTS.handRightColor),
    handLeftOpacity: boundedNumber(saved?.handLeftOpacity, 1, HAND_APPEARANCE_DEFAULTS.handLeftOpacity),
    handRightOpacity: boundedNumber(saved?.handRightOpacity, 1, HAND_APPEARANCE_DEFAULTS.handRightOpacity),
    hitEffectEnabled: saved?.hitEffectEnabled ?? HIT_EFFECT_DEFAULT,
  };
}
