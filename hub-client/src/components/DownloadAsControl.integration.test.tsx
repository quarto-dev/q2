/**
 * @vitest-environment jsdom
 *
 * "Download as" control (pandoc-host H5): menu semantics, the described disabled state, the
 * progress/cancel surface and the live region.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react';
import DownloadAsControl from './DownloadAsControl';
import { download } from '../strings';
import type { DownloadFormat, DownloadStatus } from '../pandoc/downloadController';

afterEach(cleanup);

const DOCX: DownloadFormat = { key: 'docx', label: 'Word', extension: 'docx', mime: 'application/docx' };
const idle: DownloadStatus = { phase: 'idle' };

function setup(props: Partial<React.ComponentProps<typeof DownloadAsControl>> = {}) {
  const handlers = { onSelect: vi.fn(), onCancel: vi.fn(), onDismiss: vi.fn() };
  const view = render(<DownloadAsControl formats={[DOCX]} status={idle} {...handlers} {...props} />);
  return { ...handlers, ...view };
}

const mk = (key: string, label: string, extension: string): DownloadFormat => ({ key, label, extension, mime: 'x/y' });
const EPUB = mk('epub', 'EPUB (.epub)', 'epub');
const PDF2 = mk('pdf', 'PDF (.pdf)', 'pdf');
const TYPST2 = mk('typst', 'Typst source (.typ)', 'typ');
const PPTX = mk('pptx', 'PowerPoint (.pptx)', 'pptx');

describe('DownloadAsControl: book chapters (R9)', () => {
  const open = () => fireEvent.click(screen.getByRole('button', { name: 'Download as' }));
  const items = () => screen.getAllByRole('menuitem').map((i) => i.textContent);

  it('a chapter gets "Download book as" plus "This chapter only" for typst, pdf and epub; docx and pptx keep one entry', () => {
    setup({ formats: [DOCX, PPTX, EPUB, TYPST2, PDF2], book: { chapter: true } });
    open();
    expect(items()).toEqual([
      'Word',
      'PowerPoint (.pptx)',
      'Download book as EPUB (.epub)',
      'This chapter onlyEPUB (.epub)',
      'Download book as Typst source (.typ)',
      'This chapter onlyTypst source (.typ)',
      'Download book as PDF (.pdf)',
      'This chapter onlyPDF (.pdf)',
    ]);
  });

  it('selecting passes the scope: auto for the book, chapter for this chapter only; ordinary entries pass the format alone', () => {
    const { onSelect } = setup({ formats: [DOCX, EPUB], book: { chapter: true } });
    open();
    fireEvent.click(screen.getByRole('menuitem', { name: /Download book as EPUB/ }));
    expect(onSelect).toHaveBeenLastCalledWith(EPUB, 'auto');
    open();
    fireEvent.click(screen.getByRole('menuitem', { name: /This chapter only/ }));
    expect(onSelect).toHaveBeenLastCalledWith(EPUB, 'chapter');
    open();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Word' }));
    expect(onSelect).toHaveBeenLastCalledWith(DOCX);
  });

  it('a page that is not a chapter, and a document outside a book, get the ordinary entries with no mention of the book', () => {
    for (const book of [{ chapter: false }, null, undefined]) {
      setup({ formats: [DOCX, EPUB], book });
      open();
      expect(items()).toEqual(['Word', 'EPUB (.epub)']);
      expect(screen.queryByText(/book|chapter/i)).toBeNull();
      cleanup();
    }
  });

  it('opening the menu asks the host to refresh the book information', () => {
    const onOpen = vi.fn();
    setup({ book: { chapter: true }, onOpen });
    open();
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it('shows "Rendering chapter i of N: file" in the panel and the live region, and the book summary when done', () => {
    setup({ formats: [EPUB], status: { phase: 'working', clickId: 1, format: EPUB, stage: 'chapter', chapter: { index: 2, total: 5, file: 'sub/two.qmd' } } });
    expect(screen.getByTestId('download-status').textContent).toContain('Rendering chapter 2 of 5: sub/two.qmd');
    expect(screen.getByRole('status', { name: 'Download status' }).textContent).toBe('Rendering chapter 2 of 5: sub/two.qmd');
    cleanup();
    setup({ formats: [EPUB], status: { phase: 'done', clickId: 1, format: EPUB, fileName: 'B.epub', warnings: [], notices: [], unexecutedCells: 0, book: { chapters: 5 } } });
    expect(screen.getByRole('status', { name: 'Download status' }).textContent).toBe('Book downloaded (5 chapters)');
    expect(screen.getByTestId('download-status').textContent).toContain('Book downloaded (5 chapters)');
  });
});

describe('DownloadAsControl', () => {
  it('opens a menu of the formats and selects one', () => {
    const { onSelect } = setup();
    const button = screen.getByRole('button', { name: 'Download as' });
    expect(button.getAttribute('aria-haspopup')).toBe('menu');
    expect(button.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(button);
    expect(button.getAttribute('aria-expanded')).toBe('true');
    const menu = screen.getByRole('menu', { name: 'Download as' });
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Word' }));
    expect(onSelect).toHaveBeenCalledWith(DOCX);
  });

  it('Escape closes the menu and returns focus to the button', () => {
    setup();
    const button = screen.getByRole('button', { name: 'Download as' });
    fireEvent.click(button);
    const menu = screen.getByRole('menu');
    fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it('mentions the one-time download in the menu', () => {
    setup();
    fireEvent.click(screen.getByRole('button', { name: 'Download as' }));
    expect(screen.getByText(/about 16 MB/)).toBeTruthy();
  });

  it('with PDF in the menu, the hint states the larger first-use total', () => {
    const PDF: DownloadFormat = { key: 'pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };
    setup({ formats: [DOCX, PDF] });
    fireEvent.click(screen.getByRole('button', { name: 'Download as' }));
    expect(screen.getByText(/about 33 MB in all/)).toBeTruthy();
  });

  it('shows the typst stages of a PDF download and a typst diagnostic with its location', () => {
    const PDF: DownloadFormat = { key: 'pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };
    setup({ formats: [PDF], status: { phase: 'working', clickId: 1, format: PDF, stage: 'typst-compiling' } });
    expect(screen.getByRole('status', { name: 'Download status' }).textContent).toBe(download.compilingPdf);
    cleanup();
    setup({
      formats: [PDF],
      status: {
        phase: 'failed',
        clickId: 1,
        format: PDF,
        state: 'typst-error',
        notices: [],
        diagnostics: [{ origin: 'typst', kind: 'error', message: 'unknown variable: x', path: '/doc/doc.typ', range: '3:1-3:2', stage: 'typst' }],
      },
    });
    expect(screen.getByText(/unknown variable: x/)).toBeTruthy();
    expect(screen.getByText(/\/doc\/doc.typ 3:1-3:2/)).toBeTruthy();
  });

  it('a disabled control is aria-disabled, described by text, and does not open', () => {
    setup({ disabledReason: 'Download is unavailable: documents with format latex cannot be converted.' });
    const button = screen.getByRole('button', { name: 'Download as' });
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(button.getAttribute('aria-haspopup')).toBeNull();
    const describedBy = button.getAttribute('aria-describedby')!;
    expect(document.getElementById(describedBy)?.textContent).toMatch(/latex/);
    fireEvent.click(button);
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('shows byte progress while the converter downloads, and Cancel calls onCancel', () => {
    const { onCancel } = setup({
      status: { phase: 'working', clickId: 1, format: DOCX, stage: 'loading', load: { phase: 'download', loaded: 5 * 1048576, total: 10 * 1048576 } },
    });
    const text = screen.getAllByText(/Downloading the converter… 5.0 MB of 10.0 MB/);
    expect(text.length).toBeGreaterThan(0);
    const bar = screen.getByRole('progressbar');
    expect(bar.getAttribute('value')).toBe(String(5 * 1048576));
    expect(bar.getAttribute('max')).toBe(String(10 * 1048576));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalled();
  });

  it('the live region announces phase changes without byte counts', () => {
    setup({
      status: { phase: 'working', clickId: 1, format: DOCX, stage: 'loading', load: { phase: 'download', loaded: 5 * 1048576, total: 10 * 1048576 } },
    });
    const live = screen.getByRole('status', { name: 'Download status' });
    expect(live.textContent).toBe('Downloading the converter…');
  });

  it('reports a finished download with its warnings and unexecuted cells', () => {
    const { onDismiss } = setup({
      status: {
        phase: 'done',
        clickId: 1,
        format: DOCX,
        fileName: 'report.docx',
        notices: [],
        unexecutedCells: 2,
        warnings: [{ origin: 'rust', kind: 'warning', code: 'Q-11-1', title: 'pandoc warning', problem: 'missing figure.png' }],
      },
    });
    expect(screen.getByRole('status', { name: 'Download status' }).textContent).toBe('Downloaded report.docx.');
    expect(screen.getByText('2 code cells were not executed; their output is not in the file.')).toBeTruthy();
    expect(screen.getByText('1 warning')).toBeTruthy();
    expect(screen.getByText(/missing figure.png/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalled();
  });

  it('a finished typst download notes that the .typ may have dangling resource references; other formats do not', () => {
    const TYPST: DownloadFormat = { key: 'typst', label: 'Typst source', extension: 'typ', mime: 'text/plain' };
    const done = (format: DownloadFormat): DownloadStatus => ({ phase: 'done', clickId: 1, format, fileName: `doc.${format.extension}`, notices: [], unexecutedCells: 0, warnings: [] });
    setup({ formats: [DOCX, TYPST], status: done(TYPST) });
    expect(screen.getByText(download.typstDangling)).toBeTruthy();
    cleanup();
    setup({ status: done(DOCX) });
    expect(screen.queryByText(download.typstDangling)).toBeNull();
  });

  it('a failure shows the plain-language state and the diagnostics, and announces it', () => {
    setup({
      status: {
        phase: 'failed',
        clickId: 1,
        format: DOCX,
        state: 'pandoc-error',
        notices: [],
        diagnostics: [{ origin: 'rust', kind: 'error', code: 'Q-20-3', title: 'pandoc failed', problem: 'exit status: 83' }],
      },
    });
    expect(screen.getByRole('status', { name: 'Download status' }).textContent).toMatch(/converter reported an error/);
    expect(screen.getByText(/\[Q-20-3\]/)).toBeTruthy();
    expect(screen.getByText(/exit status: 83/)).toBeTruthy();
  });

  it('every failure state has its own plain-language copy', () => {
    const states = Object.keys(download.failed).filter((s) => s !== 'done' && s !== 'cancelled');
    expect(states.sort()).toEqual(
      ['blocked', 'crashed', 'download-failed', 'invalid-request', 'native-error', 'native-failed', 'offline', 'out-of-memory', 'package-error', 'pandoc-error', 'request-failed', 'timeout', 'typst-error', 'unsupported'].sort(),
    );
    for (const state of states) {
      const { unmount } = setup({ status: { phase: 'failed', clickId: 1, format: DOCX, state: state as never, notices: [], diagnostics: [] } });
      const text = screen.getByRole('status', { name: 'Download status' }).textContent;
      expect(text, state).toContain((download.failed as Record<string, string>)[state]);
      expect((download.failed as Record<string, string>)[state].length, state).toBeGreaterThan(10);
      unmount();
    }
  });

  it('a host failure shows the host diagnostic message', () => {
    setup({
      status: {
        phase: 'failed',
        clickId: 1,
        format: DOCX,
        state: 'timeout',
        notices: [],
        diagnostics: [{ origin: 'host', kind: 'error', code: 'pandoc-timeout', message: 'pandoc did not finish within 120 s' }],
      },
    });
    expect(screen.getByText(/did not finish within 120 s/)).toBeTruthy();
  });
});
