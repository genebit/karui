'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { CircleAlert, FileVideo, FolderOpen, Play, RefreshCw, Square, Trash2 } from 'lucide-react';

import { CardsBar } from '@/components/cards/CardsBar';
import { Brand, Credit } from '@/components/Credit';
import { LogPanel, type LogEntry } from '@/components/logs/LogPanel';
import { PreviewPane } from '@/components/preview/PreviewPane';
import { QueueList } from '@/components/queue/QueueList';
import { SettingsPanel } from '@/components/settings/SettingsPanel';
import { UpdateDialog } from '@/components/UpdateDialog';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Toaster } from '@/components/ui/sonner';
import type {
  Basis,
  CardSummary,
  CompressEvent,
  LogLine,
  Summary,
  ToolStatus,
} from '@/lib/bindings';
import { playChime } from '@/lib/chime';
import * as ipc from '@/lib/ipc';
import { notify } from '@/lib/notify';
import {
  VIDEO_EXTENSIONS,
  applyEvent,
  markQueued,
  merge,
  move,
  runnable,
  unqueue,
  withOutputs,
  type Entry,
} from '@/lib/queue';
import {
  DEFAULT_SETTINGS,
  defaultCrf,
  loadSettings,
  saveSettings,
  type Settings,
} from '@/lib/settings';
import {
  cn,
  formatBytes,
  formatChange,
  formatDuration,
  formatEstimate,
} from '@/lib/utils';

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

/** Older lines are dropped past this. A batch of hundreds of files logs
 * thousands, and every one kept is a node in the DOM. */
const MAX_LOG_LINES = 1000;

/** Lets settings changes and added files settle before asking for times. */
const ESTIMATE_DEBOUNCE_MS = 300;

/** Size samples are real encodes, so a dragged slider should come to rest
 * before one starts. */
const SIZE_DEBOUNCE_MS = 600;

interface EstimateState {
  secs: Map<string, number>;
  basis: Basis | null;
}

const NO_ESTIMATES: EstimateState = { secs: new Map(), basis: null };

