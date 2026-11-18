import React from 'react';

// Kept for user TSX that styled its overrides like the historical
// bordered boxes (and for the renderer-surface global in entry.tsx).
export const blockStyle: React.CSSProperties = {
    border: '1px solid #666',
    padding: '4px',
    margin: '4px 0',
    backgroundColor: '#f5f5f5',
    position: 'relative',
};

export const inlineStyle: React.CSSProperties = {
    border: '1px solid #999',
    padding: '2px',
    margin: '1px',
    backgroundColor: '#e8e8e8',
    display: 'inline-block'
};

/**
 * The inspector's stylesheet, injected once by `AstRenderer`. Browser
 * devtools conventions: a monospace tree, one row per block node with a
 * disclosure triangle and an indented child list; inline nodes flow and
 * wrap as small chips inside their block's row; strings are just their
 * text, whitespace is a single glyph.
 */
export const DEBUG_STYLESHEET = `
.q2d {
  font: 12px/1.6 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  color: #1f2937;
  padding: 6px 10px 24px;
  -webkit-font-smoothing: antialiased;
}
.q2d-corner {
  position: fixed; right: 8px; bottom: 6px; z-index: 1;
  display: flex; align-items: center; gap: 3px;
  padding: 1px 6px; border: 1px solid #e5e7eb; border-radius: 10px;
  background: #ffffff; color: #9ca3af; font: 10px system-ui, sans-serif; cursor: pointer; user-select: none;
}
.q2d-corner:hover { color: #374151; border-color: #d1d5db; }
.q2d-corner input { margin: 0; width: 10px; height: 10px; }

.q2d-children { padding-left: 14px; border-left: 1px solid #f3f4f6; }
.q2d-children:hover { border-left-color: #e5e7eb; }

.q2d-row { display: flex; align-items: baseline; gap: 5px; flex-wrap: wrap; cursor: default; border-radius: 3px; }
.q2d-row:hover { background: #f3f4f6; }
.q2d-row.q2d-selected { background: #e0e7ff; }
.q2d-twisty {
  flex: none; width: 10px; color: #9ca3af; font-size: 9px; cursor: pointer; user-select: none; text-align: center;
}
.q2d-twisty.q2d-leaf { visibility: hidden; }
.q2d-tag { color: #881391; cursor: pointer; }
.q2d-tag.q2d-tag-inline { color: #0b7285; }
.q2d-tag.q2d-tag-custom { color: #b45309; }
.q2d-tag.q2d-tag-unknown { color: #b91c1c; }
.q2d-prop { color: #6b7280; }
.q2d-prop .q2d-key { color: #0d47a1; }
.q2d-prop .q2d-val { color: #1a1aa6; }
.q2d-attr-id { color: #1a1aa6; }
.q2d-attr-class { color: #1a1aa6; }
.q2d-ref { font-size: 9px; color: #9ca3af; vertical-align: super; line-height: 0; cursor: help; }
.q2d-summary { color: #6b7280; cursor: pointer; }
.q2d-count { color: #9ca3af; }

.q2d-inlines { display: inline; }
.q2d-inline { display: inline; white-space: nowrap; }
.q2d-inline > .q2d-inline-body { display: inline; white-space: normal; }
.q2d-chip {
  display: inline-block; vertical-align: baseline; margin: 1px 1px;
  padding: 0 3px; border-radius: 3px; border: 1px solid transparent; line-height: 1.4;
}
.q2d-str { background: #f3f4f6; color: #111827; white-space: pre-wrap; }
.q2d-str:hover { border-color: #d1d5db; }
.q2d-ws { color: #9ca3af; padding: 0 1px; }
.q2d-ws:hover { color: #4b5563; }
.q2d-container {
  display: inline; padding: 0 2px; border-radius: 3px; border: 1px solid #e5e7eb; background: #ffffff;
}
.q2d-container:hover { border-color: #c7d2fe; }
.q2d-container > .q2d-tag { margin-right: 3px; }
.q2d-container.q2d-closed { color: #9ca3af; }
.q2d-string { color: #c41a16; white-space: pre-wrap; }
.q2d-number { color: #1c00cf; }
.q2d-bool, .q2d-null { color: #0d22aa; }
.q2d-pre {
  margin: 2px 0 2px 14px; padding: 4px 6px; max-height: 240px; overflow: auto;
  background: #f9fafb; border: 1px solid #e5e7eb; border-radius: 4px;
  white-space: pre; color: #111827;
}
.q2d-json { display: inline; }
.q2d-json-block { margin-left: 14px; }
.q2d-json-row { display: flex; gap: 4px; align-items: baseline; }
.q2d-item { color: #6b7280; }
.q2d-slot { color: #6b7280; }
`;
