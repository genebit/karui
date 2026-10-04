export { cn } from 'cn';

/**
 * Decimal units, as file managers show them. Mirrors `karui_core::units`, so
 * the app and the CLI quote the same numbers.
 */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  const units = ['kB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 'B';
  for (const next of units) {
    if (value < 1000) break;
    value /= 1000;
    unit = next;
  }
  return `${value.toFixed(1)} ${unit}`;
}

/** `75.4` → `1:15`, `3725` → `1:02:05`. */
export function formatDuration(secs: number): string {
  const total = Math.max(0, Math.round(secs));
  const h = Math.floor(total / 3600);
  const m = Math.floor(total / 60) % 60;
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** Size change as a signed percentage, e.g. `−74%`. */
export function formatChange(input: number, output: number): string {
  if (input === 0) return 'n/a';
  const pct = Math.round((output / input - 1) * 100);
  if (pct < 0) return `−${-pct}%`;
  if (pct > 0) return `+${pct}%`;
  return '±0%';
}

/** The last component of a path. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** Everything before the last component. */
export function dirName(path: string): string {
  const index = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return index > 0 ? path.slice(0, index) : '';
}
