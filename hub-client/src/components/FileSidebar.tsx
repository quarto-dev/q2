/**
 * File Sidebar Component
 *
 * Displays project files in a tree-like list with:
 * - File type icons
 * - Selection highlighting
 * - Drag-and-drop for image upload
 * - Context menu for file operations
 */

import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { isBinaryExtension } from '@quarto/preview-renderer/types/project';
import {
  buildFileTree,
  computeExpandedFolders,
  type FileTreeNode,
} from '../utils/fileTree';
import { resolveDefaultDestination } from './fileUpload';
import { buildSnippet, type SearchFiles, type SearchResult } from '../services/search';
import { openPrintableDocument } from '../services/printableDocument';
import { FilePlusIcon, UploadIcon, PrintIcon, MoreIcon } from './icons';
import { Menu, MenuItem } from './Menu';
import Tooltip from './Tooltip';
import './FileSidebar.css';

export interface FileSidebarProps {
  files: FileEntry[];
  currentFile: FileEntry | null;
  onSelectFile: (file: FileEntry) => void;
  onNewFile: () => void;
  /**
   * Open the asset dialog. `files` may be empty (e.g. when the user clicks
   * the Upload button). `destination` is the folder the dialog should seed
   * the destination input with (empty string = project root).
   */
  onUploadFiles: (files: File[], destination: string) => void;
  onDeleteFile?: (file: FileEntry) => void;
  onRenameFile?: (file: FileEntry, newPath: string) => void;
  /** Open a file in a new browser tab */
  onOpenInNewTab?: (file: FileEntry) => void;
  /** Copy a link to a file to clipboard */
  onCopyLink?: (file: FileEntry) => void;
  /**
   * Preview format of the current file (e.g. `q2-preview`, `q2-slides`,
   * `revealjs`, or `null`). Drives the "Open printable version" button:
   * shown only for formats that produce a printable standalone document.
   */
  currentFormat?: string | null;
  /**
   * Full-text search over the open project. When provided, a search box is
   * shown and a query replaces the file tree with ranked results. Absent
   * means search is disabled (the tree renders as before).
   */
  searchFiles?: SearchFiles;
  /**
   * Live text content per path, used only to render match snippets in search
   * results. Optional; without it results show the path alone.
   */
  fileContents?: Map<string, string>;
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
  file: FileEntry | null;
  /** The element that opened the menu (kebab button, or the tree row
   *  itself for Shift+F10) — focus returns here when the menu closes. */
  trigger?: HTMLElement | null;
}

/** A row of the flattened, currently-visible tree (or of the search
 *  results): the unit of keyboard navigation. */
interface NavItem {
  path: string;
  name: string;
  type: 'folder' | 'file';
  parent: string | null;
  file?: FileEntry;
}

/** Image extensions for drag-drop detection */
const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico', 'bmp', 'tiff', 'tif'];

/** Check if a file path is an image */
function isImageFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return IMAGE_EXTENSIONS.includes(ext);
}

/** Check if a file path is a renderable source file (.qmd or .md) */
function isSourceFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ext === 'qmd' || ext === 'md';
}

/** Get file icon based on extension */
function getFileIcon(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || '';

  // Images
  if (IMAGE_EXTENSIONS.includes(ext)) {
    return '🖼️';
  }
  // Documents
  if (ext === 'pdf') return '📕';
  // Quarto/Markdown
  if (['qmd', 'md'].includes(ext)) return '📝';
  // Config
  if (['yml', 'yaml', 'json'].includes(ext)) return '⚙️';
  // Code
  if (['js', 'ts', 'tsx', 'jsx', 'css', 'html'].includes(ext)) return '📄';
  // Default
  return '📄';
}


