/**
 * Live (non-replay) coverage for the attribution hook: the payload must
 * always describe the text of the render it is returned in, however fast
 * edits arrive, and on a local branch it must come from the branch's doc.
 * Uses the real Automerge producer.
 *
 * @vitest-environment jsdom
 */

import { renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';

const getFileHandle = vi.fn();
vi.mock('@quarto/preview-runtime', () => ({
  getFileHandle: (path: string) => getFileHandle(path),
}));
const getBranchDoc = vi.fn();
vi.mock('../services/branchService', () => ({
  getBranchDoc: (path: string, branchId: string) => getBranchDoc(path, branchId),
}));

import { useAttribution } from './useAttribution';
import type { UseAttributionResult } from './useAttribution';

interface TDoc { text: string }

const ALICE = 'a'.repeat(32);
const BOB = 'b'.repeat(32);
const CAROL = 'c'.repeat(32);
const IDENTITIES = {};

interface Props { sourceText: string; branchId?: string | null }

function runsOf(payload: string | null) {
  expect(payload).not.toBeNull();
  return (JSON.parse(payload!) as {
    runs: Array<{ start: number; end: number; actor: string; time: number }>;
  }).runs;
}

/** Render the hook and record every render, to check no render pairs a payload with another text. */
function renderLive(initialProps: Props) {
  const seen: Array<{ props: Props; result: UseAttributionResult }> = [];
  const hook = renderHook(
    (props: Props) => {
      const result = useAttribution({
        enabled: true,
        filePath: 'doc.qmd',
        identities: IDENTITIES,
        sourceText: props.sourceText,
        branchId: props.branchId,
      });
      seen.push({ props, result });
      return result;
    },
    { initialProps },
  );
  const expectNoMismatchedPayload = () => {
    for (const { props, result } of seen) {
      if (result.payload === null) continue;
      expect(runsOf(result.payload).at(-1)!.end).toBe(props.sourceText.length);
    }
  };
  return { ...hook, expectNoMismatchedPayload };
}

function seed(): Doc<TDoc> {
  const doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
  return A.change(doc, { time: 1 }, d => A.splice(d, ['text'], 0, 0, 'alpha beta gamma'));
}

beforeEach(() => {
  getFileHandle.mockReset();
  getBranchDoc.mockReset();
});

describe('useAttribution, live', () => {
  it('follows every edit in the same update, with no quiet period needed', async () => {
    let doc = seed();
    getFileHandle.mockReturnValue({ doc: () => doc });
    const { result, rerender, expectNoMismatchedPayload } = renderLive({ sourceText: doc.text });
    await waitFor(() => expect(result.current.payload).not.toBeNull());

    // Two collaborators type into the sentence without pausing.
    let bob = A.clone(doc, { actor: BOB });
    let carol = A.clone(doc, { actor: CAROL });
    for (let i = 0; i < 10; i++) {
      bob = A.change(bob, { time: 2 }, d => A.splice(d, ['text'], 6, 0, 'B'));
      doc = A.merge(doc, bob);
      rerender({ sourceText: doc.text });
      expect(result.current.pending).toBe(false);
      expect(runsOf(result.current.payload).at(-1)!.end).toBe(doc.text.length);

      carol = A.change(carol, { time: 3 }, d => A.splice(d, ['text'], 0, 0, 'C'));
      doc = A.merge(doc, carol);
      rerender({ sourceText: doc.text });
      expect(result.current.pending).toBe(false);
      expect(runsOf(result.current.payload).at(-1)!.end).toBe(doc.text.length);
    }

    expect(runsOf(result.current.payload)).toEqual([
      { start: 0, end: 10, actor: CAROL, time: 3 },
      { start: 10, end: 16, actor: ALICE, time: 1 },
      { start: 16, end: 26, actor: BOB, time: 2 },
      { start: 26, end: 36, actor: ALICE, time: 1 },
    ]);
    expect(result.current.generating).toBe(false);
    expectNoMismatchedPayload();
  });

  it('shows no overlay rather than a shifted one while the text lags the document', async () => {
    let doc = seed();
    getFileHandle.mockReturnValue({ doc: () => doc });
    const { result, rerender, expectNoMismatchedPayload } = renderLive({ sourceText: doc.text });
    await waitFor(() => expect(result.current.payload).not.toBeNull());

    // The document has two edits; React state has caught up with only one.
    const afterOne = A.change(doc, { time: 2 }, d => A.splice(d, ['text'], 0, 0, 'X'));
    doc = A.change(afterOne, { time: 3 }, d => A.splice(d, ['text'], 0, 0, 'Y'));
    rerender({ sourceText: afterOne.text });
    expect(result.current).toEqual({ payload: null, generating: false, pending: false });

    rerender({ sourceText: doc.text });
    expect(runsOf(result.current.payload).at(-1)!.end).toBe(doc.text.length);
    expectNoMismatchedPayload();
  });

  it('builds from the branch document on a local branch', async () => {
    const main = seed();
    let branch = A.clone(main, { actor: BOB });
    branch = A.change(branch, { time: 5 }, d => A.splice(d, ['text'], 0, 0, 'branch '));
    getFileHandle.mockReturnValue({ doc: () => main });
    getBranchDoc.mockImplementation((_path: string, id: string) => (id === 'br-1' ? branch : null));

    const { result, rerender, expectNoMismatchedPayload } = renderLive({ sourceText: branch.text, branchId: 'br-1' });
    await waitFor(() => expect(result.current.payload).not.toBeNull());
    expect(runsOf(result.current.payload)).toEqual([
      { start: 0, end: 7, actor: BOB, time: 5 },
      { start: 7, end: 23, actor: ALICE, time: 1 },
    ]);

    // A further branch edit is followed at once.
    branch = A.change(branch, { time: 6 }, d => A.splice(d, ['text'], branch.text.length, 0, '!'));
    rerender({ sourceText: branch.text, branchId: 'br-1' });
    expect(runsOf(result.current.payload).at(-1)).toEqual({ start: 23, end: 24, actor: BOB, time: 6 });

    // Back on main: main's own attribution, not the branch's.
    rerender({ sourceText: main.text, branchId: null });
    await waitFor(() => expect(result.current.payload).not.toBeNull());
    expect(runsOf(result.current.payload)).toEqual([{ start: 0, end: 16, actor: ALICE, time: 1 }]);
    expectNoMismatchedPayload();
  });
});
