/**
 * New File Dialog
 *
 * Modal dialog for creating a new text file the user will edit in Monaco.
 * Supports filename input and optional starter template.
 *
 * Binary asset uploads go through `NewAssetDialog` (a sibling component).
 */

import { useState, useCallback, useRef, useEffect } from 'react';
import { normalizeProjectPath } from '@quarto/preview-renderer/types/project';
import { discoverTemplates, type ProjectTemplate } from '../services/templateService';
import ModalDialog from './ModalDialog';
import FolderPicker from './FolderPicker';
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

/** Split `notes/intro.qmd` into folder `notes` and name `intro.qmd`. */
function splitInitial(initial: string): { folder: string; name: string } {
  const normalized = normalizeProjectPath(initial);
  const endsWithSlash = initial.trim().endsWith('/');
  if (endsWithSlash) return { folder: normalized, name: '' };
  const lastSlash = normalized.lastIndexOf('/');
  if (lastSlash < 0) return { folder: '', name: normalized };
  return { folder: normalized.slice(0, lastSlash), name: normalized.slice(lastSlash + 1) };
}

export default function NewFileDialog({
  isOpen,
  existingPaths,
  folders = [],
  onClose,
  onCreateTextFile,
  initialFilename,
}: NewFileDialogProps) {
  const [filename, setFilename] = useState('');
  const [folder, setFolder] = useState('');
  const [error, setError] = useState<string | null>(null);

  // Template state
  const [templates, setTemplates] = useState<ProjectTemplate[]>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<ProjectTemplate | null>(null);
  const [loadingTemplates, setLoadingTemplates] = useState(false);

  const filenameInputRef = useRef<HTMLInputElement>(null);

  // Seed the folder picker and filename input on open.
  useEffect(() => {
    if (isOpen && initialFilename) {
      const { folder: f, name } = splitInitial(initialFilename);
      setFolder(f);
      setFilename(name);
    }
  }, [isOpen, initialFilename]);

  // Load templates when the dialog opens.
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

  // Focus the filename input when the dialog opens.
  useEffect(() => {
    if (isOpen) {
      setTimeout(() => filenameInputRef.current?.focus(), 100);
    }
  }, [isOpen]);

  // Reset state when the dialog closes.
  useEffect(() => {
    if (!isOpen) {
      setFilename('');
      setFolder('');
      setError(null);
      setTemplates([]);
      setSelectedTemplate(null);
      setLoadingTemplates(false);
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
    const path = normalizeProjectPath(folder ? `${folder}/${filename}` : filename);
    const validationError = validateFilename(path);
    if (validationError) {
      setError(validationError);
      return;
    }
    const content = selectedTemplate?.strippedContent ?? '';
    onCreateTextFile(path, content);
    onClose();
  }, [folder, filename, selectedTemplate, validateFilename, onCreateTextFile, onClose]);

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
            <div className="filename-input">
              <label htmlFor="filename">{dialogs.newFile.filenameLabel}</label>
              <input
                ref={filenameInputRef}
                id="filename"
                type="text"
                className="qh-input focus-accent"
                value={filename}
                onChange={(e) => {
                  setFilename(e.target.value);
                  setError(null);
                }}
                placeholder={dialogs.newFile.filenamePlaceholder}
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
