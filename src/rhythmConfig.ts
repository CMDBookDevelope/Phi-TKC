import type { RenderConfig } from './model';

export const RHYTHM_DEFAULTS = {
  rhythmAnalysis: false,
  rhythmTap: true,
  rhythmDrag: false,
  rhythmHold: true,
  rhythmFlick: false,
  rhythmSpeed: 1.0,
};

export const RHYTHM_SPEED_MIN = 0.1;
export const RHYTHM_SPEED_MAX = 10;

export function isRhythmSpeed(value: unknown): boolean {
  if (typeof value !== 'number' && (typeof value !== 'string' || value.trim() === '')) return false;
  const speed = Number(value);
  return Number.isFinite(speed) && speed >= RHYTHM_SPEED_MIN && speed <= RHYTHM_SPEED_MAX;
}

// Saved presets and local batch configurations from older versions have no
// rhythm fields. Keep their existing output and seed the same note selection
// that a newly opened render form uses.
export function withRhythmDefaults(config: RenderConfig): RenderConfig & typeof RHYTHM_DEFAULTS {
  return {
    ...config,
    rhythmAnalysis: config.rhythmAnalysis ?? RHYTHM_DEFAULTS.rhythmAnalysis,
    rhythmTap: config.rhythmTap ?? RHYTHM_DEFAULTS.rhythmTap,
    rhythmDrag: config.rhythmDrag ?? RHYTHM_DEFAULTS.rhythmDrag,
    rhythmHold: config.rhythmHold ?? RHYTHM_DEFAULTS.rhythmHold,
    rhythmFlick: config.rhythmFlick ?? RHYTHM_DEFAULTS.rhythmFlick,
    rhythmSpeed: config.rhythmSpeed ?? RHYTHM_DEFAULTS.rhythmSpeed,
  };
}

export function rhythmNoteNames(config: RenderConfig): string[] {
  const settings = withRhythmDefaults(config);
  return [
    settings.rhythmTap ? 'Tap' : '',
    settings.rhythmDrag ? 'Drag' : '',
    settings.rhythmHold ? 'Hold' : '',
    settings.rhythmFlick ? 'Flick' : '',
  ].filter(Boolean);
}

export function rhythmOutputResolution(config: RenderConfig): number[] {
  const [width, height] = config.resolution;
  // Video encoders require even dimensions. Round W / 10 to the nearest even
  // height, matching the Rust renderer; the original chart canvas stays intact.
  return [width, height + (config.rhythmAnalysis ? Math.round(width / 20) * 2 : 0)];
}
