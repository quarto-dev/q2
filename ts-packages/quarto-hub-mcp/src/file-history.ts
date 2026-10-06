/**
 * File history plumbing for `get_file_history` and `restore_file_version`
 * (CAP-9 / CAP-19, Phase 3, bd-3qe7unp7).
 *
 * Pure client-side automerge: change summaries come from decoding the
 * document's own change set; attribution follows the author-ID
 * transition contract (`change.author` ?? the actor's seq-1 author
 * footer ?? the bare actor — see hub-client/src/services/
 * attribution-runs.ts), with display names from the index document's
 * `identities` map. Diffs between two heads are rendered as unified
 * line diffs computed here (no new dependency).
 */

import { next as A } from '@automerge/automerge';
import { createHash } from 'node:crypto';
import type { Doc, DecodedChange } from '@automerge/automerge';
import {
  isTextDocument,
  type ActorIdentity,
  type FileDocumentContent,
} from '@quarto/quarto-sync-client';

/** One change summary in a `get_file_history` listing. */
export interface HistoryEntry {
  /** The change's hash — the handle for `from_hash`/`to_hash` and `restore_file_version`. */
  head: string;
  /** sha256 (`sha256:<hex>`) of the file's text after this change — the MCP hash scheme. */
  hash: string;
  seq: number;
  /** Change timestamp (ms epoch), as recorded by its author. */
  time: number;
  /** Attribution key: author ID when known, else the bare actor ID. */
  author: string;
  /** Display identity from the index `identities` map, when recorded. */
  name: string | null;
  color: string | null;
  added_chars: number;
  removed_chars: number;
}

export interface HistoryListResult {
  /** The document's current heads (what a fresh `view` sees). */
  heads: string[];
  entries: HistoryEntry[];
  totalChanges: number;
  truncated: boolean;
}

export class UnknownChangeHashError extends Error {
  override readonly name = 'UnknownChangeHashError';
  constructor(hash: string) {
    super(`unknown change hash "${hash}"`);
  }
}

/** sha256 of a text payload in the MCP hash scheme (matches connection-manager's hashPayload). */
function hashText(text: string): string {
  return `sha256:${createHash('sha256').update(text, 'utf8').digest('hex')}`;
}

/**
 * Character stats of one change: splices count as insertions, deletions
 * as removals (automerge del patches carry no length ⇒ 1 char each).
 */
function changeCharStats(doc: Doc<FileDocumentContent>, decoded: DecodedChange): {
  added: number;
  removed: number;
} {
  const patches = A.diff(doc, decoded.deps, [decoded.hash]);
  let added = 0;
  let removed = 0;
  for (const p of patches) {
    if (p.action === 'splice') {
      added += p.value.length;
    } else if (p.action === 'del') {
      removed += p.length ?? 1;
    }
  }
  return { added, removed };
}

/**
 * Ordered (newest-first) change summaries for a text document.
 *
 * The author footer rides only the actor's seq-1 change (by design —
 * see attribution-runs.ts), so one pass builds actor→author and every
 * later change by the same actor resolves through it.
 */
export function listFileHistory(
  doc: Doc<FileDocumentContent>,
  identities: Record<string, ActorIdentity>,
  limit: number,
): HistoryListResult {
  if (!isTextDocument(doc)) {
    throw new Error('not a text document');
  }
  const heads = A.getHeads(doc);
  const changes = A.getAllChanges(doc);
  const decoded = changes.map((c) => A.decodeChange(c));

  const authorByActor = new Map<string, string>();
  for (const d of decoded) {
    const author = (d as { author?: string | null }).author;
    if (author) authorByActor.set(d.actor, author);
  }

  const entries: HistoryEntry[] = [];
  // getAllChanges is dependency-ordered (oldest first); reverse for
  // newest-first, then bound by limit.
  for (let i = decoded.length - 1; i >= 0 && entries.length < limit; i--) {
    const d = decoded[i]!;
    const author = (d as { author?: string | null }).author ?? authorByActor.get(d.actor) ?? d.actor;
    const atHead = A.view(doc, [d.hash]);
    const text = isTextDocument(atHead) ? atHead.text : '';
    const stats = changeCharStats(doc, d);
    const identity = identities[author];
    entries.push({
      head: d.hash,
      hash: hashText(text),
      seq: d.seq,
      time: d.time,
      author,
      name: identity?.name ?? null,
      color: identity?.color ?? null,
      added_chars: stats.added,
      removed_chars: stats.removed,
    });
  }

  return {
    heads,
    entries,
    totalChanges: decoded.length,
    truncated: decoded.length > entries.length,
  };
}

