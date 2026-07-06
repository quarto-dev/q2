/**
 * Turn a standalone-but-not-self-contained render into a single,
 * fully self-contained HTML document that renders correctly when
 * opened as a bare top-level browser tab (issue #315, bd-vhdknrvl).
 *
 * The WASM HTML pipeline (`render_qmd_to_html` → `ApplyTemplateStage`,
 * and the reveal.js `assemble` path for slides) emits a complete
 * `<!DOCTYPE html>` document, but its generated assets are referenced
 * externally:
 *
 *   - theme CSS / bootstrap JS / reveal.js+css / fonts as
 *     `<link href="/.quarto/project-artifacts/…">` / `<script src=…>`,
 *     which only resolve inside the hub-client app's iframe (served
 *     out of the WASM VFS by the post-processor / service worker), and
 *   - user images as their **original relative `src`** (e.g.
 *     `figures/plot.png`), never rewritten — resolved against the
 *     document's directory in the VFS.
 *
 * Opened as a top-level tab (which is how we get correct pagination
 * and `@media print` behaviour — see the plan for issue #315), none of
 * those resolve. This function reads each asset's bytes from the VFS
 * and inlines them: stylesheets → `<style>`, scripts → executing inline
 * `<script>` (reveal.js must run), images/fonts → `data:` URIs.
 *
 * VFS access is injected (see {@link SelfContainedReaders}) so this
 * module is decoupled from the WASM singleton and trivially testable.
 * The production binding lives in {@link selfContainedReadersFromVfs}.
 */

import { resolveRelativePath, guessMimeType } from './vfsPaths';

/**
 * VFS accessors, injected so the transform is pure with respect to
 * the WASM runtime. `readText` returns UTF-8 text (CSS, JS);
 * `readBinaryBase64` returns base64-encoded bytes (images, fonts).
 * Both return `null` when the VFS has no entry for the path.
 */
export interface SelfContainedReaders {
  readText(vfsPath: string): string | null;
  readBinaryBase64(vfsPath: string): string | null;
}

/**
 * True for references we must not touch: absolute URLs (http/https or
 * protocol-relative), `data:` URIs (already inlined), pure fragments,
 * and non-resource schemes.
 */
function isExternalRef(url: string): boolean {
  return (
    /^[a-z][a-z0-9+.-]*:/i.test(url) || // any scheme: http:, https:, data:, mailto:, …
    url.startsWith('//') || // protocol-relative
    url.startsWith('#') // same-document fragment
  );
}

/**
 * Map a document-level asset reference to the VFS key its bytes live
 * under. Mirrors the resolution the live iframe post-processor uses
 * (`iframePostProcessor.ts`):
 *   - `/.quarto/…` artifact paths are absolute VFS keys, used as-is.
 *   - `libs/…` paths are emitted project-relative and read as-is.
 *   - everything else is resolved against the current document's
 *     directory, then the leading slash stripped (VFS keys have no
 *     leading slash, e.g. `project/sub/figures/plot.png`).
 */
function toVfsKey(url: string, currentFilePath: string): string {
  if (url.startsWith('/.quarto/') || url.startsWith('libs/')) {
    return url;
  }
  const resolved = resolveRelativePath(currentFilePath, url);
  // `/.quarto/…` artifact keys keep their leading slash (that is how
  // the VFS stores/serves them); user-file keys are stored without a
  // leading slash (e.g. `project/sub/figures/plot.png`). This matters
  // when a relative `url(...)` inside an artifact stylesheet resolves
  // back into the `/.quarto/…` namespace.
  if (resolved.startsWith('/.quarto/')) return resolved;
  return resolved.startsWith('/') ? resolved.slice(1) : resolved;
}

/**
 * Rewrite `url(...)` references inside a stylesheet's text to `data:`
 * URIs. Font/image URLs in CSS are relative to the **stylesheet's**
 * location, so they resolve against `cssVfsKey`'s directory (passed as
 * a file path to {@link resolveRelativePath}). Best-effort: any URL we
 * can't resolve (external, or absent from the VFS) is left verbatim.
 */
function inlineCssUrls(
  css: string,
  cssVfsKey: string,
  readers: SelfContainedReaders,
): string {
  return css.replace(
    /url\(\s*(['"]?)([^'")]+)\1\s*\)/g,
    (whole, _quote: string, ref: string) => {
      const url = ref.trim();
      if (isExternalRef(url)) return whole;
      const key = toVfsKey(url, cssVfsKey);
      const b64 = readers.readBinaryBase64(key);
      if (b64 == null) return whole;
      return `url(data:${guessMimeType(url)};base64,${b64})`;
    },
  );
}

/**
 * Produce a self-contained HTML string. Pure (aside from the injected
 * readers); safe to call on the main thread. Requires a DOM
 * implementation (`DOMParser`) — available in browsers and jsdom.
 */
export function makeSelfContainedHtml(
  html: string,
  currentFilePath: string,
  readers: SelfContainedReaders,
): string {
  const doc = new DOMParser().parseFromString(html, 'text/html');

  // Stylesheets → inline <style> (with url() refs resolved to data URIs).
  doc.querySelectorAll('link[rel~="stylesheet"]').forEach((link) => {
    const href = link.getAttribute('href');
    if (!href || isExternalRef(href)) return;
    const key = toVfsKey(href, currentFilePath);
    const css = readers.readText(key);
    if (css == null) return;
    const style = doc.createElement('style');
    style.textContent = inlineCssUrls(css, key, readers);
    const media = link.getAttribute('media');
    if (media) style.setAttribute('media', media);
    link.replaceWith(style);
  });

  // Scripts → executing inline <script>. Reveal.js and code-copy JS
  // must actually run in the printable tab, unlike the sandboxed
  // preview iframe where script inlining is deliberately disabled.
  doc.querySelectorAll('script[src]').forEach((script) => {
    const src = script.getAttribute('src');
    if (!src || isExternalRef(src)) return;
    const key = toVfsKey(src, currentFilePath);
    const js = readers.readText(key);
    if (js == null) return;
    const inline = doc.createElement('script');
    const type = script.getAttribute('type');
    if (type) inline.setAttribute('type', type);
    inline.textContent = js;
    script.replaceWith(inline);
  });

  // Images → data: URIs (both /.quarto artifacts and user images).
  doc.querySelectorAll('img[src]').forEach((img) => {
    const src = img.getAttribute('src');
    if (!src || isExternalRef(src)) return;
    const key = toVfsKey(src, currentFilePath);
    const b64 = readers.readBinaryBase64(key);
    if (b64 == null) return;
    img.setAttribute('src', `data:${guessMimeType(src)};base64,${b64}`);
  });

  return '<!DOCTYPE html>\n' + doc.documentElement.outerHTML;
}
