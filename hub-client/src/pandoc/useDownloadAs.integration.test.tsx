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
  setIdentities: vi.fn(),
  bookInfoFor: vi.fn(),
  embed: false,
  idle: { phase: 'idle' as const },
}));

vi.mock('@quarto/preview-runtime', () => ({ isWasmReady: () => true, resolvePandocFormats: () => null }));
vi.mock('./bookInfo', () => ({ bookInfoFor: h.bookInfoFor }));
vi.mock('./downloadAttribution', () => ({ setDownloadIdentities: h.setIdentities }));
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
  h.setIdentities.mockClear();
  h.bookInfoFor.mockReset();
  h.embed = false;
});

describe('useDownloadAs: author table', () => {
  it('a hook instance with no table (the download-mode pane) leaves the editor\'s table alone', () => {
    const table = { bear: { name: 'Kind Bear', color: '#ff0000' } };
    renderHook(() => useDownloadAs('one.qmd', '', true, undefined, table));
    renderHook(() => useDownloadAs('one.qmd', '', true));
    expect(h.setIdentities).toHaveBeenLastCalledWith(table);
    expect(h.setIdentities).not.toHaveBeenCalledWith({});
  });
});

describe('useDownloadAs: books', () => {
  it('exposes the resolver\'s book field for the open document', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['index.qmd', 'one.qmd'] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', '', true, captures));
    expect(result.current.book).toEqual({ chapter: true, chapters: ['index.qmd', 'one.qmd'] });
    expect(h.bookInfoFor).toHaveBeenCalledWith('one.qmd');
  });

  it('has no book without a path or before the wasm is ready', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['one.qmd'] });
    expect(renderHook(() => useDownloadAs(null, '', true, captures)).result.current.book).toBeNull();
    expect(renderHook(() => useDownloadAs('one.qmd', '', false, captures)).result.current.book).toBeNull();
  });

  it('offers the book entries in the native embed too, and "Download book as" sends book-scope book there', () => {
    h.embed = true;
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['index.qmd', 'one.qmd'] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', 'text', true));
    expect(result.current.book).toEqual({ chapter: true, chapters: ['index.qmd', 'one.qmd'] });
    act(() => result.current.start(EPUB, { 'book-scope': 'book' }));
    // The embed has no capture table (the server resolves captures itself, E10), so the ids are empty and the native runner ignores them.
    expect(h.start).toHaveBeenLastCalledWith({ path: 'one.qmd', format: EPUB, content: 'text', metadata: { 'book-scope': 'book' }, captureDocIds: {} });
    act(() => result.current.start(EPUB, { 'book-scope': 'chapter' }));
    expect(h.start).toHaveBeenLastCalledWith({ path: 'one.qmd', format: EPUB, content: 'text', metadata: { 'book-scope': 'chapter' } });
  });

  it('refreshBook re-reads the resolver (the project\'s chapter list may have changed)', () => {
    h.bookInfoFor.mockReturnValue({ chapter: false, chapters: ['index.qmd'] });
    const { result } = renderHook(() => useDownloadAs('new.qmd', '', true, captures));
    expect(result.current.book?.chapter).toBe(false);
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['index.qmd', 'new.qmd'] });
    act(() => result.current.refreshBook());
    expect(result.current.book?.chapter).toBe(true);
  });

  it('"Download book as" starts with book-scope book and the capture doc ids of the chapters that have one, resolved fresh at click time', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['index.qmd', 'one.qmd', 'two.qmd'] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', 'text', true, captures));
    act(() => result.current.start(EPUB, { 'book-scope': 'book' }));
    expect(h.start).toHaveBeenCalledWith({ path: 'one.qmd', format: EPUB, content: 'text', metadata: { 'book-scope': 'book' }, captureDocIds: { 'index.qmd': 'c0', 'one.qmd': 'c1' } });
  });

  it('a "Download book as" click on a page that is no longer a chapter still sends book-scope book (Rust returns the page and the notice) with no capture ids', () => {
    h.bookInfoFor.mockReturnValue({ chapter: false, chapters: [] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', '', true, captures));
    act(() => result.current.start(EPUB, { 'book-scope': 'book' }));
    expect(h.start).toHaveBeenCalledWith({ path: 'one.qmd', format: EPUB, content: '', metadata: { 'book-scope': 'book' } });
  });

  it('"This chapter only" starts with book-scope chapter and the page\'s own capture doc id; an ordinary pick passes no metadata', () => {
    h.bookInfoFor.mockReturnValue({ chapter: true, chapters: ['one.qmd'] });
    const { result } = renderHook(() => useDownloadAs('one.qmd', '', true, captures));
    act(() => result.current.start(EPUB, { 'book-scope': 'chapter' }));
    expect(h.start).toHaveBeenLastCalledWith({ path: 'one.qmd', format: EPUB, content: '', metadata: { 'book-scope': 'chapter' }, captureDocId: 'c1' });
    act(() => result.current.start(EPUB));
    expect(h.start).toHaveBeenLastCalledWith({ path: 'one.qmd', format: EPUB, content: '', captureDocId: 'c1' });
  });
});
