/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Default Automerge sync server URL (set at build time) */
  readonly VITE_DEFAULT_SYNC_SERVER?: string
  /** Google OAuth2 client ID. When set, enables authentication. */
  readonly VITE_GOOGLE_CLIENT_ID?: string
  /** Base path the hub is reverse-proxied at. */
  readonly VITE_HUB_BASE_PATH?: string
  /**
   * Set to '1' for E2E test builds: exposes window.__quartoTest and
   * admits #/dev harness routes. Never set for deployed builds.
   */
  readonly VITE_E2E?: string
  /** Set to '1' by the `q2 preview` embed build: "Download as" uses the native render. */
  readonly VITE_PREVIEW_EMBED?: string
  /** Set to '0' to build without the pandoc.wasm "Download as" path (the preview embed does). */
  readonly VITE_PANDOC_WASM?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

declare const __GIT_COMMIT_HASH__: string
declare const __GIT_COMMIT_DATE__: string
declare const __BUILD_TIME__: string
/** True when the build ships `public/pandoc/pandoc.wasm.gz` and the feature is not killed (`src/pandoc/buildFlag.ts`). */
declare const __PANDOC_WASM_ENABLED__: boolean

/**
 * Default export = the contents of `resources/attribution/viewer.css`,
 * embedded at build time by `attributionViewerCssPlugin` in
 * `vite.config.ts`. Shared with the CLI's
 * `AttributionViewerTransform` via `include_str!`.
 */
declare module 'virtual:quarto-attribution-viewer-css' {
  const content: string
  export default content
}
