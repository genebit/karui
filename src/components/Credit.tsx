'use client';

import { useEffect, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Settings2 } from 'lucide-react';

import { AppearanceDialog } from '@/components/AppearanceDialog';
import { Logo } from '@/components/Logo';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { applyAppearance, loadAppearance } from '@/lib/appearance';
import { currentVersion } from '@/lib/version';

const GITHUB = 'https://github.com/genebit';

/**
 * The GitHub mark.
 *
 * Drawn here rather than imported: lucide dropped its brand icons at v1, and
 * `lucide-react@1.47` exports nothing matching /github/i.
 */
function GithubMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      aria-hidden="true"
      className={className}
      fill="currentColor"
    >
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.42 7.42 0 0 1 2-.27c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

/**
 * Who wrote this, at the foot of the sidebar.
 *
 * The link opens in the system browser through the opener plugin — a webview
 * has nowhere to navigate to, and `opener:allow-open-url` is already granted
 * in `capabilities/default.json`.
 */
export function Credit() {
  return (
    <button
      type="button"
      onClick={() => void openUrl(GITHUB).catch(() => undefined)}
      title="Open github.com/genebit"
      className="hover:bg-accent/60 flex w-full items-center gap-2.5 border-t border-border px-3 py-2.5 text-left"
    >
      <span className="bg-muted text-muted-foreground flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-[11px] font-semibold">
        GB
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-xs font-semibold">Gene T. Bitara</span>
        <span className="text-muted-foreground block truncate text-[11px]">
          Developer
        </span>
      </span>
      <GithubMark className="text-muted-foreground h-4 w-4 shrink-0" />
    </button>
  );
}

/**
 * The application mark, name, and version, at the head of the sidebar.
 *
 * The version is what to quote in a bug report, and what the update prompt
 * compares against, so it is shown where anyone would look for it. Beside
 * it, the appearance settings.
 */
export function Brand() {
  const [version, setVersion] = useState<string | null>(null);
  const [appearanceOpen, setAppearanceOpen] = useState(false);
  useEffect(() => {
    void currentVersion().then(setVersion);
    // The layout's pre-paint script sets the theme and density; zoom needs
    // the webview, which only this side can reach.
    applyAppearance(loadAppearance());
  }, []);

  return (
    <div className="flex shrink-0 items-center gap-2 px-3 pb-1 pt-3">
      <Logo className="h-5 w-auto shrink-0" />
      <span className="text-sm font-semibold tracking-tight">karui</span>
      <span className="text-muted-foreground text-xs" lang="ja">
        軽い
      </span>
      <div className="ml-auto flex items-center gap-1.5">
        {version && (
          <span className="text-muted-foreground font-mono text-[10px]">v{version}</span>
        )}
        {version && (
          <Separator orientation="vertical" className="h-3.5 data-vertical:self-center" />
        )}
        <Button
          variant="ghost"
          size="icon-xs"
          title="Appearance"
          aria-label="Appearance"
          onClick={() => setAppearanceOpen(true)}
          className="text-muted-foreground"
        >
          <Settings2 />
        </Button>
      </div>
      <AppearanceDialog open={appearanceOpen} onOpenChange={setAppearanceOpen} />
    </div>
  );
}
