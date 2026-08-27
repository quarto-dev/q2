/**
 * Document Top Bar
 *
 * Document-scoped chrome heading the document column. Left: sidebar
 * toggle + current file path. Right: fullscreen-preview action.
 * (Sync status lives in the bottom bars' SyncStatusBadge; the
 * editor/preview split is resized by dragging the pane divider.)
 */

import { PreviewIcon, PanelLeftIcon } from './icons';
import Tooltip from './Tooltip';
import { header } from '../strings';
import './TopBars.css';

interface DocumentTopBarProps {
  currentFilePath: string | null;
  onToggleFullscreenPreview?: () => void;
  isFullscreenPreview?: boolean;
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

export default function DocumentTopBar({
  currentFilePath,
  onToggleFullscreenPreview,
  isFullscreenPreview = false,
  sidebarOpen,
  onToggleSidebar,
  sidebarToggleRef,
}: DocumentTopBarProps) {
  return (
    <header className="top-bar document-top-bar">
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
        <div className="header-doc">
          <span className={`file-path qh-truncate${currentFilePath ? '' : ' empty'}`}>
            {currentFilePath ?? header.noFileSelected}
          </span>
        </div>
      </div>
      <div className="header-right">
        {onToggleFullscreenPreview && !isFullscreenPreview && (
          <Tooltip content={header.fullscreenPreview}>
            <button
              className="qh-icon-btn boxed preview-btn"
              onClick={onToggleFullscreenPreview}
              aria-label={header.fullscreenPreview}
            >
              <PreviewIcon />
            </button>
          </Tooltip>
        )}
      </div>
    </header>
  );
}
