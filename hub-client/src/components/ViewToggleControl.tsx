import { useViewMode } from './ViewModeContext';
import './ViewToggleControl.css';

/**
 * A toggle control that sits at the middle center of the divider between panes.
 * Allows users to switch between markup-focused, both, and preview-focused views.
 *
 * Layout: [◀] [|] [▶]
 * Arrows indicate divider movement direction:
 * - ◀ moves divider left (expands preview)
 * - | returns to even split (both)
 * - ▶ moves divider right (expands markup)
 */
export default function ViewToggleControl() {
  const { viewMode, setViewMode } = useViewMode();

  const isMarkup = viewMode === 'markup';
  const isPreview = viewMode === 'preview';
  const isBoth = viewMode === 'both';

  return (
    <div className="view-toggle-control">
      <button
        className={`view-toggle-btn view-toggle-left${isPreview ? ' active' : ''}`}
        onClick={() => setViewMode('preview')}
        title="Move divider left (expand preview)"
        aria-label="Preview view"
      >
        ◀
      </button>
      <button
        className={`view-toggle-btn view-toggle-center${isBoth ? ' active' : ''}`}
        onClick={() => setViewMode('both')}
        title="Show both panes equally"
        aria-label="Split view"
      >
        |
      </button>
      <button
        className={`view-toggle-btn view-toggle-right${isMarkup ? ' active' : ''}`}
        onClick={() => setViewMode('markup')}
        title="Move divider right (expand markup)"
        aria-label="Markup view"
      >
        ▶
      </button>
    </div>
  );
}
