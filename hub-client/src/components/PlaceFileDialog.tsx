/**
 * PlaceFileDialog — choose a folder and name for a file: either an
 * existing project file being moved, or an incoming browser `File` being
 * added. Opened from the file menu's "Move…", from a drag (internal move
 * or external drop) whose destination already holds a same-named file,
 * and from an inline rename that collides with an existing file.
 *
 * A collision is reported the moment it exists; the name field is the
 * way out. One dialog, one code path, for every "where does this file
 * go" question.
 */

import { useCallback, useMemo, useState } from 'react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import ModalDialog from './ModalDialog';
import FolderPicker from './FolderPicker';
import { common, dialogs } from '../strings';
import { joinPath } from '../utils/uniquePath';
import './NewFileDialog.css';

export type PlaceRequest =
  | {
      kind: 'move';
      file: FileEntry;
      /** Folder to start with (default: the file's current folder). */
      folder?: string;
      /** Name to start with (default: the file's current name). */
      name?: string;
    }
  | {
      kind: 'add';
      file: File;
      /** Folder to start with. */
      folder: string;
      /** Name to start with (default: the browser file's name). */
      name?: string;
    };

export interface PlaceFileDialogProps {
  /** What to place; null = dialog closed. */
  request: PlaceRequest | null;
  /** Every folder in the project (explicit and file-derived). */
  folders: string[];
  existingPaths: string[];
  onClose: () => void;
  /** Perform the move or add to `newPath`. */
  onConfirm: (request: PlaceRequest, newPath: string) => void;
}

export default function PlaceFileDialog({ request, ...rest }: PlaceFileDialogProps) {
  // Mount the form fresh per open so its state starts from the request
  // without reset effects.
  if (!request) return null;
  const key = request.kind === 'move' ? `move:${request.file.path}` : `add:${request.file.name}`;
  return <PlaceFileForm key={key} request={request} {...rest} />;
}

function PlaceFileForm({
  request,
  folders,
  existingPaths,
  onClose,
  onConfirm,
}: Omit<PlaceFileDialogProps, 'request'> & { request: PlaceRequest }) {
  const isMove = request.kind === 'move';
  const currentPath = isMove ? request.file.path : null;
  const lastSlash = currentPath?.lastIndexOf('/') ?? -1;
  const currentFolder = currentPath && lastSlash >= 0 ? currentPath.slice(0, lastSlash) : '';
  const currentName = isMove
    ? lastSlash >= 0
      ? request.file.path.slice(lastSlash + 1)
      : request.file.path
    : request.file.name;

  const [folder, setFolder] = useState(request.folder ?? currentFolder);
  const [name, setName] = useState(request.name ?? currentName);

  const taken = useMemo(() => new Set(existingPaths), [existingPaths]);
  const trimmedName = name.trim();
  const newPath = trimmedName ? joinPath(folder, trimmedName) : '';
  const unchanged = isMove && newPath === currentPath;
  const conflict = !!newPath && !unchanged && taken.has(newPath);
  const canConfirm = !!newPath && !unchanged && !conflict;

  const handleMove = useCallback(() => {
    if (!canConfirm) return;
    onConfirm(request, newPath);
    onClose();
  }, [canConfirm, onConfirm, request, newPath, onClose]);

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
      title={isMove ? dialogs.moveFile.title(currentName) : dialogs.moveFile.addTitle(currentName)}
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
              onChange={setFolder}
            />
          </div>
          <div className="filename-input">
            <label htmlFor="move-file-name">{dialogs.moveFile.nameLabel}</label>
            <input
              id="move-file-name"
              type="text"
              className="qh-input focus-accent"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          {conflict && <div className="qh-error inline">{dialogs.moveFile.errorExists}</div>}
        </div>
      </div>

      <div className="dialog-actions">
        <button className="qh-btn outline" onClick={onClose}>
          {common.cancel}
        </button>
        <button className="qh-btn primary" onClick={handleMove} disabled={!canConfirm}>
          {isMove ? dialogs.moveFile.move : dialogs.moveFile.add}
        </button>
      </div>
    </ModalDialog>
  );
}
