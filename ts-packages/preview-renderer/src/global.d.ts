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

/**
 * Vite virtual module exposing the attribution viewer CSS as a string.
 * Resolved at build time by `attributionViewerCssPlugin` in
 * `hub-client/vite.config.ts`; shared with the CLI's
 * `AttributionViewerTransform` via `include_str!` so the two surfaces
 * stay in lockstep. See `resources/attribution/README.md`.
 *
 * Declared here (mirror of `hub-client/src/vite-env.d.ts`) so the
 * preview-renderer package's standalone `tsc --noEmit` typecheck sees
 * the module — the package is referenced from hub-client via project
 * references, but its own typecheck runs without Vite.
 */
declare module 'virtual:quarto-attribution-viewer-css' {
    const content: string;
    export default content;
}

/**
 * `import.meta.env` typing for the dev-mode tripwire in
 * `utils/iframePostProcessor.ts`. Declared locally for the same reason
 * as the module shims above: the package's standalone `tsc --noEmit`
 * runs without Vite. The shapes match `vite/client`'s own declarations
 * exactly, so the two merge cleanly when hub-client compiles this
 * package's sources with `vite/client` in scope.
 */
interface ImportMetaEnv {
    DEV: boolean;
}

interface ImportMeta {
    readonly env: ImportMetaEnv;
}
