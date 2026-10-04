/**
 * Typed wrappers over the Tauri commands.
 *
 * Every call funnels through `call`, which normalises rejections into an
 * `AppError`. The backend already serialises one; this only has to cope with
 * the case where something failed before reaching a command.
 */

import { invoke } from '@tauri-apps/api/core';
import type {
  AppError,
  CompressOptions,
  LogLine,
  PlannedJob,
  Probed,
  ToolStatus,
} from './bindings';

/** Event names. Mirrored by the constants in `src-tauri`. */
export const LOG_EVENT = 'log://line';
export const COMPRESS_EVENT = 'compress://event';

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

/** Lines logged before the window could subscribe, e.g. during startup. */
export const logBacklog = () => call<LogLine[]>('log_backlog');
