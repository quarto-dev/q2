/**
 * FileTreeRow — one row of a file tree: a file (type icon + name) or a
 * folder (chevron + folder icon + name), indented by depth.
 *
 * Purely presentational. The sidebar's tree and the New-file dialog's
 * folder picker both render their rows through this component so the two
 * trees look identical; each owns its own interaction (roving tabindex,
 * drag/drop, menu semantics) and passes it in as ordinary div attributes.
 */

import type { HTMLAttributes, ReactNode } from 'react';
import { isBinaryExtension, isImageExtension } from '@quarto/preview-renderer/types/project';
import { FolderIcon } from './icons';
import { getFileIcon, treeRowIndent } from './fileTreeRowHelpers';
import './FileTreeRow.css';

export interface FileTreeRowProps extends HTMLAttributes<HTMLDivElement> {
  type: 'file' | 'folder';
  /** Display name (last path segment). */
  name: string;
  /** Full path; for files it selects the icon. */
  path: string;
  depth: number;
  /** Folders only: chevron direction. */
  expanded?: boolean;
  /**
   * Folders only. `toggle` (default) draws the chevron; `spacer` reserves
   * its width but draws nothing (leaf folders, so icons stay aligned with
   * expandable siblings); `none` omits it entirely.
   */
  chevron?: 'toggle' | 'spacer' | 'none';
  /** Left padding of a depth-0 row, in px (default 12). */
  indentBase?: number;
  /** Selected row (files: the open document; folders: the picked folder). */
  active?: boolean;
  /**
   * Folders only: when given, the chevron becomes its own click target
   * that toggles without activating the row (the row's own onClick does
   * not fire).
   */
  onToggle?: () => void;
  /** Replaces the name span (e.g. an inline rename input). Falsy = show the name. */
  nameSlot?: ReactNode;
  /** Trailing content after the name (e.g. a kebab button). */
  trailing?: ReactNode;
}

export default function FileTreeRow({
  type,
  name,
  path,
  depth,
  expanded = false,
  chevron = 'toggle',
  indentBase = 12,
  active = false,
  onToggle,
  nameSlot,
  trailing,
  className = '',
  style,
  children,
  ...rest
}: FileTreeRowProps) {
  // Binary files that the app cannot show (images can) are dimmed.
  const isBinary = type === 'file' && isBinaryExtension(path) && !isImageExtension(path);
  const classes = [
    type === 'folder' ? 'folder-header' : 'file-item qh-row-hover',
    active ? 'active' : '',
    isBinary ? 'binary' : '',
    className,
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <div
      className={classes}
      style={{ paddingLeft: `${treeRowIndent(depth, indentBase)}px`, ...style }}
      {...rest}
    >
      {type === 'folder' ? (
        <>
          {chevron === 'toggle' && (
            <span
              className="folder-chevron"
              onClick={
                onToggle
                  ? (e) => {
                      e.stopPropagation();
                      onToggle();
                    }
                  : undefined
              }
            >
              {expanded ? '▼' : '▶'}
            </span>
          )}
          {chevron === 'spacer' && <span className="folder-chevron" aria-hidden="true" />}
          <span className="folder-icon"><FolderIcon size={16} /></span>
        </>
      ) : (
        getFileIcon(path)
      )}
      {/* `||`, not `??`: callers often pass `cond && <input/>`, which is
          `false` when off and must still fall back to the name. */}
      {nameSlot || (
        <span className={`${type === 'folder' ? 'folder-name' : 'file-name'} qh-truncate`}>
          {name}
        </span>
      )}
      {trailing}
      {children}
    </div>
  );
}
