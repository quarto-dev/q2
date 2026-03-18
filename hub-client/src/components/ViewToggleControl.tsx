import { useViewMode } from './ViewModeContext';
import './ViewToggleControl.css';

/**
 * Compact horizontal view toggle at the top of the sidebar.
 * Three small square buttons with layout-split icons.
 */
export default function ViewToggleControl() {
  const { viewMode, setViewMode } = useViewMode();

  return (
    <div className="view-toggle-control">
      <button
        className={`view-toggle-btn${viewMode === 'preview' ? ' active' : ''}`}
        onClick={() => setViewMode('preview')}
        title="Expand preview"
        aria-label="Preview view"
      >
        <svg width="12" height="10" viewBox="0 0 12 10">
          <rect x="0" y="0" width="3" height="10" rx="0.5" fill="currentColor" opacity="0.25" />
          <rect x="5" y="0" width="7" height="10" rx="0.5" fill="currentColor" />
        </svg>
      </button>
      <button
        className={`view-toggle-btn${viewMode === 'both' ? ' active' : ''}`}
        onClick={() => setViewMode('both')}
        title="Split equally"
        aria-label="Split view"
      >
        <svg width="12" height="10" viewBox="0 0 12 10">
          <rect x="0" y="0" width="5" height="10" rx="0.5" fill="currentColor" />
          <rect x="7" y="0" width="5" height="10" rx="0.5" fill="currentColor" />
        </svg>
      </button>
      <button
        className={`view-toggle-btn${viewMode === 'markup' ? ' active' : ''}`}
        onClick={() => setViewMode('markup')}
        title="Expand markup"
        aria-label="Markup view"
      >
        <svg width="12" height="10" viewBox="0 0 12 10">
          <rect x="0" y="0" width="7" height="10" rx="0.5" fill="currentColor" />
          <rect x="9" y="0" width="3" height="10" rx="0.5" fill="currentColor" opacity="0.25" />
        </svg>
      </button>
    </div>
  );
}
