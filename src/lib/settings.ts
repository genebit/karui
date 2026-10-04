/**
 * Compression settings, remembered between launches.
 *
 * Kept in `localStorage`, which is unavailable during the static export's
 * prerender, so callers read them after mount.
 */

import type { CompressOptions } from './bindings';

export interface Settings {
  options: CompressOptions;
  /** Play the three-tone chime when a batch ends. */
  chime: boolean;
  /** Start compressing a camera card's new videos as soon as it is inserted. */
  autoImport: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  options: {
    codec: 'h265',
    engine: 'software',
    crf: null,
    preset: 'medium',
    content: 'general',
    maxFps: null,
    maxResolution: null,
    audio: 'aac',
    outputDir: null,
    importDir: null,
    overwrite: false,
  },
  chime: true,
  autoImport: true,
};

const KEY = 'karui.settings';

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULT_SETTINGS;
    const stored = JSON.parse(raw) as Partial<Settings>;
    // Merged field by field, so a setting added in a later version gets its
    // default rather than `undefined`.
    return {
      ...DEFAULT_SETTINGS,
      ...stored,
      options: { ...DEFAULT_SETTINGS.options, ...stored.options },
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(settings: Settings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    /* storage full or disabled: settings last for this session only */
  }
}

/** Mirrors `Codec::default_crf` in `karui-core`. */
export function defaultCrf(codec: CompressOptions['codec']): number {
  return codec === 'h264' ? 23 : 28;
}
