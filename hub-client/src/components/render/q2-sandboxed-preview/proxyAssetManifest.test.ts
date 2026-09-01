import { describe, it, expect } from 'vitest';
import { buildProxyAssetManifest } from './proxyAssetManifest';

function astWithImages(...urls: string[]): string {
  return JSON.stringify({
    'pandoc-api-version': [1, 23, 1],
    meta: {},
    blocks: urls.map((u) => ({
      t: 'Para',
      c: [{ t: 'Image', c: [['', [], []], [], [u, '']] }],
    })),
  });
}

describe('buildProxyAssetManifest', () => {
  it('maps document-relative image paths to proxy URLs resolved against currentFilePath', () => {
    const manifest = buildProxyAssetManifest(
      astWithImages('images/pic.png'),
      '/project/sub/doc.qmd',
    );
    expect(manifest).toEqual({
      'images/pic.png': '__q2_vfs__/project/sub/images/pic.png',
    });
  });

  it('normalizes ../ segments (same resolution as the q2-preview asset walker)', () => {
    const manifest = buildProxyAssetManifest(
      astWithImages('../shared/pic.png'),
      '/project/sub/doc.qmd',
    );
    expect(manifest['../shared/pic.png']).toBe('__q2_vfs__/project/shared/pic.png');
  });

  it('passes root-absolute paths through unresolved (leading slash stripped)', () => {
    const manifest = buildProxyAssetManifest(
      astWithImages('/project/top.png'),
      '/project/sub/doc.qmd',
    );
    expect(manifest['/project/top.png']).toBe('__q2_vfs__/project/top.png');
  });

  it('skips external URLs', () => {
    const manifest = buildProxyAssetManifest(
      astWithImages('https://cdn.example/x.png', 'data:image/png;base64,AAAA', '//cdn.example/y.png', 'local.png'),
      '/project/doc.qmd',
    );
    expect(Object.keys(manifest)).toEqual(['local.png']);
  });

  it('returns an empty manifest for unparseable AST JSON', () => {
    expect(buildProxyAssetManifest('not json', '/project/doc.qmd')).toEqual({});
  });
});
