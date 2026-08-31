/**
 * New File Dialog
 *
 * Modal dialog for creating a new text file the user will edit in Monaco.
 * Supports filename input and optional starter template.
 *
 * Binary asset uploads go through `NewAssetDialog` (a sibling component).
 */

import { useState, useCallback, useRef, useEffect } from 'react';
import { discoverTemplates, type ProjectTemplate } from '../services/templateService';
import ModalDialog from './ModalDialog';
import { common, dialogs } from '../strings';
import './NewFileDialog.css';

export interface NewFileDialogProps {
  isOpen: boolean;
  existingPaths: string[];
  onClose: () => void;
  onCreateTextFile: (path: string, content: string) => void;
  /** Optional initial filename (e.g., from clicking a link to a non-existent file) */
  initialFilename?: string;
}

export default function NewFileDialog({
  isOpen,
  existingPaths,
  onClose,
  onCreateTextFile,
  initialFilename,
}: NewFileDialogProps) {
  const [filename, setFilename] = useState('');
  const [error, setError] = useState<string | null>(null);

  // Template state
  const [templates, setTemplates] = useState<ProjectTemplate[]>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<ProjectTemplate | null>(null);
  const [loadingTemplates, setLoadingTemplates] = useState(false);

  const filenameInputRef = useRef<HTMLInputElement>(null);

  // Seed the filename input on open.
  useEffect(() => {
    if (isOpen && initialFilename) {
      setFilename(initialFilename);
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
      setError(null);
      setTemplates([]);
      setSelectedTemplate(null);
      setLoadingTemplates(false);
    }
  }, [isOpen]);

  const validateFilename = useCallback(
    (name: string): string | null => {
      if (!name.trim()) {
        return dialogs.newFile.errorRequired;
      }
      if (/[<>:"|?*\\]/.test(name)) {
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
    const validationError = validateFilename(filename);
    if (validationError) {
      setError(validationError);
      return;
    }
    const content = selectedTemplate?.strippedContent ?? '';
    onCreateTextFile(filename, content);
    onClose();
  }, [filename, selectedTemplate, validateFilename, onCreateTextFile, onClose]);

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
