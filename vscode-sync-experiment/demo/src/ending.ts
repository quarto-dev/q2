/** What the launcher does once a scripted story has finished: stay up for free play, or end the run. */
export interface EndPlan {
  /** Wait for Ctrl-C (and `reset` on stdin) after the story, as free play does. */
  hold: boolean;
  /** How long to leave the windows up, in milliseconds, before ending the run. */
  lingerMs: number;
}

/** How long `--exit` leaves the final windows up by default: long enough to see how the story ended. */
export const DEFAULT_LINGER_MS = 5000;

/** `--linger <ms>`: a non-negative number of milliseconds; anything else is the default. */
function parseLinger(raw: string | undefined): number {
  if (raw === undefined || raw.trim() === '') return DEFAULT_LINGER_MS;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? Math.round(n) : DEFAULT_LINGER_MS;
}

/**
 * The paced story used to leave the demo running for free play until Ctrl-C, which stops a run from being followed by
 * another. `--exit` ends the run when the story ends, so stories can run one after another; `--fast` has always ended at
 * once. A failed story ends the run either way (the launcher exits 1).
 */
export function endPlan(o: { play: boolean; fast: boolean; exit: boolean; linger?: string }): EndPlan {
  if (!o.play) return { hold: true, lingerMs: 0 }; // free play: no story to finish
  if (o.fast) return { hold: false, lingerMs: 0 };
  if (o.exit) return { hold: false, lingerMs: parseLinger(o.linger) };
  return { hold: true, lingerMs: 0 };
}
