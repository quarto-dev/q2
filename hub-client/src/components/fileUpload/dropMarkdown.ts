/**
 * Build the markdown inserted into the editor when a file is dropped on it
 * (external image upload or internal sidebar drag).
 *
 * Markdown link/image targets resolve relative to the *containing
 * document's* directory, while project file paths are project-root
 * relative — so the target must be relativized against the current file
 * (bd-jzqswvh0). `targetPath` should be the final created path (for
 * uploads, `CreateBinaryFileResult.path`, which can be hash-suffix renamed
 * on conflict), not the requested one.
 */

import { relativePathBetween } from '@quarto/preview-renderer/utils/vfsPaths';

export type DropMarkdownKind = 'image' | 'link';

export function buildDropMarkdown(
  kind: DropMarkdownKind,
  currentFilePath: string | null,
  targetPath: string
): string {
  const href = currentFilePath
    ? relativePathBetween(currentFilePath, targetPath)
    : targetPath;

  if (kind === 'image') {
    return `![](${href})`;
  }
  const fileName = targetPath.split('/').pop() || targetPath;
  return `[${fileName}](${href})`;
}
