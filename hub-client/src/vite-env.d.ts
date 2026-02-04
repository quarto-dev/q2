/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Default Automerge sync server URL (set at build time) */
  readonly VITE_DEFAULT_SYNC_SERVER?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

declare const __GIT_COMMIT_HASH__: string
declare const __GIT_COMMIT_DATE__: string
declare const __BUILD_TIME__: string
