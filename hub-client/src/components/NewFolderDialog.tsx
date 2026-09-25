/**
 * NewFolderDialog — name an empty folder to create under `parent`.
 *
 * Mirrors NewFileDialog's shape (ModalDialog chrome, Enter submits, inline
 * validation). The name may contain slashes to create several levels at
 * once; leading/trailing slashes are stripped.
 */

import { useState, useCallback, useEffect, useRef } from 'react';
import ModalDialog from './ModalDialog';
import { normalizeProjectPath } from '@quarto/preview-renderer/types/project';
import { common, dialogs } from '../strings';
import './NewFileDialog.css';

export interface NewFolderDialogProps {
  isOpen: boolean;
  /** Folder to create inside ('' = project root). */
  parent: string;
  /** Existing folder paths (explicit and file-derived), for collision checks. */
  existingFolders: string[];
  existingPaths: string[];
  onClose: () => void;
  onCreateFolder: (path: string) => void;
}

export default function NewFolderDialog({
  isOpen,
  parent,
  existingFolders,
  existingPaths,
  onClose,
  onCreateFolder,
}: NewFolderDialogProps) {
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) {
      setTimeout(() => inputRef.current?.focus(), 100);
    }
  }, [isOpen]);

  // Reset the form as part of closing (Cancel, Escape, or after Create).
  const close = useCallback(() => {
    setName('');
    setError(null);
    onClose();
  }, [onClose]);

  const fullPath = useCallback(
    (raw: string) => {
      const trimmed = normalizeProjectPath(raw);
      return normalizeProjectPath(parent && trimmed ? `${parent}/${trimmed}` : trimmed);
    },
    [parent]
  );

  const handleCreate = useCallback(() => {
    const path = fullPath(name);
    if (!path) {
      setError(dialogs.newFolder.errorRequired);
      return;
    }
    if (/[<>:"|?*\\]/.test(path) || path.split('/').includes('..')) {
      setError(dialogs.newFolder.errorInvalidChars);
      return;
    }
    if (existingFolders.includes(path)) {
      setError(dialogs.newFolder.errorExists);
      return;
    }
    if (existingPaths.includes(path)) {
      setError(dialogs.newFolder.errorFileExists);
      return;
    }
    onCreateFolder(path);
    close();
  }, [name, fullPath, existingFolders, existingPaths, onCreateFolder, close]);

  // Enter submits; Escape and Tab containment are owned by ModalDialog.
  // See NewFileDialog for why the button check and preventDefault matter.
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        if (e.target instanceof HTMLButtonElement) return;
        e.preventDefault();
        handleCreate();
      }
    },
    [handleCreate]
  );

  if (!isOpen) return null;

  return (
    <ModalDialog
      title={dialogs.newFolder.title}
      className="new-file-dialog"
      onClose={close}
      onKeyDown={handleKeyDown}
    >
      <div className="dialog-content">
        <div className="text-file-form">
          <div className="filename-input">
            <label htmlFor="folder-name">{dialogs.newFolder.nameLabel(parent)}</label>
            <input
              ref={inputRef}
              id="folder-name"
              type="text"
              className="qh-input focus-accent"
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                setError(null);
              }}
              placeholder={dialogs.newFolder.namePlaceholder}
            />
          </div>
          {error && <div className="qh-error inline">{error}</div>}
        </div>
      </div>

      <div className="dialog-actions">
        <button className="qh-btn outline" onClick={close}>
          {common.cancel}
        </button>
        <button
          className="qh-btn primary"
          onClick={handleCreate}
          disabled={!name.trim()}
        >
          {common.create}
        </button>
      </div>
    </ModalDialog>
  );
}
