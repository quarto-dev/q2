/**
 * The pdf.js viewer for compiled PDFs (host phase H9, design T7).
 *
 * The stock viewer (`public/pdfjs/`, fetched and patched by `scripts/fetch-pandoc-wasm.mjs`)
 * runs in a same-origin iframe. Its worker is patched to report a constant document
 * fingerprint, so the viewer's own saved zoom and scroll (`localStorage['pdfjs.history']`,
 * written on every viewarea change) restore when a recompile is opened in place. That history
 * is keyed by fingerprint alone, so the host clears it when the viewer moves to another file.
 */

/** The slice of pdf.js's `PDFViewerApplication` this module uses. */
interface ViewerApp {
  initializedPromise: Promise<void>;
  eventBus: { on(name: string, fn: (e: unknown) => void): void; off(name: string, fn: (e: unknown) => void): void };
  open(args: { data: Uint8Array; filename?: string }): Promise<void>;
  pdfViewer?: { currentScaleValue: string | number; currentPageNumber: number; container: HTMLElement };
}

type ViewerWindow = Window & { PDFViewerApplication?: ViewerApp };

export interface PdfViewerHandle {
  /**
   * Show `pdf`. A call with the same `key` as the previous one keeps the reader's zoom and scroll;
   * a new `key` starts from the top. Resolves when the pages are laid out.
   */
  show(pdf: Uint8Array, options: { key: string; fileName: string }): Promise<void>;
  dispose(): void;
}

export const PDF_HISTORY_KEY = 'pdfjs.history';

/** URL of the stock viewer under the app's base path. */
export function viewerUrl(base: string = import.meta.env.BASE_URL, file?: string): string {
  const root = `${base.endsWith('/') ? base : `${base}/`}pdfjs/web/viewer.html`;
  return file === undefined ? root : `${root}?file=${encodeURIComponent(file)}#pagemode=none`;
}

export function mountPdfViewer(container: HTMLElement, base?: string): PdfViewerHandle {
  let iframe: HTMLIFrameElement | null = null;
  let lastKey: string | null = null;
  let blobUrl: string | null = null;
  let disposed = false;

  const app = () => (iframe?.contentWindow as ViewerWindow | null)?.PDFViewerApplication;

  const pagesLoaded = (a: ViewerApp) =>
    new Promise<void>((resolve) => {
      const done = () => {
        a.eventBus.off('pagesloaded', done);
        resolve();
      };
      a.eventBus.on('pagesloaded', done);
    });

  async function open(pdf: Uint8Array, fileName: string): Promise<void> {
    const current = app();
    if (iframe && current) {
      await current.initializedPromise;
      const loaded = pagesLoaded(current);
      // The viewer takes ownership of the buffer it is given.
      await current.open({ data: pdf.slice(), filename: fileName });
      return loaded;
    }
    // First document: the viewer opens whatever `?file=` names, so there is no default sample to suppress.
    blobUrl = URL.createObjectURL(new Blob([pdf as BlobPart], { type: 'application/pdf' }));
    iframe = document.createElement('iframe');
    iframe.title = 'PDF preview';
    iframe.style.cssText = 'width:100%;height:100%;border:0;display:block';
    const loaded = new Promise<void>((resolve, reject) => {
      iframe!.addEventListener('load', () => {
        const a = app();
        if (!a) return reject(new Error('The PDF viewer did not start'));
        a.initializedPromise.then(() => pagesLoaded(a)).then(resolve, reject);
      });
    });
    iframe.src = viewerUrl(base, blobUrl);
    container.appendChild(iframe);
    return loaded;
  }

  return {
    async show(pdf, { key, fileName }) {
      if (disposed) return;
      if (lastKey !== null && lastKey !== key) localStorage.removeItem(PDF_HISTORY_KEY);
      if (lastKey === null) localStorage.removeItem(PDF_HISTORY_KEY);
      lastKey = key;
      await open(pdf, fileName);
    },
    dispose() {
      disposed = true;
      iframe?.remove();
      iframe = null;
      if (blobUrl) URL.revokeObjectURL(blobUrl);
      blobUrl = null;
    },
  };
}
