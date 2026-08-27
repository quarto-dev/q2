import { useViewMode } from './ViewModeContext';
import { LayoutMarkupIcon, LayoutSplitIcon, LayoutPreviewIcon } from './icons';
import Tooltip from './Tooltip';
import { useMediaQuery } from '../hooks/useMediaQuery';
import { viewToggle } from '../strings';
import './ViewToggleControl.css';

/**
 * Compact horizontal view toggle in the header.
 * Three small square buttons with layout-split icons.
 */
export default function ViewToggleControl() {
  const { viewMode, setViewMode } = useViewMode();
  // Phase 5: split view collapses to the editor pane at ≤700px
  // (Editor.css), so the split option is disabled this narrow. The mode
  // itself is left untouched — widening the window restores the split.
  const splitUnavailable = useMediaQuery('(max-width: 700px)');

  return (
    <div className="view-toggle-control">
      <Tooltip content={viewToggle.expandMarkup}>
        <button
          className={`view-toggle-btn${viewMode === 'markup' ? ' active' : ''}`}
          onClick={() => setViewMode('markup')}
          aria-label={viewToggle.markupView}
          aria-pressed={viewMode === 'markup'}
        >
          <LayoutMarkupIcon />
        </button>
      </Tooltip>
      <Tooltip
        content={splitUnavailable ? viewToggle.splitUnavailable : viewToggle.splitEqually}
      >
        <button
          className={`view-toggle-btn${viewMode === 'both' ? ' active' : ''}`}
          onClick={() => setViewMode('both')}
          aria-label={viewToggle.splitView}
          aria-pressed={viewMode === 'both'}
          disabled={splitUnavailable}
        >
          <LayoutSplitIcon />
        </button>
      </Tooltip>
      <Tooltip content={viewToggle.expandPreview}>
        <button
          className={`view-toggle-btn${viewMode === 'preview' ? ' active' : ''}`}
          onClick={() => setViewMode('preview')}
          aria-label={viewToggle.previewView}
          aria-pressed={viewMode === 'preview'}
        >
          <LayoutPreviewIcon />
        </button>
      </Tooltip>
    </div>
  );
}
