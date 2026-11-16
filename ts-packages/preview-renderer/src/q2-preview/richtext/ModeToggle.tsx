// bd-igpm0xur — rich/plain toggle: one Markdown-mark icon on the EditToolbar,
// shown only when the block is rich-supported. Highlight-only (the icon never
// flips; state is the active tint + aria-pressed). A11y: an icon has no text, so a
// stable aria-label + aria-pressed (WAI-ARIA toggle-button pattern), not flipping
// text; the hover `title` flips as a sighted-user hint.

import { useContext, type MouseEvent } from 'react';
import { PreviewContext } from '../PreviewContext';
import { MarkdownIcon } from './ToolbarIcons';

export function ModeToggle() {
  const ctx = useContext(PreviewContext);
  if (!ctx) return null;
  const isPlain = (ctx.editorMode ?? 'rich') === 'plain';

  // mousedown+preventDefault avoids a blur that would commit/close before the swap.
  // The switch-ref (read by RichTextEditor's commit/focusout) suppresses the outgoing
  // surface's unmount-blur; cleared next tick. Ported from EditAffordance.choose.
  const onMouseDown = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (ctx.editorModeSwitchRef) ctx.editorModeSwitchRef.current = true;
    ctx.setEditorMode?.(isPlain ? 'rich' : 'plain');
    setTimeout(() => {
      if (ctx.editorModeSwitchRef) ctx.editorModeSwitchRef.current = false;
    }, 0);
  };

  return (
    <button
      type="button"
      className={`q2-rt-tb-btn q2-rt-tb-mode${isPlain ? ' q2-rt-tb-active' : ''}`}
      aria-label="Toggle plain-text editing"
      aria-pressed={isPlain}
      title={isPlain ? 'Edit as rich text' : 'Edit as plain text'}
      onMouseDown={onMouseDown}
    >
      {/* Markdown icon (currentColor — same glyph color as the rest of the toolbar). */}
      <MarkdownIcon />
    </button>
  );
}
