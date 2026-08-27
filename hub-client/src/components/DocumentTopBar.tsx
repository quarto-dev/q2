/**
 * Document Top Bar
 *
 * Document-scoped chrome. Left: sidebar toggle + current file path.
 * Right: online status, layout toggle, fullscreen-preview action.
 * Sits to the right of ProjectTopBar in the `.top-bars` row; eventually
 * this bar lives with the rest of the document UI in a right-hand column.
 */

import { useRef, useState } from 'react';
import ViewToggleControl from './ViewToggleControl';
import { PreviewIcon, PanelLeftIcon, MoreIcon } from './icons';
import ConnectionStatusDialog from './ConnectionStatusDialog';
import Tooltip from './Tooltip';
import { Menu, MenuItem } from './Menu';
import { header } from '../strings';
import './TopBars.css';

interface DocumentTopBarProps {
  currentFilePath: string | null;
  onToggleFullscreenPreview?: () => void;
  isFullscreenPreview?: boolean;
  /** Whether the project is connected to the sync server */
  isOnline?: boolean;
  /**
   * Sidebar drawer toggle (Phase 5): rendered only at ≤900px, when the
   * sidebar is a drawer — the caller (Editor) gates on the breakpoint
   * via onToggleSidebar's presence. The ref gets focus on drawer close.
   */
  sidebarOpen?: boolean;
  onToggleSidebar?: () => void;
  sidebarToggleRef?: React.RefObject<HTMLButtonElement | null>;
}

export default function DocumentTopBar({
  currentFilePath,
  onToggleFullscreenPreview,
  isFullscreenPreview = false,
  isOnline = true,
  sidebarOpen,
  onToggleSidebar,
  sidebarToggleRef,
}: DocumentTopBarProps) {
  const [showConnectionStatus, setShowConnectionStatus] = useState(false);
  const [overflowOpen, setOverflowOpen] = useState(false);
  const overflowTriggerRef = useRef<HTMLButtonElement | null>(null);
  // Secondary actions collapse into the overflow menu at ≤700px (CSS
  // hides the inline buttons and reveals the kebab). Rendered only when
  // at least one collapsible action exists.
  const hasCollapsibleActions = !!(
    onToggleFullscreenPreview && !isFullscreenPreview
  );

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
        {hasCollapsibleActions && (
          <div className="qh-menu-anchor header-overflow">
            <Tooltip content={header.moreActions}>
              <button
                ref={overflowTriggerRef}
                className="qh-icon-btn boxed"
                onClick={() => setOverflowOpen((v) => !v)}
                aria-label={header.moreActions}
                aria-expanded={overflowOpen}
              >
                <MoreIcon />
              </button>
            </Tooltip>
            {overflowOpen && (
              <Menu
                className="qh-menu-right"
                triggerRef={overflowTriggerRef}
                onClose={() => setOverflowOpen(false)}
                aria-label={header.moreActions}
              >
                {onToggleFullscreenPreview && !isFullscreenPreview && (
                  <MenuItem
                    onSelect={() => {
                      setOverflowOpen(false);
                      onToggleFullscreenPreview();
                    }}
                  >
                    {header.fullscreenPreview}
                  </MenuItem>
                )}
              </Menu>
            )}
          </div>
        )}
      </div>
    </header>
  );
}
