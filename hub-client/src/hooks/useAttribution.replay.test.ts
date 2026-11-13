/**
 * Replay coverage for the attribution hook: the payload must describe the
 * history step whose text is shown, never the live heads or another step.
 * Uses the real Automerge producer.
 *
 * @vitest-environment jsdom
 */

import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';

const getFileHandle = vi.fn();
vi.mock('@quarto/preview-runtime', () => ({
  getFileHandle: (path: string) => getFileHandle(path),
}));

import { useAttribution } from './useAttribution';
import type { UseAttributionResult } from './useAttribution';

interface TDoc { text: string }

const ALICE = 'a'.repeat(32);
const BOB = 'b'.repeat(32);
const CAROL = 'c'.repeat(32);
const IDENTITIES = {};

interface Step { heads: string[]; text: string }

/** alice: "alpha beta gamma"; bob inserts "bravo "; carol prepends "charlie ". */
function fixture(): { doc: Doc<TDoc>; steps: Step[] } {
  let doc = A.from<TDoc>({ text: '' }, { actor: ALICE });
  doc = A.change(doc, { time: 1 }, d => A.splice(d, ['text'], 0, 0, 'alpha beta gamma'));
  doc = A.clone(doc, { actor: BOB });
  doc = A.change(doc, { time: 2 }, d => A.splice(d, ['text'], 6, 0, 'bravo '));
  doc = A.clone(doc, { actor: CAROL });
  doc = A.change(doc, { time: 3 }, d => A.splice(d, ['text'], 0, 0, 'charlie '));
  // Skip the empty `A.from` change.
  const steps = A.topoHistoryTraversal(doc).slice(1).map(hash => ({
    heads: [hash],
    text: A.view(doc, [hash]).text,
  }));
  return { doc, steps };
}

const EXPECTED = [
  [{ start: 0, end: 16, actor: ALICE, time: 1 }],
  [
    { start: 0, end: 6, actor: ALICE, time: 1 },
    { start: 6, end: 12, actor: BOB, time: 2 },
    { start: 12, end: 22, actor: ALICE, time: 1 },
  ],
  [
    { start: 0, end: 8, actor: CAROL, time: 3 },
    { start: 8, end: 14, actor: ALICE, time: 1 },
    { start: 14, end: 20, actor: BOB, time: 2 },
    { start: 20, end: 30, actor: ALICE, time: 1 },
  ],
];

interface Props { sourceText: string; atHeads: string[] | null; enabled?: boolean }

function runsOf(payload: string | null) {
  expect(payload).not.toBeNull();
  return (JSON.parse(payload!) as { runs: Array<{ start: number; end: number }> }).runs;
}

/**
 * Render the hook and record every render. Each recorded payload must cover
 * exactly the text of the render it was returned in: runs built for other
 * heads would end at another length.
 */