/** The set of change hashes a document knows (for from/to validation). */
export function knownChangeHashes(doc: Doc<FileDocumentContent>): Set<string> {
  return new Set(A.getAllChanges(doc).map((c) => A.decodeChange(c).hash));
}

/**
 * The document's text at `head`. Throws {@link UnknownChangeHashError}
 * when the hash is not in this document's change set, and a plain Error
 * for a non-text document.
 */
export function textAtHead(doc: Doc<FileDocumentContent>, head: string): string {
  if (!knownChangeHashes(doc).has(head)) {
    throw new UnknownChangeHashError(head);
  }
  const at = A.view(doc, [head]);
  if (!isTextDocument(at)) {
    throw new Error('not a text document');
  }
  return at.text;
}

/**
 * The two texts for `get_file_history`'s diff mode: `fromHash` is
 * required; `toHash` defaults to the document's current state (its
 * first head is reported as the resolved `to_hash`). Both must be known
 * change hashes ({@link UnknownChangeHashError} otherwise).
 */
export function diffFileHistory(
  doc: Doc<FileDocumentContent>,
  fromHash: string,
  toHash?: string,
): { fromText: string; toText: string; resolvedTo: string } {
  if (!isTextDocument(doc)) {
    throw new Error('not a text document');
  }
  const fromText = textAtHead(doc, fromHash);
  if (toHash === undefined) {
    const heads = A.getHeads(doc);
    return { fromText, toText: doc.text, resolvedTo: heads[0] ?? '' };
  }
  return { fromText, toText: textAtHead(doc, toHash), resolvedTo: toHash };
}

// ---------------------------------------------------------------------------
// Unified line diff
// ---------------------------------------------------------------------------

export interface UnifiedDiff {
  diff: string;
  addedLines: number;
  removedLines: number;
}

type DiffOp = { type: ' ' | '-' | '+'; line: string };

/**
 * Above this middle-window size the LCS matrix gets silly; emit one
 * coarse replace hunk instead — still a valid diff, just not minimal.
 */
const MAX_LCS_LINES = 2000;

/**
 * Line-level op list: common prefix/suffix trimmed first (the common
 * case is a small edit in a large file), then a full LCS on the
 * differing middle.
 */
function diffOps(oldLines: string[], newLines: string[]): DiffOp[] {
  let start = 0;
  while (
    start < oldLines.length &&
    start < newLines.length &&
    oldLines[start] === newLines[start]
  ) {
    start++;
  }
  let oldEnd = oldLines.length;
  let newEnd = newLines.length;
  while (oldEnd > start && newEnd > start && oldLines[oldEnd - 1] === newLines[newEnd - 1]) {
    oldEnd--;
    newEnd--;
  }

  const ops: DiffOp[] = oldLines.slice(0, start).map((line) => ({ type: ' ', line }));
  const oldMid = oldLines.slice(start, oldEnd);
  const newMid = newLines.slice(start, newEnd);

  if (oldMid.length > MAX_LCS_LINES || newMid.length > MAX_LCS_LINES) {
    for (const line of oldMid) ops.push({ type: '-', line });
    for (const line of newMid) ops.push({ type: '+', line });
  } else {
    // LCS backtrack over the middle window.
    const m = oldMid.length;
    const n = newMid.length;
    const table = new Uint32Array((m + 1) * (n + 1));
    for (let i = m - 1; i >= 0; i--) {
      for (let j = n - 1; j >= 0; j--) {
        table[i * (n + 1) + j] =
          oldMid[i] === newMid[j]
            ? table[(i + 1) * (n + 1) + j + 1]! + 1
            : Math.max(table[(i + 1) * (n + 1) + j]!, table[i * (n + 1) + j + 1]!);
      }
    }
    let i = 0;
    let j = 0;
    while (i < m && j < n) {
      if (oldMid[i] === newMid[j]) {
        ops.push({ type: ' ', line: oldMid[i]! });
        i++;
        j++;
      } else if (table[(i + 1) * (n + 1) + j]! >= table[i * (n + 1) + j + 1]!) {
        ops.push({ type: '-', line: oldMid[i]! });
        i++;
      } else {
        ops.push({ type: '+', line: newMid[j]! });
        j++;
      }
    }
    for (; i < m; i++) ops.push({ type: '-', line: oldMid[i]! });
    for (; j < n; j++) ops.push({ type: '+', line: newMid[j]! });
  }

  for (const line of oldLines.slice(oldEnd)) ops.push({ type: ' ', line });
  return ops;
}

