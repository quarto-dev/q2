/** React binding for the download controller: status, menu formats and the click handler. */
import { useCallback, useEffect, useMemo, useRef, useSyncExternalStore } from 'react';
import { isWasmReady } from '@quarto/preview-runtime';
import { getDownloadController, menuFormats, downloadAvailable } from './downloadService';
import type { DownloadFormat, DownloadStatus } from './downloadController';

export interface DownloadAs {
  /** "Download as" is offered in this build. */
  available: boolean;
  formats: DownloadFormat[];
  status: DownloadStatus;
  start: (format: DownloadFormat) => void;
  cancel: () => void;
  dismiss: () => void;
}

/**
 * `path` is the open document (as the preview uses it); `content` is the editor's current
 * text, which only the native executor reads. `wasmReady` re-derives the menu once the hub
 * wasm (which owns the format table) is up.
 */
export function useDownloadAs(path: string | null, content: string, wasmReady: boolean): DownloadAs {
  const controller = getDownloadController();
  const status = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const contentRef = useRef(content);
  useEffect(() => {
    contentRef.current = content;
  }, [content]);
  const available = downloadAvailable();

  const formats = useMemo(() => (available && wasmReady && isWasmReady() ? menuFormats() : []), [available, wasmReady]);

  // A finished or failed status belongs to the document it was for.
  useEffect(() => {
    controller.dismiss();
  }, [controller, path]);

  const start = useCallback(
    (format: DownloadFormat) => {
      if (!path) return;
      void controller.start({ path, format, content: contentRef.current });
    },
    [controller, path],
  );
  const cancel = useCallback(() => controller.cancel(), [controller]);
  const dismiss = useCallback(() => controller.dismiss(), [controller]);
  return { available, formats, status, start, cancel, dismiss };
}
