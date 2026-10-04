'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { CircleAlert, FileVideo, FolderOpen, Play, RefreshCw, Square, Trash2 } from 'lucide-react';

import { Brand, Credit } from '@/components/Credit';
import { LogPanel } from '@/components/logs/LogPanel';
import { QueueList } from '@/components/queue/QueueList';
import { SettingsPanel } from '@/components/settings/SettingsPanel';
import { UpdateDialog } from '@/components/UpdateDialog';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Toaster } from '@/components/ui/sonner';
import type { CompressEvent, LogLine, Summary, ToolStatus } from '@/lib/bindings';
import { playChime } from '@/lib/chime';
import * as ipc from '@/lib/ipc';
import { notify } from '@/lib/notify';
import {
  VIDEO_EXTENSIONS,
  applyEvent,
  markQueued,
  merge,
  runnable,
  unqueue,
  withOutputs,
  type Entry,
} from '@/lib/queue';
import { DEFAULT_SETTINGS, loadSettings, saveSettings, type Settings } from '@/lib/settings';
import { cn, formatBytes, formatChange, formatDuration } from '@/lib/utils';

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

export default function Page() {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [tools, setTools] = useState<ToolStatus | null>(null);
  const [toolError, setToolError] = useState<{ message: string; detail: string | null } | null>(
    null,
  );
  const [lines, setLines] = useState<LogLine[]>([]);
  const [running, setRunning] = useState(false);
  const [adding, setAdding] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [logHeight, setLogHeight] = useState(140);
  const resizing = useRef(false);
  // Read inside event listeners registered once, which would otherwise see
  // the values from the first render.
  const entriesRef = useRef(entries);
  const settingsRef = useRef(settings);
  entriesRef.current = entries;
  settingsRef.current = settings;

  // Settings live in localStorage, which is unavailable during the static
  // export's prerender, so they are read after mount.
  useEffect(() => setSettings(loadSettings()), []);

  const changeSettings = useCallback((next: Settings) => {
    setSettings(next);
    saveSettings(next);
  }, []);

  const log = useCallback((level: string, message: string) => {
    setLines((current) => [...current, { level, message }]);
  }, []);

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

  const addPaths = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
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
      } catch (error) {
        fail("Couldn't read those files", error);
      } finally {
        setAdding(false);
      }
    },
    [log, fail],
  );

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
    const logs = listen<LogLine>(ipc.LOG_EVENT, (event) => {
      setLines((current) => [...current, event.payload]);
    });
    const batches = listen<CompressEvent>(ipc.COMPRESS_EVENT, (event) => {
      const payload = event.payload;
      setEntries((current) => applyEvent(current, payload));
      if (payload.type === 'done') finish(payload.summary);
    });
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
      for (const pending of [logs, batches, drops]) void pending.then((unlisten) => unlisten());
    };
  }, [finish, addPaths]);

  useEffect(() => {
    if (!ipc.inTauri()) return;
    // Startup happens before this component can listen, so collect whatever
    // was logged in the meantime.
    ipc
      .logBacklog()
      .then((backlog) => {
        if (backlog.length > 0) setLines((current) => [...backlog, ...current]);
      })
      .catch(() => {});
    void checkTools();
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

  const start = useCallback(async () => {
    const paths = runnable(entriesRef.current).map((e) => e.path);
    if (paths.length === 0 || running) return;
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
  }, [running, fail]);

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

          <Separator orientation="vertical" className="mx-1 h-5" />

          <span className="text-muted-foreground truncate font-mono text-[11px]">
            {entries.length > 0 && `${plural(entries.length, 'video')} · ${formatBytes(totalBytes)}`}
            {done.length > 0 &&
              ` · saved ${formatBytes(Math.max(doneIn - doneOut, 0))} (${formatChange(doneIn, doneOut)})`}
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

        <section className="min-h-0 flex-1">
          <QueueList
            entries={entries}
            locked={running}
            adding={adding}
            onRemove={(path) => setEntries((c) => c.filter((e) => e.path !== path))}
            onAddFiles={() => void addFiles()}
            onAddFolder={() => void addFolder()}
          />
        </section>

        <div
          role="separator"
          aria-orientation="horizontal"
          onMouseDown={() => {
            resizing.current = true;
          }}
          className="h-1 shrink-0 cursor-row-resize border-t border-border hover:bg-accent"
        />
        <div style={{ height: logHeight }} className="shrink-0 bg-card">
          <LogPanel lines={lines} onClear={() => setLines([])} />
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

      <Toaster position="bottom-right" offset={{ bottom: logHeight + 16, right: 16 }} />
      <UpdateDialog onLog={log} />
    </div>
  );
}
