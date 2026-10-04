'use client';

import { memo, useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import type { Entry } from '@/lib/queue';
import { formatBitrate, type Monitor, type Sample } from '@/lib/monitor';
import { baseName, cn, formatBytes, formatChange, formatDuration } from '@/lib/utils';

const CHART_HEIGHT = 104;
const PAD = { top: 14, right: 8, bottom: 16, left: 8 };

/** A round top for the y axis, so the gridline reads as a number. */
function niceMax(value: number): number {
  if (value <= 0) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  const step = [1, 2, 2.5, 5, 10].find((s) => s * magnitude >= value) ?? 10;
  return step * magnitude;
}

/**
 * Encode speed over this file's encode. One series on one axis, so no
 * legend: the header names it. A crosshair snaps to the nearest report.
 */
function SpeedChart({ samples, average }: { samples: Sample[]; average: number | null }) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [hover, setHover] = useState<number | null>(null);

  useEffect(() => {
    const node = box.current;
    if (!node) return;
    const observer = new ResizeObserver(([item]) => setWidth(item.contentRect.width));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const peak = samples.reduce((max, s) => Math.max(max, s.fps), 0);
  const top = niceMax(Math.max(peak, average ?? 0) * 1.1);
  const last = samples.length > 0 ? samples[samples.length - 1].t : 0;
  const first = samples.length > 0 ? samples[0].t : 0;
  const span = Math.max(last - first, 1);
  const plotW = Math.max(width - PAD.left - PAD.right, 1);
  const plotH = CHART_HEIGHT - PAD.top - PAD.bottom;
  const x = (t: number) => PAD.left + ((t - first) / span) * plotW;
  const y = (fps: number) => PAD.top + plotH - (fps / top) * plotH;
  const baseline = PAD.top + plotH;

  const line = samples
    .map((s, i) => `${i === 0 ? 'M' : 'L'}${x(s.t)},${y(s.fps)}`)
    .join('');
  const area =
    samples.length > 1 ? `${line}L${x(last)},${baseline}L${x(first)},${baseline}Z` : '';
  const hovered = hover !== null ? samples[hover] : null;

  return (
    <div ref={box} className="relative min-w-0 flex-1">
      <svg
        width={width}
        height={CHART_HEIGHT}
        role="img"
        aria-label={`Encode speed over time. Now ${samples.at(-1)?.fps.toFixed(1) ?? 'unknown'} frames a second, peak ${peak.toFixed(1)}.`}
        className="block overflow-visible"
        onPointerMove={(event) => {
          if (samples.length === 0) return;
          const left = event.currentTarget.getBoundingClientRect().left;
          const t = first + ((event.clientX - left - PAD.left) / plotW) * span;
          let nearest = 0;
          for (let i = 1; i < samples.length; i++) {
            if (Math.abs(samples[i].t - t) < Math.abs(samples[nearest].t - t))
              nearest = i;
          }
          setHover(nearest);
        }}
        onPointerLeave={() => setHover(null)}
      >
        {/* Recessive grid: the baseline and one line at the top value. */}
        {[0, top / 2, top].map((v) => (
          <line
            key={v}
            x1={PAD.left}
            x2={PAD.left + plotW}
            y1={y(v)}
            y2={y(v)}
            className="stroke-border"
            strokeWidth={1}
          />
        ))}
        <text x={PAD.left} y={PAD.top - 4} className="fill-muted-foreground text-[10px]">
          {top} fps
        </text>
        {average !== null && average > 0 && (
          <line
            x1={PAD.left}
            x2={PAD.left + plotW}
            y1={y(average)}
            y2={y(average)}
            className="stroke-muted-foreground"
            strokeWidth={1}
            strokeDasharray="3 3"
          />
        )}
        {area && <path d={area} className="fill-foreground/10" />}
        {samples.length > 1 && (
          <path
            d={line}
            fill="none"
            className="stroke-foreground"
            strokeWidth={2}
            strokeLinejoin="round"
            strokeLinecap="round"
          />
        )}
        <text
          x={PAD.left}
          y={CHART_HEIGHT - 3}
          className="fill-muted-foreground font-mono text-[10px]"
        >
          {formatDuration(first)}
        </text>
        <text
          x={PAD.left + plotW}
          y={CHART_HEIGHT - 3}
          textAnchor="end"
          className="fill-muted-foreground font-mono text-[10px]"
        >
          {formatDuration(last)}
        </text>
        {hovered && (
          <>
            <line
              x1={x(hovered.t)}
              x2={x(hovered.t)}
              y1={PAD.top}
              y2={baseline}
              className="stroke-muted-foreground"
              strokeWidth={1}
            />
            <circle
              cx={x(hovered.t)}
              cy={y(hovered.fps)}
              r={4}
              className="fill-foreground stroke-card"
              strokeWidth={2}
            />
          </>
        )}
      </svg>
      {hovered && (
        <div
          className="bg-popover text-popover-foreground pointer-events-none absolute top-0 rounded-md px-2 py-1 font-mono text-[11px] shadow-md ring-1 ring-foreground/10"
          style={{
            left: Math.min(Math.max(x(hovered.t) - 50, 0), Math.max(width - 110, 0)),
          }}
        >
          {formatDuration(hovered.t)} · {hovered.fps.toFixed(1)} fps
        </div>
      )}
    </div>
  );
}

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="min-w-0">
      <div className="truncate font-mono text-[13px] font-medium">{value}</div>
      <div className="text-muted-foreground truncate text-[10px] tracking-wide uppercase">
        {label}
        {note && <span className="normal-case tracking-normal"> · {note}</span>}
      </div>
    </div>
  );
}

