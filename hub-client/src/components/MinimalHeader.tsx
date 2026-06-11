/**
 * Minimal Header Component
 *
 * Slim header bar. Left: switch/share actions + project / file identity.
 * Right: online status, layout toggle, fullscreen-preview action.
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

/** Grid of four squares — "switch / all projects". */
function SwitchIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="3" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </svg>
  );
}

/** Connected nodes — "share". */
function ShareIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="18" cy="5" r="3" />
      <circle cx="6" cy="12" r="3" />
      <circle cx="18" cy="19" r="3" />
      <line x1="8.59" y1="13.51" x2="15.42" y2="17.49" />
      <line x1="15.41" y1="6.51" x2="8.59" y2="10.49" />
    </svg>
  );
}

/** Outward corners — "fullscreen preview". */
function PreviewIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M8 3H5a2 2 0 0 0-2 2v3" />
      <path d="M21 8V5a2 2 0 0 0-2-2h-3" />
      <path d="M3 16v3a2 2 0 0 0 2 2h3" />
      <path d="M16 21h3a2 2 0 0 0 2-2v-3" />
    </svg>
  );
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
          <span className={`file-path${currentFilePath ? '' : ' empty'}`}>
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
