/**
 * The owner hook (GE-10): the clone controller calls `applyDecision` after each decision with what the owner needs to
 * write into the pointer (`files[path]` in the index). The default does nothing; GE-12 supplies the owner
 * implementation. This plan never writes the pointer.
 */
export interface PointerBatch {
  /** Paths whose doc changed (checkout, or a doc created from disk). */
  flips: { path: string; docId: string }[];
  /** Tracked files git removed. */
  removals: string[];
  /** Files that returned, at the registry's doc. */
  relists: { path: string; docId: string }[];
  /** Files git added; the owner publishes a fresh doc each. */
  additions: string[];
  resets: { path: string; retiredDocId: string; newDocId: string; discarded: number }[];
}

export interface PointerWriter {
  /** One index change per call. */
  applyDecision(batch: PointerBatch): Promise<void>;
}

export const NOOP_POINTER_WRITER: PointerWriter = { async applyDecision() {} };

export const emptyBatch = (): PointerBatch => ({ flips: [], removals: [], relists: [], additions: [], resets: [] });

export const isEmptyBatch = (b: PointerBatch): boolean =>
  b.flips.length + b.removals.length + b.relists.length + b.additions.length + b.resets.length === 0;
