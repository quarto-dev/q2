/**
 * Minimal Header Component
 *
 * Slim header bar. Left: switch/share actions + project / file identity.
 * Right: online status, layout toggle, fullscreen-preview action.
 */

import ViewToggleControl from './ViewToggleControl';
import { SwitchIcon, ShareIcon, PreviewIcon } from './icons';
import Tooltip from './Tooltip';
import { header } from '../strings';
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
        <Tooltip content={header.switchProject}>
          <button
            className="qh-icon-btn boxed"
            onClick={onChooseNewProject}
            aria-label={header.switchProject}
          >
            <SwitchIcon />
          </button>
        </Tooltip>
        {onShare && (
          <Tooltip content={header.shareProject}>
            <button
              className="qh-icon-btn boxed"
              onClick={onShare}
              aria-label={header.shareProject}
            >
              <ShareIcon />
            </button>
          </Tooltip>
        )}
        <span className="header-divider" aria-hidden="true" />
        <div className="header-doc">
          <span className="project-name">{projectName}</span>
          <span className="path-sep" aria-hidden="true">
            |
          </span>
          <span className={`file-path qh-truncate${currentFilePath ? '' : ' empty'}`}>
            {currentFilePath ?? header.noFileSelected}
          </span>
        </div>
      </div>
      <div className="header-right">
        <Tooltip
          content={
            isOnline ? header.connectedTooltip : header.offlineTooltip
          }
        >
          <div
            className={`connection-indicator ${isOnline ? 'online' : 'offline'}`}
            tabIndex={0}
          >
            <span className="connection-dot" aria-hidden="true" />
            <span className="connection-text">{isOnline ? header.online : header.offline}</span>
          </div>
        </Tooltip>
        <ViewToggleControl />
        {onToggleFullscreenPreview && !isFullscreenPreview && (
          <Tooltip content={header.fullscreenPreview}>
            <button
              className="preview-btn"
              onClick={onToggleFullscreenPreview}
              aria-label={header.fullscreenPreview}
            >
              <PreviewIcon />
              <span>{header.preview}</span>
            </button>
          </Tooltip>
        )}
      </div>
    </header>
  );
}
