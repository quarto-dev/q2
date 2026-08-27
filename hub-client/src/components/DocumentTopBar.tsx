/**
 * Document Top Bar
 *
 * Document-scoped chrome heading the document column. Left: sidebar
 * toggle + current file path. Right: layout toggle, fullscreen-preview
 * action. (Sync status lives in the bottom bars' SyncStatusBadge.)
 */

import { useRef, useState } from 'react';
import ViewToggleControl from './ViewToggleControl';
import { PreviewIcon, PanelLeftIcon, MoreIcon } from './icons';
import Tooltip from './Tooltip';
import { Menu, MenuItem } from './Menu';
import { header } from '../strings';
import './TopBars.css';

interface DocumentTopBarProps {
  currentFilePath: string | null;
  onToggleFullscreenPreview?: () => void;
  isFullscreenPreview?: boolean;
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
  sidebarOpen,
  onToggleSidebar,
  sidebarToggleRef,
}: DocumentTopBarProps) {
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
        <ViewToggleControl />
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
