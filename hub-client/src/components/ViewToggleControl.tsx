import { LayoutMarkupIcon, LayoutSplitIcon, LayoutPreviewIcon } from './icons';
import Tooltip from './Tooltip';
import { viewToggle } from '../strings';
import './ViewToggleControl.css';

/**
 * Split presets for the editor/preview divider — clicking a button is
 * exactly equivalent to dragging the pane divider to that position
 * (markup = the drag's editor-max clamp, preview = its editor-min
 * clamp), except the move is animated (Editor's split-animating class).
 */
export const SPLIT_PRESETS = {
  markup: 0.85,
  split: 0.5,
  preview: 0.15,
} as const;

interface ViewToggleControlProps {
  /** Current editor-pane fraction of the split. */
  fraction?: number;
  /** Jump the divider to a preset fraction (animated by the caller). */
  onSelect?: (fraction: number) => void;
}

function isActive(fraction: number | undefined, preset: number): boolean {
  return fraction !== undefined && Math.abs(fraction - preset) < 0.01;
}

export default function ViewToggleControl({ fraction, onSelect }: ViewToggleControlProps) {
  return (
    <div className="view-toggle-control">
      <Tooltip content={viewToggle.expandMarkup}>
        <button
          className={`view-toggle-btn${isActive(fraction, SPLIT_PRESETS.markup) ? ' active' : ''}`}
          onClick={() => onSelect?.(SPLIT_PRESETS.markup)}
          aria-label={viewToggle.markupView}
          aria-pressed={isActive(fraction, SPLIT_PRESETS.markup)}
        >
          <LayoutMarkupIcon />
        </button>
      </Tooltip>
      <Tooltip content={viewToggle.splitEqually}>
        <button
          className={`view-toggle-btn${isActive(fraction, SPLIT_PRESETS.split) ? ' active' : ''}`}
          onClick={() => onSelect?.(SPLIT_PRESETS.split)}
          aria-label={viewToggle.splitView}
          aria-pressed={isActive(fraction, SPLIT_PRESETS.split)}
        >
          <LayoutSplitIcon />
        </button>
      </Tooltip>
      <Tooltip content={viewToggle.expandPreview}>
        <button
          className={`view-toggle-btn${isActive(fraction, SPLIT_PRESETS.preview) ? ' active' : ''}`}
          onClick={() => onSelect?.(SPLIT_PRESETS.preview)}
          aria-label={viewToggle.previewView}
          aria-pressed={isActive(fraction, SPLIT_PRESETS.preview)}
        >
          <LayoutPreviewIcon />
        </button>
      </Tooltip>
    </div>
  );
}
