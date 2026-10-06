/**
 * MoreActionsIconButton — the "⋯" kebab at the end of a tree row. Clicking it
 * toggles the row's actions menu, anchored below the button; right-click
 * or the Menu key on it opens the same menu at the pointer.
 *
 * Not a tab stop: a tree is one tab stop (roving tabindex) and keyboard
 * users open the menu with Shift+F10 on the row itself.
 */

import type { MouseEvent } from 'react';
import { MoreIcon } from './icons';

export interface MoreActionsIconButtonProps {
  /** Accessible name, e.g. "Actions for intro.qmd". */
  label: string;
  /** Whether this row's menu is currently open. */
  expanded: boolean;
  /** Open the menu at `x`/`y`; `trigger` receives focus when it closes. */
  onOpen: (anchor: { x: number; y: number; trigger: HTMLElement }) => void;
  onClose: () => void;
  /** Right-click / Menu key on the button. */
  onContextMenu: (e: MouseEvent) => void;
}

export default function MoreActionsIconButton({
  label,
  expanded,
  onOpen,
  onClose,
  onContextMenu,
}: MoreActionsIconButtonProps) {
  return (
    <button
      type="button"
      className="qh-icon-btn file-kebab"
      tabIndex={-1}
      aria-label={label}
      aria-haspopup="menu"
      aria-expanded={expanded}
      onClick={(e) => {
        e.stopPropagation();
        if (expanded) {
          onClose();
          return;
        }
        const rect = e.currentTarget.getBoundingClientRect();
        onOpen({ x: rect.left, y: rect.bottom + 4, trigger: e.currentTarget });
      }}
      onContextMenu={(e) => {
        e.stopPropagation();
        onContextMenu(e);
      }}
    >
      <MoreIcon />
    </button>
  );
}
