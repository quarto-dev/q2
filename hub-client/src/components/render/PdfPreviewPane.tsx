/**
 * The third preview kind (pandoc-host H9): a document whose own format is `pdf` is compiled in the
 * browser (pandoc.wasm, then the typst worker) and shown in the pdf.js viewer, in the slot the react and
 * full-DOM previews use. It recompiles on edit (debounced) through its own `DownloadController`, so
 * the top bar's "Download as" status is untouched, and a recompile reopens in place, keeping the
 * reader's page and zoom (`pdfViewer.ts`).
 *
 * The first compile fetches about 33 MB, so until a PDF exists the pane shows a spinner with the
 * fetch progress. A failed compile keeps the last PDF that compiled on screen under an error banner.
 */
import { useEffect, useRef, useState } from 'react';
import { createPdfPreviewController, formatByKey } from '../../pandoc/downloadService';
import { pandocWarmEnabled } from '../../pandoc/featureFlag';
import type { DownloadController, DownloadStatus } from '../../pandoc/downloadController';
import { workingText } from '../../pandoc/downloadText';
import { mountPdfViewer } from '../../pandoc/pdfViewer';
import { DiagnosticList } from '../DownloadAsControl';
import { pdfPreview } from '../../strings';
import LoadingIndicator from '../Loading';
import '../DownloadAsControl.css';
import './PdfPreviewPane.css';

/** Edits closer together than this share one compile (the pandoc step alone is 0.2-0.3 s). */
export const PDF_PREVIEW_DEBOUNCE_MS = 500;

/**
 * The warm path's debounce. H10a Task 0(f) derived the warm whole refresh (request, fonts, pandoc, compile,
 * viewer) at 344 ms in Chromium and 424 ms in WebKit, above the 200 ms bar that would have allowed 250 ms, so
 * it stays at 500 ms: a shorter debounce would mostly start renders that overlap their predecessors.
 */
export const PDF_PREVIEW_WARM_DEBOUNCE_MS = 500;

export default function PdfPreviewPane({
  path,
  content,
  projectKey,
  debounceMs,
  warm: warmOverride,
}: {
  path: string | null;
  content: string;
  /** The project the path belongs to: the pane stays mounted when the project changes, and two projects can share a path. */
  projectKey?: string;
  debounceMs?: number;
  /** A test seam: overrides the build-time flag (the harness compares both paths in one bundle). */
  warm?: boolean;
}) {
  // The flag is a build-time constant: one reading per mount, so the pane never changes path while it lives.
  const [warm] = useState(() => warmOverride ?? pandocWarmEnabled());
  const wait = debounceMs ?? (warm ? PDF_PREVIEW_WARM_DEBOUNCE_MS : PDF_PREVIEW_DEBOUNCE_MS);
  const [status, setStatus] = useState<DownloadStatus>({ phase: 'idle' });
  const [hasPdf, setHasPdf] = useState(false);
  const hostRef = useRef<HTMLDivElement>(null);
  const controllerRef = useRef<DownloadController | null>(null);
  const [retry, setRetry] = useState(0);

  useEffect(() => {
    if (!hostRef.current) return;
    const viewer = mountPdfViewer(hostRef.current);
    const controller = createPdfPreviewController(
      (pdf, info) => {
        setHasPdf(true);
        void viewer.show(pdf, { key: info.path, fileName: info.fileName });
      },
      { warm },
    );
    controllerRef.current = controller;
    // The pool refcount covers StrictMode's mount, cleanup, mount: the workers outlive the gap.
    controller.acquire();
    const unsubscribe = controller.subscribe(() => setStatus(controller.getSnapshot()));
    return () => {
      unsubscribe();
      controller.cancel();
      controller.release();
      controllerRef.current = null;
      viewer.dispose();
      setHasPdf(false);
    };
  }, [warm]);

  const firstRef = useRef(true);
  useEffect(() => {
    firstRef.current = true;
  }, [path, projectKey]);

  useEffect(() => {
    const controller = controllerRef.current;
    const format = formatByKey('pdf');
    if (!controller || !format || !path) return;
    // The first compile after mount (or a retry) starts at once; later edits are debounced.
    const delay = firstRef.current ? 0 : wait;
    firstRef.current = false;
    const timer = setTimeout(() => void controller.start({ path, format, projectKey }), delay);
    return () => clearTimeout(timer);
  }, [path, projectKey, content, retry, wait]);

  return (
    <div className="pdf-preview-pane" data-testid="pdf-preview-pane">
      <div ref={hostRef} className="pdf-preview-host" />
      {status.phase === 'working' && !hasPdf && (
        <div className="pdf-preview-loading" data-testid="pdf-preview-loading">
          <LoadingIndicator label={workingText(status)} />
        </div>
      )}
      {status.phase === 'working' && hasPdf && (
        <div className="pdf-preview-status" role="status" data-testid="pdf-preview-status">
          {pdfPreview.updating}
        </div>
      )}
      {status.phase === 'failed' && (
        <div className="pdf-preview-error" role="alert" data-testid="pdf-preview-error">
          <strong>{pdfPreview.failedTitle}</strong> {hasPdf && pdfPreview.failedKeeping}
          {status.message && <div>{status.message}</div>}
          <DiagnosticList diagnostics={status.diagnostics} />
          <button type="button" className="qh-btn small outline" onClick={() => setRetry((n) => n + 1)}>
            {pdfPreview.retry}
          </button>
        </div>
      )}
    </div>
  );
}
