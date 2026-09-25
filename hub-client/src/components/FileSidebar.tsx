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
import { isImageExtension, normalizeProjectPath } from '@quarto/preview-renderer/types/project';
import {
  buildFileTree,
  computeExpandedFolders,
  type FileTreeNode,
} from '../utils/fileTree';
import { resolveDefaultDestination } from './fileUpload';
import { buildSnippet, type SearchFiles, type SearchResult } from '../services/search';
import {
  FilePlusIcon,
  UploadIcon,
  FolderPlusIcon,
  ShareIcon,
  DownloadIcon,
} from './icons';
import FileTreeRow from './FileTreeRow';
import MoreActionsIconButton from './MoreActionsIconButton';
import { getFileIcon, treeRowIndent } from './fileTreeRowHelpers';
import { Menu, MenuItem } from './Menu';
import Tooltip from './Tooltip';
import { fileSidebar } from '../strings';
import './FileSidebar.css';

export interface FileSidebarProps {
  files: FileEntry[];
  /**
   * Explicitly created folders (IndexDocument V3 `folders`). Rendered even
   * when no file lives under them; folders implied by file paths need not
   * be listed.
   */
  folders?: string[];
  currentFile: FileEntry | null;
  onSelectFile: (file: FileEntry) => void;
  onNewFile: () => void;
  /** Open the new-file dialog seeded with `folder` (folder context menu). */
  onNewFileIn?: (folder: string) => void;
  /**
   * Create an empty folder under `parent` ('' = project root). Shown as a
   * header button and as a folder context-menu entry when provided.
   */
  onNewFolder?: (parent: string) => void;
  /** Open the move dialog for a file (file context menu). */
  onMoveFile?: (file: FileEntry) => void;
  /** Delete an explicitly created folder; only offered when it is empty. */
  onDeleteFolder?: (path: string) => void;
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

interface FolderMenuState {
  visible: boolean;
  x: number;
  y: number;
  /** Folder path the menu is for. */
  path: string;
  /** Whether the folder has no children (files or subfolders). */
  isEmpty: boolean;
  trigger?: HTMLElement | null;
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

/** dataTransfer type for sidebar-originated drags (read by Editor.tsx too). */
const HUB_FILE_TYPE = 'application/x-hub-file';

/** Check if a file path is an image (shared with the editor's image viewer) */
function isImageFile(path: string): boolean {
  return isImageExtension(path);
}

/** Check if a file path is a renderable source file (.qmd or .md) */
function isSourceFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ext === 'qmd' || ext === 'md';
}


export default function FileSidebar({
  files,
  folders,
  currentFile,
  onSelectFile,
  onNewFile,
  onNewFileIn,
  onNewFolder,
  onDeleteFolder,
  onMoveFile,
  onUploadFiles,
  onDeleteFile,
  onRenameFile,
  onOpenInNewTab,
  onCopyLink,
  searchFiles,
  fileContents,
}: FileSidebarProps) {
  const [isDragOver, setIsDragOver] = useState(false);
  // Drag-to-move (sidebar-internal drag of a file row): the folder path
  // currently hovered as a drop destination ('' = project root), or null
  // when no valid move is being hovered. `draggingPathRef` remembers the
  // row being dragged because dataTransfer.getData is unreadable during
  // dragover — only the payload's *type* is exposed until drop.
  const [moveTarget, setMoveTarget] = useState<string | null>(null);
  const draggingPathRef = useRef<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
  const [folderMenu, setFolderMenu] = useState<FolderMenuState>({
    visible: false,
    x: 0,
    y: 0,
    path: '',
    isEmpty: false,
  });
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

  // Build file tree from flat file list
  const fileTree = useMemo(() => buildFileTree(files, folders), [files, folders]);

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

  /**
   * Where a sidebar-internal drag of `sourcePath` would land if dropped on
   * `target`: the enclosing folder's path via the nearest
   * `data-folder-path` ancestor, or the project root when the drop lands
   * on tree background. Returns the new full path, or null when the move
   * is a no-op (same folder) or would overwrite an existing file.
   */
  const resolveMove = useCallback(
    (sourcePath: string, target: EventTarget | null): { folder: string; newPath: string } | null => {
      const el = target instanceof Element ? target.closest('[data-folder-path]') : null;
      const folder = el?.getAttribute('data-folder-path') ?? '';
      const name = sourcePath.split('/').pop() || sourcePath;
      const newPath = normalizeProjectPath(folder ? `${folder}/${name}` : name);
      if (newPath === sourcePath || filesByPath.has(newPath)) return null;
      return { folder, newPath };
    },
    [filesByPath]
  );

  // Drag and drop handlers. Two kinds of drag reach the sidebar: external
  // files (upload — shows the drop overlay) and the sidebar's own file
  // rows (move — highlights the destination folder).
  const handleDragOver = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.dataTransfer.types.includes(HUB_FILE_TYPE)) {
        const source = draggingPathRef.current;
        const move = source && onRenameFile ? resolveMove(source, e.target) : null;
        e.dataTransfer.dropEffect = move ? 'move' : 'none';
        setMoveTarget(move ? move.folder : null);
        return;
      }
      setIsDragOver(true);
    },
    [onRenameFile, resolveMove]
  );

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);
    setMoveTarget(null);
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setIsDragOver(false);
      setMoveTarget(null);

      const internal = e.dataTransfer.getData(HUB_FILE_TYPE);
      if (internal) {
        const { path } = JSON.parse(internal) as { path: string };
        const source = filesByPath.get(path);
        if (source && onRenameFile) {
          const move = resolveMove(path, e.target);
          if (move) onRenameFile(source, move.newPath);
        }
        return;
      }

      const droppedFiles = Array.from(e.dataTransfer.files);
      if (droppedFiles.length > 0) {
        const destination = resolveDefaultDestination({
          dropTarget: e.target,
          selection: currentFile?.path,
        });
        onUploadFiles(droppedFiles, destination);
      }
    },
    [onUploadFiles, currentFile, filesByPath, onRenameFile, resolveMove]
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

  const closeFolderMenu = useCallback(() => {
    setFolderMenu((prev) => ({ ...prev, visible: false }));
  }, []);

  const handleFolderContextMenu = useCallback(
    (e: React.MouseEvent, node: FileTreeNode) => {
      if (!onNewFileIn && !onNewFolder && !onDeleteFolder && !onUploadFiles) return;
      e.preventDefault();
      e.stopPropagation();
      setFolderMenu({
        visible: true,
        x: e.clientX,
        y: e.clientY,
        path: node.path,
        isEmpty: node.children.length === 0,
        trigger: e.currentTarget as HTMLElement,
      });
    },
    [onNewFileIn, onNewFolder, onDeleteFolder, onUploadFiles]
  );

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
      const newPath = normalizeProjectPath(renameValue);
      // Only rename if the path actually changed; same path = cancel
      if (newPath && newPath !== renamingFile.path) {
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
      if (onDeleteFile && window.confirm(fileSidebar.confirmDelete(file.path))) {
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

  // Drag start handler for file items: the same payload serves dropping
  // into the editor (insert image/link) and onto a sidebar folder (move).
  const handleFileDragStart = useCallback((e: React.DragEvent, file: FileEntry) => {
    draggingPathRef.current = file.path;
    // Determine the type of file for markdown insertion
    let fileType: 'image' | 'qmd' | 'other' = 'other';
    if (isImageFile(file.path)) {
      fileType = 'image';
    } else if (isSourceFile(file.path)) {
      fileType = 'qmd';
    }

    // Set custom data for internal drag detection
    e.dataTransfer.setData(HUB_FILE_TYPE, JSON.stringify({
      path: file.path,
      type: fileType,
    }));
    e.dataTransfer.effectAllowed = 'copyMove';
  }, []);

  const handleFileDragEnd = useCallback(() => {
    draggingPathRef.current = null;
    setMoveTarget(null);
  }, []);

  const hasFileActions = !!(
    onOpenInNewTab || onCopyLink || onRenameFile || onMoveFile || onDeleteFile
  );

  // Render a file item with depth-based indentation
  const renderFileItem = (file: FileEntry, depth: number) => {
    const fileName = file.path.split('/').pop() || file.path;
    const isActive = currentFile?.path === file.path;
    const isRenaming = renamingFile?.path === file.path;
    // Every row can be dragged onto a folder to move it; images and
    // sources can additionally be dropped into the editor.
    const isDraggable = !isRenaming;
    // Parent folder of this file, used by resolveDefaultDestination when a
    // drop lands on a file row (the drop target is the file, but the
    // destination for an upload is the enclosing folder).
    const lastSlash = file.path.lastIndexOf('/');
    const parentFolderPath = lastSlash >= 0 ? file.path.slice(0, lastSlash) : '';

    const rowTip = fileSidebar.rowTooltip(file.path, !!onOpenInNewTab);
    return (
      <Tooltip key={file.path} block content={rowTip}>
        <FileTreeRow
          type="file"
          name={fileName}
          path={file.path}
          depth={depth}
          role="treeitem"
          aria-level={depth + 1}
          aria-selected={isActive}
          active={isActive}
          tabIndex={tabbablePath === file.path ? 0 : -1}
          data-tree-path={file.path}
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
          onDragEnd={isDraggable ? handleFileDragEnd : undefined}
          nameSlot={
            isRenaming &&
            <input
              ref={renameInputRef}
              type="text"
              className="rename-input"
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onBlur={handleRenameSubmit}
              onKeyDown={handleRenameKeyDown}
            />
          }
        >
          {!isRenaming && hasFileActions && (
            <MoreActionsIconButton
              label={fileSidebar.actionsFor(fileName)}
              expanded={contextMenu.visible && contextMenu.file?.path === file.path}
              onOpen={({ x, y, trigger }) =>
                setContextMenu({ visible: true, x, y, file, trigger })
              }
              onClose={closeContextMenu}
              onContextMenu={(e) => handleContextMenu(e, file)}
            />
          )}
        </FileTreeRow>
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
      <div
        key={node.path}
        className={`tree-folder ${moveTarget === node.path ? 'drop-target' : ''}`}
        data-folder-path={node.path}
      >
        <FileTreeRow
          type="folder"
          name={node.name}
          path={node.path}
          depth={depth}
          expanded={isExpanded}
          role="treeitem"
          aria-level={depth + 1}
          aria-expanded={isExpanded}
          tabIndex={tabbablePath === node.path ? 0 : -1}
          data-tree-path={node.path}
          onClick={() => {
            setFocusedPath(node.path);
            toggleFolder(node.path);
          }}
          onFocus={() => setFocusedPath(node.path)}
          onContextMenu={(e) => handleFolderContextMenu(e, node)}
        />
        {isExpanded && node.children.length > 0 && (
          <div className="folder-children" role="group">
            {node.children.map((child) => renderTreeNode(child, depth + 1))}
          </div>
        )}
        {isExpanded && node.children.length === 0 && (
          <div
            className="folder-empty-hint"
            style={{ paddingLeft: `${treeRowIndent(depth + 1) + 16}px` }}
          >
            {fileSidebar.folderEmpty}
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
          <p>{fileSidebar.noMatches}</p>
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
            {getFileIcon(result.path)}
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
        <Tooltip content={fileSidebar.newFile}>
          <button
            className="qh-btn small outline new-file-btn"
            onClick={onNewFile}
            aria-label={fileSidebar.newFile}
          >
            <FilePlusIcon />
          </button>
        </Tooltip>
        {onNewFolder && (
          <Tooltip content={fileSidebar.newFolder}>
            <button
              className="qh-btn small outline new-folder-btn"
              onClick={() =>
                onNewFolder(resolveDefaultDestination({ selection: currentFile?.path }))
              }
              aria-label={fileSidebar.newFolder}
            >
              <FolderPlusIcon />
            </button>
          </Tooltip>
        )}
        <Tooltip content={fileSidebar.addAsset}>
          <button
            className="qh-btn small outline upload-asset-btn"
            onClick={handleUploadClick}
            aria-label={fileSidebar.addAsset}
          >
            <UploadIcon />
          </button>
        </Tooltip>
      </div>

      {searchFiles && (
        <div className="sidebar-search">
          <input
            ref={searchInputRef}
            type="search"
            className="sidebar-search-input"
            placeholder={fileSidebar.searchPlaceholder}
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
            aria-label={fileSidebar.searchLabel}
          />
          {isSearching && (
            <Tooltip content={fileSidebar.clearSearch}>
              <button
                className="sidebar-search-clear"
                onClick={() => setSearchQuery('')}
                aria-label={fileSidebar.clearSearch}
              >
                ✕
              </button>
            </Tooltip>
          )}
        </div>
      )}

      {/* APG treeview (file tree) / listbox (search results): one tab
          stop via roving tabindex; arrows/Home/End/type-ahead navigate,
          Enter activates, Shift+F10 opens the row's context menu. An
          empty tree/listbox carries no widget role — a role with zero
          items violates aria-required-children; the empty state is plain
          text. */}
      <div
        className={`file-list ${moveTarget === '' ? 'drop-target' : ''}`}
        role={
          isSearching
            ? searchResults.length > 0
              ? 'listbox'
              : undefined
            : files.length > 0
              ? 'tree'
              : undefined
        }
        aria-label={
          isSearching
            ? searchResults.length > 0
              ? fileSidebar.resultsLabel
              : undefined
            : files.length > 0
              ? fileSidebar.treeLabel
              : undefined
        }
        onKeyDown={handleNavKeyDown}
      >
        {isSearching ? (
          renderSearchResults()
        ) : files.length === 0 ? (
          <div className="empty-state">
            <FilePlusIcon size={16} />
            <p>{fileSidebar.emptyTitle}</p>
            <p className="hint">{fileSidebar.emptyHint}</p>
          </div>
        ) : (
          renderTreeNode(fileTree)
        )}
      </div>

      {isDragOver && (
        <div className="drop-overlay">
          <div className="drop-message">
            <span className="drop-icon"><DownloadIcon size={32} /></span>
            <span>{fileSidebar.dropOverlay}</span>
          </div>
        </div>
      )}

      {/* Folder context menu */}
      {folderMenu.visible && (
        <Menu
          key={`folder:${folderMenu.path}`}
          fixed={{ x: folderMenu.x, y: folderMenu.y }}
          onClose={closeFolderMenu}
          triggerRef={{ current: folderMenu.trigger ?? null }}
          aria-label={fileSidebar.folderActionsFor(folderMenu.path)}
        >
          {onNewFileIn && (
            <MenuItem icon={<FilePlusIcon />} onSelect={() => onNewFileIn(folderMenu.path)}>
              {fileSidebar.menuNewFileInside}
            </MenuItem>
          )}
          {onNewFolder && (
            <MenuItem icon={<FolderPlusIcon />} onSelect={() => onNewFolder(folderMenu.path)}>
              {fileSidebar.menuNewFolderInside}
            </MenuItem>
          )}
          <MenuItem icon={<UploadIcon />} onSelect={() => onUploadFiles([], folderMenu.path)}>
            {fileSidebar.menuNewAssetInside}
          </MenuItem>
          {onDeleteFolder && (
            <MenuItem
              danger
              disabled={!folderMenu.isEmpty}
              subtext={folderMenu.isEmpty ? undefined : fileSidebar.deleteFolderNotEmpty}
              onSelect={() => onDeleteFolder(folderMenu.path)}
            >
              {fileSidebar.menuDeleteFolder}
            </MenuItem>
          )}
        </Menu>
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
          aria-label={fileSidebar.actionsFor(contextMenu.file.path)}
        >
          {onOpenInNewTab && (
            <MenuItem onSelect={() => handleOpenInNewTab(contextMenu.file!)}>
              {fileSidebar.menuOpenInNewTab}
            </MenuItem>
          )}
          {onCopyLink && (
            <MenuItem icon={<ShareIcon />} onSelect={() => handleCopyLink(contextMenu.file!)}>
              {fileSidebar.menuCopyLink}
            </MenuItem>
          )}
          {onRenameFile && (
            <MenuItem onSelect={() => startRename(contextMenu.file!)}>
              {fileSidebar.menuRename}
            </MenuItem>
          )}
          {onMoveFile && (
            <MenuItem onSelect={() => onMoveFile(contextMenu.file!)}>
              {fileSidebar.menuMove}
            </MenuItem>
          )}
          {onDeleteFile && (
            <MenuItem danger onSelect={() => handleDelete(contextMenu.file!)}>
              {fileSidebar.menuDelete}
            </MenuItem>
          )}
        </Menu>
      )}
    </div>
  );
}
