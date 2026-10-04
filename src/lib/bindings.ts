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

/** `karui_core::options::Engine`: x264/x265 on the CPU, or the hardware encoder. */
export type Engine = 'software' | 'hardware';

/** `karui_core::hardware::Hardware`: the hardware encoder that works here. */
export interface Hardware {
  backend: 'videotoolbox' | 'vaapi' | 'nvenc' | 'amf' | 'qsv';
  /** e.g. "Apple VideoToolbox" or "AMD GPU (VA-API)". */
  name: string;
  codecs: Codec[];
}

/** `karui_core::options::CompressOptions`. Every field may be omitted. */
export interface CompressOptions {
  codec: Codec;
  engine: Engine;
  /** `null` takes the codec's default: 23 for H.264, 28 for H.265. */
  crf: number | null;
  preset: Preset;
  maxFps: number | null;
  /** Ceiling on the shorter side, so 720 means 720p in either orientation. */
  maxResolution: number | null;
  audio: Audio;
  /**
   * `null` writes each output beside its source as `<name>-compressed.mp4`,
   * except files on a camera card, which go to `importDir`.
   */
  outputDir: string | null;
  /** Where camera card files go without `outputDir`. `null` is `defaultImport()`. */
  importDir: string | null;
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
  /** Bits per second of the audio stream, when recorded. */
  audioBitrate: number | null;
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
  /** `null` when no hardware encoder works with this ffmpeg. */
  hardware: Hardware | null;
}

/** `karui_core::preview::Rating`: a plain-language reading of SSIM. */
export type Rating = 'transparent' | 'slight' | 'noticeable' | 'heavy';

/** `karui_core::preview::Stage`: progress while a comparison is made. */
export type PreviewStage =
  | {
      type: 'original';
      atSecs: number;
      width: number;
      height: number;
      /** PNG path, read with `previewImage`. */
      original: string;
    }
  /** The sample encode, from 0 to 1. */
  | { type: 'sampling'; fraction: number };

/** `karui_core::preview::Comparison`. */
export interface Comparison {
  /** The moment compared, after clamping to the file's length. */
  atSecs: number;
  /** Both stills share these dimensions. */
  width: number;
  height: number;
  /** PNG paths, read with `previewImage`. */
  original: string;
  compressed: string;
  /** `compressed` came from the finished output, not a sample encode. */
  fromOutput: boolean;
  encodedWidth: number;
  encodedHeight: number;
  videoCodec: string;
  ssim: number | null;
  /** dB. `null` for identical frames. */
  psnr: number | null;
  rating: Rating | null;
  /** Whole-file size at the sample's bitrate. Samples only. */
  estimatedBytes: number | null;
  notes: string[];
}

/** `karui_core::estimate::Basis`: a benchmark, or learnt from real encodes. */
export type Basis = 'benchmark' | 'measured';

/** `commands::estimate::Estimates`. */
export interface Estimates {
  /** Seconds per item, in the order sent. `null` where it cannot be known. */
  secs: (number | null)[];
  /** `null` when there is no rate for these settings yet. */
  basis: Basis | null;
}

/** `karui_core::devices::CardSummary`: a mounted camera card. */
export interface CardSummary {
  /** The volume's mount point, e.g. `/Volumes/Untitled`. */
  root: string;
  name: string;
  videos: number;
  bytes: number;
  /** Videos never imported before, in recording order. */
  fresh: string[];
  freshBytes: number;
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

/** One progress report, as the live encode monitor reads it. */
export type ProgressEvent = Extract<CompressEvent, { type: 'progress' }>;

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
      /** Frames written, and how many there will be. */
      frame: number | null;
      totalFrames: number | null;
      /** Frames a second right now, smoothed over the last few reports. */
      fps: number | null;
      /** Frames a second over the whole encode so far. */
      averageFps: number | null;
      bitrateKbps: number | null;
      /** The quantizer of the latest frame: what the CRF works out to there. */
      quantizer: number | null;
      writtenBytes: number | null;
      /** The output's final size at the rate so far. */
      projectedBytes: number | null;
      /** Since this file's encode began. */
      elapsedSecs: number;
    }
  | {
      type: 'finished';
      index: number;
      input: string;
      output: string;
      inputBytes: number;
      outputBytes: number;
      elapsedSecs: number;
      /** The encode rate achieved, which the backend learns from. */
      pixelsPerSec: number | null;
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