export default function FileSidebar({
  files,
  currentFile,
  onSelectFile,
  onNewFile,
  onUploadFiles,
  onDeleteFile,
  onRenameFile,
  onOpenInNewTab,
  onCopyLink,
  currentFormat,
  searchFiles,
  fileContents,
}: FileSidebarProps) {
  const [isDragOver, setIsDragOver] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({
    visible: false,
    x: 0,
    y: 0,
    file: null,
  });
  const [renamingFile, setRenamingFile] = useState<FileEntry | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(
    new Set()
  );
  // Roving-tabindex focus for the file tree / search results (APG
  // treeview/listbox): exactly one row is tabbable at a time.
  const [focusedPath, setFocusedPath] = useState<string | null>(null);
  const typeAheadRef = useRef({ buffer: '', timer: 0 });
  const searchInputRef = useRef<HTMLInputElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const sidebarRef = useRef<HTMLDivElement>(null);

  // "Open printable version" (issue #315). The React preview formats
  // can't be printed in place (sandboxed iframe → clipped single page).
  // Instead we render a standalone, self-contained document and open it
  // in a new tab. Shown only for formats that yield a printable document.
  const [isPreparingPrintable, setIsPreparingPrintable] = useState(false);
  const [printableError, setPrintableError] = useState<string | null>(null);
  const canOpenPrintable =
    !!currentFile &&
    (currentFormat === 'q2-preview' ||
      currentFormat === 'q2-slides' ||
      currentFormat === 'revealjs');
  const handleOpenPrintable = useCallback(() => {
    const path = currentFile?.path;
    if (!path) return;
    setPrintableError(null);
    setIsPreparingPrintable(true);
    openPrintableDocument(path, currentFormat ?? null)
      .catch((err: unknown) => {
        const message = err instanceof Error ? err.message : String(err);
        console.error('[printable] failed to open printable version:', err);
        setPrintableError(message);
      })
      .finally(() => setIsPreparingPrintable(false));
  }, [currentFile?.path, currentFormat]);

  // Build file tree from flat file list
  const fileTree = useMemo(() => buildFileTree(files), [files]);

  // Resolve a search result's path back to its FileEntry.
  const filesByPath = useMemo(() => {
    const m = new Map<string, FileEntry>();
    for (const f of files) m.set(f.path, f);
    return m;
  }, [files]);

  const isSearching = searchQuery.trim() !== '';

  // Debounced full-text search; ignore stale async resolutions. All state
  // updates happen inside the timer callback (never synchronously in the
  // effect body) to avoid cascading renders.
  useEffect(() => {
    if (!searchFiles) return;
    let cancelled = false;
    const handle = setTimeout(
      () => {
        if (!isSearching) {
          if (!cancelled) setSearchResults([]);
          return;
        }
        void searchFiles(searchQuery, { limit: 50 }).then((results) => {
          if (!cancelled) setSearchResults(results);
        });
      },
      isSearching ? 120 : 0
    );
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [searchFiles, searchQuery, isSearching]);

  // Toggle a folder's expanded state
  const toggleFolder = useCallback((path: string) => {
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  // Auto-expand folders when selected file changes
  useEffect(() => {
    if (currentFile) {
      setExpandedFolders((prev) =>
        computeExpandedFolders(prev, currentFile.path)
      );
    }
  }, [currentFile?.path]);

  // Flatten the visible tree (expanded folders only) into keyboard-
  // navigation order — the same order the rows render in.
  const visibleItems = useMemo(() => {
    const out: NavItem[] = [];
    const walk = (node: FileTreeNode, parent: string | null) => {
      for (const child of node.children) {
        out.push({
          path: child.path,
          name: child.name,
          type: child.type,
          parent,
          file: child.file,
        });
        if (child.type === 'folder' && expandedFolders.has(child.path)) {
          walk(child, child.path);
        }
      }
    };
    walk(fileTree, null);
    return out;
  }, [fileTree, expandedFolders]);

  // Search mode navigates the flat result list with the same keys.
  const navItems: NavItem[] = useMemo(() => {
    if (!isSearching) return visibleItems;
    return searchResults.flatMap((result) => {
      const file = filesByPath.get(result.path);
      if (!file) return [];
      return [
        {
          path: result.path,
          name: result.path.split('/').pop() || result.path,
          type: 'file' as const,
          parent: null,
          file,
        },
      ];
    });
  }, [isSearching, visibleItems, searchResults, filesByPath]);

  // The roving tab stop: the focused row when it's visible, else the
  // active file, else the first row.
  const tabbablePath = useMemo(() => {
    if (navItems.length === 0) return null;
    if (focusedPath && navItems.some((i) => i.path === focusedPath)) {
      return focusedPath;
    }
    if (currentFile && navItems.some((i) => i.path === currentFile.path)) {
      return currentFile.path;
    }
    return navItems[0].path;
  }, [focusedPath, navItems, currentFile]);

  const focusNavItem = useCallback((path: string) => {
    setFocusedPath(path);
    sidebarRef.current
      ?.querySelector<HTMLElement>(`[data-tree-path="${CSS.escape(path)}"]`)
      ?.focus();
  }, []);

  const activateNavItem = useCallback(
    (item: NavItem) => {
      if (item.type === 'folder') {
        toggleFolder(item.path);
      } else if (item.file) {
        setFocusedPath(item.path);
        onSelectFile(item.file);
      }
    },
    [toggleFolder, onSelectFile]
  );

  // Shared keyboard handler for the tree (role="tree") and the search
  // results (role="listbox"): arrows/Home/End/type-ahead/Enter, plus
  // Right/Left expand/collapse for folders and Shift+F10 for the context
  // menu. Rows only — nested controls (kebab button, rename input) keep
  // their own key behavior.
  const handleNavKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const role = target.getAttribute('role');
      if (role !== 'treeitem' && role !== 'option') return;
      const path = target.getAttribute('data-tree-path');
      const idx = navItems.findIndex((i) => i.path === path);
      if (!path || idx === -1) return;
      const item = navItems[idx];

      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          if (idx < navItems.length - 1) focusNavItem(navItems[idx + 1].path);
          return;
        case 'ArrowUp':
          e.preventDefault();
          if (idx > 0) focusNavItem(navItems[idx - 1].path);
          return;
        case 'Home':
          e.preventDefault();
          if (navItems.length > 0) focusNavItem(navItems[0].path);
          return;
        case 'End':
          e.preventDefault();
          if (navItems.length > 0) {
            focusNavItem(navItems[navItems.length - 1].path);
          }
          return;
        case 'ArrowRight':
          e.preventDefault();
          if (item.type === 'folder') {
            if (!expandedFolders.has(item.path)) {
              toggleFolder(item.path);
            } else if (
              idx + 1 < navItems.length &&
              navItems[idx + 1].parent === item.path
            ) {
              focusNavItem(navItems[idx + 1].path);
            }
          }
          return;
        case 'ArrowLeft':
          e.preventDefault();
          if (item.type === 'folder' && expandedFolders.has(item.path)) {
            toggleFolder(item.path);
          } else if (item.parent) {
            focusNavItem(item.parent);
          }
          return;
        case 'Enter':
        case ' ':
          e.preventDefault();
          activateNavItem(item);
          return;
        case 'Escape':
          if (isSearching) {
            e.preventDefault();
            setSearchQuery('');
            searchInputRef.current?.focus();
          }
          return;
        default:
          break;
      }

      // Shift+F10 / the Menu key opens the row's context menu, anchored
      // to the row so focus returns here when the menu closes.
      if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
        e.preventDefault();
        if (item.type === 'file' && item.file) {
          const rect = target.getBoundingClientRect();
          setContextMenu({
            visible: true,
            x: rect.left,
            y: rect.bottom + 4,
            file: item.file,
            trigger: target,
          });
        }
        return;
      }

      // Type-ahead: printable characters (no modifiers) move focus to the
      // next row whose name starts with the buffer. Repeating one
      // character cycles through its matches.
      if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        const ta = typeAheadRef.current;
        window.clearTimeout(ta.timer);
        ta.buffer += e.key.toLowerCase();
        ta.timer = window.setTimeout(() => {
          ta.buffer = '';
        }, 500);
        let match: NavItem | undefined;
        if (/^(.)\1*$/.test(ta.buffer)) {
          const matches = navItems.filter((i) =>
            i.name.toLowerCase().startsWith(ta.buffer[0])
          );
          if (matches.length > 0) {
            const cur = matches.findIndex((i) => i.path === path);
            match = matches[(cur + 1) % matches.length];
          }
        } else {
          match = navItems.find((i) =>
            i.name.toLowerCase().startsWith(ta.buffer)
          );
        }
        if (match) focusNavItem(match.path);
      }
    },
    [
      navItems,
      expandedFolders,
      toggleFolder,
      focusNavItem,
      activateNavItem,
      isSearching,
    ]
  );

  // Drag and drop handlers
  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setIsDragOver(false);

      const droppedFiles = Array.from(e.dataTransfer.files);
      if (droppedFiles.length > 0) {
        const destination = resolveDefaultDestination({
          dropTarget: e.target,
          selection: currentFile?.path,
        });
        onUploadFiles(droppedFiles, destination);
      }
    },
    [onUploadFiles, currentFile]
  );

  // "Upload" button: open the asset dialog with no pre-filled files.
  const handleUploadClick = useCallback(() => {
    const destination = resolveDefaultDestination({
      selection: currentFile?.path,
    });
    onUploadFiles([], destination);
  }, [onUploadFiles, currentFile]);

  // Context menu handlers
  const handleContextMenu = useCallback((e: React.MouseEvent, file: FileEntry) => {
    e.preventDefault();
    setContextMenu({
      visible: true,
      x: e.clientX,
      y: e.clientY,
      file,
    });
  }, []);

  const closeContextMenu = useCallback(() => {
    setContextMenu((prev) => ({ ...prev, visible: false }));
  }, []);

  // Rename handlers
  const startRename = useCallback((file: FileEntry) => {
    setRenamingFile(file);
    setRenameValue(file.path);
    closeContextMenu();
    // Focus input and select all text after render
    setTimeout(() => {
      renameInputRef.current?.focus();
      renameInputRef.current?.select();
    }, 0);
  }, [closeContextMenu]);

  const handleRenameSubmit = useCallback(() => {
    if (renamingFile && renameValue.trim() && onRenameFile) {
      const newPath = renameValue.trim();
      // Only rename if the path actually changed; same path = cancel
      if (newPath !== renamingFile.path) {
        onRenameFile(renamingFile, newPath);
      }
    }
    setRenamingFile(null);
    setRenameValue('');
  }, [renamingFile, renameValue, onRenameFile]);

  const handleRenameKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        handleRenameSubmit();
      } else if (e.key === 'Escape') {
        setRenamingFile(null);
        setRenameValue('');
      }
    },
    [handleRenameSubmit]
  );

  // Delete handler
  const handleDelete = useCallback(
    (file: FileEntry) => {
      closeContextMenu();
      if (onDeleteFile && window.confirm(`Delete ${file.path}?`)) {
        onDeleteFile(file);
      }
    },
    [onDeleteFile, closeContextMenu]
  );

  // Open in new tab handler
  const handleOpenInNewTab = useCallback(
    (file: FileEntry) => {
      closeContextMenu();
      onOpenInNewTab?.(file);
    },
    [onOpenInNewTab, closeContextMenu]
  );

  // Copy link handler
  const handleCopyLink = useCallback(
    (file: FileEntry) => {
      closeContextMenu();
      onCopyLink?.(file);
    },
    [onCopyLink, closeContextMenu]
  );

  // File click handler - supports Ctrl/Cmd+click for new tab
  const handleFileClick = useCallback(
    (e: React.MouseEvent, file: FileEntry) => {
      // Ctrl/Cmd+click opens in new tab
      if ((e.ctrlKey || e.metaKey) && onOpenInNewTab) {
        e.preventDefault();
        onOpenInNewTab(file);
      } else {
        onSelectFile(file);
      }
    },
    [onSelectFile, onOpenInNewTab]
  );

  // Drag start handler for file items (for dragging to editor)
  const handleFileDragStart = useCallback((e: React.DragEvent, file: FileEntry) => {
    // Determine the type of file for markdown insertion
    let fileType: 'image' | 'qmd' | 'other' = 'other';
    if (isImageFile(file.path)) {
      fileType = 'image';
    } else if (isSourceFile(file.path)) {
      fileType = 'qmd';
    }

    // Set custom data for internal drag detection
    e.dataTransfer.setData('application/x-hub-file', JSON.stringify({
      path: file.path,
      type: fileType,
    }));
    e.dataTransfer.effectAllowed = 'copy';
  }, []);

  // Render a file item with depth-based indentation
  const renderFileItem = (file: FileEntry, depth: number) => {
    const fileName = file.path.split('/').pop() || file.path;
    const isActive = currentFile?.path === file.path;
    const isBinary = isBinaryExtension(file.path);
    const isRenaming = renamingFile?.path === file.path;
    // Only make images and qmd files draggable (for editor insertion)
    const isDraggable =
      !isRenaming && (isImageFile(file.path) || isSourceFile(file.path));
    // Parent folder of this file, used by resolveDefaultDestination when a
    // drop lands on a file row (the drop target is the file, but the
    // destination for an upload is the enclosing folder).
    const lastSlash = file.path.lastIndexOf('/');
    const parentFolderPath = lastSlash >= 0 ? file.path.slice(0, lastSlash) : '';

    const rowTip = onOpenInNewTab
      ? `${file.path} — Ctrl/Cmd+click to open in new tab`
      : file.path;
    return (
      <Tooltip block content={rowTip}>
      <div
        key={file.path}
        role="treeitem"
        aria-level={depth + 1}
        aria-selected={isActive}
        tabIndex={tabbablePath === file.path ? 0 : -1}
        data-tree-path={file.path}
        className={`file-item qh-row-hover ${isActive ? 'active' : ''} ${isBinary ? 'binary' : ''}`}
        style={{ paddingLeft: `${12 + depth * 16}px` }}
        data-folder-path={parentFolderPath}
        onClick={(e) => {
          if (isRenaming) return;
          setFocusedPath(file.path);
          handleFileClick(e, file);
        }}
        onFocus={() => setFocusedPath(file.path)}
        onContextMenu={(e) => handleContextMenu(e, file)}
        draggable={isDraggable}
        onDragStart={
          isDraggable ? (e) => handleFileDragStart(e, file) : undefined
        }
      >
        <span className="file-icon">{getFileIcon(file.path)}</span>
        {isRenaming ? (
          <input
            ref={renameInputRef}
            type="text"
            className="rename-input"
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onBlur={handleRenameSubmit}
            onKeyDown={handleRenameKeyDown}
          />
        ) : (
          <span className="file-name qh-truncate">{fileName}</span>
        )}
        {!isRenaming && (onOpenInNewTab || onCopyLink || onRenameFile || onDeleteFile) && (
          <button
            type="button"
            className="qh-icon-btn file-kebab"
            // Not a tab stop: the tree is one tab stop (roving tabindex)
            // and keyboard users open this menu with Shift+F10 on the row.
            tabIndex={-1}
            aria-label={`Actions for ${fileName}`}
            aria-haspopup="menu"
            aria-expanded={contextMenu.visible && contextMenu.file?.path === file.path}
            onClick={(e) => {
              e.stopPropagation();
              if (contextMenu.visible && contextMenu.file?.path === file.path) {
                closeContextMenu();
                return;
              }
              const rect = e.currentTarget.getBoundingClientRect();
              setContextMenu({
                visible: true,
                x: rect.left,
                y: rect.bottom + 4,
                file,
                trigger: e.currentTarget,
              });
            }}
            onContextMenu={(e) => {
              // Menu key / right-click on the kebab opens the same menu.
              e.stopPropagation();
              handleContextMenu(e, file);
            }}
          >
            <MoreIcon />
          </button>
        )}
      </div>
      </Tooltip>
    );
  };

  // Recursively render a tree node (folder or file)
  const renderTreeNode = (node: FileTreeNode, depth: number = 0): React.ReactNode => {
    if (node.type === 'file' && node.file) {
      return renderFileItem(node.file, depth);
    }

    // For folders
    const isExpanded = expandedFolders.has(node.path);

    // Special case: root node renders children directly without a folder header
    if (node.path === '') {
      return node.children.map((child) => renderTreeNode(child, depth));
    }

    return (
      <div key={node.path} className="tree-folder" data-folder-path={node.path}>
        <div
          className="folder-header"
          role="treeitem"
          aria-level={depth + 1}
          aria-expanded={isExpanded}
          tabIndex={tabbablePath === node.path ? 0 : -1}
          data-tree-path={node.path}
          style={{ paddingLeft: `${12 + depth * 16}px` }}
          onClick={() => {
            setFocusedPath(node.path);
            toggleFolder(node.path);
          }}
          onFocus={() => setFocusedPath(node.path)}
        >
          <span className="folder-chevron">{isExpanded ? '▼' : '▶'}</span>
          <span className="folder-icon">📁</span>
          <span className="folder-name">{node.name}</span>
        </div>
        {isExpanded && (
          <div className="folder-children" role="group">
            {node.children.map((child) => renderTreeNode(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  // Render the ranked search results (replaces the tree while searching).
  const renderSearchResults = (): React.ReactNode => {
    if (searchResults.length === 0) {
      return (
        <div className="empty-state">
          <p>No matches</p>
        </div>
      );
    }
    return searchResults.map((result) => {
      const file = filesByPath.get(result.path);
      if (!file) return null; // result for a file no longer listed
      const fileName = result.path.split('/').pop() || result.path;
      const dir = result.path.slice(0, result.path.length - fileName.length);
      const content = fileContents?.get(result.path);
      const snippet = content ? buildSnippet(content, result.terms) : [];
      const isActive = currentFile?.path === result.path;
      return (
        <div
          key={result.path}
          role="option"
          aria-selected={isActive}
          tabIndex={tabbablePath === result.path ? 0 : -1}
          data-tree-path={result.path}
          className={`search-result qh-row-hover qh-active-accent-row ${isActive ? 'active' : ''}`}
          onClick={() => {
            setFocusedPath(result.path);
            onSelectFile(file);
          }}
          onFocus={() => setFocusedPath(result.path)}
        >
          <div className="search-result-header">
            <span className="file-icon">{getFileIcon(result.path)}</span>
            <span className="search-result-name qh-truncate">{fileName}</span>
            {dir && <span className="search-result-path qh-truncate">{dir}</span>}
          </div>
          {snippet.length > 0 && (
            <div className="search-result-snippet qh-truncate">
              {snippet.map((seg, i) =>
                seg.match ? (
                  <mark key={i}>{seg.text}</mark>
                ) : (
                  <span key={i}>{seg.text}</span>
                )
              )}
            </div>
          )}
        </div>
      );
    });
  };

  return (
    <div
      ref={sidebarRef}
      className={`file-sidebar ${isDragOver ? 'drag-over' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <div className="sidebar-header">
        <Tooltip content="New file">
          <button
            className="qh-btn small outline new-file-btn"
            onClick={onNewFile}
            aria-label="New file"
          >
            <FilePlusIcon />
          </button>
        </Tooltip>
        <Tooltip content="Upload asset">
          <button
            className="qh-btn small outline upload-asset-btn"
            onClick={handleUploadClick}
            aria-label="Upload asset"
          >
            <UploadIcon />
          </button>
        </Tooltip>
        {canOpenPrintable && (
          <Tooltip content="Open a printable version in a new tab (use the browser's Print to save as PDF)">
            <button
              className="qh-btn small outline print-file-btn"
              onClick={handleOpenPrintable}
              disabled={isPreparingPrintable}
              aria-label="Open printable version in a new tab"
            >
              {isPreparingPrintable ? '…' : <PrintIcon />}
            </button>
          </Tooltip>
        )}
      </div>
      {printableError && (
        <div className="sidebar-printable-error" role="alert">
          {printableError}
          <button
            className="sidebar-printable-error-dismiss"
            onClick={() => setPrintableError(null)}
            aria-label="Dismiss"
          >
            ✕
          </button>
        </div>
      )}

      {searchFiles && (
        <div className="sidebar-search">
          <input
            ref={searchInputRef}
            type="search"
            className="sidebar-search-input"
            placeholder="Search files…"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              // ArrowDown moves into the results list; Escape clears.
              if (e.key === 'ArrowDown' && navItems.length > 0) {
                e.preventDefault();
                focusNavItem(navItems[0].path);
              } else if (e.key === 'Escape' && isSearching) {
                e.preventDefault();
                setSearchQuery('');
              }
            }}
            aria-label="Search files"
          />
          {isSearching && (
            <Tooltip content="Clear search">
              <button
                className="sidebar-search-clear"
                onClick={() => setSearchQuery('')}
                aria-label="Clear search"
              >
                ✕
              </button>
            </Tooltip>
          )}
        </div>
      )}

      {/* APG treeview (file tree) / listbox (search results): one tab
          stop via roving tabindex; arrows/Home/End/type-ahead navigate,
          Enter activates, Shift+F10 opens the row's context menu. */}
      <div
        className="file-list"
        role={isSearching ? 'listbox' : 'tree'}
        aria-label={isSearching ? 'Search results' : 'Files'}
        onKeyDown={handleNavKeyDown}
      >
        {isSearching ? (
          renderSearchResults()
        ) : files.length === 0 ? (
          <div className="empty-state">
            <p>No files yet</p>
            <p className="hint">Drop files here or click + to create</p>
          </div>
        ) : (
          renderTreeNode(fileTree)
        )}
      </div>

      {isDragOver && (
        <div className="drop-overlay">
          <div className="drop-message">
            <span className="drop-icon">📥</span>
            <span>Drop files to upload</span>
          </div>
        </div>
      )}

      {/* Context Menu */}
      {contextMenu.visible && contextMenu.file && (
        <Menu
          // Key by file so re-opening for a different row remounts:
          // first-item focus and the viewport flip are mount-time effects.
          key={contextMenu.file.path}
          fixed={{ x: contextMenu.x, y: contextMenu.y }}
          onClose={() => closeContextMenu()}
          triggerRef={{ current: contextMenu.trigger ?? null }}
          aria-label={`Actions for ${contextMenu.file.path}`}
        >
          {onOpenInNewTab && (
            <MenuItem onSelect={() => handleOpenInNewTab(contextMenu.file!)}>
              Open in New Tab
            </MenuItem>
          )}
          {onCopyLink && (
            <MenuItem onSelect={() => handleCopyLink(contextMenu.file!)}>
              Copy Link
            </MenuItem>
          )}
          {onRenameFile && (
            <MenuItem onSelect={() => startRename(contextMenu.file!)}>
              Rename
            </MenuItem>
          )}
          {onDeleteFile && (
            <MenuItem danger onSelect={() => handleDelete(contextMenu.file!)}>
              Delete
            </MenuItem>
          )}
        </Menu>
      )}
    </div>
  );
}
