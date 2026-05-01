/**
 * Integration tests for `postProcessIframe`'s click-interception
 * behavior on website-rewritten cross-doc links (bd-lnd3).
 *
 * Runs in jsdom because the post-processor mutates an iframe's
 * Document. We mock `services/wasmRenderer` so the resource-
 * resolution passes (CSS / image data-URI rewrites) don't try to
 * touch a real VFS.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// Stub the WASM-backed VFS reads. The click-interception passes
// don't depend on these, but the resource-rewriting passes that
// run earlier in `postProcessIframe` do.
vi.mock('../services/wasmRenderer', () => ({
  vfsReadFile: vi.fn().mockReturnValue({ success: false }),
  vfsReadBinaryFile: vi.fn().mockReturnValue({ success: false }),
}));

import { postProcessIframe } from './iframePostProcessor';

const PROJECT_FILES: readonly string[] = [
  'index.qmd',
  'about.qmd',
  'posts/first.qmd',
];

/**
 * Build a hidden iframe with the given inner HTML (body) and
 * return both the iframe element and a click-event tracker. The
 * iframe is appended to `document.body` so jsdom wires up the
 * contentDocument; tests must remove it on teardown.
 */
function makeIframe(bodyHtml: string): {
  iframe: HTMLIFrameElement;
  clicks: Array<{ path?: string; anchor: string | null }>;
} {
  const iframe = document.createElement('iframe');
  // Use srcdoc-equivalent: write HTML into the contentDocument
  // synchronously so post-processing can run immediately.
  document.body.appendChild(iframe);
  iframe.contentDocument!.open();
  iframe.contentDocument!.write(`<!doctype html><html><body>${bodyHtml}</body></html>`);
  iframe.contentDocument!.close();
  return { iframe, clicks: [] };
}

function clickAnchor(iframe: HTMLIFrameElement, selector: string): boolean {
  const a = iframe.contentDocument!.querySelector(selector) as HTMLElement;
  expect(a).toBeTruthy();
  const evt = new MouseEvent('click', { bubbles: true, cancelable: true });
  return a.dispatchEvent(evt); // false if a handler called preventDefault
}

describe('postProcessIframe: cross-doc click interception (bd-lnd3)', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('intercepts artifact-rooted .html links that map to a known project file', () => {
    const { iframe, clicks } = makeIframe(
      `<a id="t" href="/.quarto/project-artifacts/about.html">About</a>`,
    );
    postProcessIframe(iframe, {
      currentFilePath: 'index.qmd',
      projectFilePaths: PROJECT_FILES,
      onQmdLinkClick: (arg) => {
        if ('path' in arg) clicks.push({ path: arg.path, anchor: arg.anchor });
        else clicks.push({ anchor: arg.anchor });
      },
    });

    const proceeded = clickAnchor(iframe, '#t');
    expect(proceeded).toBe(false); // preventDefault was called
    expect(clicks).toEqual([{ path: 'about.qmd', anchor: null }]);
    expect(iframe.contentDocument!.querySelector('#t')!.getAttribute('data-internal-link'))
      .toBe('true');
  });

  it('preserves anchors through the reverse-mapped click', () => {
    const { iframe, clicks } = makeIframe(
      `<a id="t" href="/.quarto/project-artifacts/about.html#intro">About §intro</a>`,
    );
    postProcessIframe(iframe, {
      currentFilePath: 'index.qmd',
      projectFilePaths: PROJECT_FILES,
      onQmdLinkClick: (arg) => {
        if ('path' in arg) clicks.push({ path: arg.path, anchor: arg.anchor });
      },
    });

    clickAnchor(iframe, '#t');
    expect(clicks).toEqual([{ path: 'about.qmd', anchor: 'intro' }]);
  });

  it('handles subdirectory artifact-rooted .html links', () => {
    const { iframe, clicks } = makeIframe(
      `<a id="t" href="/.quarto/project-artifacts/posts/first.html">first post</a>`,
    );
    postProcessIframe(iframe, {
      currentFilePath: 'index.qmd',
      projectFilePaths: PROJECT_FILES,
      onQmdLinkClick: (arg) => {
        if ('path' in arg) clicks.push({ path: arg.path, anchor: arg.anchor });
      },
    });

    clickAnchor(iframe, '#t');
    expect(clicks).toEqual([{ path: 'posts/first.qmd', anchor: null }]);
  });

  it('does NOT intercept artifact-rooted .html links with no matching source', () => {
    const { iframe, clicks } = makeIframe(
      `<a id="t" href="/.quarto/project-artifacts/missing.html">Missing</a>`,
    );
    postProcessIframe(iframe, {
      currentFilePath: 'index.qmd',
      projectFilePaths: PROJECT_FILES,
      onQmdLinkClick: () => {
        clicks.push({ anchor: null });
      },
    });

    const proceeded = clickAnchor(iframe, '#t');
    expect(proceeded).toBe(true); // not intercepted
    expect(clicks).toEqual([]);
    expect(iframe.contentDocument!.querySelector('#t')!.getAttribute('data-internal-link'))
      .toBeNull();
  });

  it('still intercepts source-shape .qmd links (no regression)', () => {
    const { iframe, clicks } = makeIframe(
      `<a id="t" href="./about.qmd">About</a>`,
    );
    postProcessIframe(iframe, {
      currentFilePath: 'index.qmd',
      projectFilePaths: PROJECT_FILES,
      onQmdLinkClick: (arg) => {
        if ('path' in arg) clicks.push({ path: arg.path, anchor: arg.anchor });
      },
    });

    const proceeded = clickAnchor(iframe, '#t');
    expect(proceeded).toBe(false);
    // The .qmd handler resolves relative paths against the
    // current file dir, yielding an absolute project path.
    expect(clicks.length).toBe(1);
    expect(clicks[0].path).toMatch(/about\.qmd$/);
  });

  it('does nothing when projectFilePaths is omitted (the artifact pass becomes a no-op)', () => {
    const { iframe, clicks } = makeIframe(
      `<a id="t" href="/.quarto/project-artifacts/about.html">About</a>`,
    );
    postProcessIframe(iframe, {
      currentFilePath: 'index.qmd',
      // no projectFilePaths
      onQmdLinkClick: () => {
        clicks.push({ anchor: null });
      },
    });

    const proceeded = clickAnchor(iframe, '#t');
    expect(proceeded).toBe(true);
    expect(clicks).toEqual([]);
  });

  it('does not collide with same-document anchor links', () => {
    const { iframe, clicks } = makeIframe(
      `<a id="t" href="#section">Section</a>`,
    );
    postProcessIframe(iframe, {
      currentFilePath: 'index.qmd',
      projectFilePaths: PROJECT_FILES,
      onQmdLinkClick: (arg) => {
        clicks.push({ anchor: 'anchor' in arg ? arg.anchor : null });
      },
    });

    clickAnchor(iframe, '#t');
    expect(clicks).toEqual([{ anchor: 'section' }]);
  });
});
