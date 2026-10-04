import type { Metadata } from 'next';
import './globals.css';
import { Inter } from 'next/font/google';
import { TooltipProvider } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

const inter = Inter({ subsets: ['latin'], variable: '--font-sans' });

/**
 * Applies the saved theme and density before first paint, so a light window
 * does not flash dark while React loads. The key mirrors `APPEARANCE_KEY` in
 * `lib/appearance.ts`, which applies the same choices, and the zoom, after
 * mount.
 */
const PREPAINT = `try {
  var a = JSON.parse(localStorage.getItem('karui.appearance') || '{}');
  var r = document.documentElement;
  if (a.theme === 'light') { r.classList.remove('dark'); r.style.colorScheme = 'light'; }
  if (a.density) r.dataset.density = a.density;
} catch (e) {}`;

export const metadata: Metadata = {
  title: 'karui',
  description: 'Make videos lighter with ffmpeg',
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    // Dark unless the appearance settings say light. The pre-paint script
    // changes the class before React hydrates, hence the warning suppressed.
    <html
      lang="en"
      className={cn('dark font-sans', inter.variable)}
      style={{ colorScheme: 'dark' }}
      suppressHydrationWarning
    >
      <head>
        <script dangerouslySetInnerHTML={{ __html: PREPAINT }} />
      </head>
      <body>
        <TooltipProvider delayDuration={400}>
          <div id="app-root">{children}</div>
        </TooltipProvider>
      </body>
    </html>
  );
}
