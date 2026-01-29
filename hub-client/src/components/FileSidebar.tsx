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
import type { FileEntry } from '../types/project';
import { isBinaryExtension } from '../types/project';
import {
  buildFileTree,
  computeExpandedFolders,
  type FileTreeNode,
} from '../utils/fileTree';
import './FileSidebar.css';

export interface FileSidebarProps {
  files: FileEntry[];
  currentFile: FileEntry | null;
  onSelectFile: (file: FileEntry) => void;
  onNewFile: () => void;
  onUploadFiles: (files: File[]) => void;
  onDeleteFile?: (file: FileEntry) => void;
  onRenameFile?: (file: FileEntry, newPath: string) => void;
  /** Open a file in a new browser tab */
  onOpenInNewTab?: (file: FileEntry) => void;
  /** Copy a link to a file to clipboard */
  onCopyLink?: (file: FileEntry) => void;
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
  file: FileEntry | null;
}

/** Image extensions for drag-drop detection */
const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico', 'bmp', 'tiff', 'tif'];

/** Check if a file path is an image */
function isImageFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return IMAGE_EXTENSIONS.includes(ext);
}

/** Check if a file path is a qmd file */
function isQmdFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ext === 'qmd';
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
}: FileSidebarProps) {
  const [isDragOver, setIsDragOver] = useState(false);
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
  const renameInputRef = useRef<HTMLInputElement>(null);
  const sidebarRef = useRef<HTMLDivElement>(null);

  // Build file tree from flat file list
  const fileTree = useMemo(() => buildFileTree(files), [files]);

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
        onUploadFiles(droppedFiles);
      }
    },
    [onUploadFiles]
  );

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

  // Handle clicks outside context menu
  const handleSidebarClick = useCallback(() => {
    if (contextMenu.visible) {
      closeContextMenu();
    }
  }, [contextMenu.visible, closeContextMenu]);

  // Rename handlers
  const startRename = useCallback((file: FileEntry) => {
    setRenamingFile(file);
    setRenameValue(file.path);
    closeContextMenu();
    // Focus input after render
    setTimeout(() => renameInputRef.current?.focus(), 0);
  }, [closeContextMenu]);

  const handleRenameSubmit = useCallback(() => {
    if (renamingFile && renameValue.trim() && onRenameFile) {
      onRenameFile(renamingFile, renameValue.trim());
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
    } else if (isQmdFile(file.path)) {
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
      !isRenaming && (isImageFile(file.path) || isQmdFile(file.path));

    return (
      <div
        key={file.path}
        className={`file-item ${isActive ? 'active' : ''} ${isBinary ? 'binary' : ''}`}
        style={{ paddingLeft: `${12 + depth * 16}px` }}
        onClick={(e) => !isRenaming && handleFileClick(e, file)}
        onContextMenu={(e) => handleContextMenu(e, file)}
        draggable={isDraggable}
        onDragStart={
          isDraggable ? (e) => handleFileDragStart(e, file) : undefined
        }
        title={
          onOpenInNewTab
            ? `${file.path}\nCtrl/Cmd+click to open in new tab`
            : file.path
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
          <span className="file-name">{fileName}</span>
        )}
      </div>
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
      <div key={node.path} className="tree-folder">
        <div
          className="folder-header"
          style={{ paddingLeft: `${12 + depth * 16}px` }}
          onClick={() => toggleFolder(node.path)}
        >
          <span className="folder-chevron">{isExpanded ? '▼' : '▶'}</span>
          <span className="folder-icon">📁</span>
          <span className="folder-name">{node.name}</span>
        </div>
        {isExpanded && (
          <div className="folder-children">
            {node.children.map((child) => renderTreeNode(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div
      ref={sidebarRef}
      className={`file-sidebar ${isDragOver ? 'drag-over' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      onClick={handleSidebarClick}
    >
      <div className="sidebar-header">
        <button className="new-file-btn" onClick={onNewFile} title="New file">
          + New
        </button>
      </div>

      <div className="file-list">
        {files.length === 0 ? (
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
        <div
          className="context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
        >
          {onOpenInNewTab && (
            <button onClick={() => handleOpenInNewTab(contextMenu.file!)}>
              Open in New Tab
            </button>
          )}
          {onCopyLink && (
            <button onClick={() => handleCopyLink(contextMenu.file!)}>
              Copy Link
            </button>
          )}
          {onRenameFile && (
            <button onClick={() => startRename(contextMenu.file!)}>
              Rename
            </button>
          )}
          {onDeleteFile && (
            <button
              className="danger"
              onClick={() => handleDelete(contextMenu.file!)}
            >
              Delete
            </button>
          )}
        </div>
      )}
    </div>
  );
}
