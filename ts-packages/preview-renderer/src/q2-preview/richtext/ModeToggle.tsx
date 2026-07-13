// bd-igpm0xur — rich/plain toggle: one Markdown-mark icon on the EditToolbar,
// shown only when the block is rich-supported. Highlight-only (the icon never
// flips; state is the active tint + aria-pressed). A11y: an icon has no text, so a
// stable aria-label + aria-pressed (WAI-ARIA toggle-button pattern), not flipping
// text; the hover `title` flips as a sighted-user hint.

import { useContext, type MouseEvent } from 'react';
import { PreviewContext } from '../PreviewContext';

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
      {/* Markdown mark (dcurtis/markdown-mark); currentColor tracks the active state. */}
      <svg viewBox="0 0 208 128" aria-hidden="true" focusable="false">
        <rect
          x="5"
          y="5"
          width="198"
          height="118"
          ry="10"
          fill="none"
          stroke="currentColor"
          strokeWidth="10"
        />
        <path
          fill="currentColor"
          d="M30 98V30h20l20 25 20-25h20v68H90V59L70 84 50 59v39zm125 0l-30-33h20V30h20v35h20z"
        />
      </svg>
    </button>
  );
}