/**
 * Render a unified diff between two texts (git-style, 3 lines of
 * context). A trailing newline is a line terminator, not a separate
 * empty line; a missing one is flagged `\ No newline at end of file`.
 */
export function formatUnifiedDiff(
  oldText: string,
  newText: string,
  path: string,
  context = 3,
): UnifiedDiff {
  const oldLines = oldText === '' ? [] : oldText.split('\n');
  const newLines = newText === '' ? [] : newText.split('\n');
  // split('\n') leaves a trailing '' for newline-terminated text; drop it
  // so every array entry is a real line, and remember the terminator.
  const oldHadNl = oldText.endsWith('\n');
  const newHadNl = newText.endsWith('\n');
  if (oldLines.length > 0 && oldLines[oldLines.length - 1] === '' && oldHadNl) oldLines.pop();
  if (newLines.length > 0 && newLines[newLines.length - 1] === '' && newHadNl) newLines.pop();

  const ops = diffOps(oldLines, newLines);

  // Indices of changed ops; hunks group changes whose unchanged gap is
  // at most 2*context (so each hunk shows every change with `context`
  // lines of runaround on each side).
  const changedIdx: number[] = [];
  for (let k = 0; k < ops.length; k++) {
    if (ops[k]!.type !== ' ') changedIdx.push(k);
  }
  interface Hunk {
    oldStart: number;
    newStart: number;
    lines: DiffOp[];
  }
  const hunks: Hunk[] = [];
  let addedLines = 0;
  let removedLines = 0;
  for (const op of ops) {
    if (op.type === '-') removedLines++;
    else if (op.type === '+') addedLines++;
  }
  let g = 0;
  while (g < changedIdx.length) {
    // One group: from changedIdx[g] through every following change
    // separated by at most 2*context unchanged ops.
    let last = g;
    while (
      last + 1 < changedIdx.length &&
      changedIdx[last + 1]! - changedIdx[last]! <= 2 * context
    ) {
      last++;
    }
    const from = Math.max(0, changedIdx[g]! - context);
    const to = Math.min(ops.length - 1, changedIdx[last]! + context);
    // Line numbers at the hunk start (1-based).
    let oldStart = 1;
    let newStart = 1;
    for (let k = 0; k < from; k++) {
      if (ops[k]!.type !== '+') oldStart++;
      if (ops[k]!.type !== '-') newStart++;
    }
    hunks.push({ oldStart, newStart, lines: ops.slice(from, to + 1) });
    g = last + 1;
  }

  let diff = `--- a/${path}\n+++ b/${path}\n`;
  for (const h of hunks) {
    const oldCount = h.lines.filter((l) => l.type !== '+').length;
    const newCount = h.lines.filter((l) => l.type !== '-').length;
    diff += `@@ -${h.oldStart},${oldCount} +${h.newStart},${newCount} @@\n`;
    for (const l of h.lines) {
      diff += `${l.type}${l.line}\n`;
    }
  }
  // No-newline markers, mirroring git (only when the texts actually differ).
  if (oldText !== newText) {
    if (!oldHadNl && oldText !== '') {
      diff = diff.replace(/(-[^\n]*)\n$/, '$1\n\\ No newline at end of file\n');
    }
    if (!newHadNl && newText !== '') {
      diff = diff.replace(/(\+[^\n]*)\n$/, '$1\n\\ No newline at end of file\n');
    }
  }
  return { diff, addedLines, removedLines };
}
