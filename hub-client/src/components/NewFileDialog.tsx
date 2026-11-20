/**
 * New File Dialog
 *
 * Modal dialog for creating a new text file the user will edit in Monaco.
 * Supports filename input and optional starter template.
 *
 * Binary asset uploads go through `NewAssetDialog` (a sibling component).
 */

import { useState, useCallback, useEffect } from 'react';
import { normalizeProjectPath } from '@quarto/preview-renderer/types/project';
import { discoverTemplates, type ProjectTemplate } from '../services/templateService';
import ModalDialog from './ModalDialog';
import FolderPicker from './FolderPicker';
import FileTypePicker, { type FileTypeChoice } from './FileTypePicker';
import { common, dialogs } from '../strings';
import './NewFileDialog.css';

export interface NewFileDialogProps {
  isOpen: boolean;
  existingPaths: string[];
  /** Every folder in the project (explicit and file-derived), for the picker. */
  folders?: string[];
  onClose: () => void;
  onCreateTextFile: (path: string, content: string) => void;
  /**
   * Optional initial path (e.g., from clicking a link to a non-existent
   * file, or the current file's folder as `notes/`). A directory part
   * seeds the folder picker; the rest seeds the filename.
   */
  initialFilename?: string;
}

/** Map an extension (no dot, lowercase) to a picker choice; unknown → other. */
function choiceForExtension(ext: string): FileTypeChoice {
  return ext === 'qmd' || ext === 'md' || ext === 'yml' ? ext : 'other';
}

/**
 * Split `notes/intro.qmd` into folder `notes`, name `intro`, and
 * extension `qmd` (empty when the seed has none).
 */
function splitInitial(initial: string): { folder: string; name: string; ext: string } {
  const normalized = normalizeProjectPath(initial);
  const endsWithSlash = initial.trim().endsWith('/');
  if (endsWithSlash) return { folder: normalized, name: '', ext: '' };
  const lastSlash = normalized.lastIndexOf('/');
  const folder = lastSlash < 0 ? '' : normalized.slice(0, lastSlash);
  const base = lastSlash < 0 ? normalized : normalized.slice(lastSlash + 1);
  const lastDot = base.lastIndexOf('.');
  if (lastDot <= 0) return { folder, name: base, ext: '' };
  return { folder, name: base.slice(0, lastDot), ext: base.slice(lastDot + 1).toLowerCase() };
}

