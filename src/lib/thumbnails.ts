/**
 * Thumbnails for the list, fetched once per file and kept for the session.
 *
 * Requests wait their turn, two at a time: adding a folder of hundreds would
 * otherwise start hundreds of ffmpeg processes at once.
 */

import type { MediaInfo } from './bindings';
import * as ipc from './ipc';

const CONCURRENCY = 2;

/** A `data:` URL, or `null` when there is no frame to show. */
const cache = new Map<string, Promise<string | null>>();
/** The same, once settled, for rows to show on their first render. */
const settled = new Map<string, string | null>();
const waiting: (() => void)[] = [];
let active = 0;

async function turn<T>(work: () => Promise<T>): Promise<T> {
  // A finished request hands its slot straight to the next in line, so a
  // new one arriving in between cannot take it as well.
  if (active >= CONCURRENCY) await new Promise<void>((go) => waiting.push(go));
  else active++;
  try {
    return await work();
  } finally {
    const next = waiting.shift();
    if (next) next();
    else active--;
  }
}

/**
 * A `data:` URL rather than an object URL. At a couple of kilobytes the
 * encoding costs nothing, and the string stays valid for as long as anything
 * holds it, with nothing to revoke.
 */
function dataUrl(bytes: ArrayBuffer): string {
  let binary = '';
  for (const byte of new Uint8Array(bytes)) binary += String.fromCharCode(byte);
  return `data:image/jpeg;base64,${btoa(binary)}`;
}

/** The size is part of the key, so a file replaced on disk gets a new one. */
const key = (path: string, info: MediaInfo) => `${path}\0${info.sizeBytes}`;

export function thumbnail(path: string, info: MediaInfo): Promise<string | null> {
  const k = key(path, info);
  let pending = cache.get(k);
  if (!pending) {
    pending = turn(() => ipc.videoThumbnail(path, info))
      .then(dataUrl, () => null)
      .then((url) => {
        settled.set(k, url);
        return url;
      });
    cache.set(k, pending);
  }
  return pending;
}

/** Already fetched, so a remounted row shows it without a flash. */
export const cachedThumbnail = (path: string, info: MediaInfo) =>
  settled.get(key(path, info));

/** The image would not display, so rows show the fallback from now on. */
export function thumbnailFailed(path: string, info: MediaInfo) {
  const k = key(path, info);
  cache.set(k, Promise.resolve(null));
  settled.set(k, null);
}