export default function Page() {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [tools, setTools] = useState<ToolStatus | null>(null);
  const [toolError, setToolError] = useState<{ message: string; detail: string | null } | null>(
    null,
  );
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [estimates, setEstimates] = useState<EstimateState>(NO_ESTIMATES);
  // Bumped when an encode teaches the backend a better rate.
  const [learnt, setLearnt] = useState(0);
  const nextLineId = useRef(0);
  const [cards, setCards] = useState<CardSummary[]>([]);
  const [defaultImport, setDefaultImport] = useState<string | null>(null);
  // Card roots seen so far; `null` until the first listing, so cards already
  // in at launch are offered but not started on their own.
  const seenCards = useRef<Set<string> | null>(null);
  const [running, setRunning] = useState(false);
  const [adding, setAdding] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [logHeight, setLogHeight] = useState(140);
  const [selected, setSelected] = useState<string | null>(null);
  const [previewCollapsed, setPreviewCollapsed] = useState(false);
  const [logCollapsed, setLogCollapsed] = useState(false);
  const resizing = useRef(false);
  // Read inside event listeners registered once, which would otherwise see
  // the values from the first render.
  const entriesRef = useRef(entries);
  const settingsRef = useRef(settings);
  const runningRef = useRef(running);
  entriesRef.current = entries;
  settingsRef.current = settings;
  runningRef.current = running;

  // Settings live in localStorage, which is unavailable during the static
  // export's prerender, so they are read after mount.
  useEffect(() => setSettings(loadSettings()), []);

  const changeSettings = useCallback((next: Settings) => {
    setSettings(next);
    saveSettings(next);
  }, []);

  /** Add lines, oldest first, keeping only the newest `MAX_LOG_LINES`. */
  const addLines = useCallback((more: LogLine[], before = false) => {
    const tagged = more.map((line) => ({ ...line, id: nextLineId.current++ }));
    setLines((current) => {
      const next = before ? [...tagged, ...current] : [...current, ...tagged];
      return next.length > MAX_LOG_LINES ? next.slice(-MAX_LOG_LINES) : next;
    });
  }, []);

  const log = useCallback(
    (level: string, message: string) => addLines([{ level, message }]),
    [addLines],
  );

  const clearLines = useCallback(() => setLines([]), []);

  /** A toast for the headline; the full story goes to the Output panel. */
  const fail = useCallback(
    (title: string, error: unknown) => {
      const message = ipc.errorMessage(error);
      const detail = ipc.errorDetail(error);
      if (ipc.isAppError(error) && error.kind === 'tool-missing') {
        setToolError({ message, detail });
      }
      notify.error(title, message);
      log('error', detail ? `${message}. ${detail}` : message);
    },
    [log],
  );

  const checkTools = useCallback(async () => {
    try {
      const status = await ipc.toolStatus();
      setTools(status);
      setToolError(null);
      log('info', `ffmpeg ${status.version} at ${status.ffmpeg}`);
      if (!status.encoders.includes('h265')) {
        log('warn', 'This ffmpeg was built without libx265, so H.265 is unavailable.');
        if (settingsRef.current.options.codec === 'h265') {
          changeSettings({
            ...settingsRef.current,
            options: { ...settingsRef.current.options, codec: 'h264', crf: null },
          });
        }
      }
    } catch (error) {
      setTools(null);
      setToolError({ message: ipc.errorMessage(error), detail: ipc.errorDetail(error) });
      log('error', ipc.errorMessage(error));
    }
  }, [log, changeSettings]);

  /** Probe and list `paths`. Resolves with every readable video among them. */
  const addPaths = useCallback(
    async (paths: string[]): Promise<string[]> => {
      if (paths.length === 0) return [];
      setAdding(true);
      try {
        const probed = await ipc.probePaths(paths);
        const known = new Set(entriesRef.current.map((e) => e.path));
        const fresh = probed.items.filter((item) => !known.has(item.path));
        const unreadable = fresh.filter((item) => !item.info);
        setEntries((current) => merge(current, probed.items));

        for (const skipped of probed.skipped) {
          log('warn', `Skipped ${skipped.path}: ${skipped.reason}`);
        }
        for (const item of unreadable) log('warn', `${item.path}: ${item.error}`);

        if (fresh.length === 0) {
          notify.warning(
            probed.items.length > 0 ? 'Already in the list' : 'No videos found',
            probed.skipped.length > 0 ? `${plural(probed.skipped.length, 'path')} skipped` : undefined,
          );
        } else {
          notify.success(
            `Added ${plural(fresh.length, 'video')}`,
            unreadable.length > 0 ? `${unreadable.length} unreadable; see Output` : undefined,
          );
        }
        return probed.items.filter((item) => item.info).map((item) => item.path);
      } catch (error) {
        fail("Couldn't read those files", error);
        return [];
      } finally {
        setAdding(false);
      }
    },
    [log, fail],
  );

  // `start` is declared further down; card handling reaches it through this.
  const startRef = useRef<(only?: string[]) => Promise<void>>(async () => {});

  /** Add a card's new videos and, when asked, compress them right away. */
  const importVideos = useCallback(
    async (paths: string[], compress: boolean) => {
      const added = await addPaths(paths);
      if (compress && added.length > 0) await startRef.current(added);
    },
    [addPaths],
  );

  const refreshCards = useCallback(async () => {
    try {
      const list = await ipc.listCards();
      setCards(list);
      const seen = seenCards.current;
      seenCards.current = new Set(list.map((card) => card.root));
      if (seen === null) return;
      for (const card of list.filter((c) => !seen.has(c.root))) {
        const fresh = card.fresh.length;
        if (fresh === 0) {
          notify.info(`${card.name} connected`, 'Every video on it is already imported');
        } else if (settingsRef.current.autoImport && !runningRef.current) {
          log('info', `Compressing ${plural(fresh, 'new video')} from ${card.name}`);
          void importVideos(card.fresh, true);
        } else {
          notify.info(
            `${card.name}: ${plural(fresh, 'new video')}`,
            runningRef.current ? 'Added to the list for after this batch' : undefined,
          );
          if (runningRef.current) void importVideos(card.fresh, false);
        }
      }
    } catch (error) {
      log('warn', `Couldn't read camera cards: ${ipc.errorMessage(error)}`);
    }
  }, [log, importVideos]);

  const finish = useCallback(
    (summary: Summary) => {
      setRunning(false);
      if (settingsRef.current.chime) playChime();
      const saved =
        summary.succeeded > 0
          ? `${formatBytes(summary.inputBytes)} → ${formatBytes(summary.outputBytes)} (${formatChange(summary.inputBytes, summary.outputBytes)})`
          : undefined;
      if (summary.failed > 0) {
        notify.error(`${plural(summary.failed, 'video')} failed`, saved);
      } else if (summary.cancelled > 0) {
        notify.info('Stopped', saved);
      } else {
        notify.success(`Compressed ${plural(summary.succeeded, 'video')}`, saved);
      }
      log(
        'info',
        `Batch finished in ${formatDuration(summary.elapsedSecs)}: ${summary.succeeded} done, ` +
          `${summary.failed} failed, ${summary.cancelled} cancelled`,
      );
    },
    [log],
  );

  useEffect(() => {
    if (!ipc.inTauri()) return;
    const logs = listen<LogLine>(ipc.LOG_EVENT, (event) => addLines([event.payload]));
    const batches = listen<CompressEvent>(ipc.COMPRESS_EVENT, (event) => {
      const payload = event.payload;
      setEntries((current) => applyEvent(current, payload));
      if (payload.type === 'finished' && payload.pixelsPerSec !== null) {
        setLearnt((n) => n + 1);
      }
      if (payload.type === 'done') {
        finish(payload.summary);
        // Imported clips are no longer new.
        void refreshCards();
      }
    });
    const devices = listen(ipc.DEVICES_EVENT, () => void refreshCards());
    const drops = getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === 'enter' || payload.type === 'over') setDragging(true);
      else if (payload.type === 'leave') setDragging(false);
      else {
        setDragging(false);
        void addPaths(payload.paths);
      }
    });
    return () => {
      for (const pending of [logs, batches, drops, devices]) {
        void pending.then((unlisten) => unlisten());
      }
    };
  }, [finish, addPaths, addLines, refreshCards]);

  useEffect(() => {
    if (!ipc.inTauri()) return;
    // Startup happens before this component can listen, so collect whatever
    // was logged in the meantime.
    ipc
      .logBacklog()
      .then((backlog) => {
        if (backlog.length > 0) addLines(backlog, true);
      })
      .catch(() => {});
    void checkTools();
    void refreshCards();
    ipc
      .defaultImport()
      .then(setDefaultImport)
      .catch(() => {});
    ipc
      .launchPaths()
      .then((paths) => addPaths(paths))
      .catch(() => {});
    // Once, at mount: re-running would re-probe the launch paths.
  }, []);

  useEffect(() => {
    const move = (event: MouseEvent) => {
      if (!resizing.current) return;
      const fromBottom = window.innerHeight - event.clientY;
      setLogHeight(Math.min(Math.max(fromBottom, 80), window.innerHeight - 240));
    };
    const stop = () => {
      resizing.current = false;
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', stop);
    return () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', stop);
    };
  }, []);

  const addFiles = useCallback(async () => {
    const picked = await openDialog({
      multiple: true,
      filters: [{ name: 'Videos', extensions: VIDEO_EXTENSIONS }],
    });
    if (picked) await addPaths(picked);
  }, [addPaths]);

  const addFolder = useCallback(async () => {
    const picked = await openDialog({ directory: true, multiple: true });
    if (picked) await addPaths(picked);
  }, [addPaths]);

  const pending = runnable(entries);

  /** Compress everything waiting, or only `only` when given. */
  const start = useCallback(async (only?: string[]) => {
    const waiting = runnable(entriesRef.current).map((e) => e.path);
    let paths = waiting;
    if (only) {
      // Just-added paths may not be in `entriesRef` yet; anything listed and
      // already done or running is left alone.
      const listed = new Map(entriesRef.current.map((e) => [e.path, e]));
      const ready = new Set(waiting);
      paths = only.filter((p) => !listed.has(p) || ready.has(p));
    }
    if (paths.length === 0 || runningRef.current) return;
    const batch = new Set(paths);
    setEntries((current) => markQueued(current, batch));
    setRunning(true);
    try {
      const planned = await ipc.startCompression(paths, settingsRef.current.options);
      setEntries((current) =>
        withOutputs(current, new Map(planned.map((job) => [job.input, job.output]))),
      );
    } catch (error) {
      setEntries((current) => unqueue(current, batch));
      setRunning(false);
      fail("Couldn't start", error);
    }
  }, [fail]);
  startRef.current = start;

  const stop = useCallback(async () => {
    try {
      await ipc.cancelCompression();
      log('info', 'Stopping…');
    } catch (error) {
      fail("Couldn't stop", error);
    }
  }, [log, fail]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (mod && event.key.toLowerCase() === 'o') {
        event.preventDefault();
        void (event.shiftKey ? addFolder() : addFiles());
      } else if (mod && event.key === 'Enter') {
        event.preventDefault();
        void (running ? stop() : start());
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [addFiles, addFolder, start, stop, running]);

  const options = settings.options;
  // Only what changes encode time. The paths list changes when files are
  // added or removed, not on progress, so ticks never re-request.
  const timingKey = JSON.stringify([
    options.codec,
    // The effective CRF: `null` and the codec's default are the same encode,
    // and keying them apart would measure every file twice.
    options.crf ?? defaultCrf(options.codec),
    options.preset,
    options.maxFps,
    options.maxResolution,
  ]);
  const estimateKey = JSON.stringify([
    timingKey,
    learnt,
    entries.filter((e) => e.info).map((e) => e.path),
  ]);
  const lastTimingKey = useRef(timingKey);

  // Estimated output sizes by settings and path. Kept across settings, so
  // going back to earlier ones shows their sizes at once. `null` marks a file
  // that could not be sampled, so it is not tried again and again.
  const sizeCache = useRef(new Map<string, number | null>());
  const [sizeVersion, setSizeVersion] = useState(0);
  const [sizing, setSizing] = useState<string | null>(null);
  const sizeKey = JSON.stringify([timingKey, options.audio]);
  const sizeOf = (path: string) => sizeCache.current.get(`${sizeKey}\n${path}`);
  // One file at a time, in list order: the samples are encodes, and two
  // at once would only slow each other and any preview.
  const nextToSize =
    entries.find(
      (e) =>
        e.info?.durationSecs &&
        (e.status === 'ready' || e.status === 'queued') &&
        !sizeCache.current.has(`${sizeKey}\n${e.path}`),
    )?.path ?? null;

  useEffect(() => {
    if (!ipc.inTauri() || running || nextToSize === null) return;
    const entry = entriesRef.current.find((e) => e.path === nextToSize);
    if (!entry?.info) return;
    const key = `${sizeKey}\n${nextToSize}`;
    let stale = false;
    const timer = setTimeout(async () => {
      setSizing(nextToSize);
      try {
        const bytes = await ipc.estimateSize(
          nextToSize,
          entry.info!,
          settingsRef.current.options,
        );
        // Kept even if stale: the key names the settings it was measured with.
        sizeCache.current.set(key, bytes);
      } catch (error) {
        // A preview or a batch took the encoder; this file is asked for again.
        const retry =
          ipc.isAppError(error) && (error.kind === 'cancelled' || error.kind === 'busy');
        if (!retry) {
          sizeCache.current.set(key, null);
          log('warn', `Couldn't estimate the size of ${nextToSize}: ${ipc.errorMessage(error)}`);
        }
      } finally {
        if (!stale) {
          setSizing(null);
          setSizeVersion((v) => v + 1);
        }
      }
    }, SIZE_DEBOUNCE_MS);
    return () => {
      stale = true;
      clearTimeout(timer);
      setSizing(null);
    };
  }, [nextToSize, sizeKey, running, sizeVersion, log]);

  const sizes = new Map<string, number>();
  for (const e of entries) {
    const bytes = sizeOf(e.path);
    if (typeof bytes === 'number') sizes.set(e.path, bytes);
  }

  useEffect(() => {
    if (!ipc.inTauri()) return;
    // Times for other settings would mislead while the new ones are worked out.
    if (lastTimingKey.current !== timingKey) {
      lastTimingKey.current = timingKey;
      setEstimates(NO_ESTIMATES);
    }
    const items = entriesRef.current.filter((e) => e.info);
    if (items.length === 0) return;
    let stale = false;
    const timer = setTimeout(async () => {
      try {
        const result = await ipc.estimateTimes(
          items.map((e) => e.info!),
          settingsRef.current.options,
        );
        if (stale) return;
        const secs = new Map<string, number>();
        result.secs.forEach((value, i) => {
          if (value !== null) secs.set(items[i].path, value);
        });
        setEstimates({ secs, basis: result.basis });
      } catch (error) {
        // A newer request or a starting batch cancels a benchmark in flight.
        if (stale || (ipc.isAppError(error) && error.kind === 'cancelled')) return;
        log('warn', `Couldn't estimate compression times: ${ipc.errorMessage(error)}`);
      }
    }, ESTIMATE_DEBOUNCE_MS);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [estimateKey, timingKey, log]);

  // Stable, so memoised rows and panes skip re-rendering on progress ticks.
  const removeEntry = useCallback(
    (path: string) => setEntries((c) => c.filter((e) => e.path !== path)),
    [],
  );
  const moveEntry = useCallback(
    (path: string, to: number) => setEntries((c) => move(c, path, to)),
    [],
  );
  const selectEntry = useCallback((path: string) => {
    setSelected(path);
    setPreviewCollapsed(false);
  }, []);
  const togglePreview = useCallback(() => setPreviewCollapsed((c) => !c), []);
  const toggleLog = useCallback(() => setLogCollapsed((c) => !c), []);
  const closePreview = useCallback(() => setSelected(null), []);
  const onAddFiles = useCallback(() => void addFiles(), [addFiles]);
  const compressCard = useCallback(
    (paths: string[]) => void importVideos(paths, true),
    [importVideos],
  );
  const listCard = useCallback(
    (paths: string[]) => void importVideos(paths, false),
    [importVideos],
  );
  const onAddFolder = useCallback(() => void addFolder(), [addFolder]);

  // Derived rather than stored, so removing or clearing the row closes the pane.
  const previewing = entries.find((e) => e.path === selected && e.info) ?? null;

  // Time left for everything not yet done: the live ETA of the running file,
  // and the estimate for each one waiting.
  let remainingSecs = 0;
  let timed = false;
  for (const e of entries) {
    const estimate = estimates.secs.get(e.path);
    if (e.status === 'running') {
      const left =
        e.etaSecs ?? (estimate !== undefined ? estimate * (1 - (e.fraction ?? 0)) : null);
      if (left !== null) {
        remainingSecs += left;
        timed = true;
      }
    } else if (estimate !== undefined && e.status !== 'done') {
      remainingSecs += estimate;
      timed = true;
    }
  }

  const done = entries.filter((e) => e.status === 'done');
  const totalBytes = entries.reduce((sum, e) => sum + (e.info?.sizeBytes ?? 0), 0);
  const doneIn = done.reduce((sum, e) => sum + (e.info?.sizeBytes ?? 0), 0);
  const doneOut = done.reduce((sum, e) => sum + (e.outputBytes ?? 0), 0);

  return (
    <div className="flex h-full">
      <aside className="flex w-[272px] shrink-0 flex-col border-r border-border bg-card">
        <Brand />
        <ScrollArea className="min-h-0 flex-1">
          <SettingsPanel
            settings={settings}
            tools={tools}
            disabled={running}
            defaultImport={defaultImport}
            onChange={changeSettings}
          />
        </ScrollArea>
        <Credit />
      </aside>

      <main className="relative flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-1.5 border-b border-border px-3">
          <Button variant="ghost" size="sm" onClick={() => void addFiles()} title="Add videos (⌘O)">
            <FileVideo />
            Add videos
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void addFolder()}
            title="Add folder (⇧⌘O)"
          >
            <FolderOpen />
            Add folder
          </Button>

          <Separator orientation="vertical" className="mx-1 h-5 data-vertical:self-center" />

          <span className="text-muted-foreground truncate font-mono text-[11px]">
            {entries.length > 0 && `${plural(entries.length, 'video')} · ${formatBytes(totalBytes)}`}
            {done.length > 0 &&
              ` · saved ${formatBytes(Math.max(doneIn - doneOut, 0))} (${formatChange(doneIn, doneOut)})`}
            {timed &&
              ` · ${formatEstimate(remainingSecs)} ${running ? 'left' : 'to compress'}`}
          </span>

          <div className="ml-auto flex items-center gap-1.5">
            {done.length > 0 && !running && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setEntries((c) => c.filter((e) => e.status !== 'done'))}
              >
                Clear done
              </Button>
            )}
            {entries.length > 0 && !running && (
              <Button
                variant="ghost"
                size="icon-sm"
                title="Clear the list"
                onClick={() => setEntries([])}
              >
                <Trash2 />
              </Button>
            )}
            {running ? (
              <Button variant="destructive" size="sm" onClick={() => void stop()} title="Stop (⌘↵)">
                <Square />
                Stop
              </Button>
            ) : (
              <Button
                size="sm"
                disabled={pending.length === 0 || toolError !== null}
                onClick={() => void start()}
                title="Compress (⌘↵)"
              >
                <Play />
                Compress{pending.length > 0 && ` ${pending.length}`}
              </Button>
            )}
          </div>
        </header>

        {toolError && (
          <div className="flex items-start gap-3 border-b border-border bg-destructive/10 px-4 py-3">
            <CircleAlert className="text-destructive mt-0.5 size-4 shrink-0" />
            <div className="min-w-0 flex-1 text-xs">
              <div className="font-medium">ffmpeg is needed to compress videos: {toolError.message}</div>
              {toolError.detail && (
                <div className="text-muted-foreground mt-0.5">{toolError.detail}</div>
              )}
            </div>
            <Button variant="outline" size="xs" onClick={() => void checkTools()}>
              <RefreshCw />
              Check again
            </Button>
          </div>
        )}

        <CardsBar
          cards={cards}
          destination={options.outputDir ?? options.importDir ?? defaultImport}
          running={running}
          onCompress={compressCard}
          onAdd={listCard}
        />

        <section className="min-h-24 flex-1">
          <QueueList
            entries={entries}
            locked={running}
            adding={adding}
            selected={previewing?.path ?? null}
            estimates={estimates.secs}
            basis={estimates.basis}
            sizes={sizes}
            sizing={sizing}
            onRemove={removeEntry}
            onSelect={selectEntry}
            onMove={moveEntry}
            onAddFiles={onAddFiles}
            onAddFolder={onAddFolder}
          />
        </section>

        {previewing && (
          <section
            className={cn(
              'border-t border-border bg-card',
              previewCollapsed ? 'shrink-0' : 'min-h-0 flex-[1.6]',
            )}
          >
            <PreviewPane
              key={previewing.path}
              entry={previewing}
              options={options}
              running={running}
              collapsed={previewCollapsed}
              onToggle={togglePreview}
              onClose={closePreview}
            />
          </section>
        )}

        {!logCollapsed && (
          <div
            role="separator"
            aria-orientation="horizontal"
            onMouseDown={() => {
              resizing.current = true;
            }}
            className="h-1 shrink-0 cursor-row-resize border-t border-border hover:bg-accent"
          />
        )}
        <div
          style={logCollapsed ? undefined : { height: logHeight }}
          className={cn('shrink-0 bg-card', logCollapsed && 'border-t border-border')}
        >
          <LogPanel
            lines={lines}
            collapsed={logCollapsed}
            onToggle={toggleLog}
            onClear={clearLines}
          />
        </div>

        <div
          className={cn(
            'pointer-events-none absolute inset-2 rounded-xl border-2 border-dashed border-ring bg-background/70 transition-opacity',
            'flex items-center justify-center text-sm font-medium',
            dragging ? 'opacity-100' : 'opacity-0',
          )}
        >
          Drop to add
        </div>
      </main>

      <Toaster
        position="bottom-right"
        // A collapsed log is only its header, which is h-9.
        offset={{ bottom: (logCollapsed ? 36 : logHeight) + 16, right: 16 }}
      />
      <UpdateDialog onLog={log} />
    </div>
  );
}
