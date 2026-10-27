/**
 * Characterization tests for useDocBranches, pinned before the
 * useSyncExternalStore conversion (bd-06s9sufx): the hook exposes the
 * branchService state for the given path and re-renders on service
 * notifications.
 *
 * Drives the real branchService with an injected fake file handle, the
 * same pattern as branchService.test.ts.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { renderHook, act, cleanup } from '@testing-library/react';
import { next as A } from '@automerge/automerge';
import { useDocBranches } from './useDocBranches';
import {
  createBranch,
  setActiveBranch,
  deleteBranch,
  type BranchMeta,
  _resetForTesting,
  _setHandleGetterForTesting,
} from '../services/branchService';

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
  };
}

const PATH = 'report.qmd';

describe('useDocBranches', () => {
  beforeEach(() => {
    localStorage.clear();
    _resetForTesting();
    const handle = makeFakeHandle('hello world');
    _setHandleGetterForTesting((path: string) => (path === PATH ? handle : null));
  });

  afterEach(() => {
    cleanup();
    _resetForTesting();
  });

  it('returns empty state for a null path', () => {
    const { result } = renderHook(() => useDocBranches(null));
    expect(result.current.branches).toEqual([]);
    expect(result.current.activeBranchId).toBeNull();
  });

  it('starts with no branches and main active for a known path', () => {
    const { result } = renderHook(() => useDocBranches(PATH));
    expect(result.current.branches).toEqual([]);
    expect(result.current.activeBranchId).toBeNull();
  });

  it('reflects a branch created through the service', () => {
    const { result } = renderHook(() => useDocBranches(PATH));

    let meta: BranchMeta | null = null;
    act(() => {
      meta = createBranch(PATH, 'draft');
    });

    expect(meta).not.toBeNull();
    expect(result.current.branches).toEqual([meta]);
    // createBranch makes the new branch active.
    expect(result.current.activeBranchId).toBe(meta!.id);
  });

  it('follows setActiveBranch back to main', () => {
    const { result } = renderHook(() => useDocBranches(PATH));
    act(() => {
      createBranch(PATH, 'draft');
    });
    act(() => {
      setActiveBranch(PATH, null);
    });
    expect(result.current.activeBranchId).toBeNull();
  });

  it('reflects deleteBranch (and resets to main when the active branch goes)', () => {
    const { result } = renderHook(() => useDocBranches(PATH));
    let meta: BranchMeta | null = null;
    act(() => {
      meta = createBranch(PATH, 'draft');
    });
    act(() => {
      deleteBranch(PATH, meta!.id);
    });
    expect(result.current.branches).toEqual([]);
    expect(result.current.activeBranchId).toBeNull();
  });

  it('returns empty state for an unknown path', () => {
    const { result } = renderHook(() => useDocBranches('other.qmd'));
    act(() => {
      createBranch(PATH, 'draft');
    });
    expect(result.current.branches).toEqual([]);
    expect(result.current.activeBranchId).toBeNull();
  });

  it('re-resolves when the path prop changes', () => {
    const { result, rerender } = renderHook(({ path }) => useDocBranches(path), {
      initialProps: { path: null as string | null },
    });
    expect(result.current.branches).toEqual([]);

    let meta: BranchMeta | null = null;
    act(() => {
      meta = createBranch(PATH, 'draft');
    });
    rerender({ path: PATH });
    expect(result.current.branches).toEqual([meta]);
    expect(result.current.activeBranchId).toBe(meta!.id);
  });
});
