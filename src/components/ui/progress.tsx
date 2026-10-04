'use client';

import * as React from 'react';
import { Progress as ProgressPrimitive } from 'radix-ui';

import { cn } from '@/lib/utils';

/**
 * A progress bar with two honest modes.
 *
 * `indeterminate` sweeps, for work whose length is genuinely unknown — opening
 * a database connection reports no position and can take ten seconds or more.
 * Otherwise it fills to `value`, which callers should only pass when the
 * number means something.
 */
function Progress({
  className,
  value,
  indeterminate = false,
  ...props
}: React.ComponentProps<typeof ProgressPrimitive.Root> & {
  indeterminate?: boolean;
}) {
  return (
    <ProgressPrimitive.Root
      data-slot="progress"
      value={indeterminate ? null : value}
      className={cn(
        'relative h-1.5 w-full overflow-hidden rounded-full bg-primary/20',
        className,
      )}
      {...props}
    >
      {indeterminate ? (
        <ProgressPrimitive.Indicator
          data-slot="progress-indicator"
          className="progress-sweep h-full w-1/5 rounded-full bg-primary"
        />
      ) : (
        <ProgressPrimitive.Indicator
          data-slot="progress-indicator"
          className="h-full w-full flex-1 bg-primary transition-transform duration-300 ease-out"
          style={{ transform: `translateX(-${100 - (value ?? 0)}%)` }}
        />
      )}
    </ProgressPrimitive.Root>
  );
}

export { Progress };
