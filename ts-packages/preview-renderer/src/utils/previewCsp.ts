/**
 * CSP injection for the sandboxed HTML preview iframes
 * (quarto-dev/q2#128, bd-sxx1az83).
 *
 * The preview iframe sandbox carries `allow-scripts` so that WebKit runs
 * parent-attached event listeners on the frame — WebKit bug 218086
 * (https://bugs.webkit.org/show_bug.cgi?id=218086) blocks them on
 * sandboxed frames without it, which broke link interception, scroll
 * sync, click-to-position, and selection sync in Safari. To preserve the
 * sandbox's no-scripts guarantee, every preview HTML payload gets the CSP
 * meta below injected: it blocks all script execution *inside* the
 * document (script elements, inline `on*` handlers, `javascript:` URLs)
 * while leaving `addEventListener`-registered listeners working.
 *
 * SECURITY: post-fix this meta is the *only* script mitigation in the
 * preview iframe — an injection miss is a same-origin script-execution
 * escape (`allow-scripts` + `allow-same-origin`). Every srcdoc/innerHTML
 * preview path must route its payload through `injectPreviewCsp`.
 */

/**
 * The injected policy. `script-src 'none'` blocks every script-
 * execution surface in the document. User content can only add *stricter*
 * CSPs (multiple CSPs intersect), never loosen this one.
 *
 * Exported separately from the meta tag so consumers that match the
 * parsed meta (the dev-mode tripwire in iframePostProcessor.ts) derive
 * from the same source instead of hardcoding a drift-prone second copy.
 */
export const PREVIEW_CSP_CONTENT = "script-src 'none'";

/** The injected meta tag, built from `PREVIEW_CSP_CONTENT`. */
export const PREVIEW_CSP_META = `<meta http-equiv="Content-Security-Policy" content="${PREVIEW_CSP_CONTENT}">`;

/**
 * Matches a leading DOCTYPE, including any whitespace/comments the HTML
 * parser's initial insertion mode would skip before it. Case-insensitive,
 * and anchored at the very start of the payload so a `<!doctype` string
 * inside user content can't move the injection point.
 *
 * The comment alternatives include the abrupt closings (`<!-->`,
 * `<!--->`) the HTML5 spec accepts. Anything this pattern doesn't
 * recognize simply falls back to byte-0 insertion, which is always safe
 * for payloads without a DOCTYPE.
 */
const LEADING_DOCTYPE =
  /^([\s\uFEFF]|<!--[\s\S]*?-->|<!--->|<!-->)*<!doctype[^>]*>/i;

/**
 * Return `html` with the preview CSP meta injected as the first element
 * in document order: immediately after the DOCTYPE if one is present,
 * otherwise at byte 0 (the parser places a leading `<meta>` in the
 * implied `<head>`). The operative invariant is meta-first — nothing may
 * precede it, so it always beats any `<script>` in the payload. Keeping
 * the DOCTYPE first is payload preservation, not Quirks Mode insurance:
 * the HTML spec forces no-quirks for iframe srcdoc documents no matter
 * what precedes the DOCTYPE; a leading `<meta>` would only break the
 * DOCTYPE if the same payload were served as a standalone document.
 *
 * This is deliberately NOT a "first child of <head>" string search:
 * head-like markup inside comments/titles/scripts/textareas, or uppercase
 * tags, would spoof the insertion point and let a `<script>` precede the
 * meta — which the CSP would then fail to block.
 *
 * Idempotent: a payload already carrying the meta *at the injection
 * point* is returned unchanged. A match anywhere else (e.g. quoted inside
 * user content) does NOT count — that would make the check a spoofable
 * fail-open hole.
 */
export function injectPreviewCsp(html: string): string {
  const match = LEADING_DOCTYPE.exec(html);
  const insertAt = match ? match[0].length : 0;
  if (html.startsWith(PREVIEW_CSP_META, insertAt)) return html;
  return html.slice(0, insertAt) + PREVIEW_CSP_META + html.slice(insertAt);
}
