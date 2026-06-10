/**
 * Minimal Header Component
 *
 * A slim header bar that displays the current file path on the left
 * and project name with navigation on the right.
 */

import ViewToggleControl from './ViewToggleControl';
import './MinimalHeader.css';

interface MinimalHeaderProps {
  currentFilePath: string | null;
  projectName: string;
  onChooseNewProject: () => void;
  /** Called when user wants to share the project */
  onShare?: () => void;
  onToggleFullscreenPreview?: () => void;
  isFullscreenPreview?: boolean;
  /** Whether the project is connected to the sync server */
  isOnline?: boolean;
}

export default function MinimalHeader({
  currentFilePath,
  projectName,
  onChooseNewProject,
  onShare,
  onToggleFullscreenPreview,
  isFullscreenPreview = false,
  isOnline = true,
}: MinimalHeaderProps) {
  return (
    <header className="minimal-header">
      <div className="header-left">
        <ViewToggleControl />
        {currentFilePath ? (
          <span className="file-path">{currentFilePath}</span>
        ) : (
          <span className="file-path empty">No file selected</span>
        )}
      </div>
      <div className="header-right">
        <div
          className={`connection-indicator ${isOnline ? 'online' : 'offline'}`}
          title={
            isOnline
              ? 'Online'
              : 'Working offline. Changes are saved locally and will sync when connection is restored.'
          }
        >
          <span className="connection-dot" />
          <span className="connection-text">{isOnline ? 'Online' : 'Offline'}</span>
        </div>
        <span className="project-name">{projectName}</span>
        {onShare && (
          <button className="share-btn" onClick={onShare} title="Share this project">
            Share
          </button>
        )}
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
