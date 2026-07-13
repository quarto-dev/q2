// Phase 1a (bd-sjb4pzx8) — one-time CSS for the rich-text editor.
//
// Goal: the editor should look like the rendered page. The heavy lifting is done
// by the theme stylesheet already loaded in the iframe (it styles the editor's
// <p>/<em>/<strong>/<a> for free). This sheet only (a) strips ProseMirror's
// default editor chrome so it doesn't fight the theme, (b) zeroes the inner
// block margin since the measured box already reproduces the block's spacing,
// and (c) gives chips a subtle, source-token pill look.

let injected = false;

const CSS = `
.q2-richtext-editor { position: relative; }

/* Formatting toolbar — a small box floating just above the top-left of the edit
   box. Out of flow (absolute) so it never reflows the content; solid background
   + shadow so it reads over whatever is behind it. */
.q2-rt-toolbar {
  position: absolute;
  bottom: 100%;
  left: -2px;
  margin-bottom: 4px;
  z-index: 20;
  display: flex;
  align-items: center;
  gap: 1px;
  padding: 2px 3px;
  background: #fff;
  border: 1px solid rgba(59, 130, 246, 0.35);
  border-radius: 5px;
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.12);
  user-select: none;
  font-size: 0.8rem;
  line-height: 1;
}
/* bd-pvcnea83: flip below the edit box when there is no room above (e.g. the
   first block of a title-less document, flush against the viewport top — the
   default bottom:100% placement would clip the toolbar above the scroll area). */
.q2-rt-toolbar.q2-rt-toolbar-below {
  bottom: auto;
  top: 100%;
  margin-bottom: 0;
  margin-top: 4px;
}
.q2-rt-tb-btn {
  appearance: none;
  border: none;
  background: none;
  border-radius: 3px;
  min-width: 1.6em;
  padding: 0.25em 0.4em;
  font-size: 0.8rem;
  cursor: pointer;
  color: #334155;
}
.q2-rt-tb-btn:hover { background: rgba(59, 130, 246, 0.12); }
.q2-rt-tb-active { background: rgba(59, 130, 246, 0.18); color: rgb(37, 99, 235); }
/* Mode toggle — the Markdown-mark SVG. inline-flex centers it; the SVG's
   currentColor recolors with the button's active state (highlight-only). */
.q2-rt-tb-mode {
  display: inline-flex;
  align-items: center;
}
.q2-rt-tb-mode svg {
  display: block;
  height: 0.95em;
  width: auto;
}
.q2-rt-tb-bold { font-weight: 700; }
.q2-rt-tb-italic { font-style: italic; }
.q2-rt-tb-strike { text-decoration: line-through; }
.q2-rt-tb-sep { width: 1px; align-self: stretch; margin: 2px 2px; background: rgba(0, 0, 0, 0.12); }
.q2-rt-link-editor { display: flex; align-items: center; gap: 2px; }
.q2-rt-link-input {
  font-size: 0.78rem;
  padding: 0.15em 0.4em;
  border: 1px solid rgba(0, 0, 0, 0.2);
  border-radius: 3px;
  width: 16rem;
  max-width: 40vw;
}

.q2-richtext-editor .ProseMirror {
  outline: none;
  white-space: pre-wrap;
  word-wrap: break-word;
  /* "Edit mode" affordance: the WYSIWYG render is faithful enough that the user
     can't otherwise tell editing is live. A subtle tint + ring signals it. The
     padding is cancelled by an equal negative margin so the text does NOT shift
     (zero reflow) — only the tinted area extends slightly around the content. */
  background: rgba(59, 130, 246, 0.08);
  box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.35);
  border-radius: 3px;
  padding: 2px 5px;
  margin: -2px -5px;
}
.q2-richtext-editor .ProseMirror:focus { outline: none; }
/* The measured edit box reproduces the original block's margin/padding; the
   editor's own block must not add a second margin. */
.q2-richtext-editor .ProseMirror > * { margin-top: 0; margin-bottom: 0; }

/* Opaque source-token chips (v1: pills, not rendered). */
.q2-chip {
  font-family: var(--bs-font-monospace, ui-monospace, SFMono-Regular, Menlo, monospace);
  font-size: 0.85em;
  background: rgba(120, 120, 160, 0.14);
  border: 1px solid rgba(120, 120, 160, 0.30);
  border-radius: 3px;
  padding: 0 0.25em;
  white-space: nowrap;
  cursor: default;
  user-select: all;
}
.q2-chip-math { background: rgba(80, 160, 120, 0.14); border-color: rgba(80, 160, 120, 0.30); }
.q2-chip-cite, .q2-chip-shortcode { background: rgba(160, 120, 80, 0.14); border-color: rgba(160, 120, 80, 0.30); }
`;

/** Inject the rich-text editor stylesheet into the document head once. */
export function ensureRichTextStyles(): void {
  if (injected || typeof document === 'undefined') return;
  injected = true;
  const style = document.createElement('style');
  style.setAttribute('data-q2-richtext', '1');
  style.textContent = CSS;
  document.head.appendChild(style);
}
