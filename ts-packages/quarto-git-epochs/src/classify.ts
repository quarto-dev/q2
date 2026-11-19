/**
 * Classifier (G2): is a change to the working tree a commit or a checkout?
 * Pure; no I/O. See claude-notes/designs/git-epoch-registry-contract.md, "Classifier".
 */
export type Verdict = 'commit' | 'checkout';

export interface ChangedFile {
  path: string;
  /** Disk content (CRLF-normalized text, or a byte hash for binary files). */
  disk: string;
  /** The content last synced to the doc. */
  synced: string;
  /** The content of the blob at the new HEAD. */
  blob: string;
}

export interface ClassifyInput {
  peer: 'hub' | 'extension';
  /** Reflog subjects since the last decision; null (or empty) when there is no reflog, or under colocated jj. */
  reflog: string[] | null;
  changed: ChangedFile[];
}

/** `commit: ...`, `commit (amend): ...`, `commit (initial): ...`; not `commit (merge)`. */
const PLAIN_COMMIT = /^commit(?: \((?:amend|initial)\))?:/;

export function classify(input: ClassifyInput): Verdict {
  // Content rule first: every changed file's disk equals what was last synced.
  if (input.changed.every((f) => f.disk === f.synced)) return 'commit';
  // Tiebreaker: a span made only of plain commits means the author edited and committed.
  const log = input.reflog;
  if (log && log.length > 0 && log.every((s) => PLAIN_COMMIT.test(s))) return 'commit';
  return 'checkout';
}
