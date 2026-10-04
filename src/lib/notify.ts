/**
 * Notifications for what an action did.
 *
 * A toast says that something happened, in a few words; it is not where the
 * detail goes. Anything long — a driver error, a parser warning — belongs in
 * the Output panel, where it stays to be read and copied. So every message
 * here is clamped, and a caller with more to say logs it separately.
 */

import { toast } from 'sonner';

const TITLE_LIMIT = 48;
const DESCRIPTION_LIMIT = 72;

function clamp(text: string, limit: number): string {
  const flat = text.replace(/\s+/g, ' ').trim();
  return flat.length <= limit ? flat : `${flat.slice(0, limit - 1).trimEnd()}…`;
}

function options(description?: string) {
  return description ? { description: clamp(description, DESCRIPTION_LIMIT) } : undefined;
}

export const notify = {
  success: (title: string, description?: string) =>
    toast.success(clamp(title, TITLE_LIMIT), options(description)),
  info: (title: string, description?: string) =>
    toast.info(clamp(title, TITLE_LIMIT), options(description)),
  warning: (title: string, description?: string) =>
    toast.warning(clamp(title, TITLE_LIMIT), options(description)),
  error: (title: string, description?: string) =>
    toast.error(clamp(title, TITLE_LIMIT), options(description)),
};

/** The last component of a path, for naming a file without its directory. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}
