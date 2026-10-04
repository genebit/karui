/**
 * The live encode monitor's history for the file being compressed.
 *
 * The engine reports twice a second; this keeps the encode speed over the
 * whole file for the graph, the peak, and a short tail of ffmpeg-style status
 * lines. It decides nothing, it only records.
 */

import type { CompressEvent, ProgressEvent } from './bindings';
import { formatBytes, formatDuration } from './utils';

/** Points kept for the graph. Past this, neighbours are averaged together,
 * so a four-hour encode costs the same as a four-minute one. */
const MAX_POINTS = 240;

/** Status lines kept for the log. */
const MAX_LINES = 4;

export interface Sample {
  /** Seconds since this file's encode began. */
  t: number;
  fps: number;
}

export interface Monitor {
  path: string;
  samples: Sample[];
  peakFps: number;
  latest: ProgressEvent | null;
  lines: string[];
}

function halve(samples: Sample[]): Sample[] {
  const out: Sample[] = [];
  for (let i = 0; i < samples.length; i += 2) {
    const pair = samples.slice(i, i + 2);
    out.push({
      t: pair[pair.length - 1].t,
      fps: pair.reduce((sum, s) => sum + s.fps, 0) / pair.length,
    });
  }
  return out;
}

/** `frame= 3456 fps=8.1 q=31.5 size=412.3 MB time=0:02:24 bitrate=21.4 Mbps speed=0.34x`,
 * the line ffmpeg prints while it works, in this app's units. */
export function statusLine(p: ProgressEvent): string {
  const parts = [
    `frame=${p.frame ?? '–'}`,
    `fps=${p.fps?.toFixed(1) ?? '–'}`,
    `q=${p.quantizer?.toFixed(1) ?? '–'}`,
    `size=${p.writtenBytes !== null ? formatBytes(p.writtenBytes) : '–'}`,
    `time=${formatDuration(p.outTimeSecs)}`,
    `bitrate=${p.bitrateKbps !== null ? formatBitrate(p.bitrateKbps) : '–'}`,
    `speed=${p.speed !== null ? `${p.speed.toFixed(2)}x` : '–'}`,
  ];
  return parts.join(' ');
}

/** `21410` kbit/s → `21.4 Mbps`; decimal, like the byte sizes. */
export function formatBitrate(kbps: number): string {
  return kbps >= 1000 ? `${(kbps / 1000).toFixed(1)} Mbps` : `${Math.round(kbps)} kbps`;
}

/** Fold one batch event into the monitor. A file starting replaces it. */
export function record(monitor: Monitor | null, event: CompressEvent): Monitor | null {
  if (event.type === 'started') {
    return { path: event.input, samples: [], peakFps: 0, latest: null, lines: [] };
  }
  if (event.type !== 'progress' || monitor === null || monitor.path !== event.input) {
    return monitor;
  }
  let samples = monitor.samples;
  if (event.fps !== null) {
    samples = [...samples, { t: event.elapsedSecs, fps: event.fps }];
    if (samples.length > MAX_POINTS) samples = halve(samples);
  }
  return {
    path: monitor.path,
    samples,
    peakFps: Math.max(monitor.peakFps, event.fps ?? 0),
    latest: event,
    lines: [...monitor.lines, statusLine(event)].slice(-MAX_LINES),
  };
}
