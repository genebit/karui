'use client';

import { memo, useEffect, useState } from 'react';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import {
  CircleAlert,
  CircleCheck,
  CircleSlash,
  Eye,
  FileVideo,
  FolderOpen,
  FolderSearch,
  Loader2,
  Timer,
  X,
} from 'lucide-react';

import { Logo } from '@/components/Logo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { ScrollArea } from '@/components/ui/scroll-area';
import type { Basis, MediaInfo } from '@/lib/bindings';
import { isActive, type Entry, type Status } from '@/lib/queue';
import { cachedThumbnail, thumbnail, thumbnailFailed } from '@/lib/thumbnails';
import {
  baseName,
  cn,
  dirName,
  formatBytes,
  formatChange,
  formatDuration,
  formatEstimate,
} from '@/lib/utils';

const STATUS: Record<Status, { label: string; className: string }> = {
  ready: { label: 'Ready', className: 'text-muted-foreground' },
  unreadable: { label: 'Unreadable', className: 'text-destructive' },
  queued: { label: 'Queued', className: 'text-muted-foreground' },
  running: { label: 'Compressing', className: 'text-foreground' },
  done: { label: 'Done', className: 'text-foreground' },
  failed: { label: 'Failed', className: 'text-destructive' },
  cancelled: { label: 'Cancelled', className: 'text-muted-foreground' },
};

function StatusIcon({ status }: { status: Status }) {
  const className = 'size-4 shrink-0';
  switch (status) {
    case 'running':
      return <Loader2 className={cn(className, 'animate-spin')} />;
    case 'done':
      return <CircleCheck className={className} />;
    case 'failed':
    case 'unreadable':
      return <CircleAlert className={cn(className, 'text-destructive')} />;
    case 'cancelled':
      return <CircleSlash className={cn(className, 'text-muted-foreground')} />;
    default:
      return null;
  }
}

/** A frame of the video, with the row's status over it once it has one. */
function Thumbnail({ entry }: { entry: Entry }) {
  const { path, info } = entry;
  const [url, setUrl] = useState(() => (info ? cachedThumbnail(path, info) : null));

  useEffect(() => {
    if (!info || url !== undefined) return;
    let live = true;
    void thumbnail(path, info).then((next) => {
      if (live) setUrl(next);
    });
    return () => {
      live = false;
    };
  }, [path, info, url]);

  // Ready and queued rows are plain; the label beside them says which.
  const marked = entry.status !== 'ready' && entry.status !== 'queued';
  return (
    <div className="bg-muted relative flex h-9 w-16 shrink-0 items-center justify-center overflow-hidden rounded-md">
      {url ? (
        <img
          src={url}
          alt=""
          draggable={false}
          className="size-full object-cover"
          onError={() => {
            if (info) thumbnailFailed(path, info);
            setUrl(null);
          }}
        />
      ) : (
        !marked && <FileVideo className="text-muted-foreground size-4" />
      )}
      {marked && (
        <div
          className={cn(
            'absolute inset-0 flex items-center justify-center',
            url && 'bg-background/60',
          )}
        >
          <StatusIcon status={entry.status} />
        </div>
      )}
    </div>
  );
}

/** `1920×1080 · hevc · 29.97 fps · 1:23 · 120.4 MB`, as a player shows it. */
function describe(info: MediaInfo): string {
  const portrait = info.rotation % 180 === 90;
  const [w, h] = portrait ? [info.height, info.width] : [info.width, info.height];
  return [
    `${w}×${h}`,
    info.videoCodec,
    info.fps ? `${Number(info.fps.toFixed(2))} fps` : null,
    info.durationSecs ? formatDuration(info.durationSecs) : null,
    formatBytes(info.sizeBytes),
  ]
    .filter(Boolean)
    .join(' · ');
}

const BASIS: Record<Basis, string> = {
  benchmark: 'from a quick speed test on this computer',
  measured: 'from how fast earlier files compressed on this computer',
};

