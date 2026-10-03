/** The resolver's `book` field for one document (pandoc-host H5, R9). */
import { isWasmReady, resolvePandocFormats, type ResolvePandocFormatsResponse } from '@quarto/preview-runtime';

export interface BookInfo {
  /** This document is one of the book's chapters. */
  chapter: boolean;
  /** The book's file-bearing chapters in order, as sidecar keys (`/`-normalized, relative to the VFS project root). */
  chapters: string[];
}

/** `null` outside a book project, and when the resolve failed. */
export function bookInfoFrom(res: ResolvePandocFormatsResponse | null | undefined): BookInfo | null {
  if (!res || !res.success || !res.book) return null;
  return { chapter: res.book.chapter, chapters: res.book.chapters };
}

/** The book information for `path`, read from the hub wasm; `null` while it is not ready. */
export function bookInfoFor(path: string): BookInfo | null {
  return isWasmReady() ? bookInfoFrom(resolvePandocFormats(path)) : null;
}
