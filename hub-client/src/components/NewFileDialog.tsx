/**
 * New File Dialog Component
 *
 * Modal dialog for creating new files or uploading images.
 * Supports:
 * - Text input for filename
 * - Drag-and-drop zone for images
 * - File browser button
 * - Validation
 */

import { useState, useCallback, useRef, useEffect } from 'react';
import { isBinaryExtension, isTextExtension } from '../types/project';
import { validateFileSize, FILE_SIZE_LIMITS } from '../services/resourceService';
import './NewFileDialog.css';

export interface NewFileDialogProps {
  isOpen: boolean;
  existingPaths: string[];
  onClose: () => void;
  onCreateTextFile: (path: string, content: string) => void;
  onUploadBinaryFile: (file: File) => void;
  /** Optional pre-filled files from drag-and-drop */
  initialFiles?: File[];
}

interface FilePreview {
  file: File;
  preview?: string; // Data URL for image preview
  error?: string;
}

export default function NewFileDialog({
  isOpen,
  existingPaths,
  onClose,
  onCreateTextFile,
  onUploadBinaryFile,
  initialFiles,
}: NewFileDialogProps) {
  const [mode, setMode] = useState<'text' | 'upload'>('text');
  const [filename, setFilename] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [isDragOver, setIsDragOver] = useState(false);
  const [filePreviews, setFilePreviews] = useState<FilePreview[]>([]);
  const [isUploading, setIsUploading] = useState(false);

  const filenameInputRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Process dropped/selected files (defined before effects that use it)
  const processFiles = useCallback((files: File[]) => {
    const previews: FilePreview[] = [];

    for (const file of files) {
      const preview: FilePreview = { file };

      // Validate file size
      const sizeValidation = validateFileSize(file.size);
      if (!sizeValidation.valid) {
        preview.error = sizeValidation.error;
        previews.push(preview);
        continue;
      }

      // Check if it's an allowed type
      if (!isBinaryExtension(file.name) && !isTextExtension(file.name)) {
        // Allow common binary types
        const ext = file.name.split('.').pop()?.toLowerCase();
        if (!ext) {
          preview.error = 'Unknown file type';
          previews.push(preview);
          continue;
        }
      }

      // Generate preview for images
      if (file.type.startsWith('image/')) {
        const reader = new FileReader();
        reader.onload = (e) => {
          setFilePreviews((prev) =>
            prev.map((p) =>
              p.file === file ? { ...p, preview: e.target?.result as string } : p
            )
          );
        };
        reader.readAsDataURL(file);
      }

      previews.push(preview);
    }

    setFilePreviews(previews);
  }, []);

  // Handle initial files from drag-and-drop
  useEffect(() => {
    if (isOpen && initialFiles && initialFiles.length > 0) {
      setMode('upload');
      processFiles(initialFiles);
    }
  }, [isOpen, initialFiles, processFiles]);

  // Focus filename input when dialog opens in text mode
  useEffect(() => {
    if (isOpen && mode === 'text') {
      setTimeout(() => filenameInputRef.current?.focus(), 100);
    }
  }, [isOpen, mode]);

  // Reset state when dialog closes
  useEffect(() => {
    if (!isOpen) {
      setFilename('');
      setError(null);
      setFilePreviews([]);
      setMode('text');
      setIsDragOver(false);
      setIsUploading(false);
    }
  }, [isOpen]);

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
        setMode('upload');
        processFiles(droppedFiles);
      }
    },
    [processFiles]
  );

  // File browser handler
  const handleFileSelect = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const selectedFiles = Array.from(e.target.files || []);
      if (selectedFiles.length > 0) {
        processFiles(selectedFiles);
      }
    },
    [processFiles]
  );

  // Validate filename
  const validateFilename = useCallback(
    (name: string): string | null => {
      if (!name.trim()) {
        return 'Filename is required';
      }

      // Check for invalid characters
      if (/[<>:"|?*\\]/.test(name)) {
        return 'Filename contains invalid characters';
      }

      // Check for existing file
      if (existingPaths.includes(name)) {
        return 'A file with this name already exists';
      }

      return null;
    },
    [existingPaths]
  );

  // Handle create text file
  const handleCreateTextFile = useCallback(() => {
    const validationError = validateFilename(filename);
    if (validationError) {
      setError(validationError);
      return;
    }

    onCreateTextFile(filename, '');
    onClose();
  }, [filename, validateFilename, onCreateTextFile, onClose]);

  // Handle upload files
  const handleUploadFiles = useCallback(async () => {
    const validFiles = filePreviews.filter((p) => !p.error);
    if (validFiles.length === 0) {
      setError('No valid files to upload');
      return;
    }

    setIsUploading(true);
    setError(null);

    try {
      for (const { file } of validFiles) {
        onUploadBinaryFile(file);
      }
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Upload failed');
    } finally {
      setIsUploading(false);
    }
  }, [filePreviews, onUploadBinaryFile, onClose]);

  // Remove a file from preview
  const removeFilePreview = useCallback((file: File) => {
    setFilePreviews((prev) => prev.filter((p) => p.file !== file));
  }, []);

  // Handle key press
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter' && mode === 'text') {
        handleCreateTextFile();
      } else if (e.key === 'Escape') {
        onClose();
      }
    },
    [mode, handleCreateTextFile, onClose]
  );

  if (!isOpen) return null;

  const maxMB = FILE_SIZE_LIMITS.MAX_FILE_SIZE / (1024 * 1024);

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <div
        className="new-file-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <div className="dialog-header">
          <h2>Add File</h2>
          <button className="close-btn" onClick={onClose}>
            &times;
          </button>
        </div>

        <div className="dialog-tabs">
          <button
            className={`tab ${mode === 'text' ? 'active' : ''}`}
            onClick={() => setMode('text')}
          >
            New Text File
          </button>
          <button
            className={`tab ${mode === 'upload' ? 'active' : ''}`}
            onClick={() => setMode('upload')}
          >
            Upload File
          </button>
        </div>

        <div className="dialog-content">
          {mode === 'text' ? (
            <div className="text-file-form">
              <label htmlFor="filename">Filename:</label>
              <input
                ref={filenameInputRef}
                id="filename"
                type="text"
                value={filename}
                onChange={(e) => {
                  setFilename(e.target.value);
                  setError(null);
                }}
                placeholder="e.g., chapter1.qmd"
              />
              {error && <div className="error-message">{error}</div>}
            </div>
          ) : (
            <div className="upload-form">
              <div className={`drop-zone ${isDragOver ? 'drag-over' : ''}`}>
                {filePreviews.length === 0 ? (
                  <>
                    <span className="drop-icon">📥</span>
                    <p>Drag & drop files here</p>
                    <p className="hint">or</p>
                    <button
                      className="browse-btn"
                      onClick={() => fileInputRef.current?.click()}
                    >
                      Browse Files
                    </button>
                    <p className="size-hint">Max file size: {maxMB}MB</p>
                  </>
                ) : (
                  <div className="file-previews">
                    {filePreviews.map(({ file, preview, error: fileError }) => (
                      <div
                        key={file.name}
                        className={`file-preview ${fileError ? 'has-error' : ''}`}
                      >
                        {preview ? (
                          <img src={preview} alt={file.name} />
                        ) : (
                          <span className="file-icon">📄</span>
                        )}
                        <div className="file-info">
                          <span className="file-name">{file.name}</span>
                          <span className="file-size">
                            {(file.size / 1024).toFixed(1)} KB
                          </span>
                          {fileError && (
                            <span className="file-error">{fileError}</span>
                          )}
                        </div>
                        <button
                          className="remove-btn"
                          onClick={() => removeFilePreview(file)}
                        >
                          &times;
                        </button>
                      </div>
                    ))}
                    <button
                      className="add-more-btn"
                      onClick={() => fileInputRef.current?.click()}
                    >
                      + Add more
                    </button>
                  </div>
                )}
              </div>
              <input
                ref={fileInputRef}
                type="file"
                multiple
                accept="image/*,.pdf,.svg"
                onChange={handleFileSelect}
                style={{ display: 'none' }}
              />
              {error && <div className="error-message">{error}</div>}
            </div>
          )}
        </div>

        <div className="dialog-actions">
          <button className="cancel-btn" onClick={onClose}>
            Cancel
          </button>
          {mode === 'text' ? (
            <button
              className="create-btn"
              onClick={handleCreateTextFile}
              disabled={!filename.trim()}
            >
              Create
            </button>
          ) : (
            <button
              className="upload-btn"
              onClick={handleUploadFiles}
              disabled={
                filePreviews.length === 0 ||
                filePreviews.every((p) => !!p.error) ||
                isUploading
              }
            >
              {isUploading ? 'Uploading...' : 'Upload'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
