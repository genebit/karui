/**
 * Shapes crossing the IPC boundary.
 *
 * These mirror the `Serialize`/`Deserialize` types in `src-tauri` and
 * `karui-core`. There is no codegen: change a Rust type and this file in the
 * same commit, or the mismatch surfaces at runtime.
 */

export type Codec = 'h264' | 'h265';

export type Preset =
  | 'ultrafast'
  | 'superfast'
  | 'veryfast'
  | 'faster'
  | 'fast'
  | 'medium'
  | 'slow'
  | 'slower'
  | 'veryslow';

export type Audio = 'aac' | 'copy' | 'remove';

/** `karui_core::options::CompressOptions`. Every field may be omitted. */
export interface CompressOptions {
  codec: Codec;
  /** `null` takes the codec's default: 23 for H.264, 28 for H.265. */
  crf: number | null;
  preset: Preset;
  maxFps: number | null;
  /** Ceiling on the shorter side, so 720 means 720p in either orientation. */
  maxResolution: number | null;
  audio: Audio;
  /** `null` writes each output beside its source as `<name>-compressed.mp4`. */
  outputDir: string | null;
  overwrite: boolean;
}

/** `karui_core::probe::MediaInfo`. */
export interface MediaInfo {
  durationSecs: number | null;
  sizeBytes: number;
  /** Coded dimensions, before `rotation` is applied. */
  width: number;
  height: number;
  /** Clockwise degrees: 0, 90, 180, or 270. */
  rotation: number;
  fps: number | null;
  videoCodec: string;
  pixFmt: string | null;
  videoStream: number;
  audioCodec: string | null;
}

/** `commands::queue::QueueItem`. */
export interface QueueItem {
  path: string;
  info: MediaInfo | null;
  error: string | null;
}

export interface Skipped {
  path: string;
  reason: string;
}

export interface Probed {
  items: QueueItem[];
  skipped: Skipped[];
}

export interface PlannedJob {
  input: string;
  output: string;
}

/** `karui_core::tools::ToolStatus`. */
export interface ToolStatus {
  ffmpeg: string;
  ffprobe: string;
  version: string;
  encoders: Codec[];
}

/** `karui_core::batch::Summary`. Byte totals cover successful jobs only. */
export interface Summary {
  succeeded: number;
  failed: number;
  cancelled: number;
  inputBytes: number;
  outputBytes: number;
  elapsedSecs: number;
}

/** `karui_core::batch::Event`, emitted on `compress://event`. */
export type CompressEvent =
  | { type: 'started'; index: number; total: number; input: string; output: string }
  | {
      type: 'progress';
      index: number;
      input: string;
      /** `null` when the source's duration is unknown. */
      fraction: number | null;
      speed: number | null;
      etaSecs: number | null;
      outTimeSecs: number;
    }
  | {
      type: 'finished';
      index: number;
      input: string;
      output: string;
      inputBytes: number;
      outputBytes: number;
      elapsedSecs: number;
    }
  | { type: 'failed'; index: number; input: string; message: string }
  | { type: 'cancelled'; index: number; input: string }
  | { type: 'done'; summary: Summary };

export interface LogLine {
  level: string;
  message: string;
}

/** `src-tauri::error::AppError`, as serialised. */
export interface AppError {
  kind: string;
  message: string;
  detail: string | null;
}
