/**
 * Back button injected into the stock pdf.js toolbar (P11-D3). The viewer's CSP
 * (`script-src 'self'; style-src 'self'`) blocks inline `style` attributes set from the
 * parent and inline `<style>` elements, so the button is styled through CSSOM (`style.cssText`)
 * only.
 */

/** The one place pdf.js's toolbar is anchored; a pdf.js bump that drops it must fail loudly. */
const ANCHOR_ID = 'toolbarViewerLeft';
export const BACK_BUTTON_ID = 'q2-preview-back';

/** pdf.js `LinkTarget.BLANK`: external links in a PDF must not navigate the iframe away. */
const LINK_TARGET_BLANK = 2;

interface LinkServiceWindow extends Window {
  PDFViewerApplication?: { pdfLinkService?: { externalLinkTarget: number } };
}

export function installBackButton(win: Window, doc: Document, onBack: (() => void) | null): void {
  const anchor = doc.getElementById(ANCHOR_ID);
  if (!anchor) {
    throw new Error(`pdf.js viewer has no #${ANCHOR_ID}: the Back button cannot be placed (pdf.js changed?)`);
  }
  const linkService = (win as LinkServiceWindow).PDFViewerApplication?.pdfLinkService;
  if (linkService) linkService.externalLinkTarget = LINK_TARGET_BLANK;
  doc.getElementById(BACK_BUTTON_ID)?.remove();
  if (!onBack) return;
  const button = doc.createElement('button');
  button.id = BACK_BUTTON_ID;
  button.type = 'button';
  button.textContent = '← Back';
  button.title = 'Back';
  // CSSOM only (see the header). `inherit`/`currentColor` keep it legible in the viewer's light and dark themes.
  button.style.cssText =
    'font:inherit;color:inherit;background:transparent;border:1px solid currentColor;border-radius:4px;' +
    'padding:2px 8px;margin:0 6px;cursor:pointer;white-space:nowrap';
  button.addEventListener('click', onBack);
  anchor.insertBefore(button, anchor.firstChild);
}
