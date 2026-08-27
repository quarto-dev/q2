/**
 * Minimal Header Component
 *
 * Slim header bar. Left: switch/share actions + project / file identity.
 * Right: online status, layout toggle, fullscreen-preview action.
 */

import { useState } from 'react';
import ViewToggleControl from './ViewToggleControl';
import { SwitchIcon, ShareIcon, PreviewIcon, PanelLeftIcon } from './icons';
import ConnectionStatusDialog from './ConnectionStatusDialog';
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
  /**
   * Sidebar toggle (Phase 5, made permanent after design review):
   * visible at every width — hides/shows the static sidebar above
   * 900px, opens/closes the overlay drawer at ≤900px. `sidebarOpen`
   * reflects the sidebar's current on-screen presence (aria-expanded).
   * The ref gets focus on drawer close.
   */
  sidebarOpen?: boolean;
  onToggleSidebar?: () => void;
  sidebarToggleRef?: React.RefObject<HTMLButtonElement | null>;
}

export default function MinimalHeader({
  currentFilePath,
  projectName,
  onChooseNewProject,
  onShare,
  onToggleFullscreenPreview,
  isFullscreenPreview = false,
  isOnline = true,
  sidebarOpen,
  onToggleSidebar,
  sidebarToggleRef,
}: MinimalHeaderProps) {
  const [showConnectionStatus, setShowConnectionStatus] = useState(false);
  return (
    <header className="minimal-header">
      <div className="header-left">
        {onToggleSidebar && (
          <Tooltip content={header.toggleSidebar}>
            <button
              ref={sidebarToggleRef}
              className="qh-icon-btn boxed sidebar-toggle-btn"
              onClick={onToggleSidebar}
              aria-label={header.toggleSidebar}
              aria-expanded={sidebarOpen ?? false}
              aria-controls="sidebar-drawer"
            >
              <PanelLeftIcon />
            </button>
          </Tooltip>
        )}
        <Tooltip content={header.switchProject}>
          <button
            className="qh-icon-btn boxed header-switch-btn"
            onClick={onChooseNewProject}
            aria-label={header.switchProject}
          >
            <SwitchIcon />
          </button>
        </Tooltip>
        {onShare && (
          <Tooltip content={header.shareProject}>
            <button
              className="qh-icon-btn boxed header-share-btn"
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
          <button
            className={`connection-indicator ${isOnline ? 'online' : 'offline'}`}
            onClick={() => setShowConnectionStatus(true)}
          >
            <span className="connection-dot" aria-hidden="true" />
            <span className="connection-text">{isOnline ? header.online : header.offline}</span>
          </button>
        </Tooltip>
        {showConnectionStatus && (
          <ConnectionStatusDialog
            currentFilePath={currentFilePath}
            onClose={() => setShowConnectionStatus(false)}
          />
        )}
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
