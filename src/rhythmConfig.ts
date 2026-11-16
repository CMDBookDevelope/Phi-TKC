import type { RenderConfig } from './model';

export const RHYTHM_DEFAULTS = {
  rhythmAnalysis: false,
  rhythmTap: true,
  rhythmDrag: false,
  rhythmHold: true,
  rhythmFlick: false,
};

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
  // Video encoders require even dimensions. Round W / 7 to the nearest even
  // height, matching the Rust renderer; the original chart canvas stays intact.
  return [width, height + (config.rhythmAnalysis ? Math.round(width / 14) * 2 : 0)];
}
