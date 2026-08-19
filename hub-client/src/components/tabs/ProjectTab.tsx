/**
 * Project Tab Component
 *
 * Displays project information:
 * - Project name
 * - Index document ID (click to copy automerge URL)
 * - "Export ZIP" button to download all project files
 * - "Screenshot Preview" button to capture the preview pane as a PNG
 *
 * Project switching lives in the MinimalHeader switch-project button, not
 * here.
 */

import { useState, useCallback } from 'react';
import html2canvas from 'html2canvas';
import { projectFolderName } from '@quarto/quarto-sync-client';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import './ProjectTab.css';

interface ProjectTabProps {
  project: ProjectEntry;
  /**
   * Produce the project ZIP, nesting every entry under `rootDir` (the
   * project-name folder). Callers should pass the same value used for the
   * download filename stem so the folder and filename stay in lock-step.
   */
  onExportZip: (rootDir: string) => Uint8Array;
}

export default function ProjectTab({ project, onExportZip }: ProjectTabProps) {
  const [copied, setCopied] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [isCapturing, setIsCapturing] = useState(false);

  const handleCopyDocId = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(project.indexDocId);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }, [project.indexDocId]);

  const handleExportZip = useCallback(() => {
    setExporting(true);
    setExportError(null);
    try {
      // One slug drives both the in-archive top-level folder and the download
      // filename stem, so they can never drift (GH #147).
      const folderName = projectFolderName(project.description);
      const zipBytes = onExportZip(folderName);
      const blob = new Blob([zipBytes.buffer as ArrayBuffer], { type: 'application/zip' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `${folderName}.zip`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Export failed';
      setExportError(message);
      setTimeout(() => setExportError(null), 5000);
    } finally {
      setExporting(false);
    }
  }, [onExportZip, project.description]);

  const handleScreenshot = useCallback(async () => {
    try {
      setIsCapturing(true);

      // Find the preview pane element
      const previewPane = document.querySelector('.preview-pane') as HTMLElement;

      if (!previewPane) {
        alert('Preview pane not found');
        return;
      }

      // Capture the preview pane using html2canvas
      const canvas = await html2canvas(previewPane, {
        backgroundColor: '#ffffff',
        useCORS: true,
        logging: false,
      });

      // Convert canvas to blob and download
      canvas.toBlob((blob) => {
        if (blob) {
          const url = URL.createObjectURL(blob);
          const link = document.createElement('a');
          link.href = url;
          link.download = `preview-screenshot-${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.png`;
          document.body.appendChild(link);
          link.click();
          document.body.removeChild(link);
          URL.revokeObjectURL(url);
        }
      }, 'image/png');
    } catch (error) {
      console.error('Failed to capture screenshot:', error);
      alert('Failed to capture screenshot. Please try again.');
    } finally {
      setIsCapturing(false);
    }
  }, []);

  // Display truncated doc ID (remove automerge: prefix, show first 8 chars)
  const truncatedDocId = project.indexDocId.replace(/^automerge:/, '').slice(0, 12) + '...';

  return (
    <div className="project-tab">
      <div className="project-tab-section">
        <label className="section-label">Project Name</label>
        <div className="project-name">{project.description}</div>
      </div>

      <div className="project-tab-section">
        <label className="section-label">Index Document ID</label>
        <button
          className="doc-id-button"
          onClick={handleCopyDocId}
          title={`Click to copy: ${project.indexDocId}`}
        >
          <span className="doc-id-value">{truncatedDocId}</span>
          <span className="copy-indicator">{copied ? 'Copied!' : 'Copy'}</span>
        </button>
      </div>

      <div className="project-tab-section">
        <label className="section-label">Sync Server</label>
        <div className="sync-server">{project.syncServer}</div>
      </div>

      <div className="project-tab-actions">
        <button
          className="export-zip-btn"
          onClick={handleExportZip}
          disabled={exporting}
        >
          {exporting ? 'Exporting...' : 'Export ZIP'}
        </button>
        {exportError && (
          <div className="export-error">{exportError}</div>
        )}
        <button
          className="screenshot-btn"
          onClick={handleScreenshot}
          disabled={isCapturing}
        >
          {isCapturing ? 'Capturing...' : '📸 Screenshot Preview'}
        </button>
      </div>
    </div>
  );
}
