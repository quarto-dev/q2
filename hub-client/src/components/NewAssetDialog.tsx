/**
 * New Asset Dialog
 *
 * Ingests one or more opaque binary assets (images, PDFs, `.wasm` tree-sitter
 * grammars, data files, fonts, etc.) into the project at a user-chosen
 * destination folder. Unlike `NewFileDialog`, this dialog does not create
 * files the editor will open — the editor treats these assets as opaque.
 *
 * Entry points: "+" button in FileSidebar, sidebar drops, editor drops.
 */

import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import { sanitizeFilename, FILE_SIZE_LIMITS } from '../services/resourceService';
import {
  processAssetFiles,
  validateProjectPath,
  type AssetFilePreview,
} from './fileUpload';
import './NewAssetDialog.css';

export interface NewAssetDialogProps {
  isOpen: boolean;
  existingPaths: string[];
  /** Destination folder to seed the input with. Empty string = project root. */
  defaultDestination: string;
  onClose: () => void;
  /**
   * Called once per valid file on confirm. `targetPath` is the composed
   * `<destination>/<filename>` (no leading slash, validated).
   */
  onUploadAsset: (file: File, targetPath: string) => void | Promise<void>;
  /** Optional: pre-populate previews from a drag-drop event. */
  initialFiles?: File[];
}

interface PreviewEntry extends AssetFilePreview {
  /** Data URL for image preview (async-loaded). */
  previewUrl?: string;
}

