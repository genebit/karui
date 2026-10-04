/**
 * Typed wrappers over the Tauri commands.
 *
 * Every call funnels through `call`, which normalises rejections into an
 * `AppError`. The backend already serialises one; this only has to cope with
 * the case where something failed before reaching a command.
 */

import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  AppError,
  CardSummary,
  Comparison,
  PreviewStage,
  CompressOptions,
  Estimates,
  LogLine,
  MediaInfo,
  PlannedJob,
  Probed,
  ToolStatus,
} from './bindings';

/** Event names. Mirrored by the constants in `src-tauri`. */
export const LOG_EVENT = 'log://line';
export const COMPRESS_EVENT = 'compress://event';
/** Sent when a camera card is inserted or removed. */
export const DEVICES_EVENT = 'devices://changed';

export function isAppError(value: unknown): value is AppError {
  return (
    typeof value === 'object' && value !== null && 'kind' in value && 'message' in value
  );
}

export function errorMessage(error: unknown): string {
  if (isAppError(error)) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Extra explanation, such as how to install ffmpeg. */
export function errorDetail(error: unknown): string | null {
  return isAppError(error) ? error.detail : null;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (isAppError(error)) throw error;
    throw { kind: 'unknown', message: String(error), detail: null } satisfies AppError;
  }
}

/** Whether the app is running inside Tauri rather than a plain browser. */
export function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export const toolStatus = () => call<ToolStatus>('tool_status');

/** Expand files and folders into videos, and read each with ffprobe. */
export const probePaths = (paths: string[]) => call<Probed>('probe_paths', { paths });

/** Paths given on the command line. Empty after the first call. */
export const launchPaths = () => call<string[]>('launch_paths');

/**
 * Start a batch. Resolves with the planned outputs as soon as it has begun;
 * progress then arrives on `COMPRESS_EVENT`.
 */
export const startCompression = (paths: string[], options: CompressOptions) =>
  call<PlannedJob[]>('start_compression', { paths, options });

export const cancelCompression = () => call<boolean>('cancel_compression');

/**
 * How long each file would take to compress with `options`, in order. The
 * first call for a codec and preset benchmarks this machine for a few seconds.
 */
export const estimateTimes = (items: MediaInfo[], options: CompressOptions) =>
  call<Estimates>('estimate_times', { items, options });

/**
 * Estimated output bytes for one file with `options`, from three short sample
 * encodes. Rejects with kind `busy` while a batch runs, and `cancelled` when a
 * preview or a newer request needs the encoder.
 */
export const estimateSize = (path: string, info: MediaInfo, options: CompressOptions) =>
  call<number>('estimate_size', { path, info, options });

/**
 * One frame before and after compression. With `output` the frame comes from
 * the finished file; without, from a short sample encoded with `options`.
 * `onStage` hears about the original still as soon as it exists, then the
 * sample encode's progress. Rejects with kind `busy` for a sample while a
 * batch is running.
 */
export function comparePreview(
  path: string,
  output: string | null,
  options: CompressOptions,
  atSecs: number,
  onStage: (stage: PreviewStage) => void,
) {
  const channel = new Channel<PreviewStage>();
  channel.onmessage = onStage;
  return call<Comparison>('compare_preview', {
    path,
    output,
    options,
    atSecs,
    onStage: channel,
  });
}

/** A still from `comparePreview`, as PNG bytes. */
export const previewImage = (path: string) =>
  call<ArrayBuffer>('preview_image', { path });

/** A small frame of `path` for its row in the list, as JPEG bytes. */
export const videoThumbnail = (path: string, info: MediaInfo) =>
  call<ArrayBuffer>('video_thumbnail', { path, info });

/** Mounted camera cards, with which of their videos are new. */
export const listCards = () => call<CardSummary[]>('list_cards');

/** Where card videos go when no import folder is chosen, e.g. `~/Movies/karui`. */
export const defaultImport = () => call<string | null>('default_import');

/** Lines logged before the window could subscribe, e.g. during startup. */
export const logBacklog = () => call<LogLine[]>('log_backlog');
