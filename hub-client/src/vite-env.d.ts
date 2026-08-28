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
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

declare const __GIT_COMMIT_HASH__: string
declare const __GIT_COMMIT_DATE__: string
declare const __BUILD_TIME__: string

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
