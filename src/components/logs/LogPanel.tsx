'use client';

import { memo, useEffect, useRef } from 'react';
import { ChevronDown, ChevronUp, Trash2 } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import type { LogLine } from '@/lib/bindings';
import { cn } from '@/lib/utils';

const LEVEL_COLOR: Record<string, string> = {
  error: 'text-destructive',
  warn: 'text-amber-500',
  info: 'text-muted-foreground',
  debug: 'text-muted-foreground/70',
  trace: 'text-muted-foreground/70',
};

/** A line with an id that survives older lines being dropped. */
export interface LogEntry extends LogLine {
  id: number;
}

const Line = memo(function Line({ line }: { line: LogEntry }) {
  return (
    <div className="flex gap-2">
      <span
        className={cn(
          'w-10 shrink-0 uppercase',
          LEVEL_COLOR[line.level] ?? 'text-muted-foreground',
        )}
      >
        {line.level}
      </span>
      <span className="whitespace-pre-wrap break-all text-muted-foreground">
        {line.message}
      </span>
    </div>
  );
});

/**
 * Memoised, and each line too: the page re-renders on every progress tick,
 * and a long log re-rendered twice a second is the slowest thing in the app.
 */
export const LogPanel = memo(function LogPanel({
  lines,
  collapsed,
  onToggle,
  onClear,
}: {
  lines: LogEntry[];
  collapsed: boolean;
  onToggle: () => void;
  onClear: () => void;
}) {
  const bottom = useRef<HTMLDivElement>(null);

  // The newest id, not the length: once the log is capped its length stops
  // changing while lines keep arriving. Reopening scrolls too, since lines
  // that arrived while collapsed were never scrolled to.
  const newest = lines.at(-1)?.id;
  useEffect(() => {
    bottom.current?.scrollIntoView({ block: 'end' });
  }, [newest, collapsed]);

  const flagged = lines.filter((l) => l.level === 'warn' || l.level === 'error').length;

  const header = (
    <div
      className={cn(
        'flex h-9 shrink-0 items-center gap-2 px-2',
        !collapsed && 'border-b border-border',
      )}
    >
      <Button
        variant="ghost"
        size="icon-xs"
        title={collapsed ? 'Show output' : 'Collapse output'}
        onClick={onToggle}
      >
        {collapsed ? <ChevronUp /> : <ChevronDown />}
      </Button>
      <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
        Output
      </span>
      <span className="font-mono text-[11px] text-muted-foreground">
        {lines.length} line{lines.length === 1 ? '' : 's'}
        {flagged > 0 && <span className="text-amber-500"> · {flagged} flagged</span>}
      </span>
      <Button
        variant="ghost"
        size="icon-xs"
        onClick={onClear}
        title="Clear"
        className="ml-auto"
      >
        <Trash2 />
      </Button>
    </div>
  );

  if (collapsed) return header;

  return (
    <div className="flex h-full flex-col">
      {header}

      <ScrollArea className="min-h-0 flex-1">
        <div className="px-3 py-1 font-mono text-[11.5px] leading-[1.6]">
          {lines.length === 0 ? (
            <div className="py-2 text-muted-foreground">
              ffmpeg, probe, and compression messages appear here.
            </div>
          ) : (
            lines.map((line) => <Line key={line.id} line={line} />)
          )}
          <div ref={bottom} />
        </div>
      </ScrollArea>
    </div>
  );
});
