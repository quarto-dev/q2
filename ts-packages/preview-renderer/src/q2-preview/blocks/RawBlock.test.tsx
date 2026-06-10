// @vitest-environment jsdom
/**
 * RawBlock: verifies embedded-deck iframe src rewriting via the asset
 * manifest (bd-kjrpya2d), plus the plain raw-HTML / non-HTML paths.
 */
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';

import { RawBlock } from './RawBlock';
import { AssetManifestContext } from '../AssetManifestContext';

const DECK_SRC = '/examples/presentations/03-fragments/slides.html';
const EMBED_HTML = `<iframe class="embed-example-iframe" src="${DECK_SRC}" loading="lazy" allowfullscreen></iframe>`;

function node(format: string, content: string) {
  // RawBlock node shape: c = [format, content].
  return { t: 'RawBlock', c: [format, content] } as never;
}

function renderWith(manifest: Record<string, string>, fmt: string, html: string) {
  return render(
    <AssetManifestContext.Provider value={manifest}>
      <RawBlock node={node(fmt, html)} />
    </AssetManifestContext.Provider>,
  );
}

describe('RawBlock', () => {
  it('rewrites an embed-example-iframe src to its manifest blob URL', () => {
    const { container } = renderWith({ [DECK_SRC]: 'blob:deck-1' }, 'html', EMBED_HTML);
    const iframe = container.querySelector('iframe')!;
    expect(iframe.getAttribute('src')).toBe('blob:deck-1');
  });

  it('leaves the embed iframe src as-is on a manifest miss', () => {
    const { container } = renderWith({}, 'html', EMBED_HTML);
    const iframe = container.querySelector('iframe')!;
    expect(iframe.getAttribute('src')).toBe(DECK_SRC);
  });

  it('injects ordinary raw HTML untouched', () => {
    const { container } = renderWith({}, 'html', '<b id="x">hi</b>');
    expect(container.querySelector('#x')?.textContent).toBe('hi');
  });

  it('renders non-HTML raw blocks as <pre> source', () => {
    const { container } = renderWith({}, 'latex', '\\emph{x}');
    expect(container.querySelector('pre')?.textContent).toBe('\\emph{x}');
    expect(container.querySelector('iframe')).toBeNull();
  });
});
