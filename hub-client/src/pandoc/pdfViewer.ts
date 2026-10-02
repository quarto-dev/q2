/**
 * The pdf.js viewer for compiled PDFs (host phase H9, design T7).
 *
 * The stock viewer (`public/pdfjs/`, fetched and patched by `scripts/fetch-pandoc-wasm.mjs`)
 * runs in a same-origin iframe. Its worker is patched to report a constant document
 * fingerprint, so the viewer's own saved zoom and scroll (`localStorage['pdfjs.history']`,
 * written on every viewarea change) restore when a recompile is loaded. That history is keyed by
 * fingerprint alone, so the host clears it when the viewer moves to another file.
 *
 * A recompile loads into a second iframe under the live one, which it replaces once it has drawn a page:
 * reopening a document inside the live viewer empties it for a few frames, a visible flash.
 */

/** The slice of pdf.js's `PDFViewerApplication` this module uses. */
interface ViewerApp {
  initializedPromise: Promise<void>;
  eventBus: { on(name: string, fn: (e: unknown) => void): void; off(name: string, fn: (e: unknown) => void): void };
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
export function viewerUrl(base: string = import.meta.env.BASE_URL, file?: string, restore = false): string {
  const root = `${base.endsWith('/') ? base : `${base}/`}pdfjs/web/viewer.html`;
  if (file === undefined) return root;
  // Any hash bookmark outranks the viewer's saved zoom and scroll, so a restoring open carries none.
  return `${root}?file=${encodeURIComponent(file)}${restore ? '' : '#pagemode=none'}`;
}

/** A swap never waits longer than this for the new viewer's first page; slower than that, show it anyway. */
const SWAP_TIMEOUT_MS = 5000;

export function mountPdfViewer(container: HTMLElement, base?: string): PdfViewerHandle {
  interface Frame {
    iframe: HTMLIFrameElement;
    blobUrl: string;
  }
  let current: Frame | null = null;
  let pending: Frame | null = null;
  let lastKey: string | null = null;
  let disposed = false;

  const discard = (f: Frame) => {
    f.iframe.remove();
    URL.revokeObjectURL(f.blobUrl);
  };

  /**
   * Load `pdf` into a new viewer iframe, appended to `container` at `zIndex`. Resolves once its pages
   * are laid out and the first has painted. The viewer opens whatever `?file=` names, so there is no
   * default sample to suppress.
   */
  function load(pdf: Uint8Array, zIndex: number, restore: boolean): { frame: Frame; ready: Promise<void> } {
    const blobUrl = URL.createObjectURL(new Blob([pdf as BlobPart], { type: 'application/pdf' }));
    const iframe = document.createElement('iframe');
    iframe.title = 'PDF preview';
    iframe.style.cssText = `position:absolute;inset:0;width:100%;height:100%;border:0;display:block;z-index:${zIndex}`;
    const ready = new Promise<void>((resolve, reject) => {
      iframe.addEventListener('load', () => {
        const app = (iframe.contentWindow as ViewerWindow | null)?.PDFViewerApplication;
        if (!app) return reject(new Error('The PDF viewer did not start'));
        const bus = app.eventBus;
        app.initializedPromise.then(() => {
          let loaded = false;
          let painted = false;
          const timer = setTimeout(finish, SWAP_TIMEOUT_MS);
          const onLoaded = () => ((loaded = true), check());
          const onPainted = () => ((painted = true), check());
          const check = () => loaded && painted && finish();
          function finish() {
            clearTimeout(timer);
            bus.off('pagesloaded', onLoaded);
            bus.off('pagerendered', onPainted);
            resolve();
          }
          bus.on('pagesloaded', onLoaded);
          bus.on('pagerendered', onPainted);
        }, reject);
      });
    });
    iframe.src = viewerUrl(base, blobUrl, restore);
    container.appendChild(iframe);
    return { frame: { iframe, blobUrl }, ready };
  }

  async function open(pdf: Uint8Array, restore: boolean): Promise<void> {
    if (getComputedStyle(container).position === 'static') container.style.position = 'relative';
    // Reopening a document in the live viewer empties it for a few frames, which flashes the pane's
    // background. So the new bytes load in a second iframe underneath, and it replaces the first (in
    // one paint) only once it has drawn a page.
    if (pending) discard(pending);
    const { frame, ready } = load(pdf, current ? 0 : 1, restore);
    pending = frame;
    try {
      await ready;
    } catch (e) {
      if (pending === frame) {
        discard(frame);
        pending = null;
      }
      throw e;
    }
    if (pending !== frame || disposed) return; // superseded, or the pane closed
    pending = null;
    frame.iframe.style.zIndex = '2';
    if (current) discard(current);
    current = frame;
  }

  return {
    async show(pdf, { key }) {
      if (disposed) return;
      // The history is keyed by fingerprint alone: start over for another file, and for the first document.
      const restore = lastKey === key;
      if (!restore) localStorage.removeItem(PDF_HISTORY_KEY);
      lastKey = key;
      await open(pdf, restore);
    },
    dispose() {
      disposed = true;
      if (pending) discard(pending);
      if (current) discard(current);
      pending = current = null;
    },
  };
}
