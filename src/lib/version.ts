/**
 * Which version is running, and whether a newer one has been released.
 */

import { getVersion } from '@tauri-apps/api/app';
import { inTauri } from './ipc';

/** Where the update prompt sends people: every build, for every platform. */
export const RELEASES_URL = 'https://github.com/genebit/karui/releases/';

/**
 * `latest` skips drafts and pre-releases, so a build still being checked on
 * the release page is never offered to anyone.
 */
const LATEST = 'https://api.github.com/repos/genebit/karui/releases/latest';

/** Long enough for a slow link, short enough that a dead one is not waited on. */
const TIMEOUT_MS = 8000;

/**
 * The version this build was bundled as, from `tauri.conf.json` — the number
 * the installer carried, not whatever `package.json` says. `null` in a plain
 * browser, where there is no bundle to ask.
 */
export async function currentVersion(): Promise<string | null> {
  if (!inTauri()) return null;
  try {
    return await getVersion();
  } catch {
    return null;
  }
}

/** The newest published release's version, without its `v`. Throws on any failure. */
export async function latestVersion(): Promise<string> {
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), TIMEOUT_MS);
  try {
    const response = await fetch(LATEST, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: abort.signal,
    });
    // GitHub answers 404 both when nothing is published and when the
    // repository is private, and cannot be asked which without a token.
    if (response.status === 404) {
      throw new Error('no published release is visible (HTTP 404)');
    }
    if (!response.ok) throw new Error(`GitHub answered HTTP ${response.status}`);
    const body = (await response.json()) as { tag_name?: unknown };
    if (typeof body.tag_name !== 'string') throw new Error('the latest release has no tag');
    return body.tag_name.replace(/^v/, '');
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Whether `a` is a later version than `b`, comparing `major.minor.patch`
 * numerically — as strings, `1.10.0` would sort before `1.9.0`.
 */
export function isNewer(a: string, b: string): boolean {
  const parts = (v: string) =>
    v
      .split('-')[0]
      .split('.')
      .map((n) => Number.parseInt(n, 10) || 0);
  const x = parts(a);
  const y = parts(b);
  for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d !== 0) return d > 0;
  }
  return false;
}
