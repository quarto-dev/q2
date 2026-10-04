/**
 * @vitest-environment jsdom
 *
 * "Import document" control (document import P5 T2): the picker's accept filter comes from the
 * format table, choosing files hands each to `onPick`, and the control is inert until the table loads.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, renderHook, waitFor } from '@testing-library/react';
import ImportControl from './ImportControl';
import DocumentTopBar from './DocumentTopBar';
import { createStubImportService, setImportServiceForTests } from '../pandoc/importService';
import { useImportFormats } from '../pandoc/useImportFormats';
import { importAccept, isImportableName } from '../utils/importFormats';
import { importDoc } from '../strings';

afterEach(() => {
  cleanup();
  setImportServiceForTests(undefined);
});

const formatsPromise = () => createStubImportService().getImportFormats();
const input = () => screen.getByTestId('import-file-input') as HTMLInputElement;

describe('importAccept / isImportableName', () => {
  it('lists every extension and MIME type from the table', async () => {
    const accept = importAccept(await formatsPromise());
    const parts = accept.split(',');
    expect(parts).toContain('.docx');
    expect(parts).toContain('.pptx');
    expect(parts).toContain('application/rtf');
    expect(parts).toContain('text/rtf');
    expect(parts).toContain('application/epub+zip');
    expect(parts.every((p) => p.startsWith('.') || p.includes('/'))).toBe(true);
  });

  it('matches names case-insensitively by extension', async () => {
    const f = await formatsPromise();
    expect(isImportableName('Report.DOCX', f)).toBe(true);
    expect(isImportableName('a.b.odt', f)).toBe(true);
    expect(isImportableName('a.png', f)).toBe(false);
    expect(isImportableName('docx', f)).toBe(false);
  });
});

describe('ImportControl', () => {
  it('sets the file input accept from the format table', async () => {
    const formats = await formatsPromise();
    render(<ImportControl formats={formats} onPick={vi.fn()} />);
    expect(input().accept).toBe(importAccept(formats));
    expect(screen.getByRole('button', { name: importDoc.buttonLabel })).toBeTruthy();
  });

  it('opens the picker on click', async () => {
    const formats = await formatsPromise();
    render(<ImportControl formats={formats} onPick={vi.fn()} />);
    const click = vi.spyOn(input(), 'click');
    fireEvent.click(screen.getByRole('button', { name: importDoc.buttonLabel }));
    expect(click).toHaveBeenCalledTimes(1);
  });

  it('passes every chosen file to onPick, in order', async () => {
    const formats = await formatsPromise();
    const onPick = vi.fn();
    render(<ImportControl formats={formats} onPick={onPick} />);
    const a = new File(['a'], 'a.docx');
    const b = new File(['b'], 'b.odt');
    fireEvent.change(input(), { target: { files: [a, b] } });
    expect(onPick.mock.calls.map((c) => c[0])).toEqual([a, b]);
  });

  it('is inert while the format table loads, and when disabled', async () => {
    const formats = await formatsPromise();
    const { rerender } = render(<ImportControl formats={null} onPick={vi.fn()} />);
    const button = () => screen.getByRole('button', { name: importDoc.buttonLabel });
    expect(button().getAttribute('aria-disabled')).toBe('true');
    const click = vi.spyOn(input(), 'click');
    fireEvent.click(button());
    expect(click).not.toHaveBeenCalled();
    rerender(<ImportControl formats={formats} disabled onPick={vi.fn()} />);
    expect(button().getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(button());
    expect(click).not.toHaveBeenCalled();
    rerender(<ImportControl formats={formats} onPick={vi.fn()} />);
    expect(button().getAttribute('aria-disabled')).toBeNull();
  });
});

describe('DocumentTopBar import placement', () => {
  it('shows the button with no file open and no downloadAs, and hides it in fullscreen preview', async () => {
    const formats = await formatsPromise();
    const importDocument = { formats, onPick: vi.fn() };
    const { rerender } = render(<DocumentTopBar currentFilePath={null} importDocument={importDocument} />);
    expect(screen.getByRole('button', { name: importDoc.buttonLabel })).toBeTruthy();
    rerender(<DocumentTopBar currentFilePath={null} isFullscreenPreview importDocument={importDocument} />);
    expect(screen.queryByRole('button', { name: importDoc.buttonLabel })).toBeNull();
  });

  it('is absent when the host does not offer import', () => {
    render(<DocumentTopBar currentFilePath="a.qmd" />);
    expect(screen.queryByRole('button', { name: importDoc.buttonLabel })).toBeNull();
  });
});

describe('useImportFormats', () => {
  it('loads the table from the installed service, and not at all when disabled', async () => {
    const service = createStubImportService();
    const spy = vi.spyOn(service, 'getImportFormats');
    setImportServiceForTests(service);
    const off = renderHook(() => useImportFormats(false));
    expect(off.result.current).toBeNull();
    expect(spy).not.toHaveBeenCalled();
    const on = renderHook(() => useImportFormats(true));
    await waitFor(() => expect(on.result.current).not.toBeNull());
    expect(on.result.current?.formats.map((f) => f.id)).toContain('docx');
  });
});
