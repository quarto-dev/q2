/**
 * Document Top Bar
 *
 * Document-scoped chrome heading the document column. Left: sidebar
 * toggle + current file path. Right: printable-version and
 * fullscreen-preview actions.
 * (Sync status lives in the SyncStatusBadge — FILES section for the
 * project, document bottom bar for the open file; the
 * editor/preview split is resized by dragging the pane divider.)
 */

import { useCallback, useState } from 'react';
import { PreviewIcon, PanelLeftIcon, PrintIcon } from './icons';
import ViewToggleControl from './ViewToggleControl';
import Tooltip from './Tooltip';
import Toast from './Toast';
import { openPrintableDocument } from '../services/printableDocument';
import { header } from '../strings';
import './TopBars.css';

interface DocumentTopBarProps {
  currentFilePath: string | null;
  /**
   * Preview format of the current file (e.g. `q2-preview`, `q2-slides`,
   * `revealjs`, or `null`). Drives the "Open printable version" button:
   * shown only for formats that produce a printable standalone document.
   */
  currentFormat?: string | null;
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
  /** Current editor-pane fraction, for the split-preset buttons. */
  splitFraction?: number;
  /** Jump the divider to a preset fraction (animated). */
  onSetSplit?: (fraction: number) => void;
  /** Gray out the split presets (current file has no preview pane). */
  splitDisabled?: boolean;
}

export default function DocumentTopBar({
  currentFilePath,
  currentFormat = null,
  onToggleFullscreenPreview,
  isFullscreenPreview = false,
  sidebarOpen,
  onToggleSidebar,
  sidebarToggleRef,
  splitFraction,
  onSetSplit,
  splitDisabled,
}: DocumentTopBarProps) {
  // "Open printable version" (issue #315). The React preview formats
  // can't be printed in place (sandboxed iframe → clipped single page).
  // Instead we render a standalone, self-contained document and open it
  // in a new tab. Shown only for formats that yield a printable document.
  const [isPreparingPrintable, setIsPreparingPrintable] = useState(false);
  const [printableError, setPrintableError] = useState<string | null>(null);
  const canOpenPrintable =
    !!currentFilePath &&
    (currentFormat === 'q2-preview' ||
      currentFormat === 'q2-slides' ||
      currentFormat === 'revealjs');
  const handleOpenPrintable = useCallback(() => {
    if (!currentFilePath) return;
    setPrintableError(null);
    setIsPreparingPrintable(true);
    openPrintableDocument(currentFilePath, currentFormat)
      .catch((err: unknown) => {
        const message = err instanceof Error ? err.message : String(err);
        console.error('[printable] failed to open printable version:', err);
        setPrintableError(message);
      })
      .finally(() => setIsPreparingPrintable(false));
  }, [currentFilePath, currentFormat]);

  return (
    <header className="top-bar document-top-bar">
      {onToggleSidebar && (
        <div className="sidebar-toggle-box">
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
        </div>
      )}
      <div className="header-left">
        <div className="header-doc">
          <span className="doc-kicker" aria-hidden="true">
            Document
          </span>
          <span className={`file-path qh-truncate${currentFilePath ? '' : ' empty'}`}>
            {currentFilePath ?? header.noFileSelected}
          </span>
        </div>
      </div>
      <ViewToggleControl fraction={splitFraction} onSelect={onSetSplit} disabled={splitDisabled} />
      <div className="bar-actions">
      {canOpenPrintable && !isFullscreenPreview && (
        <div className="print-btn-box">
          <Tooltip content={header.printableTooltip}>
            <button
              className="qh-icon-btn boxed print-btn"
              onClick={handleOpenPrintable}
              disabled={isPreparingPrintable}
              aria-label={header.printableLabel}
            >
              {isPreparingPrintable ? '…' : <PrintIcon />}
            </button>
          </Tooltip>
        </div>
      )}
      <Toast
        message={printableError ?? ''}
        visible={printableError !== null}
        onHide={() => setPrintableError(null)}
        duration={6000}
      />
      {onToggleFullscreenPreview && !isFullscreenPreview && (
        <div className="fullscreen-btn-box">
          <Tooltip content={header.fullscreenPreview}>
            <button
              className="qh-icon-btn boxed preview-btn"
              onClick={onToggleFullscreenPreview}
              aria-label={header.fullscreenPreview}
            >
              <PreviewIcon />
            </button>
          </Tooltip>
        </div>
      )}
      </div>
    </header>
  );
}
