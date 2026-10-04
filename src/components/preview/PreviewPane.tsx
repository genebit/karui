'use client';

import { memo, useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronUp, Loader2, Pause, X } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import type { Comparison, CompressOptions, Rating } from '@/lib/bindings';
import * as ipc from '@/lib/ipc';
import type { Entry } from '@/lib/queue';
import { baseName, cn, formatBytes, formatChange, formatDuration } from '@/lib/utils';

/** Settles slider drags and settings changes before asking for an encode. */
const DEBOUNCE_MS = 350;

const RATING: Record<Rating, { label: string; className: string }> = {
  transparent: { label: 'No visible loss', className: 'text-foreground' },
  slight: { label: 'Slight loss', className: 'text-foreground' },
  noticeable: { label: 'Noticeable loss', className: 'text-amber-500' },
  heavy: { label: 'Heavy loss', className: 'text-destructive' },
};

/** `fit` shows the whole frame; the numbers are image pixels per screen pixel. */
type Zoom = 'fit' | 1 | 2;

const ZOOMS: { value: Zoom; label: string }[] = [
  { value: 'fit', label: 'Fit' },
  { value: 1, label: '100%' },
  { value: 2, label: '200%' },
];

/** Comparisons kept, so going back to one already made is instant. */
const CACHE_SIZE = 8;

interface View {
  originalUrl: string;
  /** `null` until the sample is encoded and its frame read. */
  compressedUrl: string | null;
  comparison: Comparison | null;
  /** Both stills' size. */
  width: number;
  height: number;
  /** Owned by the cache, which revokes its URLs on eviction. */
  cached: boolean;
}

type Problem = { kind: 'paused' } | { kind: 'error'; message: string };

function pngUrl(bytes: ArrayBuffer): string {
  return URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
}

function revoke(view: View) {
  URL.revokeObjectURL(view.originalUrl);
  if (view.compressedUrl) URL.revokeObjectURL(view.compressedUrl);
}

/** Least recently used first. Holds a few megabytes of PNG per entry. */
const cache = new Map<string, View>();

function recall(key: string): View | undefined {
  const view = cache.get(key);
  if (view) {
    cache.delete(key);
    cache.set(key, view);
  }
  return view;
}

/**
 * The view on screen is always the newest entry, so eviction never takes
 * it.
 */
function remember(key: string, view: View) {
  cache.delete(key);
  cache.set(key, view);
  for (const [oldest, evicted] of cache) {
    if (cache.size <= CACHE_SIZE) break;
    cache.delete(oldest);
    revoke(evicted);
  }
}

/** Free `previous` once `next` replaces it, unless the cache owns it. */
function release(previous: View | null, next: View | null) {
  if (!previous || previous.cached) return;
  if (previous.originalUrl !== next?.originalUrl) {
    URL.revokeObjectURL(previous.originalUrl);
  }
  if (previous.compressedUrl) URL.revokeObjectURL(previous.compressedUrl);
}

function Still({
  src,
  title,
  detail,
  scale,
  focus,
  pixelated,
  onFocus,
  panelRef,
  children,
}: {
  src: string | undefined;
  title: string;
  detail: string | null;
  scale: number;
  focus: { x: number; y: number } | null;
  pixelated: boolean;
  onFocus: (focus: { x: number; y: number } | null) => void;
  panelRef?: React.Ref<HTMLDivElement>;
  children?: React.ReactNode;
}) {
  return (
    <div
      ref={panelRef}
      className="relative min-h-0 overflow-hidden rounded-md border border-border bg-black/40"
      onMouseMove={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        onFocus({
          x: ((event.clientX - rect.left) / rect.width) * 100,
          y: ((event.clientY - rect.top) / rect.height) * 100,
        });
      }}
      onMouseLeave={() => onFocus(null)}
    >
      {src && (
        <img
          src={src}
          alt={title}
          draggable={false}
          className="absolute inset-0 h-full w-full object-contain"
          style={{
            transform: scale === 1 ? undefined : `scale(${scale})`,
            transformOrigin: `${focus?.x ?? 50}% ${focus?.y ?? 50}%`,
            imageRendering: pixelated ? 'pixelated' : 'auto',
          }}
        />
      )}
      <div className="pointer-events-none absolute top-2 left-2 rounded bg-background/80 px-1.5 py-0.5 text-[11px]">
        <span className="font-medium">{title}</span>
        {detail && <span className="text-muted-foreground font-mono"> · {detail}</span>}
      </div>
      {children}
    </div>
  );
}

