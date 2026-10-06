/**
 * Helpers shared by FileTreeRow and the trees that render it. Kept out of
 * FileTreeRow.tsx so that module exports only a component (React Fast
 * Refresh requirement).
 */

import type { ReactNode } from 'react';
import { isImageExtension } from '@quarto/preview-renderer/types/project';
import { FileTextIcon, ImageFileIcon, QmdFileIcon, GearIcon } from './icons';

/** Left padding for a row at `depth` (root children are depth 0). */
export function treeRowIndent(depth: number, base: number = 12): number {
  return base + depth * 16;
}

/** File icon + per-type tint, wrapped in the .file-icon span. */
export function getFileIcon(path: string): ReactNode {
  const ext = path.split('.').pop()?.toLowerCase() || '';

  if (isImageExtension(path)) {
    return (
      <span className="file-icon file-icon--image">
        <ImageFileIcon size={16} />
      </span>
    );
  }
  if (['qmd', 'md'].includes(ext)) {
    return (
      <span className="file-icon file-icon--qmd">
        <QmdFileIcon size={16} />
      </span>
    );
  }
  if (['yml', 'yaml', 'json'].includes(ext)) {
    return (
      <span className="file-icon file-icon--config">
        <GearIcon size={16} />
      </span>
    );
  }
  return (
    <span className="file-icon">
      <FileTextIcon size={16} />
    </span>
  );
}
