// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { BACK_BUTTON_ID, installBackButton } from './pdfBackButton';

function viewerDoc(): Document {
  const doc = document.implementation.createHTMLDocument('viewer');
  doc.body.innerHTML = '<div id="toolbarViewerLeft"><button id="sidebarToggle"></button></div>';
  return doc;
}

describe('installBackButton', () => {
  it('puts the button first in the toolbar and wires the click', () => {
    const doc = viewerDoc();
    const onBack = vi.fn();
    installBackButton(window, doc, onBack);
    const anchor = doc.getElementById('toolbarViewerLeft')!;
    expect(anchor.firstElementChild?.id).toBe(BACK_BUTTON_ID);
    (anchor.firstElementChild as HTMLButtonElement).click();
    expect(onBack).toHaveBeenCalledOnce();
  });

  it('replaces rather than duplicates on a second install, and removes it for null', () => {
    const doc = viewerDoc();
    installBackButton(window, doc, () => {});
    installBackButton(window, doc, () => {});
    expect(doc.querySelectorAll(`#${BACK_BUTTON_ID}`)).toHaveLength(1);
    installBackButton(window, doc, null);
    expect(doc.getElementById(BACK_BUTTON_ID)).toBeNull();
  });

  it('opens external links in a new tab', () => {
    const win = { PDFViewerApplication: { pdfLinkService: { externalLinkTarget: 0 } } } as unknown as Window;
    installBackButton(win, viewerDoc(), () => {});
    expect((win as any).PDFViewerApplication.pdfLinkService.externalLinkTarget).toBe(2);
  });

  it('fails loudly when pdf.js has no toolbar anchor', () => {
    const doc = document.implementation.createHTMLDocument('viewer');
    expect(() => installBackButton(window, doc, () => {})).toThrow(/toolbarViewerLeft/);
  });
});
