'use client';

import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  DEFAULT_APPEARANCE,
  SCALES,
  applyAppearance,
  loadAppearance,
  saveAppearance,
  type Appearance,
  type Density,
  type Theme,
} from '@/lib/appearance';
import { cn } from '@/lib/utils';

const THEMES: { value: Theme; label: string }[] = [
  { value: 'dark', label: 'Dark' },
  { value: 'light', label: 'Light' },
];

const DENSITIES: { value: Density; label: string }[] = [
  { value: 'compact', label: 'Compact' },
  { value: 'default', label: 'Default' },
  { value: 'relaxed', label: 'Relaxed' },
];

/** One row of choices, drawn like the encoder and codec pickers. */
function Choices<T extends string | number>({
  label,
  hint,
  options,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="space-y-1.5">
      <div className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
        {label}
      </div>
      <div
        className="grid gap-1.5"
        style={{ gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))` }}
      >
        {options.map((option) => (
          <Button
            key={String(option.value)}
            variant={value === option.value ? 'secondary' : 'ghost'}
            onClick={() => onChange(option.value)}
            className={cn(
              'h-8 rounded-lg border text-xs',
              value === option.value ? 'border-ring/60' : 'border-border',
            )}
          >
            {option.label}
          </Button>
        ))}
      </div>
      <p className="text-muted-foreground text-[10.5px] leading-snug">{hint}</p>
    </div>
  );
}

/** Theme, size, and list density. Each choice applies and saves at once. */
export function AppearanceDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [appearance, setAppearance] = useState<Appearance>(DEFAULT_APPEARANCE);
  // Read on opening rather than at first render, which is also the static
  // export's prerender, where there is no `localStorage`.
  useEffect(() => {
    if (open) setAppearance(loadAppearance());
  }, [open]);

  const change = (patch: Partial<Appearance>) => {
    const next = { ...appearance, ...patch };
    setAppearance(next);
    saveAppearance(next);
    applyAppearance(next);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>Appearance</DialogTitle>
          <DialogDescription>Changes apply at once and are remembered.</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <Choices
            label="Theme"
            hint="Colour stays reserved for errors and warnings in both."
            options={THEMES}
            value={appearance.theme}
            onChange={(theme) => change({ theme })}
          />
          <Choices
            label="Font size"
            hint="Scales text and the controls around it together."
            options={SCALES}
            value={appearance.scale}
            onChange={(scale) => change({ scale })}
          />
          <Choices
            label="Row density"
            hint="How much space each video in the list takes."
            options={DENSITIES}
            value={appearance.density}
            onChange={(density) => change({ density })}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}
