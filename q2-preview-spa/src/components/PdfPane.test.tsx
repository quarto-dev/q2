// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen } from '@testing-library/react';

const show = vi.fn(async (_pdf: Uint8Array, _opts: { key: string; fileName: string }) => {});
const dispose = vi.fn();
vi.mock('@quarto/pdf-viewer', () => ({ mountPdfViewer: vi.fn(() => ({ show, dispose })) }));
vi.mock('@quarto/preview-runtime', () => ({
  vfsReadFile: vi.fn(() => ({ success: true, content: 'edited text\n' })),
}));

import { DEBOUNCE_MS, PdfPane } from './PdfPane';

interface Pending {
  init: RequestInit;
  resolve: (r: Response) => void;
}
let pending: Pending[];
/** A fetch that never notices its abort (a proxy, a slow body read): only the sequence guard can drop it. */
let ignoreAbort = false;

function pdfResponse(bytes: number[], headers: Record<string, string> = {}): Response {
  return new Response(new Uint8Array(bytes), { status: 200, headers });
}
function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  pending = [];
  ignoreAbort = false;
  vi.stubGlobal(
    'fetch',
    vi.fn(
      (_url: string, init: RequestInit) =>
        new Promise<Response>((resolve, reject) => {
          pending.push({ init, resolve });
          init.signal?.addEventListener('abort', () => !ignoreAbort && reject(new DOMException('aborted', 'AbortError')));
        }),
    ),
  );
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const flush = (ms = 0) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const answer = async (i: number, r: Response) => {
  pending[i].resolve(r);
  await flush();
};

describe('PdfPane with the session’s --to / -M (F8)', () => {
  it('sends the CLI metadata under its own chapter book-scope', async () => {
    render(
      <PdfPane
        path="doc.qmd"
        contentTick={0}
        onBack={null}
        render={{ to: 'typst', metadata: { toc: true, 'book-scope': 'book' } }}
      />,
    );
    await flush();
    expect(JSON.parse(pending[0].init.body as string).metadata).toEqual({
      toc: true,
      'book-scope': 'chapter',
    });
  });

  it('with no flags sends only the chapter scope', async () => {
    render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    expect(JSON.parse(pending[0].init.body as string).metadata).toEqual({ 'book-scope': 'chapter' });
  });
});

describe('PdfPane', () => {
  it('sends content and the chapter book-scope metadata', async () => {
    render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    expect(pending).toHaveLength(1);
    expect(JSON.parse(pending[0].init.body as string)).toEqual({
      path: 'doc.qmd',
      content: 'edited text\n',
      metadata: { 'book-scope': 'chapter' },
    });
  });

  it('debounces to one POST', async () => {
    const { rerender } = render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    await answer(0, pdfResponse([1]));
    for (const tick of [1, 2, 3]) {
      rerender(<PdfPane path="doc.qmd" contentTick={tick} onBack={null} />);
      await flush(100);
    }
    expect(pending).toHaveLength(1); // still within the quiet period
    await flush(DEBOUNCE_MS);
    expect(pending).toHaveLength(2);
  });

  it('aborts the previous request', async () => {
    const { rerender } = render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    expect(pending[0].init.signal?.aborted).toBe(false);
    rerender(<PdfPane path="doc.qmd" contentTick={1} onBack={null} />);
    await flush(DEBOUNCE_MS);
    expect(pending[0].init.signal?.aborted).toBe(true);
    expect(pending).toHaveLength(2);
  });

  it('ignores a stale sequence', async () => {
    ignoreAbort = true;
    const { rerender } = render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    rerender(<PdfPane path="doc.qmd" contentTick={1} onBack={null} />);
    await flush(DEBOUNCE_MS);
    // The newer request answers first, then the older one limps in (its abort was ignored, say by a proxy).
    await answer(1, pdfResponse([2]));
    await answer(0, pdfResponse([1]));
    expect(show).toHaveBeenCalledTimes(1);
    expect(Array.from(show.mock.calls[0][0])).toEqual([2]);
  });

  it('shows the pdf with the document key and file name', async () => {
    render(<PdfPane path="notes/doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    await answer(0, pdfResponse([7]));
    expect(show).toHaveBeenCalledWith(expect.any(Uint8Array), { key: 'notes/doc.qmd', fileName: 'doc.pdf' });
  });

  it('keeps the last PDF under the banner on 422', async () => {
    const { rerender } = render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    await answer(0, pdfResponse([1]));
    rerender(<PdfPane path="doc.qmd" contentTick={1} onBack={null} />);
    await flush(DEBOUNCE_MS);
    await answer(1, jsonResponse(422, { diagnostics: [{ kind: 'error', title: 'typst error', code: 'Q-21-2', hints: [], details: [] }] }));
    expect(screen.getByRole('alert').textContent).toContain('typst error');
    expect(dispose).not.toHaveBeenCalled();
    expect(show).toHaveBeenCalledTimes(1); // the first PDF was not replaced or cleared
  });

  it('strips ANSI colors and OSC 8 hyperlinks from rendered diagnostics', async () => {
    render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    const rendered =
      '\x1b[31mError:\x1b[0m [Q-2-55] bad \x1b[38;5;246m[\x1b[0m \x1b]8;;file:///d/doc.qmd#5:19\x1b\\/d/doc.qmd:5:19\x1b]8;;\x1b\\ \x1b[38;5;246m]\x1b[0m';
    await answer(0, jsonResponse(422, { diagnostics: [{ kind: 'error', title: 'bad', code: 'Q-2-55', hints: [], details: [], rendered }] }));
    const alert = screen.getByRole('alert').textContent ?? '';
    expect(alert).toContain('Error: [Q-2-55] bad [ /d/doc.qmd:5:19 ]');
    expect(alert).not.toContain('\x1b');
  });

  it('shows the Q-21-1 text when typst is missing', async () => {
    render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    await answer(0, jsonResponse(422, {
      error: 'typst not found',
      diagnostics: [{ kind: 'error', title: 'Typst is not installed', code: 'Q-21-1', hints: ['Install typst'], details: [] }],
    }));
    const alert = screen.getByRole('alert').textContent ?? '';
    expect(alert).toContain('Q-21-1');
    expect(alert).toContain('Install typst');
  });

  it('409 superseded is silent', async () => {
    render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    await answer(0, jsonResponse(409, { superseded: true }));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(show).not.toHaveBeenCalled();
  });

  it('surfaces warnings and the unexecuted-cells notice from the response headers', async () => {
    render(<PdfPane path="doc.qmd" contentTick={0} onBack={null} />);
    await flush();
    await answer(
      0,
      pdfResponse([1], {
        'x-q2-diagnostics': encodeURIComponent(JSON.stringify([{ kind: 'warning', title: 'odd image', hints: [], details: [] }])),
        'x-q2-stats': encodeURIComponent(JSON.stringify({ unexecuted_cells: 2 })),
      }),
    );
    expect(screen.getByText(/1 warning/)).not.toBeNull();
    expect(screen.getByRole('note').textContent).toContain('2 code cells not executed');
  });
});
