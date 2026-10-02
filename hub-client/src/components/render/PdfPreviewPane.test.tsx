/** @vitest-environment jsdom */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen } from '@testing-library/react';

const starts: { path: string }[] = [];
let onPdf: (pdf: Uint8Array, info: { path: string; fileName: string }) => void = () => {};
const shown: { key: string }[] = [];
let snapshot: unknown = { phase: 'idle' };
const listeners = new Set<() => void>();
const FORMAT = { key: 'pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };

vi.mock('../../pandoc/downloadService', () => ({
  formatByKey: () => FORMAT,
  createPdfPreviewController: (cb: typeof onPdf) => {
    onPdf = cb;
    return {
      start: async (o: { path: string }) => void starts.push({ path: o.path }),
      cancel: vi.fn(),
      getSnapshot: () => snapshot,
      subscribe: (l: () => void) => (listeners.add(l), () => listeners.delete(l)),
    };
  },
}));
vi.mock('../../pandoc/pdfViewer', () => ({
  mountPdfViewer: () => ({ show: async (_b: Uint8Array, o: { key: string }) => void shown.push(o), dispose: vi.fn() }),
}));

import PdfPreviewPane from './PdfPreviewPane';

const setStatus = (s: unknown) => {
  snapshot = s;
  act(() => listeners.forEach((l) => l()));
};

beforeEach(() => {
  starts.length = 0;
  shown.length = 0;
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
    act(() => onPdf(new Uint8Array([1]), { path: '/p/doc.qmd', fileName: 'doc.pdf' }));
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
    act(() => onPdf(new Uint8Array([1]), { path: '/p/doc.qmd', fileName: 'doc.pdf' }));
    expect(shown).toEqual([{ key: '/p/doc.qmd', fileName: 'doc.pdf' }]);
    setStatus({ phase: 'failed', clickId: 2, format: FORMAT, state: 'typst-error', diagnostics: [], notices: [], message: 'boom' });
    const banner = screen.getByTestId('pdf-preview-error');
    expect(banner.textContent).toContain('boom');
    expect(banner.textContent).toContain('last PDF that compiled');
  });
});
