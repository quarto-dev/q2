/**
 * Minimal Header Component
 *
 * A slim header bar that displays the current file path on the left
 * and project name with navigation on the right.
 */

import './MinimalHeader.css';

interface MinimalHeaderProps {
  currentFilePath: string | null;
  projectName: string;
  onChooseNewProject: () => void;
  onToggleFullscreenPreview?: () => void;
  isFullscreenPreview?: boolean;
}

export default function MinimalHeader({
  currentFilePath,
  projectName,
  onChooseNewProject,
  onToggleFullscreenPreview,
  isFullscreenPreview = false,
}: MinimalHeaderProps) {
  return (
    <header className="minimal-header">
      <div className="header-left">
        {currentFilePath ? (
          <span className="file-path">{currentFilePath}</span>
        ) : (
          <span className="file-path empty">No file selected</span>
        )}
      </div>
      <div className="header-right">
        <span className="project-name">{projectName}</span>
        {onToggleFullscreenPreview && !isFullscreenPreview && (
          <button className="preview-btn" onClick={onToggleFullscreenPreview}>
            Preview
          </button>
        )}
        <button className="choose-project-btn" onClick={onChooseNewProject}>
          Switch
        </button>
      </div>
    </header>
  );
}
