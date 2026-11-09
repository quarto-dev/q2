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
/* Edit-chrome colors are CSS custom properties, READ with today's values as
   fallbacks and never SET here — so a theme or user stylesheet can override any
   of them from anywhere (e.g. \`:root { --q2-edit-toolbar-color: … }\`) without
   fighting specificity. Defining them on .q2-rt-toolbar itself would shadow a
   :root override. The set:
     --q2-edit-toolbar-color       glyph color for every toolbar button/crumb/arrow
     --q2-edit-toolbar-bg          toolbar background
     --q2-edit-toolbar-border      toolbar border
     --q2-edit-toolbar-shadow      toolbar drop shadow
     --q2-edit-toolbar-hover-bg    button hover background
     --q2-edit-toolbar-active-bg   pressed/active button background
     --q2-edit-toolbar-separator   divider rules between button groups
     --q2-edit-toolbar-notice-color  transient refusal notice text
     --q2-edit-toolbar-input-border  link URL input border
     --q2-edit-ring                ring around the rich edit box
     --q2-edit-tint                background tint of the rich edit box */
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
  color: var(--q2-edit-toolbar-color, #334155);
  background: var(--q2-edit-toolbar-bg, #fff);
  border: 1px solid var(--q2-edit-toolbar-border, rgba(59, 130, 246, 0.35));
  border-radius: 5px;
  box-shadow: var(--q2-edit-toolbar-shadow, 0 2px 6px rgba(0, 0, 0, 0.12));
  user-select: none;
  font-size: 0.8rem;
  line-height: 1;
  /* An absolutely positioned box shrink-to-fits its containing block, so on a
     narrow screen it would squeeze the buttons and wrap their labels (--, !!, ++).
     Size to content instead; the buttons never wrap or shrink. */
  width: max-content;
  white-space: nowrap;
}
/* Rich surface: left-align the toolbar's outer edge with the edit box's visible
   ring. The ring (.ProseMirror below) is the 2px box-shadow around a tint that
   bleeds 5px left of the text (padding 5px / margin -5px), so its outer left edge
   sits at -(5 + 2) = -7px. Keep in sync with the .ProseMirror padding + ring. */
.q2-richtext-editor > .q2-rt-toolbar { left: -7px; }
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
  flex-shrink: 0;
  white-space: nowrap;
  padding: 0.25em 0.4em;
  font-size: 0.8rem;
  cursor: pointer;
  /* Buttons don't inherit color by default; take the toolbar's single glyph color. */
  color: inherit;
}
.q2-rt-tb-btn:disabled { opacity: 0.35; cursor: default; }
.q2-rt-tb-btn:disabled:hover { background: none; }
.q2-rt-tb-btn:hover { background: var(--q2-edit-toolbar-hover-bg, rgba(59, 130, 246, 0.12)); }
.q2-rt-tb-active { background: var(--q2-edit-toolbar-active-bg, rgba(59, 130, 246, 0.18)); }
/* One glyph color for the whole toolbar (--q2-edit-toolbar-color on .q2-rt-toolbar):
   buttons, nesting arrows, and type crumbs all inherit it — overriding the crumb
   sheet's per-category colors and #555 arrows. Active/pressed state shows as the
   background tint only. */
.q2-rt-toolbar .q2-crumb,
.q2-rt-toolbar .q2-breadcrumb-out,
.q2-rt-toolbar .q2-breadcrumb-in { color: inherit; }
.q2-rt-toolbar .q2-breadcrumb-out:hover,
.q2-rt-toolbar .q2-breadcrumb-in:hover { background: var(--q2-edit-toolbar-hover-bg, rgba(59, 130, 246, 0.12)); }
/* SVG glyphs (comment, link, editorial marks, paragraph crumb) — currentColor. */
.q2-rt-tb-btn:has(> .q2-rt-tb-icon) { display: inline-flex; align-items: center; justify-content: center; }
.q2-rt-tb-icon { display: block; width: 1.25em; height: 1.25em; }
.q2-rt-tb-icon.q2-rt-tb-icon-wide { width: 2.5em; }
.q2-rt-tb-sep { width: 1px; align-self: stretch; margin: 2px 2px; background: var(--q2-edit-toolbar-separator, rgba(0, 0, 0, 0.12)); }
.q2-rt-link-editor { display: flex; align-items: center; gap: 2px; }
/* Transient refusal notice next to the comment button. */
.q2-rt-tb-notice { font-size: 0.72rem; color: var(--q2-edit-toolbar-notice-color, #b91c1c); padding: 0 0.4em; white-space: nowrap; }
.q2-rt-link-input {
  font-size: 0.78rem;
  padding: 0.15em 0.4em;
  border: 1px solid var(--q2-edit-toolbar-input-border, rgba(0, 0, 0, 0.2));
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
  background: var(--q2-edit-tint, rgba(59, 130, 246, 0.08));
  box-shadow: 0 0 0 2px var(--q2-edit-ring, rgba(59, 130, 246, 0.35));
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
/* Authored span (the editable span mark): a lavender tint + dashed underline so a
   span reads as a span, distinct from the comment chrome's blue and from chips.
   Solid colors on purpose (no alpha). A span carrying hidden comments gets a
   slightly stronger underline. */
.q2-rt-span { background-color: #f3e8ff; border-bottom: 1px dashed #a855f7; border-radius: 2px; }
.q2-rt-span-commented { border-bottom-style: solid; }
/* Editorial spans, colored like the rendered marks (solid equivalents of the
   theme's $editorial-* tints on white). */
.q2-rt-span-insert { background-color: #d9ecd9; border-bottom: none; }
.q2-rt-span-delete { background-color: #ffd9d9; border-bottom: none; text-decoration: line-through; }
.q2-rt-span-highlight { background-color: #fff799; border-bottom: none; }
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
