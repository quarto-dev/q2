import '@testing-library/jest-dom';
import { vi } from 'vitest';

if (!globalThis.crypto?.randomUUID) {
  const cryptoPolyfill = {
    ...globalThis.crypto,
    randomUUID: () => 'test-uuid-' + Math.random().toString(36).substring(2, 11),
  } as Crypto;
  Object.defineProperty(globalThis, 'crypto', { value: cryptoPolyfill });
}

if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
  }));
}

if (!globalThis.IntersectionObserver) {
  globalThis.IntersectionObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
    root: null,
    rootMargin: '',
    thresholds: [],
    takeRecords: () => [],
  })) as unknown as typeof IntersectionObserver;
}

// jsdom does not implement getClientRects()/getBoundingClientRect() on Text
// nodes, so ProseMirror's coordsAtPos (used by tiptap focus → scrollIntoView)
// throws "target.getClientRects is not a function" when a tiptap editor is
// mounted in a test and its autofocus rAF fires. Geometry is meaningless in
// jsdom anyway (verified in a real browser), so stub zero-size rects to keep the
// editor's focus/scroll machinery from crashing the test run.
{
  const emptyRectList = () =>
    Object.assign([], { item: () => null }) as unknown as DOMRectList;
  const zeroRect = () =>
    ({ x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) }) as DOMRect;
  // Text nodes and Ranges are the two targets ProseMirror's coordsAtPos passes to
  // singleRect(); jsdom implements getClientRects on neither (Element has it).
  for (const Ctor of [globalThis.Text, globalThis.Range]) {
    const proto = Ctor?.prototype as { getClientRects?: unknown; getBoundingClientRect?: unknown } | undefined;
    if (proto && typeof proto.getClientRects !== 'function') proto.getClientRects = emptyRectList;
    if (proto && typeof proto.getBoundingClientRect !== 'function') proto.getBoundingClientRect = zeroRect;
  }
}

// Same shape as the block above, different missing API (bd-cpyq99ps). jsdom does
// not implement elementFromPoint, and ProseMirror's posAtCoords calls
//   (view.root.elementFromPoint ? view.root : doc).elementFromPoint(x, y)
// — with neither implemented it takes the `doc` branch and throws
// "elementFromPoint is not a function" whenever a mounted tiptap editor replays
// an opening click (RichTextEditor's placement rAF, bd-q9lyghv2).
//
// That made it a FLAKE rather than a failure: because the call happens in a
// requestAnimationFrame, the throw escaped as an *unhandled* error whenever the
// frame fired after its test had finished, and vitest failed the entire run
// with every test passing.
//
// null is the honest jsdom answer — there is no layout, so no element is under
// any point. posAtCoords treats a null hit as "outside the editor" and returns
// null, which is precisely the miss that caretFromClick.ts documents and that
// its callers already handle by falling back to end-of-block focus. Pinned by
// caretFromClick.integration.test.ts against a real ProseMirror view.
{
  const proto = globalThis.Document?.prototype as
    | { elementFromPoint?: unknown }
    | undefined;
  if (proto && typeof proto.elementFromPoint !== 'function') {
    proto.elementFromPoint = () => null;
  }
}
