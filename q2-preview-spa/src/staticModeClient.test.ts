/**
 * The `q2 preview --static` reload client
 * (`crates/quarto-preview/src/static_mode/client.js`) behind a
 * path-prefix proxy. The script is plain browser JS with no test
 * harness of its own on the Rust side, so it is exercised here: it runs
 * against a fake `window`/`document`/`EventSource`, served from a page
 * under `/p/abc123/` as a path-stripping proxy would (plan
 * `claude-notes/plans/2026-11-05-q2-preview-proxy-base-path.md`).
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';

const CLIENT_JS = readFileSync(
  path.resolve(__dirname, '../../crates/quarto-preview/src/static_mode/client.js'),
  'utf8',
);

type Listener = (e: { data: string }) => void;

class FakeEventSource {
  static last: FakeEventSource | null = null;
  readonly listeners: Record<string, Listener[]> = {};
  readonly url: string;
  constructor(url: string) {
    this.url = url;
    FakeEventSource.last = this;
  }
  addEventListener(name: string, fn: Listener) {
    (this.listeners[name] ??= []).push(fn);
  }
  emit(name: string, data: unknown) {
    for (const fn of this.listeners[name] ?? []) fn({ data: JSON.stringify(data) });
  }
}

/** Run the client on the page at `href`, as injected with `data-q2-root="root"`. */
function runClient(href: string, root: string | null) {
  const url = new URL(href);
  const location = {
    href: url.href,
    pathname: url.pathname,
    search: url.search,
    replace: vi.fn(),
    reload: vi.fn(),
  };
  const window: Record<string, unknown> = { location };
  const document = {
    currentScript: { getAttribute: (name: string) => (name === 'data-q2-root' ? root : null) },
    getElementById: () => null,
  };
  FakeEventSource.last = null;
  new Function('window', 'document', 'EventSource', CLIENT_JS)(window, document, FakeEventSource);
  const source = FakeEventSource.last as FakeEventSource | null;
  if (!source) throw new Error('client opened no EventSource');
  return { location, source };
}

describe('static reload client behind a path prefix', () => {
  it('opens the event stream under the prefix', () => {
    const { source } = runClient('https://host/p/abc123/posts/a.html', '../');
    expect(source.url).toBe('https://host/p/abc123/__q2-preview/events');
  });

  it('opens the event stream at the root without a prefix', () => {
    const { source } = runClient('http://127.0.0.1:4000/index.html', './');
    expect(source.url).toBe('http://127.0.0.1:4000/__q2-preview/events');
  });

  it('re-anchors a root-absolute reload target onto the prefix', () => {
    const { location, source } = runClient('https://host/p/abc123/posts/a.html', '../');
    source.emit('reload', { type: 'reload', target: '/other page.html' });
    expect(location.replace).toHaveBeenCalledWith('https://host/p/abc123/other%20page.html');
    expect(location.reload).not.toHaveBeenCalled();
  });

  it('reloads in place when the target is the page being shown', () => {
    const { location, source } = runClient('https://host/p/abc123/posts/', '../');
    source.emit('reload', { type: 'reload', target: '/posts/index.html' });
    expect(location.reload).toHaveBeenCalled();
    expect(location.replace).not.toHaveBeenCalled();
  });

  it('treats the pdf.js viewer as showing its file parameter', () => {
    const { location, source } = runClient(
      'https://host/p/abc123/__q2-preview/pdfjs/web/viewer.html?file=../../../my%20doc.pdf',
      '../../../',
    );
    expect(source.url).toBe('https://host/p/abc123/__q2-preview/events');
    source.emit('reload', { type: 'reload', target: '/my doc.pdf' });
    expect(location.reload).toHaveBeenCalled();
    expect(location.replace).not.toHaveBeenCalled();
  });
});
