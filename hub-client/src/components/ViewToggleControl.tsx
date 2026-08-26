import { useViewMode } from './ViewModeContext';
import { LayoutMarkupIcon, LayoutSplitIcon, LayoutPreviewIcon } from './icons';
import Tooltip from './Tooltip';
import './ViewToggleControl.css';

/**
 * Compact horizontal view toggle in the header.
 * Three small square buttons with layout-split icons.
 */
export default function ViewToggleControl() {
  const { viewMode, setViewMode } = useViewMode();

  return (
    <div className="view-toggle-control">
      <Tooltip content="Expand markup">
        <button
          className={`view-toggle-btn${viewMode === 'markup' ? ' active' : ''}`}
          onClick={() => setViewMode('markup')}
          aria-label="Markup view"
          aria-pressed={viewMode === 'markup'}
        >
          <LayoutMarkupIcon />
        </button>
      </Tooltip>
      <Tooltip content="Split equally">
        <button
          className={`view-toggle-btn${viewMode === 'both' ? ' active' : ''}`}
          onClick={() => setViewMode('both')}
          aria-label="Split view"
          aria-pressed={viewMode === 'both'}
        >
          <LayoutSplitIcon />
        </button>
      </Tooltip>
      <Tooltip content="Expand preview">
        <button
          className={`view-toggle-btn${viewMode === 'preview' ? ' active' : ''}`}
          onClick={() => setViewMode('preview')}
          aria-label="Preview view"
          aria-pressed={viewMode === 'preview'}
        >
          <LayoutPreviewIcon />
        </button>
      </Tooltip>
    </div>
  );
}