function renderReplay(initialProps: Props) {
  const seen: Array<{ props: Props; result: UseAttributionResult }> = [];
  const hook = renderHook(
    (props: Props) => {
      const result = useAttribution({
        enabled: props.enabled ?? true,
        filePath: 'doc.qmd',
        identities: IDENTITIES,
        sourceText: props.sourceText,
        atHeads: props.atHeads,
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
  return { ...hook, seen, expectNoMismatchedPayload };
}

beforeEach(() => {
  getFileHandle.mockReset();
});

describe('useAttribution in replay', () => {
  it('builds the payload for the step shown, not the live heads', async () => {
    const { doc, steps } = fixture();
    getFileHandle.mockReturnValue({ doc: () => doc });

    const { result } = renderReplay({ sourceText: steps[0].text, atHeads: steps[0].heads });
    expect(result.current.pending).toBe(true);
    expect(result.current.payload).toBeNull();

    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(runsOf(result.current.payload)).toEqual(EXPECTED[0]);
  });

  it('has the next step ready in the same update when stepping forward', async () => {
    const { doc, steps } = fixture();
    getFileHandle.mockReturnValue({ doc: () => doc });
    const { result, rerender, seen, expectNoMismatchedPayload } = renderReplay({
      sourceText: steps[0].text,
      atHeads: steps[0].heads,
    });
    await waitFor(() => expect(result.current.pending).toBe(false));

    for (const i of [1, 2]) {
      const before = seen.length;
      rerender({ sourceText: steps[i].text, atHeads: steps[i].heads });
      // One render, never held: the step's runs are read during it.
      expect(seen.length).toBe(before + 1);
      expect(seen[before].result.pending).toBe(false);
      // No await: a forward step is settled by the time the update commits.
      expect(result.current.pending).toBe(false);
      expect(runsOf(result.current.payload)).toEqual(EXPECTED[i]);
    }
    expectNoMismatchedPayload();
  });

  it('rebuilds for a backward seek and shows nothing stale meanwhile', async () => {
    const { doc, steps } = fixture();
    getFileHandle.mockReturnValue({ doc: () => doc });
    const { result, rerender, expectNoMismatchedPayload } = renderReplay({
      sourceText: steps[2].text,
      atHeads: steps[2].heads,
    });
    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(runsOf(result.current.payload)).toEqual(EXPECTED[2]);

    rerender({ sourceText: steps[0].text, atHeads: steps[0].heads });
    expect(result.current.payload).toBeNull();
    expect(result.current.pending).toBe(true);
    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(runsOf(result.current.payload)).toEqual(EXPECTED[0]);

    // Back to a step already shown: served from the step cache at once.
    rerender({ sourceText: steps[2].text, atHeads: steps[2].heads });
    expect(result.current.pending).toBe(false);
    expect(runsOf(result.current.payload)).toEqual(EXPECTED[2]);
    expectNoMismatchedPayload();
  });

  it('returns to live attribution when replay ends', async () => {
    const { doc, steps } = fixture();
    getFileHandle.mockReturnValue({ doc: () => doc });
    const { result, rerender, expectNoMismatchedPayload } = renderReplay({
      sourceText: steps[0].text,
      atHeads: steps[0].heads,
    });
    await waitFor(() => expect(result.current.pending).toBe(false));

    rerender({ sourceText: doc.text, atHeads: null });
    expect(result.current.pending).toBe(false);
    await waitFor(() => expect(result.current.payload).not.toBeNull());
    expect(runsOf(result.current.payload)).toEqual(EXPECTED[2]);
    await waitFor(() => expect(result.current.generating).toBe(false));
    expectNoMismatchedPayload();
  });

  it('returns to live attribution when replay ends mid-build', async () => {
    const { doc, steps } = fixture();
    getFileHandle.mockReturnValue({ doc: () => doc });
    const { result, rerender, expectNoMismatchedPayload } = renderReplay({
      sourceText: steps[0].text,
      atHeads: steps[0].heads,
    });
    expect(result.current.pending).toBe(true);

    rerender({ sourceText: doc.text, atHeads: null });
    await waitFor(() => expect(result.current.payload).not.toBeNull());
    expect(runsOf(result.current.payload)).toEqual(EXPECTED[2]);
    await waitFor(() => expect(result.current.generating).toBe(false));
    expectNoMismatchedPayload();
  });

  it('settles a step without attribution when the file handle is gone', async () => {
    const { steps } = fixture();
    getFileHandle.mockReturnValue(null);

    const { result } = renderReplay({ sourceText: steps[0].text, atHeads: steps[0].heads });
    await act(async () => {});

    expect(result.current).toEqual({ payload: null, generating: false, pending: false });
  });

  it('is not pending when the overlay is off', () => {
    const { doc, steps } = fixture();
    getFileHandle.mockReturnValue({ doc: () => doc });

    const { result } = renderReplay({ sourceText: steps[0].text, atHeads: steps[0].heads, enabled: false });
    expect(result.current).toEqual({ payload: null, generating: false, pending: false });
  });
});
