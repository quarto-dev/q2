/**
 * quarto-synced-file: one qmd file synced through a Quarto hub, with its
 * parsed AST and the splice-based edit API hub-client's editor uses.
 * No UI; a candidate to become its own package.
 */

export type { EditorContentChange } from '@quarto/quarto-sync-client'
export {
  createSyncedFile,
  idleSnapshot,
  type SyncedFile,
  type SyncedFileParams,
  type SyncedFileSnapshot,
} from './syncedFile.ts'
export { createLocalFile } from './localFile.ts'
export { useSyncedFile, type SyncedFileState } from './useSyncedFile.ts'
export { diffToChange } from './diff.ts'
export {
  initWasm,
  parseQmdContent,
  writeQmdFromAst,
  incrementalWriteQmd,
  deriveTitleBlockMeta,
  compileDefaultThemeCss,
  type TitleBlockMeta,
} from './wasm.ts'