/** Memoised, so progress on other rows does not re-render the stills. */
export const PreviewPane = memo(function PreviewPane({
  entry,
  options,
  running,
  collapsed,
  onToggle,
  onClose,
}: {
  entry: Entry;
  options: CompressOptions;
  /** A batch is running, so no sample can be encoded. */
  running: boolean;
  collapsed: boolean;
  onToggle: () => void;
  onClose: () => void;
}) {
  const duration = entry.info?.durationSecs ?? null;
  // A third of the way in skips the fade-in or title card many clips open on.
  const [at, setAt] = useState(() => (duration ? duration / 3 : 0));
  const [scrub, setScrub] = useState(at);
  const [view, setView] = useState<View | null>(null);
  const [loading, setLoading] = useState(false);
  /** The sample encode's progress, from 0 to 1. */
  const [progress, setProgress] = useState<number | null>(null);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [zoom, setZoom] = useState<Zoom>('fit');
  const [focus, setFocus] = useState<{ x: number; y: number } | null>(null);
  const [panel, setPanel] = useState<{ width: number; height: number } | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef(view);
  viewRef.current = view;

  const output = entry.status === 'done' ? entry.output : null;
  const paused = output === null && running;
  // Only what changes the picture or the size estimate starts a new sample.
  const sampleKey =
    output === null
      ? JSON.stringify([
          options.codec,
          // Hardware and software samples of the same settings differ, and
          // must not answer for each other from the cache.
          options.engine,
          options.crf,
          options.preset,
          options.maxFps,
          options.maxResolution,
          options.audio,
        ])
      : '';
  const optionsRef = useRef(options);
  optionsRef.current = options;
  // What the stills are made from. The size stands in for the file's
  // contents, so a file replaced on disk is not answered from the cache.
  const request = JSON.stringify([
    entry.path,
    entry.info?.sizeBytes,
    output,
    at,
    sampleKey,
  ]);

  const show = (next: View) =>
    setView((previous) => {
      release(previous, next);
      return next;
    });

  useEffect(() => () => release(viewRef.current, null), []);

  useEffect(() => {
    if (collapsed || !entry.info) return;
    const known = recall(request);
    if (known) {
      show(known);
      setLoading(false);
      setProblem(null);
      return;
    }
    if (paused) {
      setLoading(false);
      setProblem({ kind: 'paused' });
      return;
    }
    let stale = false;
    setLoading(true);
    setProblem(null);
    setProgress(null);
    const timer = setTimeout(async () => {
      try {
        // Filled in from the callback, which TypeScript's narrowing cannot see.
        const first: { originalUrl?: string } = {};
        let reading = Promise.resolve();
        const comparison = await ipc.comparePreview(
          entry.path,
          output,
          optionsRef.current,
          at,
          (stage) => {
            if (stale) return;
            if (stage.type === 'sampling') {
              setProgress(stage.fraction);
              return;
            }
            // Shown on its own while the sample encodes, which takes far
            // longer than grabbing it did.
            reading = ipc.previewImage(stage.original).then((bytes) => {
              if (stale) return;
              first.originalUrl = pngUrl(bytes);
              show({
                originalUrl: first.originalUrl,
                compressedUrl: null,
                comparison: null,
                width: stage.width,
                height: stage.height,
                cached: false,
              });
            });
          },
        );
        await reading;
        const compressed = await ipc.previewImage(comparison.compressed);
        if (stale) return;
        const next: View = {
          originalUrl:
            first.originalUrl ?? pngUrl(await ipc.previewImage(comparison.original)),
          compressedUrl: pngUrl(compressed),
          comparison,
          width: comparison.width,
          height: comparison.height,
          cached: true,
        };
        remember(request, next);
        show(next);
        setProblem(null);
      } catch (error) {
        // A newer request cancels this one in the backend as well, and so
        // does starting a batch.
        if (stale || (ipc.isAppError(error) && error.kind === 'cancelled')) return;
        if (ipc.isAppError(error) && error.kind === 'busy')
          setProblem({ kind: 'paused' });
        else setProblem({ kind: 'error', message: ipc.errorMessage(error) });
      } finally {
        if (!stale) setLoading(false);
      }
    }, DEBOUNCE_MS);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
    // `request` covers the path, info, output, and time.
  }, [request, paused, collapsed]);

  useEffect(() => {
    const node = panelRef.current;
    if (!node) return;
    const observer = new ResizeObserver(([item]) => {
      setPanel({ width: item.contentRect.width, height: item.contentRect.height });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [collapsed]);

  const comparison = view?.comparison ?? null;
  // How far `object-contain` shrank the stills, so 100% means image pixels.
  const fit =
    view && panel && panel.width > 0
      ? Math.min(panel.width / view.width, panel.height / view.height)
      : 1;
  const scale = zoom === 'fit' ? 1 : Math.max(1, zoom / fit);
  const inputBytes = entry.info?.sizeBytes ?? 0;

  const header = (
    <div className="flex h-9 shrink-0 items-center gap-2 px-2">
      <Button
        variant="ghost"
        size="icon-xs"
        title={collapsed ? 'Show preview' : 'Collapse preview'}
        onClick={onToggle}
      >
        {collapsed ? <ChevronUp /> : <ChevronDown />}
      </Button>
      <span className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
        Preview
      </span>
      <span className="truncate text-[12px]" title={entry.path}>
        {baseName(entry.path)}
      </span>
      {loading && !collapsed && (
        <Loader2 className="text-muted-foreground size-3.5 shrink-0 animate-spin" />
      )}

      <div className="ml-auto flex items-center gap-1">
        {!collapsed &&
          ZOOMS.map((z) => (
            <Button
              key={String(z.value)}
              variant={zoom === z.value ? 'secondary' : 'ghost'}
              size="xs"
              onClick={() => setZoom(z.value)}
              title={z.value === 'fit' ? 'Whole frame' : 'Hover to move around the frame'}
            >
              {z.label}
            </Button>
          ))}
        <Button variant="ghost" size="icon-xs" title="Close preview" onClick={onClose}>
          <X />
        </Button>
      </div>
    </div>
  );

  if (collapsed) return header;

  const sampleNote =
    output === null ? `${options.codec.toUpperCase()} sample` : 'compressed file';

  return (
    <div className="flex h-full min-h-0 flex-col">
      {header}

      <div className="relative grid min-h-0 flex-1 grid-cols-2 gap-2 px-2">
        <Still
          panelRef={panelRef}
          src={view?.originalUrl}
          title="Original"
          detail={
            entry.info
              ? `${entry.info.rotation % 180 === 90 ? entry.info.height : entry.info.width}×${
                  entry.info.rotation % 180 === 90 ? entry.info.width : entry.info.height
                } · ${entry.info.videoCodec}`
              : null
          }
          scale={scale}
          focus={focus}
          pixelated={zoom === 2}
          onFocus={setFocus}
        />
        <Still
          src={view?.compressedUrl ?? undefined}
          title="Compressed"
          detail={
            comparison
              ? `${comparison.encodedWidth}×${comparison.encodedHeight} · ${comparison.videoCodec} · ${sampleNote}`
              : null
          }
          scale={scale}
          focus={focus}
          pixelated={zoom === 2}
          onFocus={setFocus}
        >
          {view && !view.compressedUrl && !problem && (
            <div className="text-muted-foreground absolute inset-0 flex items-center justify-center gap-2 text-xs">
              <Loader2 className="size-3.5 animate-spin" />
              {output === null
                ? `Encoding a short sample…${
                    progress === null ? '' : ` ${Math.round(progress * 100)}%`
                  }`
                : 'Reading frames…'}
            </div>
          )}
        </Still>

        {(problem || (loading && !view)) && (
          <div className="bg-background/70 absolute inset-0 mx-2 flex items-center justify-center rounded-md text-center text-xs">
            {problem?.kind === 'paused' ? (
              <div className="text-muted-foreground flex items-center gap-2">
                <Pause className="size-3.5" />
                Sample previews wait while a batch is compressing.
              </div>
            ) : problem?.kind === 'error' ? (
              <div className="text-destructive max-w-md px-4 break-words">
                {problem.message}
              </div>
            ) : (
              <div className="text-muted-foreground flex items-center gap-2">
                <Loader2 className="size-3.5 animate-spin" />
                Reading frames…
              </div>
            )}
          </div>
        )}
      </div>

      <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 px-3 py-2 text-[11.5px]">
        <div className="flex min-w-[220px] flex-1 items-center gap-2">
          <span className="text-muted-foreground w-12 shrink-0 font-mono">
            {formatDuration(scrub)}
          </span>
          <Slider
            min={0}
            max={duration ?? 0}
            step={0.1}
            value={[scrub]}
            disabled={!duration}
            onValueChange={([value]) => setScrub(value)}
            onValueCommit={([value]) => setAt(value)}
          />
          <span className="text-muted-foreground w-12 shrink-0 text-right font-mono">
            {duration ? formatDuration(duration) : '–'}
          </span>
        </div>

        {comparison && (
          <div className="flex items-center gap-3 font-mono">
            {comparison.ssim !== null && (
              <span title="Structural similarity to the original. 1.000 is identical.">
                SSIM {comparison.ssim.toFixed(3)}
              </span>
            )}
            <span title="Peak signal-to-noise ratio. Higher is closer; above about 40 dB is hard to tell apart.">
              PSNR {comparison.psnr === null ? '∞' : `${comparison.psnr.toFixed(1)} dB`}
            </span>
            {comparison.rating && (
              <span
                className={cn(
                  'font-sans font-medium',
                  RATING[comparison.rating].className,
                )}
              >
                {RATING[comparison.rating].label}
              </span>
            )}
            {comparison.estimatedBytes !== null && (
              <span title="Estimated from a short sample. Usually high.">
                ≈ {formatBytes(comparison.estimatedBytes)} (
                {formatChange(inputBytes, comparison.estimatedBytes)})
              </span>
            )}
            {comparison.fromOutput && entry.outputBytes !== null && (
              <span>
                {formatBytes(entry.outputBytes)} (
                {formatChange(inputBytes, entry.outputBytes)})
              </span>
            )}
          </div>
        )}

        {comparison && comparison.notes.length > 0 && (
          <div className="w-full text-amber-500">{comparison.notes.join('. ')}</div>
        )}
      </div>
    </div>
  );
});
