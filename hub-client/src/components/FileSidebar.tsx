/**
 * File Sidebar Component
 *
 * Displays project files in a tree-like list with:
 * - File type icons
 * - Selection highlighting
 * - Drag-and-drop for image upload
 * - Context menu for file operations
 */

import { useState, useCallback, useRef } from 'react';
import type { FileEntry } from '../types/project';
import { isBinaryExtension } from '../types/project';
import './FileSidebar.css';

export interface FileSidebarProps {
  files: FileEntry[];
  currentFile: FileEntry | null;
  onSelectFile: (file: FileEntry) => void;
  onNewFile: () => void;
  onUploadFiles: (files: File[]) => void;
  onDeleteFile?: (file: FileEntry) => void;
  onRenameFile?: (file: FileEntry, newPath: string) => void;
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
  file: FileEntry | null;
}

/** Get file icon based on extension */
function getFileIcon(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || '';

  // Images
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico', 'bmp'].includes(ext)) {
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

/** Group files by directory for tree display */
function groupFilesByDirectory(files: FileEntry[]): Map<string, FileEntry[]> {
  const groups = new Map<string, FileEntry[]>();

  for (const file of files) {
    const lastSlash = file.path.lastIndexOf('/');
    const dir = lastSlash >= 0 ? file.path.slice(0, lastSlash) : '';

    if (!groups.has(dir)) {
      groups.set(dir, []);
    }
    groups.get(dir)!.push(file);
  }

  // Sort files within each group
  for (const [, groupFiles] of groups) {
    groupFiles.sort((a, b) => {
      const aName = a.path.split('/').pop() || '';
      const bName = b.path.split('/').pop() || '';
      return aName.localeCompare(bName);
    });
  }

  return groups;
}

export default function FileSidebar({
  files,
  currentFile,
  onSelectFile,
  onNewFile,
  onUploadFiles,
  onDeleteFile,
  onRenameFile,
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
  const renameInputRef = useRef<HTMLInputElement>(null);
  const sidebarRef = useRef<HTMLDivElement>(null);

  // Group files for display
  const fileGroups = groupFilesByDirectory(files);

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

  // Render a file item
  const renderFileItem = (file: FileEntry) => {
    const fileName = file.path.split('/').pop() || file.path;
    const isActive = currentFile?.path === file.path;
    const isBinary = isBinaryExtension(file.path);
    const isRenaming = renamingFile?.path === file.path;

    return (
      <div
        key={file.path}
        className={`file-item ${isActive ? 'active' : ''} ${isBinary ? 'binary' : ''}`}
        onClick={() => !isRenaming && onSelectFile(file)}
        onContextMenu={(e) => handleContextMenu(e, file)}
        title={file.path}
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

  // Render directory group
  const renderDirectory = (dir: string, dirFiles: FileEntry[]) => {
    if (dir === '') {
      // Root files
      return dirFiles.map(renderFileItem);
    }

    return (
      <div key={dir} className="directory-group">
        <div className="directory-header">
          <span className="directory-icon">📁</span>
          <span className="directory-name">{dir}/</span>
        </div>
        <div className="directory-files">
          {dirFiles.map(renderFileItem)}
        </div>
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
        <span className="sidebar-title">Files</span>
        <button className="new-file-btn" onClick={onNewFile} title="New file">
          +
        </button>
      </div>

      <div className="file-list">
        {files.length === 0 ? (
          <div className="empty-state">
            <p>No files yet</p>
            <p className="hint">Drop files here or click + to create</p>
          </div>
        ) : (
          Array.from(fileGroups.entries())
            .sort(([a], [b]) => a.localeCompare(b))
            .map(([dir, dirFiles]) => renderDirectory(dir, dirFiles))
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
