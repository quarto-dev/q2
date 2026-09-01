/**
 * Tests for the shared asset-proxy policy module that lives in the
 * quarto-hub-sandboxed-preview project (single source of truth for the
 * service worker, the iframe bridge, and this parent — the policy skew
 * the old TODO pair warned about).
 */
import { describe, it, expect } from 'vitest';
import {
  VFS_PROXY_SEGMENT,
  proxyUrlForVfsPath,
  vfsPathForRequestUrl,
  isBinaryPath,
  mimeTypeFor,
  rewriteThemeCssUrls,
} from '../../../../quarto-hub-sandboxed-preview/src/assetPolicy';

describe('proxyUrlForVfsPath / vfsPathForRequestUrl', () => {
  it('round-trips a resolved VFS path through a page-relative proxy URL', () => {
    const url = proxyUrlForVfsPath('project/sub/pic.png');
    // Page-relative (no leading slash) so it stays inside the service
    // worker's scope under a project-path deployment like /q2/.
    expect(url).toBe(`${VFS_PROXY_SEGMENT}/project/sub/pic.png`);
    expect(vfsPathForRequestUrl(`https://quarto-dev.github.io/q2/${url}`)).toBe(
      'project/sub/pic.png',
    );
  });

  it('accepts a leading slash on the VFS path and strips it', () => {
    expect(proxyUrlForVfsPath('/project/pic.png')).toBe(
      `${VFS_PROXY_SEGMENT}/project/pic.png`,
    );
  });

  it('keeps same-named files in different directories distinct (basename-collision regression)', () => {
    const a = proxyUrlForVfsPath('project/a/pic.png');
    const b = proxyUrlForVfsPath('project/b/pic.png');
    expect(a).not.toBe(b);
    expect(vfsPathForRequestUrl(`http://127.0.0.1:8081/${a}`)).toBe('project/a/pic.png');
    expect(vfsPathForRequestUrl(`http://127.0.0.1:8081/${b}`)).toBe('project/b/pic.png');
  });

  it('round-trips paths with spaces', () => {
    const url = proxyUrlForVfsPath('project/my images/shot 1.png');
    expect(vfsPathForRequestUrl(`https://example.test/${url}`)).toBe(
      'project/my images/shot 1.png',
    );
  });

  it('returns null for URLs outside the proxy namespace', () => {
    expect(vfsPathForRequestUrl('https://quarto-dev.github.io/q2/assets/index-abc.js')).toBeNull();
    expect(vfsPathForRequestUrl('https://quarto-dev.github.io/q2/')).toBeNull();
    expect(vfsPathForRequestUrl('https://quarto-dev.github.io/q2/serviceWorker.js')).toBeNull();
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

  it('rewrites relative url() refs into the proxy namespace against the CSS dir', () => {
    const css = '@font-face { src: url(fonts/inter.woff2) format("woff2"); }';
    expect(rewriteThemeCssUrls(css, dir)).toBe(
      `@font-face { src: url(${VFS_PROXY_SEGMENT}/.quarto/project-artifacts/fonts/inter.woff2) format("woff2"); }`,
    );
  });

  it('preserves quotes and handles ../ segments', () => {
    const css = "body { background: url('../bg.png'); }";
    expect(rewriteThemeCssUrls(css, dir)).toBe(
      `body { background: url('${VFS_PROXY_SEGMENT}/.quarto/bg.png'); }`,
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
    expect(rewriteThemeCssUrls(css, dir)).toBe(css);
  });
});
