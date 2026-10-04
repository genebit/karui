/**
 * The queue as the window shows it, and how batch events move it along.
 *
 * Kept apart from the page so the transitions can be read in one place. The
 * engine decides everything; this only records what it reported.
 */

import type { CompressEvent, MediaInfo, QueueItem } from './bindings';

/** Mirrors `karui_core::discover::VIDEO_EXTENSIONS`, for the file picker. */
export const VIDEO_EXTENSIONS = [
  '3gp', 'avi', 'flv', 'm2ts', 'm4v', 'mkv', 'mov', 'mp4', 'mpeg', 'mpg', 'mts', 'mxf', 'ogv',
  'ts', 'webm', 'wmv',
];

export type Status =
  | 'ready'
  | 'unreadable'
  | 'queued'
  | 'running'
  | 'done'
  | 'failed'
  | 'cancelled';

export interface Entry {
  /** Exactly as the backend reported it; batch events are matched on this. */
  path: string;
  info: MediaInfo | null;
  status: Status;
  /** Why it is unreadable or failed. */
  error: string | null;
  fraction: number | null;
  speed: number | null;
  etaSecs: number | null;
  output: string | null;
  outputBytes: number | null;
  elapsedSecs: number | null;
  /**
   * Done, but the encode came out no smaller than the original beside it and
   * was discarded. `outputBytes` is what it came to.
   */
  keptOriginal: boolean;
}

export function fromProbe(item: QueueItem): Entry {
  return {
    path: item.path,
    info: item.info,
    status: item.info ? 'ready' : 'unreadable',
    error: item.error,
    fraction: null,
    speed: null,
    etaSecs: null,
    output: null,
    outputBytes: null,
    elapsedSecs: null,
    keptOriginal: false,
  };
}

/** Entries the Start button would send: everything readable not yet done. */
export function runnable(entries: Entry[]): Entry[] {
  return entries.filter(
    (e) => e.status === 'ready' || e.status === 'failed' || e.status === 'cancelled',
  );
}

export function isActive(status: Status): boolean {
  return status === 'queued' || status === 'running';
}

/**
 * Move `path` to insertion point `to`, counted in the list as it is now:
 * `0` puts it first and `entries.length` last. The batch compresses in list
 * order, so this is how the user chooses what goes first.
 */
export function move(entries: Entry[], path: string, to: number): Entry[] {
  const from = entries.findIndex((e) => e.path === path);
  if (from < 0) return entries;
  // Taking the entry out shifts everything after it up by one.
  const target = Math.max(0, Math.min(to > from ? to - 1 : to, entries.length - 1));
  if (target === from) return entries;
  const next = [...entries];
  const [entry] = next.splice(from, 1);
  next.splice(target, 0, entry);
  return next;
}

/** Append items not already queued, keeping the existing order. */
export function merge(entries: Entry[], items: QueueItem[]): Entry[] {
  const known = new Set(entries.map((e) => e.path));
  const fresh = items.filter((item) => !known.has(item.path)).map(fromProbe);
  return fresh.length > 0 ? [...entries, ...fresh] : entries;
}

/**
 * Mark `paths` queued before the batch is asked to start.
 *
 * Not after: the batch thread can report its first file started before the
 * command that launched it has returned, and marking rows queued then would
 * put a running row back to queued.
 */
export function markQueued(entries: Entry[], paths: Set<string>): Entry[] {
  return entries.map((e) =>
    paths.has(e.path)
      ? {
          ...e,
          status: 'queued',
          error: null,
          fraction: null,
          speed: null,
          etaSecs: null,
          output: null,
          outputBytes: null,
          elapsedSecs: null,
          keptOriginal: false,
        }
      : e,
  );
}

/** Fill in planned outputs without touching any status already reported. */
export function withOutputs(entries: Entry[], outputs: Map<string, string>): Entry[] {
  return entries.map((e) =>
    e.output === null && outputs.has(e.path) ? { ...e, output: outputs.get(e.path)! } : e,
  );
}

/** Undo `markQueued` when the batch never started. */
export function unqueue(entries: Entry[], paths: Set<string>): Entry[] {
  return entries.map((e) =>
    paths.has(e.path) && e.status === 'queued' ? { ...e, status: 'ready' } : e,
  );
}

export function applyEvent(entries: Entry[], event: CompressEvent): Entry[] {
  if (event.type === 'done') return entries;
  return entries.map((e): Entry => {
    if (e.path !== event.input) return e;
    switch (event.type) {
      case 'started':
        return { ...e, status: 'running', output: event.output, fraction: 0 };
      case 'progress':
        return {
          ...e,
          fraction: event.fraction,
          speed: event.speed,
          etaSecs: event.etaSecs,
        };
      case 'finished':
        return {
          ...e,
          status: 'done',
          fraction: 1,
          output: event.output,
          outputBytes: event.outputBytes,
          elapsedSecs: event.elapsedSecs,
          keptOriginal: false,
        };
      case 'keptOriginal':
        return {
          ...e,
          status: 'done',
          fraction: 1,
          output: null,
          outputBytes: event.outputBytes,
          elapsedSecs: event.elapsedSecs,
          keptOriginal: true,
        };
      case 'failed':
        return { ...e, status: 'failed', error: event.message, fraction: null };
      case 'cancelled':
        return { ...e, status: 'cancelled', fraction: null };
    }
  });
}
