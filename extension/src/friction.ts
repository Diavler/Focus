// How long a Delay rule makes you wait. It grows with each visit, so returning again and again
// costs more each time. The daemon counts visits for the session (crates/focus-core/src/delay.rs,
// DELAY_BASE and DELAY_MAX); this copy is the fallback when it can't be asked. Keep them in sync.

/** First wait, in seconds. */
export const BASE_DELAY = 10;

/** Longest wait, in seconds. */
export const MAX_DELAY = 120;

/** The wait for the `visits`-th visit to a site (0 for the first). */
export function delaySeconds(visits: number): number {
  return Math.min(BASE_DELAY * (Math.max(0, Math.floor(visits)) + 1), MAX_DELAY);
}
