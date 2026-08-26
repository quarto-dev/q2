import { useViewMode } from './ViewModeContext';
import { LayoutMarkupIcon, LayoutSplitIcon, LayoutPreviewIcon } from './icons';
import './ViewToggleControl.css';

/**
 * Compact horizontal view toggle in the header.
 * Three small square buttons with layout-split icons.
 */
export default function ViewToggleControl() {
  const { viewMode, setViewMode } = useViewMode();

  return (
    <div className="view-toggle-control">
      <button
        className={`view-toggle-btn${viewMode === 'markup' ? ' active' : ''}`}
        onClick={() => setViewMode('markup')}
        title="Expand markup"
        aria-label="Markup view"
        aria-pressed={viewMode === 'markup'}
      >
        <LayoutMarkupIcon />
      </button>
      <button
        className={`view-toggle-btn${viewMode === 'both' ? ' active' : ''}`}
        onClick={() => setViewMode('both')}
        title="Split equally"
        aria-label="Split view"
        aria-pressed={viewMode === 'both'}
      >
        <LayoutSplitIcon />
      </button>
      <button
        className={`view-toggle-btn${viewMode === 'preview' ? ' active' : ''}`}
        onClick={() => setViewMode('preview')}
        title="Expand preview"
        aria-label="Preview view"
        aria-pressed={viewMode === 'preview'}
      >
        <LayoutPreviewIcon />
      </button>
    </div>
  );
}
