/** Event vocabulary (contract: "Event vocabulary"); equal to fixtures/vocabulary.json. */
export const EVENTS = [
  'flipped',
  'switched',
  'detached',
  'reattached',
  'held',
  'reset',
  'retired',
  'moved-forward',
  'joined',
  'seeded',
] as const;
export type EventKind = (typeof EVENTS)[number];

/** Swap phases (G26). */
export const SWAP_EVENTS = ['pending', 'done', 'unavailable'] as const;
export type SwapEvent = (typeof SWAP_EVENTS)[number];

/** The numbers (G32; contract: "Numbers"), in milliseconds unless noted. */
export const NUMBERS = {
  quietWindowMs: 1000,
  decisionCapMs: 30_000,
  staleLockMs: 60_000,
  unavailableCanonicalMs: 5000,
  controllerTickPendingMs: 250,
  mtimeSkipMs: 500,
  registrySoftLimitPaths: 15_000,
} as const;
