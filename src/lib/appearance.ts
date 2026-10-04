/**
 * How the window looks: theme, size, and how tightly the list is packed.
 *
 * Remembered in `localStorage` like the compression settings, and applied to
 * the document rather than sent anywhere. `app/layout.tsx` applies the theme
 * and density before first paint, so a light window never flashes dark.
 */

import { getCurrentWebview } from '@tauri-apps/api/webview';

import { inTauri } from './ipc';

export type Theme = 'dark' | 'light';
export type Density = 'compact' | 'default' | 'relaxed';

export interface Appearance {
  theme: Theme;
  /** Webview zoom. Text sizes are fixed pixels throughout, so zoom is what
   * scales them, with the controls around them in proportion. */
  scale: number;
  density: Density;
}

export const DEFAULT_APPEARANCE: Appearance = {
  theme: 'dark',
  scale: 1,
  density: 'default',
};

export const SCALES: { value: number; label: string }[] = [
  { value: 0.9, label: 'Small' },
  { value: 1, label: 'Default' },
  { value: 1.1, label: 'Large' },
  { value: 1.25, label: 'Larger' },
];

/** Mirrored by the pre-paint script in `app/layout.tsx`. */
export const APPEARANCE_KEY = 'karui.appearance';

export function loadAppearance(): Appearance {
  try {
    const raw = localStorage.getItem(APPEARANCE_KEY);
    if (!raw) return DEFAULT_APPEARANCE;
    return { ...DEFAULT_APPEARANCE, ...(JSON.parse(raw) as Partial<Appearance>) };
  } catch {
    return DEFAULT_APPEARANCE;
  }
}

export function saveAppearance(appearance: Appearance) {
  try {
    localStorage.setItem(APPEARANCE_KEY, JSON.stringify(appearance));
  } catch {
    /* storage full or disabled: the look lasts for this session only */
  }
}

export function applyAppearance({ theme, scale, density }: Appearance) {
  const root = document.documentElement;
  root.classList.toggle('dark', theme === 'dark');
  // Native scrollbars and form controls follow this, not the class.
  root.style.colorScheme = theme;
  root.dataset.density = density;
  if (inTauri()) void getCurrentWebview().setZoom(scale).catch(() => undefined);
}