export default function NewFileDialog({
  isOpen,
  existingPaths,
  folders = [],
  onClose,
  onCreateTextFile,
  initialFilename,
}: NewFileDialogProps) {
  // Parents mount this dialog only while it is open, so every open is a
  // fresh mount: the form seeds once from initialFilename here, and no
  // reset-on-close effect is needed (bd-cp9wzu6o).
  const [seed] = useState(() =>
    initialFilename ? splitInitial(initialFilename) : { folder: '', name: '', ext: '' }
  );
  const seedChoice: FileTypeChoice = seed.ext ? choiceForExtension(seed.ext) : 'qmd';
  const [filename, setFilename] = useState(
    seedChoice === 'other' ? `${seed.name}.${seed.ext}` : seed.name
  );
  const [folder, setFolder] = useState(seed.folder);
  const [fileType, setFileType] = useState<FileTypeChoice>(seedChoice);
  const [error, setError] = useState<string | null>(null);

  // Template state
  const [templates, setTemplates] = useState<ProjectTemplate[]>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<ProjectTemplate | null>(null);
  const [loadingTemplates, setLoadingTemplates] = useState(false);


  // Load templates when the dialog opens (once per mount).
  useEffect(() => {
    if (isOpen) {
      setLoadingTemplates(true);
      discoverTemplates()
        .then((discovered) => {
          setTemplates(discovered);
        })
        .catch((err) => {
          console.warn('[NewFileDialog] Failed to load templates:', err);
          setTemplates([]);
        })
        .finally(() => {
          setLoadingTemplates(false);
        });
    }
  }, [isOpen]);

  const validateFilename = useCallback(
    (name: string): string | null => {
      if (!name) {
        return dialogs.newFile.errorRequired;
      }
      if (/[<>:"|?*\\]/.test(name) || name.split('/').includes('..')) {
        return dialogs.newFile.errorInvalidChars;
      }
      if (existingPaths.includes(name)) {
        return dialogs.newFile.errorExists;
      }
      return null;
    },
    [existingPaths]
  );

  const handleCreateTextFile = useCallback(() => {
    const base = filename.trim();
    if (!base) {
      setError(dialogs.newFile.errorRequired);
      return;
    }
    // "other" takes the name as typed. A specific type appends its
    // extension unless the user typed it into the name too.
    const withExt =
      fileType === 'other' || base.toLowerCase().endsWith(`.${fileType}`)
        ? base
        : `${base}.${fileType}`;
    const path = normalizeProjectPath(folder ? `${folder}/${withExt}` : withExt);
    const validationError = validateFilename(path);
    if (validationError) {
      setError(validationError);
      return;
    }
    const content = selectedTemplate?.strippedContent ?? '';
    onCreateTextFile(path, content);
    onClose();
  }, [folder, filename, fileType, selectedTemplate, validateFilename, onCreateTextFile, onClose]);

  // Enter submits; Escape and Tab containment are owned by ModalDialog.
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        // Keydowns from a focused button bubble here too; those belong to
        // the button's own activation (Cancel must not create a file, and
        // Create must not fire twice).
        if (e.target instanceof HTMLButtonElement) return;
        // Un-prevented, Enter's default action is a synthesized click on
        // whatever is focused after close — the focus-restored trigger
        // button — which reopens the dialog (GH #635).
        e.preventDefault();
        handleCreateTextFile();
      }
    },
    [handleCreateTextFile]
  );

  if (!isOpen) return null;

  return (
    <ModalDialog
      title={dialogs.newFile.title}
      className="new-file-dialog"
      onClose={onClose}
      onKeyDown={handleKeyDown}
    >
        <div className="dialog-content">
          <div className="text-file-form">
            {templates.length > 0 && (
              <div className="template-selector">
                <label htmlFor="template">{dialogs.newFile.templateLabel}</label>
                <select
                  id="template"
                  className="qh-input focus-accent"
                  value={selectedTemplate?.path ?? ''}
                  onChange={(e) => {
                    const template = templates.find((t) => t.path === e.target.value);
                    setSelectedTemplate(template ?? null);
                  }}
                  disabled={loadingTemplates}
                >
                  <option value="">{dialogs.newFile.blank}</option>
                  {templates.map((t) => (
                    <option key={t.path} value={t.path}>
                      {t.displayName}
                    </option>
                  ))}
                </select>
              </div>
            )}
            <div className="folder-input">
              <label htmlFor="new-file-folder">{dialogs.newFile.folderLabel}</label>
              <FolderPicker
                id="new-file-folder"
                folders={folders}
                value={folder}
                onChange={(f) => {
                  setFolder(f);
                  setError(null);
                }}
              />
            </div>
            <div className="file-type-input">
              <label id="new-file-type-label">{dialogs.newFile.typeLabel}</label>
              <FileTypePicker
                value={fileType}
                onChange={(t) => {
                  setFileType(t);
                  setError(null);
                }}
              />
            </div>
            <div className="filename-input">
              <label htmlFor="filename">{dialogs.newFile.filenameLabel}</label>
              <input
                // autoFocus, not a deferred focus(): a timer fires after the
                // user may already have moved focus (e.g. to Cancel) and
                // yanks it back, so Enter submits instead of cancelling.
                // ModalDialog captures its restore target during render,
                // before autoFocus runs.
                autoFocus
                id="filename"
                type="text"
                className="qh-input focus-accent"
                value={filename}
                onChange={(e) => {
                  setFilename(e.target.value);
                  setError(null);
                }}
                placeholder={
                  fileType === 'other'
                    ? dialogs.newFile.filenamePlaceholderOther
                    : dialogs.newFile.filenamePlaceholder
                }
              />
            </div>
            {error && <div className="qh-error inline">{error}</div>}
          </div>
        </div>

        <div className="dialog-actions">
          <button className="qh-btn outline" onClick={onClose}>
            {common.cancel}
          </button>
          <button
            className="qh-btn primary"
            onClick={handleCreateTextFile}
            disabled={!filename.trim()}
          >
            {common.create}
          </button>
        </div>
    </ModalDialog>
  );
}
