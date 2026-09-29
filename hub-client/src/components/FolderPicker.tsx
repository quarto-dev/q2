/**
 * FolderPicker — "Folder: [ notes ▾ ]" control whose popover is a
 * folders-only tree, rendered with the sidebar's row component so it
 * looks like the sidebar. The popover is the shared `Menu` primitive
 * (arrow keys, type-ahead, Escape, outside-click) with each folder row as
 * a menuitem; ArrowLeft/ArrowRight collapse/expand, the chevron toggles
 * without selecting.
 */

import { useCallback, useMemo, useRef, useState } from 'react';
import { buildFileTree, type FileTreeNode } from '../utils/fileTree';
import FileTreeRow from './FileTreeRow';
import { Menu } from './Menu';
import { FolderIcon, ChevronDownIcon } from './icons';
import { folderPicker } from '../strings';
import './FolderPicker.css';

export interface FolderPickerProps {
  /** Every folder path in the project (explicit and file-derived). */
  folders: string[];
  /** Selected folder ('' = project root). */
  value: string;
  onChange: (folder: string) => void;
  id?: string;
}

export default function FolderPicker({ folders, value, onChange, id }: FolderPickerProps) {
  const [open, setOpen] = useState(false);
  const [menuPos, setMenuPos] = useState({ x: 0, y: 0 });
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const triggerRef = useRef<HTMLButtonElement>(null);

  // Folders only: no files, so every node is a folder.
  const tree = useMemo(() => buildFileTree([], folders), [folders]);

  const openMenu = useCallback(() => {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (rect) setMenuPos({ x: rect.left, y: rect.bottom + 4 });
    // Everything starts expanded: folder-only trees are small.
    setCollapsed(new Set());
    setOpen(true);
  }, []);

  const toggle = useCallback((path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const select = useCallback(
    (path: string) => {
      onChange(path);
      setOpen(false);
    },
    [onChange]
  );

  const renderNode = (node: FileTreeNode, depth: number): React.ReactNode => {
    const expanded = !collapsed.has(node.path);
    const hasChildren = node.children.length > 0;
    return (
      <div key={node.path} className="tree-folder">
        <FileTreeRow
          type="folder"
          name={node.name}
          path={node.path}
          depth={depth}
          expanded={hasChildren && expanded}
          chevron={hasChildren ? 'toggle' : 'spacer'}
          indentBase={4}
          onToggle={hasChildren ? () => toggle(node.path) : undefined}
          role="menuitem"
          tabIndex={-1}
          aria-selected={value === node.path}
          active={value === node.path}
          onClick={() => select(node.path)}
          onKeyDown={(e) => {
            if (!hasChildren) return;
            if (e.key === 'ArrowRight' && !expanded) {
              e.preventDefault();
              e.stopPropagation();
              toggle(node.path);
            } else if (e.key === 'ArrowLeft' && expanded) {
              e.preventDefault();
              e.stopPropagation();
              toggle(node.path);
            }
          }}
        />
        {hasChildren && expanded && (
          <div className="folder-children">
            {node.children.map((child) => renderNode(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="folder-picker">
      <button
        ref={triggerRef}
        id={id}
        type="button"
        className="qh-input focus-accent folder-picker-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={folderPicker.label}
        onClick={() => (open ? setOpen(false) : openMenu())}
      >
        <span className="folder-icon"><FolderIcon size={16} /></span>
        <span className="folder-picker-value qh-truncate">
          {value || folderPicker.root}
        </span>
        <span className="folder-picker-caret" aria-hidden="true">
          <ChevronDownIcon size={16} />
        </span>
      </button>
      {open && (
        <Menu
          fixed={menuPos}
          triggerRef={triggerRef}
          onClose={() => setOpen(false)}
          aria-label={folderPicker.label}
          className="folder-picker-menu"
        >
          <FileTreeRow
            type="folder"
            name={folderPicker.root}
            path=""
            depth={0}
            chevron="none"
            indentBase={4}
            role="menuitem"
            tabIndex={-1}
            aria-selected={value === ''}
            active={value === ''}
            className="folder-picker-root"
            onClick={() => select('')}
          />
          {tree.children.map((child) => renderNode(child, 0))}
        </Menu>
      )}
    </div>
  );
}
