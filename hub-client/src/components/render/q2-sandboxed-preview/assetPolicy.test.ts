/**
 * Tests for the shared asset-proxy policy module that lives in the
 * quarto-hub-sandboxed-preview project (single source of truth for the
 * service worker, the iframe bridge, and this parent).
 *
 * bd-00bgt5cy: any relative path is proxied from the VFS — the service
 * worker intercepts every same-origin GET under its scope EXCEPT the
 * frame's own app files (the page, serviceWorker.js, q2-preview-assets/*). Image
 * URLs are bare resolved paths (no __q2_vfs__ prefix).
 */
import { describe, it, expect } from 'vitest';
import {
  pageRelativeUrlForVfsPath,
  vfsPathForRequestUrl,
  isBinaryPath,
  mimeTypeFor,
  rewriteThemeCssUrls,
} from '../../../../quarto-hub-sandboxed-preview/src/assetPolicy';

const PAGES_SCOPE = 'https://quarto-dev.github.io/q2/';
const LOCAL_SCOPE = 'http://127.0.0.1:8081/';

describe('pageRelativeUrlForVfsPath / vfsPathForRequestUrl', () => {
  it('round-trips a resolved VFS path as a bare page-relative URL', () => {
    const url = pageRelativeUrlForVfsPath('project/sub/pic.png');
    expect(url).toBe('project/sub/pic.png');
    expect(vfsPathForRequestUrl(`${PAGES_SCOPE}${url}`, PAGES_SCOPE)).toBe(
      'project/sub/pic.png',
    );
  });

  it('accepts a leading slash on the VFS path and strips it', () => {
    expect(pageRelativeUrlForVfsPath('/project/pic.png')).toBe('project/pic.png');
  });

  it('keeps same-named files in different directories distinct (basename-collision regression)', () => {
    const a = pageRelativeUrlForVfsPath('project/a/pic.png');
    const b = pageRelativeUrlForVfsPath('project/b/pic.png');
    expect(a).not.toBe(b);
    expect(vfsPathForRequestUrl(`${LOCAL_SCOPE}${a}`, LOCAL_SCOPE)).toBe('project/a/pic.png');
    expect(vfsPathForRequestUrl(`${LOCAL_SCOPE}${b}`, LOCAL_SCOPE)).toBe('project/b/pic.png');
  });

  it('round-trips paths with spaces', () => {
    const url = pageRelativeUrlForVfsPath('project/my images/shot 1.png');
    expect(vfsPathForRequestUrl(`https://example.test/${url}`, 'https://example.test/')).toBe(
      'project/my images/shot 1.png',
    );
  });

  it('exempts the frame app files: the page, serviceWorker.js, and q2-preview-assets/*', () => {
    expect(vfsPathForRequestUrl(`${PAGES_SCOPE}`, PAGES_SCOPE)).toBeNull();
    expect(vfsPathForRequestUrl(`${PAGES_SCOPE}index.html`, PAGES_SCOPE)).toBeNull();
    expect(vfsPathForRequestUrl(`${PAGES_SCOPE}serviceWorker.js`, PAGES_SCOPE)).toBeNull();
    expect(vfsPathForRequestUrl(`${PAGES_SCOPE}q2-preview-assets/index-abc.js`, PAGES_SCOPE)).toBeNull();
    expect(
      vfsPathForRequestUrl(`${PAGES_SCOPE}q2-preview-assets/KaTeX_Main-Regular.woff2`, PAGES_SCOPE),
    ).toBeNull();
  });

  it("proxies a project's own assets/ directory (shadowing fix: the app dir is q2-preview-assets)", () => {
    expect(vfsPathForRequestUrl(`${PAGES_SCOPE}assets/logo.png`, PAGES_SCOPE)).toBe('assets/logo.png');
  });

  it('proxies any other relative path — including ones no manifest produced', () => {
    // e.g. an <img> inside raw HTML or navbar chrome, never seen by the
    // asset walker.
    expect(vfsPathForRequestUrl(`${PAGES_SCOPE}dog_room.png`, PAGES_SCOPE)).toBe('dog_room.png');
    expect(vfsPathForRequestUrl(`${LOCAL_SCOPE}sub/images/pic.png`, LOCAL_SCOPE)).toBe(
      'sub/images/pic.png',
    );
  });

  it('returns null for URLs outside the scope', () => {
    expect(vfsPathForRequestUrl('https://other.example/pic.png', PAGES_SCOPE)).toBeNull();
    expect(vfsPathForRequestUrl('https://quarto-dev.github.io/other/pic.png', PAGES_SCOPE)).toBeNull();
  });

  it('works under a nested same-origin scope (public/ fallback copy)', () => {
    const scope = 'http://localhost:5174/q2-sandboxed-preview/';
    expect(vfsPathForRequestUrl(`${scope}images/dot.png`, scope)).toBe('images/dot.png');
    expect(vfsPathForRequestUrl(`${scope}q2-preview-assets/chunk.js`, scope)).toBeNull();
  });
});

describe('isBinaryPath', () => {
  it('classifies images and fonts as binary', () => {
    for (const p of ['a.png', 'b.JPG', 'c.jpeg', 'd.gif', 'e.webp', 'f.woff2', 'g.ttf', 'h.ico', 'i.pdf']) {
      expect(isBinaryPath(p), p).toBe(true);
    }
  });
  it('classifies text formats as text', () => {
    for (const p of ['a.css', 'b.svg', 'c.js', 'd.json', 'e.html', 'f.txt']) {
      expect(isBinaryPath(p), p).toBe(false);
    }
  });
});

describe('mimeTypeFor', () => {
  it('maps common extensions', () => {
    expect(mimeTypeFor('pic.png')).toBe('image/png');
    expect(mimeTypeFor('style.css')).toBe('text/css');
    expect(mimeTypeFor('font.woff2')).toBe('font/woff2');
    expect(mimeTypeFor('vector.svg')).toBe('image/svg+xml');
  });
  it('falls back to octet-stream', () => {
    expect(mimeTypeFor('mystery.xyz')).toBe('application/octet-stream');
  });
});

describe('rewriteThemeCssUrls', () => {
  const dir = '.quarto/project-artifacts';
  const base = 'https://quarto-dev.github.io/q2/';

  it('rewrites relative url() refs to absolute page URLs resolved against the CSS dir', () => {
    // Absolute because the theme is applied through a blob: stylesheet,
    // whose base URL cannot anchor relative refs.
    const css = '@font-face { src: url(fonts/inter.woff2) format("woff2"); }';
    expect(rewriteThemeCssUrls(css, dir, base)).toBe(
      `@font-face { src: url(${base}.quarto/project-artifacts/fonts/inter.woff2) format("woff2"); }`,
    );
  });

  it('preserves quotes and handles ../ segments', () => {
    const css = "body { background: url('../bg.png'); }";
    expect(rewriteThemeCssUrls(css, dir, base)).toBe(
      `body { background: url('${base}.quarto/bg.png'); }`,
    );
  });

  it('leaves absolute, data:, blob:, and fragment refs alone', () => {
    const css = [
      'a { background: url(https://cdn.example/x.png); }',
      'b { background: url(//cdn.example/y.png); }',
      'c { background: url(data:image/png;base64,AAAA); }',
      'd { background: url(blob:https://x/z); }',
      'e { filter: url(#f); }',
    ].join('\n');
    expect(rewriteThemeCssUrls(css, dir, base)).toBe(css);
  });
});
