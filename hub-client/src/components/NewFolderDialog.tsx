/**
 * NewFolderDialog — name an empty folder to create under `parent`.
 *
 * Mirrors NewFileDialog's shape (ModalDialog chrome, Enter submits, inline
 * validation). The name may contain slashes to create several levels at
 * once; leading/trailing slashes are stripped.
 */

import { useState, useCallback, useEffect, useRef } from 'react';
import ModalDialog from './ModalDialog';
import FolderPicker from './FolderPicker';
import { normalizeProjectPath } from '@quarto/preview-renderer/types/project';
import { common, dialogs } from '../strings';
import './NewFileDialog.css';

export interface NewFolderDialogProps {
  isOpen: boolean;
  /** Initial folder to create inside ('' = project root); editable in the dialog. */
  parent: string;
  /** Existing folder paths (explicit and file-derived), for collision checks. */
  existingFolders: string[];
  existingPaths: string[];
  onClose: () => void;
  onCreateFolder: (path: string) => void;
}

export default function NewFolderDialog({ isOpen, ...rest }: NewFolderDialogProps) {
  // The form mounts fresh on every open, so its state (including the
  // seeded parent folder) starts clean without any reset effects.
  if (!isOpen) return null;
  return <NewFolderForm {...rest} />;
}

function NewFolderForm({
  parent,
  existingFolders,
  existingPaths,
  onClose,
  onCreateFolder,
}: Omit<NewFolderDialogProps, 'isOpen'>) {
  const [name, setName] = useState('');
  const [parentFolder, setParentFolder] = useState(parent);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const t = setTimeout(() => inputRef.current?.focus(), 100);
    return () => clearTimeout(t);
  }, []);

  const close = onClose;

  const fullPath = useCallback(
    (raw: string) => {
      const trimmed = normalizeProjectPath(raw);
      return normalizeProjectPath(parentFolder && trimmed ? `${parentFolder}/${trimmed}` : trimmed);
    },
    [parentFolder]
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

  return (
    <ModalDialog
      title={dialogs.newFolder.title}
      className="new-file-dialog"
      onClose={close}
      onKeyDown={handleKeyDown}
    >
      <div className="dialog-content">
        <div className="text-file-form">
          <div className="folder-input">
            <label htmlFor="new-folder-parent">{dialogs.newFolder.parentLabel}</label>
            <FolderPicker
              id="new-folder-parent"
              folders={existingFolders}
              value={parentFolder}
              onChange={(f) => {
                setParentFolder(f);
                setError(null);
              }}
            />
          </div>
          <div className="filename-input">
            <label htmlFor="folder-name">{dialogs.newFolder.nameLabel}</label>
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
