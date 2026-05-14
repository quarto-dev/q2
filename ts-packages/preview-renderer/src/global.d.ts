/**
 * Module shims for Vite's special import suffixes used by the
 * q2-preview iframe entry. The preview-renderer package isn't always
 * compiled under Vite (`tsc --noEmit` runs standalone for the
 * library's typecheck script), so we declare these locally rather
 * than rely on `/// <reference types="vite/client" />`.
 *
 * `?raw` returns the file's contents as a string at build time. Used
 * by `q2-preview/entry.tsx` to inline-inject Bootstrap's bundled JS
 * into the sandboxed iframe (Phase F.1, bd-kw93.14).
 */

declare module '*?raw' {
    const content: string;
    export default content;
}
