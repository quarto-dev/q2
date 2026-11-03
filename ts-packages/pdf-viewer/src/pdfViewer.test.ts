// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mountPdfViewer, viewerUrl, VIEWER_TITLE, VIEWER_WATCHDOG_MS } from './pdfViewer';

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

/**
 * The swap, driven by a stand-in for pdf.js's `PDFViewerApplication` installed on each frame's window:
 * the real viewer never loads under jsdom, so the test fires the iframe's `load` event itself and
 * then the event-bus events the gate listens for.
 */
describe('mountPdfViewer swaps', () => {
  const RENDERING_FINISHED = 3;

  function fakeApp() {
    const handlers = new Map<string, Set<(e: unknown) => void>>();
    let state = 0;
    const app = {
      initializedPromise: Promise.resolve(),
      isInitialViewSet: false,
      eventBus: {
        on: (name: string, fn: (e: unknown) => void) => void (handlers.get(name) ?? handlers.set(name, new Set()).get(name)!).add(fn),
        off: (name: string, fn: (e: unknown) => void) => void handlers.get(name)?.delete(fn),
      },
      pdfViewer: {
        currentScaleValue: 'auto' as string | number,
        currentPageNumber: 1,
        container: document.createElement('div'),
        getPageView: () => ({ renderingState: state }),
      },
      emit: (name: string, e: unknown = {}) => handlers.get(name)?.forEach((fn) => fn(e)),
      /** The gate's three conditions, as pdf.js reaches them for a document whose first page has drawn. */
      paint() {
        app.emit('pagesloaded');
        app.isInitialViewSet = true;
        app.emit('documentinit');
        state = RENDERING_FINISHED;
        app.emit('pagerendered');
      },
      listening: () => [...handlers.values()].reduce((n, s) => n + s.size, 0),
    };
    return app;
  }

  let container: HTMLElement;
  const frames = () => [...container.querySelectorAll('iframe')];

  /** Starts the viewer in the newest frame: the stand-in app is installed, then the frame's `load` fires. */
  async function start(app: ReturnType<typeof fakeApp>) {
    const iframe = frames().at(-1)!;
    (iframe.contentWindow as unknown as { PDFViewerApplication: unknown }).PDFViewerApplication = app;
    iframe.dispatchEvent(new Event('load'));
    await vi.advanceTimersByTimeAsync(0); // `initializedPromise.then` installs the gate
  }

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: () => `blob:test/${Math.random()}`, revokeObjectURL: () => {} }));
    container = document.createElement('div');
    document.body.appendChild(container);
  });
  afterEach(() => {
    container.remove();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('promotes a frame once it has painted, and a recompile replaces it only once the new one has', async () => {
    const viewer = mountPdfViewer(container, '/');
    const first = viewer.show(new Uint8Array([1]), { key: 'a', fileName: 'a.pdf' });
    const appA = fakeApp();
    await start(appA);
    expect(frames()[0].title).toBe('');
    appA.paint();
    await expect(first).resolves.toBe('shown');
    expect(frames().map((f) => f.title)).toEqual([VIEWER_TITLE]);
    expect(appA.listening()).toBe(0);

    const second = viewer.show(new Uint8Array([2]), { key: 'a', fileName: 'a.pdf' });
    const appB = fakeApp();
    await start(appB);
    // Two frames while the new one loads underneath; the live one keeps its title.
    expect(frames().map((f) => [f.title, f.style.zIndex])).toEqual([[VIEWER_TITLE, '2'], ['', '0']]);
    appB.paint();
    await expect(second).resolves.toBe('shown');
    expect(frames().map((f) => [f.title, f.style.zIndex])).toEqual([[VIEWER_TITLE, '2']]);
  });

  it('a newer show before the pending frame has painted supersedes it: no watchdog error, the newer frame is promoted', async () => {
    const viewer = mountPdfViewer(container, '/');
    const first = viewer.show(new Uint8Array([1]), { key: 'a', fileName: 'a.pdf' });
    const appA = fakeApp();
    await start(appA);
    expect(appA.listening()).toBe(4);

    const second = viewer.show(new Uint8Array([2]), { key: 'a', fileName: 'a.pdf' });
    await expect(first).resolves.toBe('superseded');
    // The superseded frame is gone and no longer listening; its watchdog is disarmed.
    expect(frames()).toHaveLength(1);
    expect(appA.listening()).toBe(0);
    // Past the first load's watchdog deadline: a still-armed one would reject `first` here (and vitest
    // fails the run on an unhandled rejection).
    await vi.advanceTimersByTimeAsync(VIEWER_WATCHDOG_MS + 1_000);
    const appB = fakeApp();
    await start(appB);
    appB.paint();
    await expect(second).resolves.toBe('shown');
    expect(frames().map((f) => f.title)).toEqual([VIEWER_TITLE]);
  });

  it('the watchdog still rejects a live load that never paints, and drops its frame', async () => {
    const viewer = mountPdfViewer(container, '/');
    const first = viewer.show(new Uint8Array([1]), { key: 'a', fileName: 'a.pdf' });
    const appA = fakeApp();
    await start(appA);
    const outcome = first.then(
      (r) => ({ r }),
      (e: Error) => ({ e: e.message }),
    );
    await vi.advanceTimersByTimeAsync(VIEWER_WATCHDOG_MS + 1);
    expect(await outcome).toEqual({ e: 'The PDF viewer drew nothing within 30 s' });
    expect(frames()).toHaveLength(0);
  });

  it('dispose supersedes a pending load instead of letting its watchdog report later', async () => {
    const viewer = mountPdfViewer(container, '/');
    const first = viewer.show(new Uint8Array([1]), { key: 'a', fileName: 'a.pdf' });
    const appA = fakeApp();
    await start(appA);
    viewer.dispose();
    await expect(first).resolves.toBe('superseded');
    expect(frames()).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(VIEWER_WATCHDOG_MS + 1_000);
    await expect(viewer.show(new Uint8Array([2]), { key: 'a', fileName: 'a.pdf' })).resolves.toBe('superseded');
  });
});
