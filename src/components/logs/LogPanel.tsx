'use client';

import { useEffect, useRef } from 'react';
import { Trash2 } from 'lucide-react';

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

export function LogPanel({
  lines,
  onClear,
}: {
  lines: LogLine[];
  onClear: () => void;
}) {
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: 'end' });
  }, [lines.length]);

  const flagged = lines.filter((l) => l.level === 'warn' || l.level === 'error').length;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
        <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
          Output
        </span>
        <span className="font-mono text-[11px] text-muted-foreground">
          {lines.length} line{lines.length === 1 ? '' : 's'}
          {flagged > 0 && <span className="text-amber-500"> · {flagged} flagged</span>}
        </span>
        <Button
          variant="ghost"
          size="icon"
          onClick={onClear}
          title="Clear"
          className="ml-auto h-6 w-6"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="px-3 py-1 font-mono text-[11.5px] leading-[1.6]">
          {lines.length === 0 ? (
            <div className="py-2 text-muted-foreground">
              ffmpeg, probe, and compression messages appear here.
            </div>
          ) : (
            lines.map((line, index) => (
              <div key={index} className="flex gap-2">
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
            ))
          )}
          <div ref={bottom} />
        </div>
      </ScrollArea>
    </div>
  );
}
