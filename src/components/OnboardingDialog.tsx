'use client';

import type { ReactNode } from 'react';
import { Cpu, Eye, FileVideo, Play, SlidersHorizontal } from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

const SEEN_KEY = 'karui.onboarded';

/**
 * Whether this install has shown the guide. In `localStorage`, read after
 * mount like the settings. Storage that cannot be read counts as seen: a
 * guide that cannot remember being closed would greet every launch.
 */
export function seenOnboarding(): boolean {
  try {
    return localStorage.getItem(SEEN_KEY) === '1';
  } catch {
    return true;
  }
}

function markSeen() {
  try {
    localStorage.setItem(SEEN_KEY, '1');
  } catch {
    /* storage disabled: the guide shows again next launch */
  }
}

function Step({
  icon,
  title,
  children,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
}) {
  return (
    <li className="flex gap-3">
      <div className="bg-muted text-muted-foreground flex size-8 shrink-0 items-center justify-center rounded-lg [&_svg]:size-4">
        {icon}
      </div>
      <div className="space-y-0.5">
        <div className="text-[13px] font-medium">{title}</div>
        <p className="text-muted-foreground text-xs leading-relaxed">{children}</p>
      </div>
    </li>
  );
}

function Key({ children }: { children: ReactNode }) {
  return (
    <kbd className="bg-muted text-foreground rounded px-1 py-px font-mono text-[10.5px]">
      {children}
    </kbd>
  );
}

/** What karui does and how to use it, shown on first launch and from the toolbar. */
export function OnboardingDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  // ⌘ on a Mac, Ctrl everywhere else, to match the shortcuts in `page.tsx`.
  const mac =
    typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
  const mod = mac ? '⌘' : 'Ctrl';
  const shift = mac ? '⇧' : 'Shift';
  const enter = mac ? '↵' : 'Enter';

  const change = (next: boolean) => {
    // However it is closed, by the button, Esc, or a click outside.
    if (!next) markSeen();
    onOpenChange(next);
  };

  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Welcome to karui</DialogTitle>
          <DialogDescription>
            karui makes videos lighter. It re-encodes them into smaller MP4s with ffmpeg,
            right here on your computer.
          </DialogDescription>
        </DialogHeader>

        <ol className="space-y-4">
          <Step icon={<FileVideo />} title="Add videos">
            Drop files or folders anywhere in the window, or use Add videos (
            <Key>{mod}</Key> <Key>O</Key>) and Add folder (<Key>{shift}</Key>{' '}
            <Key>{mod}</Key> <Key>O</Key>). Insert a camera card and its new clips are
            picked up automatically. Drag the handle on a row to change the order they
            compress in.
          </Step>
          <Step icon={<SlidersHorizontal />} title="Choose settings">
            H.265 makes the smallest files; H.264 plays on anything. Each file then shows
            about how long it will take and, in green, how much smaller it should come
            out.
          </Step>
          <Step icon={<Eye />} title="Check the quality">
            Click a file to compare one frame before and after, side by side, before you
            commit.
          </Step>
          <Step icon={<Play />} title="Compress">
            Press Compress (<Key>{mod}</Key> <Key>{enter}</Key>). New files are saved
            beside the originals as <span className="font-mono">name-compressed.mp4</span>
            , or in the folder you choose; clips from a camera card go to your import
            folder. Originals are never changed.
          </Step>
        </ol>

        <div className="bg-muted/50 flex gap-3 rounded-xl border border-border p-3">
          <Cpu className="text-muted-foreground mt-0.5 size-4 shrink-0" />
          <p className="text-muted-foreground text-xs leading-relaxed">
            <span className="text-foreground font-medium">Runs on your computer.</span>{' '}
            Nothing is uploaded: compression uses this computer&apos;s own processor, so
            how fast it goes depends on its specs and on what else it is doing. Times and
            sizes shown are estimates measured on this machine, and get closer as you
            compress more.
          </p>
        </div>

        <DialogFooter className="items-center sm:justify-between">
          <span className="text-muted-foreground text-[11px]">
            Open this again with the ? button in the toolbar.
          </span>
          <Button onClick={() => change(false)}>Get started</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
