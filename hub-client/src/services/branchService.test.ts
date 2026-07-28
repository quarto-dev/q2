/**
 * Tests for branchService — local-only document branches.
 *
 * A branch is a plain `@automerge/automerge` clone of a file's doc, held in
 * memory and persisted to localStorage (never registered with a Repo, so it
 * can never sync). "Merge to main" is a CRDT merge back into the file's
 * DocHandle via `handle.update`.
 *
 * The file DocHandle is obtained through an injectable getter
 * (`_setHandleGetterForTesting`); tests drive a fake handle wrapping a real
 * automerge doc so merge semantics are exercised for real.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { next as A } from '@automerge/automerge';
import {
  getBranches,
  getActiveBranchId,
  setActiveBranch,
  createBranch,
  getBranchText,
  applyBranchEdits,
  mergeBranchToMain,
  deleteBranch,
  subscribe,
  _resetForTesting,
  _setHandleGetterForTesting,
} from './branchService';

interface TextDoc {
  text: string;
}

/** Minimal stand-in for a DocHandle over a real automerge doc. */
function makeFakeHandle(initialText: string, documentId = 'doc-test-1') {
  let doc = A.from<TextDoc>({ text: initialText });
  return {
    documentId,
    doc: () => doc,
    update: (cb: (d: A.Doc<TextDoc>) => A.Doc<TextDoc>) => {
      doc = cb(doc);
    },
    change: (cb: (d: TextDoc) => void) => {
      doc = A.change(doc, cb);
    },
  };
}

const PATH = 'report.qmd';

describe('branchService', () => {
  let handle: ReturnType<typeof makeFakeHandle>;

  beforeEach(() => {
    localStorage.clear();
    _resetForTesting();
    handle = makeFakeHandle('hello world');
    _setHandleGetterForTesting((path: string) => (path === PATH ? handle : null));
  });

  it('starts with no branches and main active', () => {
    expect(getBranches(PATH)).toEqual([]);
    expect(getActiveBranchId(PATH)).toBeNull();
  });

  it('createBranch forks the current main text and activates the branch', () => {
    const meta = createBranch(PATH, 'experiment');
    expect(meta).not.toBeNull();
    expect(meta!.name).toBe('experiment');
    expect(getBranches(PATH)).toHaveLength(1);
    expect(getActiveBranchId(PATH)).toBe(meta!.id);
    expect(getBranchText(PATH, meta!.id)).toBe('hello world');
  });

  it('generates unique default names', () => {
    const a = createBranch(PATH)!;
    const b = createBranch(PATH)!;
    expect(a.name).not.toBe(b.name);
  });

  it('returns null when the file has no handle', () => {
    expect(createBranch('nonexistent.qmd')).toBeNull();
  });

  it('applyBranchEdits edits the branch but not main', () => {
    const meta = createBranch(PATH)!;
    // Replace "world" -> "branch" (offset 6, length 5)
    applyBranchEdits(PATH, meta.id, [{ rangeOffset: 6, rangeLength: 5, text: 'branch' }]);
    expect(getBranchText(PATH, meta.id)).toBe('hello branch');
    expect((handle.doc() as TextDoc).text).toBe('hello world');
  });

  it('forking while a branch is active forks that branch state', () => {
    const first = createBranch(PATH)!;
    applyBranchEdits(PATH, first.id, [{ rangeOffset: 0, rangeLength: 0, text: 'A: ' }]);
    const second = createBranch(PATH)!;
    expect(getBranchText(PATH, second.id)).toBe('A: hello world');
  });

  it('persists branches to localStorage and restores after reset (simulated reload)', () => {
    const meta = createBranch(PATH, 'kept')!;
    applyBranchEdits(PATH, meta.id, [{ rangeOffset: 0, rangeLength: 5, text: 'howdy' }]);

    // Simulate reload: drop in-memory state, keep localStorage.
    _resetForTesting({ keepStorage: true });
    _setHandleGetterForTesting((path: string) => (path === PATH ? handle : null));

    const restored = getBranches(PATH);
    expect(restored).toHaveLength(1);
    expect(restored[0].name).toBe('kept');
    expect(getActiveBranchId(PATH)).toBeNull(); // selection is session-only
    expect(getBranchText(PATH, restored[0].id)).toBe('howdy world');
  });

  it('mergeBranchToMain merges concurrent branch and main edits (CRDT merge)', () => {
    const meta = createBranch(PATH)!;
    // Branch edit at the end...
    applyBranchEdits(PATH, meta.id, [{ rangeOffset: 11, rangeLength: 0, text: '!' }]);
    // ...while main gets a concurrent edit at the start.
    handle.change((d) => {
      A.splice(d, ['text'], 0, 0, '# ');
    });

    const ok = mergeBranchToMain(PATH, meta.id);
    expect(ok).toBe(true);
    expect((handle.doc() as TextDoc).text).toBe('# hello world!');
    // Merged branch is deleted and main becomes active again.
    expect(getBranches(PATH)).toHaveLength(0);
    expect(getActiveBranchId(PATH)).toBeNull();
  });

  it('deleteBranch removes the branch and its storage, resetting active to main', () => {
    const meta = createBranch(PATH)!;
    expect(localStorage.length).toBeGreaterThan(0);

    deleteBranch(PATH, meta.id);
    expect(getBranches(PATH)).toHaveLength(0);
    expect(getActiveBranchId(PATH)).toBeNull();
    expect(getBranchText(PATH, meta.id)).toBeNull();
    // No orphaned branch-doc blobs left behind.
    const keys = Object.keys(localStorage).filter((k) => k.includes(meta.id));
    expect(keys).toEqual([]);
  });

  it('setActiveBranch switches between main and branches', () => {
    const meta = createBranch(PATH)!;
    setActiveBranch(PATH, null);
    expect(getActiveBranchId(PATH)).toBeNull();
    setActiveBranch(PATH, meta.id);
    expect(getActiveBranchId(PATH)).toBe(meta.id);
  });

  it('notifies subscribers on structural changes', () => {
    let calls = 0;
    const unsub = subscribe(() => {
      calls += 1;
    });
    const meta = createBranch(PATH)!;
    setActiveBranch(PATH, null);
    deleteBranch(PATH, meta.id);
    expect(calls).toBeGreaterThanOrEqual(3);
    unsub();
    const before = calls;
    createBranch(PATH);
    expect(calls).toBe(before);
  });
});
