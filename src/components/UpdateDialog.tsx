'use client';

import { useEffect, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Download } from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { RELEASES_URL, currentVersion, isNewer, latestVersion } from '@/lib/version';

/**
 * Offers a newer release, once, at launch.
 *
 * It only points the way. Installing is left to the user, who picks their
 * platform's bundle from the releases page: the bundles are unsigned, so an
 * in-place updater would have nothing to verify a download against.
 */
export function UpdateDialog({
  onLog,
  deferred = false,
}: {
  onLog: (level: string, message: string) => void;
  /** Wait to appear, for instance until the first-launch guide is closed. */
  deferred?: boolean;
}) {
  const [open, setOpen] = useState(false);
  // Kept apart from `open` so the text does not blank while the dialog
  // animates closed.
  const [versions, setVersions] = useState({ current: '', latest: '' });

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const current = await currentVersion();
      if (current === null) return;
      try {
        const latest = await latestVersion();
        if (cancelled || !isNewer(latest, current)) return;
        setVersions({ current, latest });
        setOpen(true);
        onLog('info', `karui v${latest} is available; this is v${current}`);
      } catch (error) {
        // `info`, not `warn`: offline is not a fault, and a warning would
        // badge the Output panel on every launch without a connection.
        if (!cancelled) {
          const reason = error instanceof Error ? error.message : String(error);
          onLog('info', `Couldn't check for updates: ${reason}`);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [onLog]);

  return (
    <Dialog open={open && !deferred} onOpenChange={setOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Update available</DialogTitle>
          <DialogDescription>
            karui v{versions.latest} is out. You have v{versions.current}.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            Later
          </Button>
          <Button
            onClick={() => {
              void openUrl(RELEASES_URL).catch(() => undefined);
              setOpen(false);
            }}
          >
            <Download />
            Download
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
