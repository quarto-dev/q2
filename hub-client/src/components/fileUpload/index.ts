/**
 * Shared utilities for binary-asset upload flows.
 *
 * Used by NewAssetDialog and by any entry point that needs to derive a
 * destination folder from a drop event or current selection.
 */

export { validateProjectPath } from './validateProjectPath';
export {
  resolveDefaultDestination,
  FOLDER_PATH_ATTR,
  type ResolveDefaultDestinationOpts,
} from './resolveDefaultDestination';
export { processAssetFiles, type AssetFilePreview } from './processAssetFiles';
export { buildDropMarkdown, type DropMarkdownKind } from './dropMarkdown';
