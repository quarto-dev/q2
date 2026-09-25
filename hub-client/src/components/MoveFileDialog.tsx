/**
 * MoveFileDialog — pick a destination folder for an existing file. The
 * file keeps its name; confirming calls `onMove` with the new full path.
 */

import { useCallback, useState } from 'react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { normalizeProjectPath } from '@quarto/preview-renderer/types/project';
import ModalDialog from './ModalDialog';
import FolderPicker from './FolderPicker';
import { common, dialogs } from '../strings';
import './NewFileDialog.css';

export interface MoveFileDialogProps {
  /** File being moved; null = dialog closed. */
  file: FileEntry | null;
  /** Every folder in the project (explicit and file-derived). */
  folders: string[];
  existingPaths: string[];
  onClose: () => void;
  onMove: (file: FileEntry, newPath: string) => void;
}

export default function MoveFileDialog({ file, ...rest }: MoveFileDialogProps) {
  // Mount the form fresh per open so its state starts from the file's
  // current folder without reset effects.
  if (!file) return null;
  return <MoveFileForm file={file} {...rest} />;
}

function MoveFileForm({
  file,
  folders,
  existingPaths,
  onClose,
  onMove,
}: Omit<MoveFileDialogProps, 'file'> & { file: FileEntry }) {
  const lastSlash = file.path.lastIndexOf('/');
  const currentFolder = lastSlash >= 0 ? file.path.slice(0, lastSlash) : '';
  const name = lastSlash >= 0 ? file.path.slice(lastSlash + 1) : file.path;

  const [folder, setFolder] = useState(currentFolder);
  const [error, setError] = useState<string | null>(null);

  const newPath = normalizeProjectPath(folder ? `${folder}/${name}` : name);
  const unchanged = newPath === file.path;

  const handleMove = useCallback(() => {
    if (unchanged) {
      onClose();
      return;
    }
    if (existingPaths.includes(newPath)) {
      setError(dialogs.moveFile.errorExists);
      return;
    }
    onMove(file, newPath);
    onClose();
  }, [unchanged, existingPaths, newPath, onMove, file, onClose]);

  // Enter submits; Escape and Tab containment are owned by ModalDialog.
  // See NewFileDialog for why the button check and preventDefault matter.
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        if (e.target instanceof HTMLButtonElement) return;
        e.preventDefault();
        handleMove();
      }
    },
    [handleMove]
  );

  return (
    <ModalDialog
      title={dialogs.moveFile.title(name)}
      className="new-file-dialog"
      onClose={onClose}
      onKeyDown={handleKeyDown}
    >
      <div className="dialog-content">
        <div className="text-file-form">
          <div className="folder-input">
            <label htmlFor="move-file-folder">{dialogs.moveFile.folderLabel}</label>
            <FolderPicker
              id="move-file-folder"
              folders={folders}
              value={folder}
              onChange={(f) => {
                setFolder(f);
                setError(null);
              }}
            />
          </div>
          {error && <div className="qh-error inline">{error}</div>}
        </div>
      </div>

      <div className="dialog-actions">
        <button className="qh-btn outline" onClick={onClose}>
          {common.cancel}
        </button>
        <button className="qh-btn primary" onClick={handleMove} disabled={unchanged}>
          {dialogs.moveFile.move}
        </button>
      </div>
    </ModalDialog>
  );
}
