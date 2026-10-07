/**
 * Window-level fallback for dropped files (document import I5).
 *
 * The sidebar, Monaco and the Add-asset dialog's zone each handle their own drops and call
 * `stopPropagation`, so their drops never reach `document`. Anything else (the top bar, the
 * preview pane, the image viewer, no file open, the backdrop of an open dialog) would get the
 * browser default, which navigates away to the dropped file. This hook catches those drops and
 * hands the entries to `onDrop`, which routes them like an editor drop.
 *
 * It only acts on drags that carry files (`types` includes `Files`), so text and internal sidebar
 * drags are left alone, and it skips events a component handler already took (`defaultPrevented`).
 * It registers nothing unless `enabled` (import is offered: not the `q2 preview` embed).
 */
import { useEffect, useRef } from 'react';
import { collectDroppedEntries, type DroppedEntries } from '../utils/droppedEntries';

const carriesFiles = (e: DragEvent): boolean => !e.defaultPrevented && !!e.dataTransfer && Array.from(e.dataTransfer.types).includes('Files');

export function useWindowFileDrop({ enabled, onDrop }: { enabled: boolean; onDrop: (entries: DroppedEntries) => void }): void {
  // The listeners are attached once per `enabled`; they call the latest handler.
  const handler = useRef(onDrop);
  useEffect(() => {
    handler.current = onDrop;
  }, [onDrop]);

  useEffect(() => {
    if (!enabled) return;
    const onDragOver = (e: DragEvent) => {
      if (!carriesFiles(e)) return;
      // Without this the browser refuses the drop, and then navigates to the file.
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    };
    const onDropEvent = (e: DragEvent) => {
      if (!carriesFiles(e)) return;
      e.preventDefault();
      // The DataTransfer is only readable during the event: start walking it now.
      void collectDroppedEntries(e.dataTransfer!).then((entries) => {
        if (entries.files.length > 0 || entries.folders.length > 0) handler.current(entries);
      });
    };
    document.addEventListener('dragover', onDragOver);
    document.addEventListener('drop', onDropEvent);
    return () => {
      document.removeEventListener('dragover', onDragOver);
      document.removeEventListener('drop', onDropEvent);
    };
  }, [enabled]);
}