/**
 * Memoised: progress arrives twice a second and rebuilds the entry list, but
 * only the running row's entry changes, so every other row skips rendering.
 * Props are primitives or stable callbacks for that reason.
 */
const Row = memo(function Row({
  entry,
  locked,
  selected,
  estimate,
  basis,
  size,
  sizing,
  onRemove,
  onSelect,
}: {
  entry: Entry;
  locked: boolean;
  selected: boolean;
  /** Seconds to compress with the current settings, when known. */
  estimate: number | null;
  basis: Basis | null;
  /** Estimated output bytes with the current settings, when measured. */
  size: number | null;
  /** This row's size is being measured now. */
  sizing: boolean;
  onRemove: (path: string) => void;
  onSelect: (path: string) => void;
}) {
  const status = STATUS[entry.status];
  // Running rows show the live ETA and finished rows the real time instead.
  const showEstimate =
    estimate !== null && entry.status !== 'running' && entry.status !== 'done';
  const inputBytes = entry.info?.sizeBytes ?? 0;
  // An unreadable file has no frames to show.
  const selectable = entry.info !== null;

  return (
    <div
      className={cn(
        'group flex items-start gap-3 border-b border-border px-4 py-3',
        selectable && 'cursor-pointer hover:bg-muted/30',
        selected && 'bg-muted/60 hover:bg-muted/60',
      )}
      onClick={selectable ? () => onSelect(entry.path) : undefined}
      aria-selected={selected}
    >
      <Thumbnail entry={entry} />

      <div className="min-w-0 flex-1 space-y-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-[13px] font-medium" title={entry.path}>
            {baseName(entry.path)}
          </span>
          <span className="text-muted-foreground truncate text-[11px]" title={entry.path}>
            {dirName(entry.path)}
          </span>
        </div>

        {entry.info && (
          <div className="text-muted-foreground flex flex-wrap items-center gap-x-1.5 font-mono text-[11px]">
            <span>{describe(entry.info)}</span>
            {showEstimate && (
              <span
                className="text-foreground/80 inline-flex items-center gap-1"
                title={`Estimated time to compress with the current settings, ${
                  basis ? BASIS[basis] : 'on this computer'
                }`}
              >
                <span className="text-muted-foreground">·</span>
                <Timer className="size-3" />
                {formatEstimate(estimate)}
              </span>
            )}
          </div>
        )}

        {entry.status === 'running' && (
          <div className="flex items-center gap-3 pt-1">
            <Progress
              value={(entry.fraction ?? 0) * 100}
              indeterminate={entry.fraction === null}
              className="flex-1"
            />
            <span className="text-muted-foreground w-44 shrink-0 text-right font-mono text-[11px] whitespace-nowrap">
              {entry.fraction !== null && `${Math.round(entry.fraction * 100)}%`}
              {entry.speed !== null && ` · ${entry.speed.toFixed(1)}x`}
              {entry.etaSecs !== null && ` · ${formatDuration(entry.etaSecs)} left`}
            </span>
          </div>
        )}

        {entry.status === 'done' && entry.outputBytes !== null && (
          <div className="flex items-center gap-2 text-[11.5px]">
            <span className="font-mono">
              {formatBytes(inputBytes)} → {formatBytes(entry.outputBytes)}
            </span>
            <Badge
              variant={entry.outputBytes < inputBytes ? 'secondary' : 'destructive'}
              className="font-mono"
            >
              {formatChange(inputBytes, entry.outputBytes)}
            </Badge>
            {entry.elapsedSecs !== null && (
              <span className="text-muted-foreground">
                in {formatDuration(entry.elapsedSecs)}
              </span>
            )}
          </div>
        )}

        {entry.error && (entry.status === 'failed' || entry.status === 'unreadable') && (
          <div className="text-destructive text-[11.5px] break-words">{entry.error}</div>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-1">
        {size !== null && (entry.status === 'ready' || entry.status === 'queued') && (
          <span
            className={cn(
              'font-mono text-[11px] whitespace-nowrap',
              // Green for a saving; amber when it would come out larger.
              size < inputBytes ? 'text-emerald-500' : 'text-amber-500',
            )}
            title={
              size < inputBytes
                ? 'Estimated size with the current settings, from three short sample encodes'
                : 'With these settings this file would come out no smaller'
            }
          >
            {formatChange(inputBytes, size)} ({formatBytes(size)})
          </span>
        )}
        {sizing && (
          <span className="text-muted-foreground inline-flex items-center gap-1 text-[11px] whitespace-nowrap">
            <Loader2 className="size-3 animate-spin" />
            sizing
          </span>
        )}
        <span className={cn('text-[11px]', status.className)}>{status.label}</span>
        {selectable && !selected && (
          <Button
            variant="ghost"
            size="icon-sm"
            title="Preview quality"
            onClick={(event) => {
              event.stopPropagation();
              onSelect(entry.path);
            }}
            className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
          >
            <Eye />
          </Button>
        )}
        {entry.status === 'done' && entry.output && (
          <Button
            variant="ghost"
            size="icon-sm"
            title="Show in folder"
            onClick={(event) => {
              event.stopPropagation();
              void revealItemInDir(entry.output!).catch(() => undefined);
            }}
          >
            <FolderSearch />
          </Button>
        )}
        <Button
          variant="ghost"
          size="icon-sm"
          title="Remove from list"
          disabled={locked && isActive(entry.status)}
          onClick={(event) => {
            event.stopPropagation();
            onRemove(entry.path);
          }}
          className="opacity-0 group-hover:opacity-100 disabled:opacity-0"
        >
          <X />
        </Button>
      </div>
    </div>
  );
});

export function QueueList({
  entries,
  locked,
  adding,
  selected,
  estimates,
  basis,
  sizes,
  sizing,
  onRemove,
  onSelect,
  onAddFiles,
  onAddFolder,
}: {
  entries: Entry[];
  /** A batch is running; its rows cannot be removed. */
  locked: boolean;
  adding: boolean;
  /** The path shown in the preview pane. */
  selected: string | null;
  /** Seconds to compress each path with the current settings. */
  estimates: Map<string, number>;
  basis: Basis | null;
  /** Estimated output bytes for each path with the current settings. */
  sizes: Map<string, number>;
  /** The path whose size is being measured. */
  sizing: string | null;
  onRemove: (path: string) => void;
  onSelect: (path: string) => void;
  onAddFiles: () => void;
  onAddFolder: () => void;
}) {
  if (entries.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="flex w-full max-w-md flex-col items-center gap-4 rounded-2xl border border-dashed border-border px-8 py-12 text-center">
          {adding ? (
            <Loader2 className="text-muted-foreground size-8 animate-spin" />
          ) : (
            <Logo className="h-10 w-auto opacity-80" />
          )}
          <div className="space-y-1">
            <div className="text-sm font-medium">Drop videos or folders here</div>
            <div className="text-muted-foreground text-xs">
              MP4, MOV, MKV, WebM, AVI and more. Folders are read one level deep; camera cards are searched all the way down.
            </div>
          </div>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={onAddFiles}>
              <FileVideo />
              Add videos
            </Button>
            <Button variant="outline" size="sm" onClick={onAddFolder}>
              <FolderOpen />
              Add folder
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <ScrollArea className="h-full">
      {entries.map((entry) => (
        <Row
          key={entry.path}
          entry={entry}
          locked={locked}
          selected={entry.path === selected}
          estimate={estimates.get(entry.path) ?? null}
          basis={basis}
          size={sizes.get(entry.path) ?? null}
          sizing={entry.path === sizing}
          onRemove={onRemove}
          onSelect={onSelect}
        />
      ))}
      {adding && (
        <div className="text-muted-foreground flex items-center gap-2 px-4 py-3 text-xs">
          <Loader2 className="size-3.5 animate-spin" />
          Reading…
        </div>
      )}
    </ScrollArea>
  );
}
