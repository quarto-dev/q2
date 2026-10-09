/**
 * Position of the current entry in the SPA's own navigation, kept in `history.state` (P11-D4).
 * The browser restores `history.state` on Back and Forward, so the index stays right however the
 * user moves; a hand-kept counter ignores those moves and would let the Back button leave the SPA.
 * An entry with no state (the boot entry, or one pushed by older code) is index 0.
 */
export function readNavIndex(): number {
  const idx = (window.history.state as { q2NavIndex?: unknown } | null)?.q2NavIndex;
  return typeof idx === 'number' && Number.isInteger(idx) && idx >= 0 ? idx : 0;
}

/** Stamp an entry that carries no index with 0, so a later read never depends on a null state. */
export function seedNavIndex(): void {
  const state = window.history.state as { q2NavIndex?: unknown } | null;
  if (typeof state?.q2NavIndex === 'number') return;
  window.history.replaceState({ ...(state ?? {}), q2NavIndex: 0 }, '');
}

/** `history.state` for the entry a navigation pushes: one past the current one. */
export function nextNavState(): { q2NavIndex: number } {
  return { q2NavIndex: readNavIndex() + 1 };
}
