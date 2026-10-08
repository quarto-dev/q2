/** React binding for the download controller: status, menu formats and the click handler. */
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { isWasmReady, resolvePandocFormats, type ActorIdentity, type CaptureRef } from '@quarto/preview-runtime';
import { getDownloadController, menuFormats, downloadAvailable, withOutputExt } from './downloadService';
import { bookInfoFor, type BookInfo } from './bookInfo';
import { captureDocIdsFor } from './captureFetch';
import { isPreviewEmbed } from './featureFlag';
import { setDownloadIdentities } from './downloadAttribution';
import type { DownloadFormat, DownloadStatus, StartOptions } from './downloadController';

export interface DownloadAs {
  /** "Download as" is offered in this build. */
  available: boolean;
  formats: DownloadFormat[];
  status: DownloadStatus;
  /** `scope` is the book entries' pick: `'auto'` the whole book, `'chapter'` this chapter only. */
  start: (format: DownloadFormat, scope?: 'auto' | 'chapter') => void;
  /** The resolver's book field for the open document; null outside a book (and in the native embed). */
  book?: BookInfo | null;
  /** Re-read `book` (the project's chapter list may have changed since the document opened). */
  refreshBook?: () => void;
  cancel: () => void;
  dismiss: () => void;
}

/**
 * `path` is the open document (as the preview uses it); `content` is the editor's current
 * text, which only the native executor reads. `wasmReady` re-derives the menu once the hub
 * wasm (which owns the format table) is up. `captures` is the project's capture sidecar: a
 * whole-book download fetches every chapter's capture from it at click time. `identities` is
 * the author table that names who wrote a comment or tracked change in a docx or pptx.
 */
export function useDownloadAs(path: string | null, content: string, wasmReady: boolean, captures?: Record<string, CaptureRef>, identities?: Record<string, ActorIdentity>): DownloadAs {
  const controller = getDownloadController();
  // A comment's author is looked up at the click, in the editor's current author table. A caller
  // that has no table (the download-mode pane) must not write: the table is shared module state,
  // and an empty write would erase the editor's, leaving authors as raw actor-id prefixes.
  useEffect(() => {
    if (identities) setDownloadIdentities(identities);
  }, [identities]);
  const status = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const contentRef = useRef(content);
  useEffect(() => {
    contentRef.current = content;
  }, [content]);
  const capturesRef = useRef(captures);
  useEffect(() => {
    capturesRef.current = captures;
  }, [captures]);
  const available = downloadAvailable();

  // The native embed renders one chapter, so it never offers the book entries.
  const [bookVersion, setBookVersion] = useState(0);
  const book = useMemo(() => {
    void bookVersion;
    return available && wasmReady && path && !isPreviewEmbed() ? bookInfoFor(path) : null;
  }, [available, wasmReady, path, bookVersion]);
  const refreshBook = useCallback(() => setBookVersion((v) => v + 1), []);

  const formats = useMemo(() => (available && wasmReady && isWasmReady() ? menuFormats() : []), [available, wasmReady]);

  // A finished or failed status belongs to the document it was for.
  useEffect(() => {
    controller.dismiss();
  }, [controller, path]);

  const start = useCallback(
    (format: DownloadFormat, scope?: 'auto' | 'chapter') => {
      if (!path) return;
      // Resolved at the click, like the book scope below: the document's `output-ext` is what it says now.
      const named = withOutputExt(format, isWasmReady() ? resolvePandocFormats(path) : null);
      const options: StartOptions = { path, format: named, content: contentRef.current };
      if (scope === 'auto') {
        // Re-resolved now: the chapter list is what the project says at the click, not at the menu open.
        const fresh = bookInfoFor(path);
        if (fresh?.chapter) {
          options.scope = 'auto';
          options.captureDocIds = captureDocIdsFor(fresh.chapters, capturesRef.current);
        } else {
          options.scope = 'chapter';
        }
      } else if (scope === 'chapter') {
        options.scope = 'chapter';
      }
      if (options.scope !== 'auto') options.captureDocId = capturesRef.current?.[path]?.captureDocId;
      void controller.start(options);
    },
    [controller, path],
  );
  const cancel = useCallback(() => controller.cancel(), [controller]);
  const dismiss = useCallback(() => controller.dismiss(), [controller]);
  return { available, formats, status, start, book, refreshBook, cancel, dismiss };
}
