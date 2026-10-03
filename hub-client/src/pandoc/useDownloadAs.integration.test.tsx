/**
 * The "Download as" hook (pandoc-host H5, R9): the book information it hands the menu, and how a
 * menu pick becomes a controller start (scope, per-chapter capture doc ids).
 *
 * @vitest-environment jsdom
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

const h = vi.hoisted(() => ({
  start: vi.fn(async () => {}),
  bookInfoFor: vi.fn(),
  embed: false,
  idle: { phase: 'idle' as const },
}));

vi.mock('@quarto/preview-runtime', () => ({ isWasmReady: () => true, resolvePandocFormats: () => null }));
vi.mock('./bookInfo', () => ({ bookInfoFor: h.bookInfoFor }));
vi.mock('./featureFlag', () => ({ isPreviewEmbed: () => h.embed }));
vi.mock('./downloadService', () => ({
  getDownloadController: () => ({ subscribe: () => () => {}, getSnapshot: () => h.idle, dismiss: () => {}, cancel: () => {}, start: h.start }),
  menuFormats: () => [],
  downloadAvailable: () => true,
  withOutputExt: (format: unknown) => format,
}));

import { useDownloadAs } from './useDownloadAs';
import type { CaptureRef } from '@quarto/preview-runtime';

const EPUB = { key: 'epub', label: 'EPUB', extension: 'epub', mime: 'application/epub+zip' };
const captures: Record<string, CaptureRef> = {
  'index.qmd': { captureDocId: 'c0', state: 'idle' },
  'one.qmd': { captureDocId: 'c1', state: 'idle' },
  'unrelated.qmd': { captureDocId: 'cx', state: 'idle' },
};

beforeEach(() => {
  h.start.mockClear();
  h.bookInfoFor.mockReset();
  h.embed = false;
});

describe('useDownloadAs: books', () => {
  it('exposes the resolver\'s book field for the open document', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['index.qmd', 'one.qmd'] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', '', true, captures));
    expect(result.current.book).toEqual({ chapter: true, chapters: ['index.qmd', 'one.qmd'] });
    expect(h.bookInfoFor).toHaveBeenCalledWith('one.qmd');
  });

  it('has no book without a path, before the wasm is ready, and in the native embed (which renders one chapter)', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['one.qmd'] });
    expect(renderHook(() => useDownloadAs(null, '', true, captures)).result.current.book).toBeNull();
    expect(renderHook(() => useDownloadAs('one.qmd', '', false, captures)).result.current.book).toBeNull();
    h.embed = true;
    expect(renderHook(() => useDownloadAs('one.qmd', '', true, captures)).result.current.book).toBeNull();
  });

  it('refreshBook re-reads the resolver (the project\'s chapter list may have changed)', () => {
    h.bookInfoFor.mockReturnValue({ chapter: false, chapters: ['index.qmd'] });
    const { result } = renderHook(() => useDownloadAs('new.qmd', '', true, captures));
    expect(result.current.book?.chapter).toBe(false);
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['index.qmd', 'new.qmd'] });
    act(() => result.current.refreshBook());
    expect(result.current.book?.chapter).toBe(true);
  });

  it('"Download book as" starts with scope auto and the capture doc ids of the chapters that have one, resolved fresh at click time', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['index.qmd', 'one.qmd', 'two.qmd'] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', 'text', true, captures));
    act(() => result.current.start(EPUB, 'auto'));
    expect(h.start).toHaveBeenCalledWith({ path: 'one.qmd', format: EPUB, content: 'text', scope: 'auto', captureDocIds: { 'index.qmd': 'c0', 'one.qmd': 'c1' } });
  });

  it('a click on "Download book as" when the page is no longer a chapter falls back to the chapter-alone scope', () => {
    h.bookInfoFor.mockReturnValueOnce({ chapter: true, chapters: ['one.qmd'] }).mockReturnValue({ chapter: false, chapters: [] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', '', true, captures));
    act(() => result.current.start(EPUB, 'auto'));
    expect(h.start).toHaveBeenCalledWith({ path: 'one.qmd', format: EPUB, content: '', scope: 'chapter' });
  });

  it('"This chapter only" starts with scope chapter and no captures; an ordinary pick passes no scope', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['one.qmd'] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', '', true, captures));
    act(() => result.current.start(EPUB, 'chapter'));
    expect(h.start).toHaveBeenLastCalledWith({ path: 'one.qmd', format: EPUB, content: '', scope: 'chapter' });
    act(() => result.current.start(EPUB));
    expect(h.start).toHaveBeenLastCalledWith({ path: 'one.qmd', format: EPUB, content: '' });
  });
});
