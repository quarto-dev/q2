// Whether this build ships the pandoc.wasm asset and offers "Download as" through it.
// `__PANDOC_WASM_ENABLED__` is defined by vite.config.ts from `resolvePandocFlag`; it is
// undefined outside a Vite transform, which counts as off.
export function pandocWasmEnabled(): boolean {
  return typeof __PANDOC_WASM_ENABLED__ !== 'undefined' && __PANDOC_WASM_ENABLED__;
}

/**
 * True in the `q2 preview` embed build (`VITE_PREVIEW_EMBED=1`, set only by
 * `build:preview-embed`). There "Download as" goes to the native render
 * (`nativeRender.ts`), not to pandoc.wasm. Named separately from
 * `VITE_EPHEMERAL_STORAGE` (storage behaviour) and `VITE_PANDOC_WASM` (the kill switch).
 */
export function isPreviewEmbed(): boolean {
  return import.meta.env.VITE_PREVIEW_EMBED === '1';
}

/**
 * The warm preview path (H10b): persistent pandoc workers, overlapping renders and the preview's own typst
 * runner. On unless the build sets `VITE_PANDOC_WARM=0`, which restores the fresh path (one worker per render,
 * the app-wide typst runner, the 500 ms debounce). Read once per pane mount, so a test can pass the value.
 */
export function pandocWarmEnabled(value: string | undefined = import.meta.env.VITE_PANDOC_WARM): boolean {
  return value !== '0';
}
