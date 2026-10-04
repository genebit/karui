import type { Metadata } from 'next';
import './globals.css';
import { Inter } from 'next/font/google';
import { TooltipProvider } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

const inter = Inter({ subsets: ['latin'], variable: '--font-sans' });

export const metadata: Metadata = {
  title: 'karui',
  description: 'Make videos lighter with ffmpeg',
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    // Dark-only, fixed by this class rather than offered as a toggle.
    <html lang="en" className={cn('dark font-sans', inter.variable)}>
      <body>
        <TooltipProvider delayDuration={400}>
          <div id="app-root">{children}</div>
        </TooltipProvider>
      </body>
    </html>
  );
}
