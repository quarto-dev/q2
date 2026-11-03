/**
 * The pdf.js viewer for compiled PDFs (host phase H9, design T7).
 *
 * The stock viewer (`public/pdfjs/` of the host app, fetched and patched by `scripts/fetch-pandoc-wasm.mjs`)
 * runs in a same-origin iframe. Its worker is patched to report a constant document
 * fingerprint, so the viewer's own saved zoom and scroll (`localStorage['pdfjs.history']`,
 * written on every viewarea change) restore when a recompile is loaded. That history is keyed by
 * fingerprint alone, so the host clears it when the viewer moves to another file.
 *
 * A recompile loads into a second iframe under the live one, which it replaces once it has drawn a page:
 * reopening a document inside the live viewer empties it for a few frames, a visible flash. The frame on
 * top is never an empty viewer: a new document that fails to draw is dropped (see `VIEWER_WATCHDOG_MS`),
 * and the previous one stays.
 *
 * Only the live frame is titled `PDF preview` (`VIEWER_TITLE`); a frame still loading has no title and is
 * `aria-hidden` and `inert`, so assistive tech and title lookups (the e2e specs) see exactly one viewer
 * (bd-e4q7juv0). A loading frame is never hidden with CSS: pdf.js must lay it out and paint it underneath,
 * or it never becomes ready to promote.
 */

/** The slice of pdf.js's `PDFViewerApplication` this module uses. */
interface ViewerApp {
  initializedPromise: Promise<void>;
  eventBus: { on(name: string, fn: (e: unknown) => void): void; off(name: string, fn: (e: unknown) => void): void };
  /** Set (synchronously) by `setInitialView`, which applies the saved zoom and scroll, or the URL hash, on open. */
  isInitialViewSet?: boolean;
  pdfViewer?: {
    currentScaleValue: string | number;
    currentPageNumber: number;
    container: HTMLElement;
    getPageView(index: number): { renderingState: number } | undefined;
  };
}

/** pdf.js's `RenderingStates.FINISHED`: the page's canvas has been drawn at its current layout. */
const RENDERING_FINISHED = 3;

type ViewerWindow = Window & { PDFViewerApplication?: ViewerApp };

export interface PdfViewerOptions {
  /**
   * Called with each new viewer frame's window and document once its first page has drawn, just
   * before the frame replaces the live one. The host customises the viewer chrome here (the
   * viewer's CSP blocks inline styles and scripts: use CSSOM or `cssText`). A throw rejects that
   * `show()`, so a missing anchor fails loudly.
   */
  onFrameReady?(win: Window, doc: Document): void;
}

/**
 * How a `show()` ended: `shown` once its document is on screen and painted; `superseded` when a newer
 * `show()` (or `dispose()`) replaced it before it was promoted, so it was dropped without ever being
 * on screen. A superseded load is not an error: the newer document's own `show()` reports for it.
 */
export type ShowResult = 'shown' | 'superseded';

export interface PdfViewerHandle {
  /**
   * Show `pdf`. A call with the same `key` as the previous one keeps the reader's zoom and scroll;
   * a new `key` starts from the top. Resolves `shown` once the new document is on screen and
   * painted, or `superseded` if a newer call replaced it first (its watchdog and listeners are
   * cancelled, so it never reports afterwards). Rejects if the viewer fails to open it (pdf.js
   * reports an error, or nothing is drawn within `VIEWER_WATCHDOG_MS`), in which case the previous
   * document stays on screen.
   */
  show(pdf: Uint8Array, options: { key: string; fileName: string }): Promise<ShowResult>;
  dispose(): void;
}

export const PDF_HISTORY_KEY = 'pdfjs.history';

/** The live viewer frame's `title`; a frame still loading has none. */
export const VIEWER_TITLE = 'PDF preview';

/** URL of the stock viewer under the app's base path (`'./'` for the preview SPA, Vite's `BASE_URL` for hub-client). */
export function viewerUrl(base: string, file?: string, restore = false): string {
  const root = `${base.endsWith('/') ? base : `${base}/`}pdfjs/web/viewer.html`;
  if (file === undefined) return root;
  // Any hash bookmark outranks the viewer's saved zoom and scroll, so a restoring open carries none.
  return `${root}?file=${encodeURIComponent(file)}${restore ? '' : '#pagemode=none'}`;
}

/**
 * A viewer that has drawn nothing after this long is treated as dead: the swap fails and the previous
 * document stays. It is a watchdog, not a fallback: a slow viewer is waited for, however long it takes,
 * because promoting an unpainted viewer is the flash the second iframe exists to prevent (invariant
 * decided under bd-c72wsugj: the frame on top is never an empty viewer).
 */
export const VIEWER_WATCHDOG_MS = 30_000;