export default function NewAssetDialog({
  isOpen,
  existingPaths,
  defaultDestination,
  onClose,
  onUploadAsset,
  initialFiles,
}: NewAssetDialogProps) {
  const [destination, setDestination] = useState(defaultDestination);
  const [previews, setPreviews] = useState<PreviewEntry[]>([]);
  const [editedNames, setEditedNames] = useState<Map<File, string>>(new Map());
  const [isDragOver, setIsDragOver] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const fileInputRef = useRef<HTMLInputElement>(null);

  // Reset when dialog opens/closes.
  useEffect(() => {
    if (isOpen) {
      setDestination(defaultDestination);
      setSubmitError(null);
      setIsUploading(false);
      setIsDragOver(false);
      if (initialFiles && initialFiles.length > 0) {
        ingestFiles(initialFiles);
      } else {
        setPreviews([]);
        setEditedNames(new Map());
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, initialFiles, defaultDestination]);

  const ingestFiles = useCallback((files: File[]) => {
    const entries = processAssetFiles(files);
    const nextPreviews: PreviewEntry[] = entries;
    setPreviews((prev) => [...prev, ...nextPreviews]);
    setEditedNames((prev) => {
      const next = new Map(prev);
      for (const { file } of entries) {
        if (!next.has(file)) {
          next.set(file, sanitizeFilename(file.name));
        }
      }
      return next;
    });
    // Load image previews asynchronously.
    for (const { file } of entries) {
      if (file.type.startsWith('image/') && file.size > 0) {
        const reader = new FileReader();
        reader.onload = (e) => {
          const url = e.target?.result as string;
          setPreviews((prev) =>
            prev.map((p) => (p.file === file ? { ...p, previewUrl: url } : p))
          );
        };
        reader.readAsDataURL(file);
      }
    }
  }, []);

  const destinationError = useMemo(
    () => validateProjectPath(destination),
    [destination]
  );

  /** Compose the final path for a file, using its edited name. */
  const composePath = useCallback(
    (file: File): string => {
      const name = editedNames.get(file) ?? file.name;
      return destination === '' ? name : `${destination}/${name}`;
    },
    [destination, editedNames]
  );

  /**
   * Validate a single file's final path:
   * - path-level rules (validateProjectPath)
   * - collision against existing project paths
   * - collision against sibling entries in this batch
   */
  const validateEntry = useCallback(
    (file: File, preview: PreviewEntry): string | null => {
      if (preview.error) return preview.error;
      const path = composePath(file);
      const pathErr = validateProjectPath(path);
      if (pathErr) return pathErr;
      if (existingPaths.includes(path)) {
        return `"${path}" already exists in the project`;
      }
      for (const other of previews) {
        if (other.file === file) continue;
        if (other.error) continue;
        if (composePath(other.file) === path) {
          return 'Duplicate path with another file in this batch';
        }
      }
      return null;
    },
    [composePath, existingPaths, previews]
  );

  const entryErrors = useMemo(() => {
    const map = new Map<File, string | null>();
    for (const p of previews) {
      map.set(p.file, validateEntry(p.file, p));
    }
    return map;
  }, [previews, validateEntry]);

  const canUpload =
    previews.length > 0 &&
    !destinationError &&
    previews.every((p) => !p.error && !entryErrors.get(p.file)) &&
    !isUploading;

  const removePreview = useCallback((file: File) => {
    setPreviews((prev) => prev.filter((p) => p.file !== file));
    setEditedNames((prev) => {
      const next = new Map(prev);
      next.delete(file);
      return next;
    });
  }, []);

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
      const files = Array.from(e.dataTransfer.files);
      if (files.length > 0) ingestFiles(files);
    },
    [ingestFiles]
  );

  const handleFileSelect = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(e.target.files ?? []);
      if (files.length > 0) ingestFiles(files);
      // reset so selecting the same file again re-triggers change
      e.target.value = '';
    },
    [ingestFiles]
  );

  const handleUpload = useCallback(async () => {
    if (!canUpload) return;
    setIsUploading(true);
    setSubmitError(null);
    try {
      for (const p of previews) {
        if (p.error) continue;
        const err = entryErrors.get(p.file);
        if (err) throw new Error(err);
        await onUploadAsset(p.file, composePath(p.file));
      }
      onClose();
    } catch (err) {
      setSubmitError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsUploading(false);
    }
  }, [canUpload, previews, entryErrors, composePath, onUploadAsset, onClose]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    },
    [onClose]
  );

  if (!isOpen) return null;

  const maxMB = FILE_SIZE_LIMITS.MAX_FILE_SIZE / (1024 * 1024);

  return (
    <div className="ph-dialog-backdrop" onClick={onClose}>
      <div
        className="ph-dialog new-asset-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <div className="dialog-header">
          <h2>Add asset to project</h2>
          <button className="close-btn" onClick={onClose} aria-label="Close">
            &times;
          </button>
        </div>

        <div className="dialog-content">
          <div className="destination-input">
            <label htmlFor="asset-destination">Destination folder:</label>
            <input
              id="asset-destination"
              type="text"
              className="ph-input focus-accent"
              value={destination}
              placeholder="(project root)"
              onChange={(e) => setDestination(e.target.value)}
            />
            {destinationError && (
              <div className="ph-error inline">{destinationError}</div>
            )}
          </div>

          <div className={`drop-zone ${isDragOver ? 'drag-over' : ''}`}>
            {previews.length === 0 ? (
              <>
                <span className="drop-icon">📥</span>
                <p>Drag &amp; drop files here</p>
                <p className="hint">or</p>
                <button
                  className="ph-btn primary browse-btn"
                  onClick={() => fileInputRef.current?.click()}
                >
                  Browse Files
                </button>
                <p className="size-hint">Max file size: {maxMB}MB</p>
              </>
            ) : (
              <div className="file-previews">
                {previews.map((p) => {
                  const editedName = editedNames.get(p.file) ?? p.file.name;
                  const entryErr = entryErrors.get(p.file);
                  const hasError = !!(p.error || entryErr);
                  return (
                    <div
                      key={`${p.file.name}-${p.file.size}`}
                      className={`file-preview ${hasError ? 'has-error' : ''}`}
                    >
                      {p.previewUrl ? (
                        <img src={p.previewUrl} alt={editedName} />
                      ) : (
                        <span className="file-icon">📄</span>
                      )}
                      <div className="file-info">
                        <input
                          className="file-name-input"
                          type="text"
                          value={editedName}
                          onChange={(e) => {
                            const v = e.target.value;
                            setEditedNames((prev) => {
                              const next = new Map(prev);
                              next.set(p.file, v);
                              return next;
                            });
                          }}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter' || e.key === 'Escape') {
                              e.stopPropagation();
                              (e.target as HTMLInputElement).blur();
                            }
                          }}
                          disabled={!!p.error}
                        />
                        <span className="file-size">
                          {(p.file.size / 1024).toFixed(1)} KB
                        </span>
                        {p.error && <span className="file-error">{p.error}</span>}
                        {!p.error && entryErr && (
                          <span className="file-error">{entryErr}</span>
                        )}
                      </div>
                      <button
                        className="remove-btn"
                        aria-label={`Remove ${editedName}`}
                        onClick={() => removePreview(p.file)}
                      >
                        &times;
                      </button>
                    </div>
                  );
                })}
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
            onChange={handleFileSelect}
            style={{ display: 'none' }}
          />
          {submitError && <div className="ph-error inline">{submitError}</div>}
        </div>

        <div className="dialog-actions">
          <button className="ph-btn outline" onClick={onClose}>
            Cancel
          </button>
          <button
            className="ph-btn primary"
            onClick={handleUpload}
            disabled={!canUpload}
          >
            {isUploading ? 'Uploading...' : 'Upload'}
          </button>
        </div>
      </div>
    </div>
  );
}
