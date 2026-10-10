// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mountPdfViewer, viewerUrl } from './pdfViewer';

describe('viewerUrl', () => {
  it('addresses the stock viewer under the base path', () => {
    expect(viewerUrl('/')).toBe('/pdfjs/web/viewer.html');
    expect(viewerUrl('/hub')).toBe('/hub/pdfjs/web/viewer.html');
  });
  it('names the file and hides the sidebar', () => {
    expect(viewerUrl('/', 'blob:http://x/1')).toBe('/pdfjs/web/viewer.html?file=blob%3Ahttp%3A%2F%2Fx%2F1#pagemode=none');
    expect(viewerUrl('/', 'blob:http://x/1', true)).toBe('/pdfjs/web/viewer.html?file=blob%3Ahttp%3A%2F%2Fx%2F1');
  });
});

/** pdf.js's `RenderingStates`. */
const INITIAL = 0;
const FINISHED = 3;

/** A stand-in for pdf.js's `PDFViewerApplication` with the event bus and page state the swap gate reads. */
function fakeViewerApp() {
  const listeners = new Map<string, Set<(e: unknown) => void>>();
  const app = {
    initializedPromise: Promise.resolve(),
    isInitialViewSet: false,
    renderingState: INITIAL,
    eventBus: {
      on(name: string, fn: (e: unknown) => void) {
        (listeners.get(name) ?? listeners.set(name, new Set()).get(name)!).add(fn);
      },
      off(name: string, fn: (e: unknown) => void) {
        listeners.get(name)?.delete(fn);
      },
    },
    pdfViewer: {
      currentScaleValue: 'auto' as string | number,
      currentPageNumber: 1,
      container: document.createElement('div'),
      getPageView: () => ({ renderingState: app.renderingState }),
    },
    emit(name: string) {
      for (const fn of [...(listeners.get(name) ?? [])]) fn({ pageNumber: 1 });
    },
  };
  return app;
}

const settle = () => new Promise((r) => setTimeout(r, 0));

/**
 * Give the iframe its "viewer" and fire its load event, as the real viewer page would; back once the
 * host has subscribed to the viewer's events (it does so after `initializedPromise`).
 */
async function boot(iframe: HTMLIFrameElement, app: ReturnType<typeof fakeViewerApp>) {
  (iframe.contentWindow as unknown as { PDFViewerApplication: unknown }).PDFViewerApplication = app;
  iframe.dispatchEvent(new Event('load'));
  await settle();
}

describe('mountPdfViewer', () => {
  // jsdom has no object URLs; the viewer only passes them to the iframe's src.
  URL.createObjectURL ??= () => 'blob:test';
  URL.revokeObjectURL ??= () => {};

  it('promotes a recompile only once the restored view has painted, not on the first draw before it', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const viewer = mountPdfViewer(container, '/');
    const frames = () => [...container.querySelectorAll('iframe')];

    // The first document: pages loaded, initial view set, the page drawn.
    const first = viewer.show(new Uint8Array([1]), { key: 'a', fileName: 'a.pdf' });
    const [f1] = frames();
    const app1 = fakeViewerApp();
    await boot(f1, app1);
    app1.emit('pagesloaded');
    app1.isInitialViewSet = true;
    app1.renderingState = FINISHED;
    app1.emit('pagerendered');
    await first;
    expect(f1.style.zIndex).toBe('2');

    // A recompile of the same document loads underneath. pdf.js draws the first page at the default
    // zoom before it applies the saved view; that draw must not promote the new frame.
    const second = viewer.show(new Uint8Array([2]), { key: 'a', fileName: 'a.pdf' });
    const f2 = frames()[1];
    const app2 = fakeViewerApp();
    await boot(f2, app2);
    app2.emit('pagesloaded');
    app2.renderingState = FINISHED;
    app2.emit('pagerendered');
    await settle();
    expect(f2.style.zIndex).toBe('0');
    expect(frames()).toEqual([f1, f2]);

    // setInitialView applies the saved zoom and page: every page view resets, then the current page is drawn again.
    app2.isInitialViewSet = true;
    app2.renderingState = INITIAL;
    app2.emit('documentinit');
    await settle();
    expect(f2.style.zIndex).toBe('0');

    app2.renderingState = FINISHED;
    app2.emit('pagerendered');
    await second;
    expect(f2.style.zIndex).toBe('2');
    expect(frames()).toEqual([f2]);

    viewer.dispose();
    container.remove();
  });

  it('promotes on documentinit when the page drawn before it survives the initial view (no saved view to apply)', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const viewer = mountPdfViewer(container, '/');
    const shown = viewer.show(new Uint8Array([1]), { key: 'b', fileName: 'b.pdf' });
    const [f] = container.querySelectorAll('iframe');
    const app = fakeViewerApp();
    await boot(f, app);
    app.emit('pagesloaded');
    app.renderingState = FINISHED;
    app.emit('pagerendered');
    await settle();
    expect(f.style.zIndex).toBe('1');
    // No reset: the view set by setInitialView is the one already drawn.
    app.isInitialViewSet = true;
    app.emit('documentinit');
    await shown;
    expect(f.style.zIndex).toBe('2');
    viewer.dispose();
    container.remove();
  });
});
