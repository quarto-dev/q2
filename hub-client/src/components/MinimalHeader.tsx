/**
 * Minimal Header Component
 *
 * Slim header bar. Left: switch/share actions + project / file identity.
 * Right: online status, layout toggle, fullscreen-preview action.
 */

import ViewToggleControl from './ViewToggleControl';
import { SwitchIcon, ShareIcon, PreviewIcon } from './icons';
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
        <button
          className="icon-btn"
          onClick={onChooseNewProject}
          title="Switch project"
          aria-label="Switch project"
        >
          <SwitchIcon />
        </button>
        {onShare && (
          <button
            className="icon-btn"
            onClick={onShare}
            title="Share this project"
            aria-label="Share this project"
          >
            <ShareIcon />
          </button>
        )}
        <span className="header-divider" aria-hidden="true" />
        <div className="header-doc">
          <span className="project-name">{projectName}</span>
          <span className="path-sep" aria-hidden="true">
            |
          </span>
          <span className={`file-path qh-truncate${currentFilePath ? '' : ' empty'}`}>
            {currentFilePath ?? 'No file selected'}
          </span>
        </div>
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
        <ViewToggleControl />
        {onToggleFullscreenPreview && !isFullscreenPreview && (
          <button
            className="preview-btn"
            onClick={onToggleFullscreenPreview}
            title="Fullscreen preview"
            aria-label="Fullscreen preview"
          >
            <PreviewIcon />
            <span>Preview</span>
          </button>
        )}
      </div>
    </header>
  );
}
