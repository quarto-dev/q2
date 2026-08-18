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
        return 'Filename is required';
      }
      if (/[<>:"|?*\\]/.test(name)) {
        return 'Filename contains invalid characters';
      }
      if (existingPaths.includes(name)) {
        return 'A file with this name already exists';
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

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        handleCreateTextFile();
      } else if (e.key === 'Escape') {
        onClose();
      }
    },
    [handleCreateTextFile, onClose]
  );

  if (!isOpen) return null;

  return (
    <div className="ph-dialog-backdrop" onClick={onClose}>
      <div
        className="ph-dialog new-file-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="dialog-header">
          <h2>New file</h2>
          <button className="close-btn" onClick={onClose} aria-label="Close">
            &times;
          </button>
        </div>

        <div className="dialog-content">
          <div className="text-file-form">
            {templates.length > 0 && (
              <div className="template-selector">
                <label htmlFor="template">Template:</label>
                <select
                  id="template"
                  className="ph-input focus-accent"
                  value={selectedTemplate?.path ?? ''}
                  onChange={(e) => {
                    const template = templates.find((t) => t.path === e.target.value);
                    setSelectedTemplate(template ?? null);
                  }}
                  disabled={loadingTemplates}
                >
                  <option value="">Blank file</option>
                  {templates.map((t) => (
                    <option key={t.path} value={t.path}>
                      {t.displayName}
                    </option>
                  ))}
                </select>
              </div>
            )}
            <div className="filename-input">
              <label htmlFor="filename">Filename:</label>
              <input
                ref={filenameInputRef}
                id="filename"
                type="text"
                className="ph-input focus-accent"
                value={filename}
                onChange={(e) => {
                  setFilename(e.target.value);
                  setError(null);
                }}
                placeholder="e.g., chapter1.qmd"
              />
            </div>
            {error && <div className="ph-error inline">{error}</div>}
          </div>
        </div>

        <div className="dialog-actions">
          <button className="ph-btn outline" onClick={onClose}>
            Cancel
          </button>
          <button
            className="ph-btn primary"
            onClick={handleCreateTextFile}
            disabled={!filename.trim()}
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
