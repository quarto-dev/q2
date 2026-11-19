/**
 * GE-7 test specification 3, attribution part: a doc swap resets the store's incremental state.
 *
 * The store extends cached run lists against whatever handle `getFileHandle` returns. After the project owner replaces
 * a file, the new doc's history is unrelated to the old one's, so the cached state is meaningless for it.
 *
 * @vitest-environment jsdom
 */
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';

interface TDoc { text: string }

const ALICE = 'a'.repeat(32);
const BOB = 'b'.repeat(32);

let current: { documentId: string; doc: () => Doc<TDoc> } | null = null;

vi.mock('@quarto/preview-runtime', () => ({
  getFileHandle: vi.fn(() => current),
}));
vi.mock('./branchService', () => ({
  getBranchDoc: vi.fn(() => null),
}));

import { AttributionStore, ATTRIBUTION_NONE, ATTRIBUTION_PENDING } from './attribution-store';
import type { RunListAttribution } from './attribution-runs';

function docWith(actor: string, text: string, time: number): Doc<TDoc> {
  return A.change(A.from<TDoc>({ text: '' }, { actor }), { time }, d => A.splice(d, ['text'], 0, 0, text));
}

function handleFor(documentId: string, doc: Doc<TDoc>) {
  return { documentId, doc: () => doc };
}

async function settle(store: AttributionStore): Promise<void> {
  store.reconcile();
  for (let i = 0; i < 200 && store.isGenerating(); i++) await new Promise(r => setTimeout(r, 5));
  await new Promise(r => setTimeout(r, 5));
}

function totalLength(a: RunListAttribution): number {
  return a.runs.reduce((n, r) => n + (r.end - r.start), 0);
}

beforeEach(() => {
  current = null;
});

describe('AttributionStore across a doc swap (GE-7)', () => {
  it('builds for the new doc instead of extending the old doc\'s state', async () => {
    const docA = docWith(ALICE, 'alpha beta gamma', 1);
    const docB = docWith(BOB, 'zz', 2);
    current = handleFor('doc-A', docA);
    const store = new AttributionStore('index.qmd', null);

    // Cold build for doc A.
    expect(store.snapshot(null)).toBe(ATTRIBUTION_NONE);
    await settle(store);
    const built = store.snapshot(null);
    expect(built).not.toBe(ATTRIBUTION_NONE);
    expect(built).not.toBe(ATTRIBUTION_PENDING);
    expect(totalLength(built as RunListAttribution)).toBe('alpha beta gamma'.length);

    // The owner replaces the file: same path, unrelated doc.
    current = handleFor('doc-B', docB);
    expect(store.snapshot(null)).toBe(ATTRIBUTION_NONE); // the old state must not be shown for the new doc
    await settle(store);
    const rebuilt = store.snapshot(null);
    expect(rebuilt).not.toBe(ATTRIBUTION_NONE);
    expect(totalLength(rebuilt as RunListAttribution)).toBe('zz'.length);
  });

  it('starts over when the doc id changes, even if the new doc happens to extend the old history', async () => {
    const docA = docWith(ALICE, 'alpha', 1);
    // A fork of doc A that gained an edit: its history contains A's, so extending A's state would "work".
    const docB = A.change(A.clone(docA), { time: 5 }, d => A.splice(d, ['text'], 5, 0, ' beta'));
    current = handleFor('doc-A', docA);
    const store = new AttributionStore('index.qmd', null);
    store.snapshot(null);
    await settle(store);
    expect(store.snapshot(null)).not.toBe(ATTRIBUTION_NONE);

    current = handleFor('doc-B', docB);
    // The cached state belongs to doc A; the store does not carry it over to a different doc.
    expect(store.snapshot(null)).toBe(ATTRIBUTION_NONE);
    await settle(store);
    const rebuilt = store.snapshot(null) as RunListAttribution;
    expect(totalLength(rebuilt)).toBe('alpha beta'.length);
  });

  it('keeps extending incrementally when the doc id is unchanged', async () => {
    let doc = docWith(ALICE, 'alpha', 1);
    current = handleFor('doc-A', doc);
    const store = new AttributionStore('index.qmd', null);
    store.snapshot(null);
    await settle(store);
    const first = store.snapshot(null) as RunListAttribution;

    doc = A.change(doc, { time: 3 }, d => A.splice(d, ['text'], 5, 0, ' more'));
    current = handleFor('doc-A', doc);
    const second = store.snapshot(null) as RunListAttribution;
    expect(totalLength(second)).toBe('alpha more'.length);
    expect(second).not.toBe(ATTRIBUTION_PENDING);
    expect(first).not.toBe(second);
  });

  it('reset() drops the cached state and notifies subscribers', async () => {
    current = handleFor('doc-A', docWith(ALICE, 'alpha', 1));
    const store = new AttributionStore('index.qmd', null);
    store.snapshot(null);
    await settle(store);
    expect(store.snapshot(null)).not.toBe(ATTRIBUTION_NONE);

    const listener = vi.fn();
    store.subscribe(listener);
    store.reset();
    expect(listener).toHaveBeenCalled();
    expect(store.snapshot(null)).toBe(ATTRIBUTION_NONE);
  });
});
