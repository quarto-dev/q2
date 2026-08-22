/**
 * Integration tests for `MorphIframe`'s `selectionchange` → `onSelectionChange`
 * bridge (`claude-notes/plans/2026-08-22-click-align-editor-y.md`, Phase 2):
 *
 *  - `hostY` is computed from the anchor SPAN's own rect (not a containing
 *    block — the reported `SourceLocation` is already span/column
 *    precision, so a coarser block's top would desync from the very line
 *    being reported), plus the iframe's own host-page offset — mirroring
 *    `Q2PreviewIframe`'s `blockTop + iframeTop` pattern.
 *  - The `fileId` guard fix lives here too (its own commit): a selection
 *    resolving outside the current file (fileId !== 0 — e.g. inside
 *    `{{< include >}}`'d content) must not reach `onSelectionChange` at
 *    all, reusing `lineForClickTarget` rather than re-deriving the check.
 *
 * jsdom never processes an `<iframe>`'s `srcdoc` navigation — verified: no
 * `load` event fires and `contentDocument` stays the default empty document
 * no matter how long you wait. `Q2PreviewIframe`'s own tests hit the same
 * wall for its `src`-bearing iframe and work around it by writing a fixture
 * directly into `contentDocument` via `open()/write()/close()` and firing
 * its readiness signal synthetically; that file's signal is a `postMessage`
 * (`IFRAME_READY`), while `MorphIframe`'s is the iframe element's own native
 * `load` event — so the same idiom applies, just dispatching `load` instead.
 */

import { describe, test, expect, vi } from 'vitest';
import { render, act } from '@testing-library/react';

// MorphIframe's selectionchange handler runs `postProcessIframe` on load,
// which reads through @quarto/preview-runtime for link/asset rewriting.
// None of these fixtures contain rewritable links or assets, but stub the
// VFS reads anyway (mirrors Q2PreviewIframe.integration.test.tsx) so a
// future fixture addition here can't accidentally depend on real WASM.
vi.mock('@quarto/preview-runtime', () => ({
  vfsReadFile: vi.fn(() => ({ success: false })),
  vfsReadBinaryFile: vi.fn(() => ({ success: false })),
}));

import { createRef } from 'react';
import MorphIframe, { type MorphIframeHandle } from './MorphIframe';
import type { SourceLocation } from './scrollSyncDom';

const BASE_QMD = 'Paragraph one.\n\nParagraph two.\n';

/**
 * Mount the real `MorphIframe`, then write `fixtureBodyHtml` directly into
 * its `contentDocument` and fire a synthetic `load` Event on the `<iframe>`
 * element — MorphIframe's own readiness signal, which jsdom never fires on
 * its own for `srcdoc` (see file doc comment). This runs the component's
 * real `handleLoad` (post-process + `setDocumentReady(true)`), attaching
 * the real `selectionchange` listener to OUR fixture document.
 */
function mountWithFixture(
  onSelectionChange: (
    startPos: SourceLocation | null,
    endPos: SourceLocation | null,
    hostY?: number,
  ) => void,
  fixtureBodyHtml: string,
  qmdContent: string = BASE_QMD,
) {
  const utils = render(
    <MorphIframe
      ref={createRef<MorphIframeHandle>()}
      html="<!doctype html><html><body></body></html>"
      currentFilePath="doc.qmd"
      qmdContent={qmdContent}
      onNavigateToDocument={() => {}}
      onSelectionChange={onSelectionChange}
    />,
  );

  const iframe = utils.container.querySelector('iframe');
  if (!iframe) throw new Error('iframe not mounted');
  const doc = iframe.contentDocument;
  if (!doc) throw new Error('contentDocument is null');

  doc.open();
  doc.write(`<!doctype html><html><body>${fixtureBodyHtml}</body></html>`);
  doc.close();

  act(() => {
    iframe.dispatchEvent(new Event('load'));
  });

  return { iframe, doc, unmount: utils.unmount };
}

function selectWithinSpan(doc: Document, span: Element, offset: number) {
  const textNode = span.firstChild;
  if (!textNode) throw new Error('span has no text-node child');
  const range = doc.createRange();
  range.setStart(textNode, offset);
  range.setEnd(textNode, offset);
  const selection = doc.getSelection();
  if (!selection) throw new Error('doc.getSelection() is null');
  selection.removeAllRanges();
  selection.addRange(range);
  doc.dispatchEvent(new Event('selectionchange'));
}

describe('MorphIframe selectionchange -> onSelectionChange: hostY (Phase 2)', () => {
  test('reports hostY from the anchor SPAN itself, not the containing block', () => {
    const onSelectionChange = vi.fn();
    const { iframe, doc } = mountWithFixture(
      onSelectionChange,
      '<p data-loc="0:1:1-1:15">' +
        '<span data-loc="0:1:1-1:9">Paragraph</span> ' +
        '<span data-loc="0:1:11-1:15">one.</span>' +
        '</p>',
    );

    const span = doc.querySelector('span[data-loc="0:1:1-1:9"]')!;
    const block = doc.querySelector('p[data-loc]')!;
    // Distinct sentinel values: if hostY were computed from the containing
    // <p> instead of the span, or from jsdom's un-mocked zero default, this
    // row would catch either mistake.
    vi.spyOn(span, 'getBoundingClientRect').mockReturnValue({ top: 234 } as DOMRect);
    vi.spyOn(block, 'getBoundingClientRect').mockReturnValue({ top: 999 } as DOMRect);
    vi.spyOn(iframe, 'getBoundingClientRect').mockReturnValue({ top: 50 } as DOMRect);

    act(() => selectWithinSpan(doc, span, 0));

    expect(onSelectionChange).toHaveBeenCalledTimes(1);
    const [, , hostY] = onSelectionChange.mock.calls[0];
    expect(hostY).toBe(284); // span's 234 + iframe's 50 — never 999 + 50
  });
});

/**
 * Fix, own commit (decision A8): before this, `handlePreviewSelection`
 * built a Monaco range straight from `startPos`/`endPos` and never checked
 * `fileId`, so selecting inside e.g. `{{< include >}}`'d content (a
 * non-zero `fileId`) moved the editor's caret + focus to a same-numbered
 * but unrelated line of the currently-open file. Reuses `lineForClickTarget`
 * (already used for `hostY` above) as the guard, rather than re-deriving
 * the check — it must now gate the WHOLE call, not just `hostY`, which is
 * the deliberate difference from the row above: this is the fix, not the
 * alignment feature, so `setSelection`/`focus()` are correctly skipped
 * along with the reveal for this one case.
 */
describe('MorphIframe selectionchange -> onSelectionChange: fileId guard (fix, own commit)', () => {
  test('a foreign fileId (e.g. included content) is not reported at all', () => {
    const onSelectionChange = vi.fn();
    const { doc } = mountWithFixture(
      onSelectionChange,
      '<p data-loc="2:1:1-1:9"><span data-loc="2:1:1-1:9">Included</span></p>',
    );

    const span = doc.querySelector('span[data-loc]')!;
    act(() => selectWithinSpan(doc, span, 0));

    expect(onSelectionChange).not.toHaveBeenCalled();
  });

  test('control: the same shape with fileId 0 (the current file) is still reported', () => {
    const onSelectionChange = vi.fn();
    const { doc } = mountWithFixture(
      onSelectionChange,
      '<p data-loc="0:1:1-1:9"><span data-loc="0:1:1-1:9">Paragraph</span></p>',
    );

    const span = doc.querySelector('span[data-loc]')!;
    act(() => selectWithinSpan(doc, span, 0));

    expect(onSelectionChange).toHaveBeenCalledTimes(1);
  });
});
