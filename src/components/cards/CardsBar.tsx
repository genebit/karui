'use client';

import { memo } from 'react';
import { HardDrive, ListPlus, Play } from 'lucide-react';

import { Button } from '@/components/ui/button';
import type { CardSummary } from '@/lib/bindings';
import { baseName, formatBytes } from '@/lib/utils';

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

/**
 * One row per mounted camera card. Its videos are compressed where they sit,
 * so the only actions are to compress the new ones or list them first.
 */
export const CardsBar = memo(function CardsBar({
  cards,
  destination,
  running,
  onCompress,
  onAdd,
}: {
  cards: CardSummary[];
  /** Where card videos will be saved. */
  destination: string | null;
  running: boolean;
  /** Add these paths and compress them straight away. */
  onCompress: (paths: string[]) => void;
  /** Add these paths to the list without starting. */
  onAdd: (paths: string[]) => void;
}) {
  if (cards.length === 0) return null;

  return (
    <div className="border-b border-border">
      {cards.map((card) => {
        const fresh = card.fresh.length;
        return (
          <div key={card.root} className="flex items-center gap-3 bg-card px-4 py-2.5">
            <HardDrive className="text-muted-foreground size-4 shrink-0" />
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline gap-2">
                <span className="truncate text-[13px] font-medium" title={card.root}>
                  {card.name}
                </span>
                <span className="text-muted-foreground text-[11px]">camera card</span>
              </div>
              <div className="text-muted-foreground truncate font-mono text-[11px]">
                {plural(card.videos, 'video')} · {formatBytes(card.bytes)}
                {fresh > 0 ? (
                  <span className="text-foreground">
                    {' '}
                    · {fresh} new ({formatBytes(card.freshBytes)})
                  </span>
                ) : (
                  card.videos > 0 && ' · all imported'
                )}
                {destination && fresh > 0 && (
                  <span title={destination}> → {baseName(destination)}</span>
                )}
              </div>
            </div>

            {fresh > 0 && (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => onAdd(card.fresh)}
                  title="Add the new videos to the list without starting"
                >
                  <ListPlus />
                  Add to list
                </Button>
                <Button
                  size="sm"
                  disabled={running}
                  onClick={() => onCompress(card.fresh)}
                  title={
                    running
                      ? 'Available when the current batch finishes'
                      : `Compress straight from the card into ${destination ?? 'the import folder'}`
                  }
                >
                  <Play />
                  Compress {fresh} new
                </Button>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
});