/**
 * What ffmpeg is doing with the file being compressed, live: speed, frames,
 * bytes written, where the size is heading, and its own status lines.
 */
export const EncodeMonitor = memo(function EncodeMonitor({
  entry,
  monitor,
  collapsed,
  onToggle,
}: {
  entry: Entry;
  monitor: Monitor;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const p = monitor.latest;
  const inputBytes = entry.info?.sizeBytes ?? 0;
  const percent = entry.fraction !== null ? `${Math.round(entry.fraction * 100)}%` : '…';

  const header = (
    <div className="flex h-9 shrink-0 items-center gap-2 px-2">
      <Button
        variant="ghost"
        size="icon-xs"
        title={collapsed ? 'Show encoding details' : 'Collapse encoding details'}
        onClick={onToggle}
      >
        {collapsed ? <ChevronUp /> : <ChevronDown />}
      </Button>
      <span className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
        Encoding
      </span>
      <span className="truncate text-[12px]" title={entry.path}>
        {baseName(entry.path)}
      </span>
      <span className="text-muted-foreground ml-auto shrink-0 font-mono text-[11px]">
        {percent}
        {entry.etaSecs !== null && ` · ${formatDuration(entry.etaSecs)} left`}
      </span>
    </div>
  );

  if (collapsed) return header;

  return (
    <div className="flex flex-col">
      {header}
      <div className="px-3 pb-1">
        <Progress
          value={(entry.fraction ?? 0) * 100}
          indeterminate={entry.fraction === null}
        />
      </div>

      <div className="flex gap-4 px-3 pt-2">
        <div className="grid w-[300px] shrink-0 grid-cols-2 content-start gap-x-4 gap-y-2.5">
          <Stat label="Now" value={p?.fps != null ? `${p.fps.toFixed(1)} fps` : '–'} />
          <Stat
            label="Peak"
            value={monitor.peakFps > 0 ? `${monitor.peakFps.toFixed(1)} fps` : '–'}
            note={p?.averageFps != null ? `avg ${p.averageFps.toFixed(1)}` : undefined}
          />
          <Stat
            label="Frames"
            value={
              p?.frame != null
                ? `${p.frame.toLocaleString()}${
                    p.totalFrames != null ? ` / ${p.totalFrames.toLocaleString()}` : ''
                  }`
                : '–'
            }
          />
          <Stat label="Speed" value={p?.speed != null ? `${p.speed.toFixed(2)}×` : '–'} />
          <Stat
            label="Written"
            value={p?.writtenBytes != null ? formatBytes(p.writtenBytes) : '–'}
          />
          <Stat
            label="Projected"
            value={p?.projectedBytes != null ? `≈ ${formatBytes(p.projectedBytes)}` : '–'}
            note={
              p?.projectedBytes != null && inputBytes > 0
                ? formatChange(inputBytes, p.projectedBytes)
                : undefined
            }
          />
          <Stat
            label="Bitrate"
            value={p?.bitrateKbps != null ? formatBitrate(p.bitrateKbps) : '–'}
            note={p?.quantizer != null ? `q ${p.quantizer.toFixed(1)}` : undefined}
          />
          <Stat
            label="Elapsed"
            value={p ? formatDuration(p.elapsedSecs) : '–'}
            note={
              entry.info?.durationSecs
                ? `of ${formatDuration(entry.info.durationSecs)} video`
                : undefined
            }
          />
        </div>

        <SpeedChart samples={monitor.samples} average={p?.averageFps ?? null} />
      </div>

      <div
        className={cn(
          'mx-3 mt-1 mb-2 rounded-md bg-background/60 px-2 py-1 font-mono text-[10.5px] leading-[1.55]',
          'text-muted-foreground',
        )}
        aria-label="ffmpeg status"
      >
        {monitor.lines.length === 0 ? (
          <div>Waiting for ffmpeg's first report…</div>
        ) : (
          monitor.lines.map((line, i) => (
            <div
              key={i}
              className={cn(
                'truncate',
                i === monitor.lines.length - 1 && 'text-foreground/80',
              )}
            >
              {line}
            </div>
          ))
        )}
      </div>
    </div>
  );
});