export function mountPdfViewer(container: HTMLElement, base: string, options: PdfViewerOptions = {}): PdfViewerHandle {
  interface Frame {
    iframe: HTMLIFrameElement;
    blobUrl: string;
    /** Stops the load reporting: clears its watchdog and listeners and settles `ready` as superseded. */
    cancel: () => void;
  }
  let current: Frame | null = null;
  let pending: Frame | null = null;
  let lastKey: string | null = null;
  let disposed = false;

  // Cancelling before removing matters: a discarded frame's watchdog would otherwise fire up to
  // VIEWER_WATCHDOG_MS later and reject a `show()` whose document was never on screen, which the
  // host then reports as an error over a perfectly good newer document (bd-xmb7rdc7).
  const discard = (f: Frame) => {
    f.cancel();
    f.iframe.remove();
    URL.revokeObjectURL(f.blobUrl);
  };

  /**
   * Load `pdf` into a new viewer iframe, appended to `container` at `zIndex`. Resolves once its pages
   * are laid out and the page the reader is on has painted at its final layout. The viewer opens
   * whatever `?file=` names, so there is no default sample to suppress.
   *
   * "Final layout" matters: pdf.js draws the first page at the default zoom as soon as the pages
   * exist, and only then (after its stored-view read and a few worker round trips) applies the saved
   * zoom and scroll in `setInitialView`, which resets every page and draws the restored one afresh.
   * Under load the first draw can finish before that reset, so "any page rendered" would promote a
   * viewer that is about to empty itself for a few frames: the flash the second iframe exists to
   * prevent (bd-c72wsugj). The gate is therefore: pages loaded, the initial view set, and the current
   * page's view in the FINISHED state, re-checked on every `pagerendered` and on `documentinit`.
   */
  function load(pdf: Uint8Array, zIndex: number, restore: boolean): { frame: Frame; ready: Promise<boolean> } {
    const blobUrl = URL.createObjectURL(new Blob([pdf as BlobPart], { type: 'application/pdf' }));
    const iframe = document.createElement('iframe');
    iframe.setAttribute('aria-hidden', 'true');
    iframe.inert = true;
    iframe.style.cssText = `position:absolute;inset:0;width:100%;height:100%;border:0;display:block;z-index:${zIndex}`;
    // `ready` resolves true once painted, false when cancelled (superseded or disposed) first. A cancel
    // before the viewer has started just stops the handlers below from arming anything.
    let cancelled = false;
    let settle: (painted: boolean) => void = () => {};
    let cleanup = () => {};
    const cancel = () => {
      cancelled = true;
      cleanup();
      settle(false);
    };
    const ready = new Promise<boolean>((resolve, reject) => {
      settle = resolve;
      // `once`: the subscriptions and the watchdog below belong to this one document load.
      iframe.addEventListener('load', () => {
        if (cancelled) return;
        const app = (iframe.contentWindow as ViewerWindow | null)?.PDFViewerApplication;
        if (!app) return reject(new Error('The PDF viewer did not start'));
        const bus = app.eventBus;
        app.initializedPromise.then(() => {
          if (cancelled) return;
          let loaded = false;
          const timer = setTimeout(
            () => fail(new Error(`The PDF viewer drew nothing within ${VIEWER_WATCHDOG_MS / 1000} s`)),
            VIEWER_WATCHDOG_MS,
          );
          const painted = () => {
            const v = app.pdfViewer;
            return v?.getPageView(v.currentPageNumber - 1)?.renderingState === RENDERING_FINISHED;
          };
          const check = () => loaded && app.isInitialViewSet === true && painted() && finish();
          const onLoaded = () => ((loaded = true), check());
          const onError = (e: unknown) => {
            const { message, reason } = (e ?? {}) as { message?: string; reason?: string | null };
            fail(new Error(reason || message || 'The PDF viewer could not open the document'));
          };
          function done() {
            clearTimeout(timer);
            bus.off('pagesloaded', onLoaded);
            bus.off('documentinit', check);
            bus.off('pagerendered', check);
            bus.off('documenterror', onError);
          }
          function finish() {
            done();
            try {
              const win = iframe.contentWindow;
              if (win) options.onFrameReady?.(win, win.document);
              resolve(true);
            } catch (e) {
              reject(e);
            }
          }
          function fail(error: Error) {
            done();
            reject(error);
          }
          cleanup = done;
          bus.on('pagesloaded', onLoaded);
          bus.on('documentinit', check);
          bus.on('pagerendered', check);
          bus.on('documenterror', onError);
        }, reject);
      }, { once: true });
    });
    iframe.src = viewerUrl(base, blobUrl, restore);
    container.appendChild(iframe);
    return { frame: { iframe, blobUrl, cancel }, ready };
  }

  async function open(pdf: Uint8Array, restore: boolean): Promise<ShowResult> {
    if (getComputedStyle(container).position === 'static') container.style.position = 'relative';
    // Reopening a document in the live viewer empties it for a few frames, which flashes the pane's
    // background. So the new bytes load in a second iframe underneath, and it replaces the first (in
    // one paint) only once it has drawn a page.
    if (pending) discard(pending);
    const { frame, ready } = load(pdf, current ? 0 : 1, restore);
    pending = frame;
    let painted: boolean;
    try {
      painted = await ready;
    } catch (e) {
      if (pending === frame) {
        discard(frame);
        pending = null;
      }
      throw e;
    }
    if (!painted || pending !== frame || disposed) return 'superseded'; // a newer load took over, or the pane closed
    pending = null;
    frame.iframe.style.zIndex = '2';
    frame.iframe.title = VIEWER_TITLE;
    frame.iframe.removeAttribute('aria-hidden');
    frame.iframe.inert = false;
    if (current) discard(current);
    current = frame;
    return 'shown';
  }

  return {
    async show(pdf, { key }) {
      if (disposed) return 'superseded';
      // The history is keyed by fingerprint alone: start over for another file, and for the first document.
      const restore = lastKey === key;
      if (!restore) localStorage.removeItem(PDF_HISTORY_KEY);
      lastKey = key;
      return open(pdf, restore);
    },
    dispose() {
      disposed = true;
      if (pending) discard(pending);
      if (current) discard(current);
      pending = current = null;
    },
  };
}
