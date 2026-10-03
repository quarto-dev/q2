/** @vitest-environment jsdom */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen } from '@testing-library/react';

const starts: { path: string; projectKey?: string }[] = [];
let onPdf: (pdf: Uint8Array, info: { path: string; fileName: string; seq: number }) => void = () => {};
const shown: { key: string }[] = [];
let warmFlag = true;
const controllerOptions: { warm?: boolean }[] = [];
const lifecycle = { cancel: vi.fn(), acquire: vi.fn(), release: vi.fn() };
let snapshot: unknown = { phase: 'idle' };
const listeners = new Set<() => void>();
const FORMAT = { key: 'typst-pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };

vi.mock('../../pandoc/downloadService', () => ({
  formatByKey: () => FORMAT,
  createPdfPreviewController: (cb: typeof onPdf, options: { warm?: boolean } = {}) => {
    onPdf = cb;
    controllerOptions.push(options);
    return {
      start: async (o: { path: string; projectKey?: string }) => void starts.push({ path: o.path, projectKey: o.projectKey }),
      cancel: lifecycle.cancel,
      acquire: lifecycle.acquire,
      release: lifecycle.release,
      getSnapshot: () => snapshot,
      subscribe: (l: () => void) => (listeners.add(l), () => listeners.delete(l)),
    };
  },
}));
vi.mock('../../pandoc/featureFlag', () => ({ pandocWarmEnabled: () => warmFlag }));
vi.mock('../../pandoc/pdfViewer', () => ({
  mountPdfViewer: () => ({ show: async (_b: Uint8Array, o: { key: string }) => void shown.push(o), dispose: vi.fn() }),
}));

import PdfPreviewPane, { PDF_PREVIEW_DEBOUNCE_MS, PDF_PREVIEW_WARM_DEBOUNCE_MS } from './PdfPreviewPane';

const setStatus = (s: unknown) => {
  snapshot = s;
  act(() => listeners.forEach((l) => l()));
};

beforeEach(() => {
  starts.length = 0;
  shown.length = 0;
  warmFlag = true;
  controllerOptions.length = 0;
  lifecycle.cancel.mockClear();
  lifecycle.acquire.mockClear();
  lifecycle.release.mockClear();
  snapshot = { phase: 'idle' };
  vi.useFakeTimers();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('PdfPreviewPane', () => {
  it('compiles at once on mount, with no start button', async () => {
    render(<PdfPreviewPane path="/p/doc.qmd" content="a" />);
    expect(screen.queryByRole('button')).toBeNull();
    await act(async () => void vi.advanceTimersByTime(0));
    expect(starts).toEqual([{ path: '/p/doc.qmd' }]);
  });

  it('shows a spinner with the progress text until the first PDF arrives, then the small updating badge', async () => {
    render(<PdfPreviewPane path="/p/doc.qmd" content="a" />);
    await act(async () => void vi.advanceTimersByTime(0));
    setStatus({ phase: 'working', clickId: 1, format: FORMAT, stage: 'loading', load: { phase: 'download', loaded: 5 * 1048576, total: 16 * 1048576 } });
    const loading = screen.getByTestId('pdf-preview-loading');
    expect(loading.querySelector('.qh-spinner')).toBeTruthy();
    expect(loading.textContent).toContain('5.0 MB');
    expect(screen.queryByTestId('pdf-preview-status')).toBeNull();
    act(() => onPdf(new Uint8Array([1]), { path: '/p/doc.qmd', fileName: 'doc.pdf', seq: 1 }));
    expect(screen.queryByTestId('pdf-preview-loading')).toBeNull();
    expect(screen.getByTestId('pdf-preview-status')).toBeTruthy();
  });

  it('debounces edits into one recompile', async () => {
    const view = render(<PdfPreviewPane path="/p/doc.qmd" content="a" />);
    await act(async () => void vi.advanceTimersByTime(0));
    expect(starts).toHaveLength(1);
    for (const c of ['ab', 'abc', 'abcd']) {
      view.rerender(<PdfPreviewPane path="/p/doc.qmd" content={c} />);
      await act(async () => void vi.advanceTimersByTime(100));
    }
    expect(starts).toHaveLength(1);
    await act(async () => void vi.advanceTimersByTime(500));
    expect(starts).toHaveLength(2);
  });

  it('feeds each compiled PDF to the viewer keyed by the document, and keeps it under an error banner', async () => {
    render(<PdfPreviewPane path="/p/doc.qmd" content="a" />);
    await act(async () => void vi.advanceTimersByTime(0));
    act(() => onPdf(new Uint8Array([1]), { path: '/p/doc.qmd', fileName: 'doc.pdf', seq: 1 }));
    expect(shown).toEqual([{ key: '/p/doc.qmd', fileName: 'doc.pdf' }]);
    setStatus({ phase: 'failed', clickId: 2, format: FORMAT, state: 'typst-error', diagnostics: [], notices: [], message: 'boom' });
    const banner = screen.getByTestId('pdf-preview-error');
    expect(banner.textContent).toContain('boom');
    expect(banner.textContent).toContain('last PDF that compiled');
  });

  it('passes the project key to start, and a project change with the same path starts at once with the new key', async () => {
    const view = render(<PdfPreviewPane path="index.qmd" content="a" projectKey="p1" />);
    await act(async () => void vi.advanceTimersByTime(0));
    expect(starts).toEqual([{ path: 'index.qmd', projectKey: 'p1' }]);
    // The pane stays mounted when the project changes: the first compile for the new project is not debounced.
    view.rerender(<PdfPreviewPane path="index.qmd" content="b" projectKey="p2" />);
    await act(async () => void vi.advanceTimersByTime(0));
    expect(starts).toEqual([
      { path: 'index.qmd', projectKey: 'p1' },
      { path: 'index.qmd', projectKey: 'p2' },
    ]);
  });

  it('takes the pool at mount, and at unmount cancels the live runs and gives the pool back', async () => {
    const view = render(<PdfPreviewPane path="/p/doc.qmd" content="a" />);
    await act(async () => void vi.advanceTimersByTime(0));
    expect(lifecycle.acquire).toHaveBeenCalledTimes(1);
    expect(lifecycle.cancel).not.toHaveBeenCalled();
    view.unmount();
    expect(lifecycle.cancel).toHaveBeenCalledTimes(1);
    expect(lifecycle.release).toHaveBeenCalledTimes(1);
  });

  it('builds the warm controller when the flag is on and the fresh one when it is off', async () => {
    render(<PdfPreviewPane path="/p/doc.qmd" content="a" />);
    expect(controllerOptions).toEqual([{ warm: true }]);
    cleanup();
    warmFlag = false;
    controllerOptions.length = 0;
    render(<PdfPreviewPane path="/p/doc.qmd" content="a" />);
    expect(controllerOptions).toEqual([{ warm: false }]);
  });

  it('debounces edits by PDF_PREVIEW_WARM_DEBOUNCE_MS (warm) or PDF_PREVIEW_DEBOUNCE_MS (fresh), and by the prop when given', async () => {
    expect(PDF_PREVIEW_WARM_DEBOUNCE_MS).toBe(500);
    expect(PDF_PREVIEW_DEBOUNCE_MS).toBe(500);
    for (const [warm, prop, expected] of [
      [true, undefined, 500],
      [false, undefined, 500],
      [true, 120, 120],
      [false, 120, 120],
    ] as const) {
      cleanup();
      starts.length = 0;
      warmFlag = warm;
      const view = render(<PdfPreviewPane path="/p/doc.qmd" content="a" debounceMs={prop} />);
      await act(async () => void vi.advanceTimersByTime(0));
      view.rerender(<PdfPreviewPane path="/p/doc.qmd" content="b" debounceMs={prop} />);
      await act(async () => void vi.advanceTimersByTime(expected - 1));
      expect(starts, `warm=${warm} prop=${prop}`).toHaveLength(1);
      await act(async () => void vi.advanceTimersByTime(1));
      expect(starts, `warm=${warm} prop=${prop}`).toHaveLength(2);
    }
  });
});
