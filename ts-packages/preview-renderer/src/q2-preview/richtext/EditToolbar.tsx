// bd-sjb4pzx8 / bd-igpm0xur — the single pop-up edit-chrome host for every editable
// block (generalized from the rich-only RichTextToolbar). Optional `editor`: present
// on the rich surface (marks + link editor render), absent on the plain surface (code
// chunks, CustomBlocks, plain-mode blocks) where only the mode toggle + type indicator
// show.
//
// Mark buttons (bold/italic/strike/sub/sup) are second triggers for the same
// commands Cmd-B/I fire — `toggleMark` over the current selection (ProseMirror
// applies/removes the mark across the range; an empty selection sets a stored
// mark so the next typed text gets it). The link button opens a small URL input
// and uses `extendMarkRange('link')` so an existing link can be edited/removed by
// placing the cursor anywhere inside it.
//
// All buttons use mousedown-preventDefault so clicking them never blurs the
// editor (which would collapse the selection before the command runs). The link
// input DOES take focus; the editor's commit is scoped to "focus left the whole
// edit box" (see RichTextEditor), so focusing the input keeps the session open.

import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from 'react';
import type { Editor } from '@tiptap/core';
import { shouldPlaceChromeBelow } from '../editChromeGeometry';
import { ensureRichTextStyles } from './styles';
import { ModeToggle } from './ModeToggle';
import { EditTypeIndicator } from './EditTypeIndicator';

/** Gap (px) between the toolbar and the edit box, matching the CSS margin. */
const TOOLBAR_GAP = 4;

interface MarkSpec {
  name: string;
  label: string;
  title: string;
}

const MARKS: MarkSpec[] = [
  { name: 'bold', label: 'B', title: 'Bold (⌘B)' },
  { name: 'italic', label: 'I', title: 'Italic (⌘I)' },
  { name: 'strike', label: 'S', title: 'Strikethrough' },
  { name: 'subscript', label: 'x₂', title: 'Subscript' },
  { name: 'superscript', label: 'x²', title: 'Superscript' },
];

export function EditToolbar({
  editor,
  richSupported,
}: {
  /** The live tiptap editor when the rich surface is mounted; null/undefined on
   *  the plain surface (no marks then). */
  editor?: Editor | null;
  /** True when the block is rich-supported (Para/Header/Plain with richText on) —
   *  the gate for showing the rich/plain mode toggle. */
  richSupported: boolean;
}) {
  // Plain surface mounts this without RichTextEditor (the other caller), so inject here.
  ensureRichTextStyles();

  // Re-render on selection/content changes so isActive() highlights stay current.
  // No-op when there is no editor (plain surface).
  const [, force] = useState(0);
  useEffect(() => {
    if (!editor) return;
    const bump = () => force((n) => n + 1);
    editor.on('selectionUpdate', bump);
    editor.on('transaction', bump);
    return () => {
      editor.off('selectionUpdate', bump);
      editor.off('transaction', bump);
    };
  }, [editor]);

  // Vertical placement (bd-pvcnea83): the toolbar floats ABOVE the edit box by
  // default, but flips BELOW when there isn't room above (e.g. editing the first
  // block of a title-less document, flush against the viewport top — otherwise it
  // is clipped above the scroll area, with no way to scroll up to it).
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const [placeBelow, setPlaceBelow] = useState(false);
  useLayoutEffect(() => {
    const tb = toolbarRef.current;
    // Offset parent differs per surface: `.q2-richtext-editor` (rich) vs
    // `#q2-active-edit-region` (plain wrapper). Measure whichever we're mounted in.
    const box = tb?.closest('.q2-richtext-editor, #q2-active-edit-region');
    if (!tb || !box) return;
    const height = tb.offsetHeight;
    // Degenerate layout (jsdom zero-rects): keep the default 'above' placement.
    if (height <= 0) return;
    const surfaceTop = box.getBoundingClientRect().top;
    setPlaceBelow(shouldPlaceChromeBelow(surfaceTop, height, TOOLBAR_GAP));
    // Mount-only: the toolbar remounts per edit target, and the edit box's top is
    // stable for a given target, so a single measurement suffices.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [linkOpen, setLinkOpen] = useState(false);
  const [linkUrl, setLinkUrl] = useState('');
  const linkInputRef = useRef<HTMLInputElement | null>(null);

  // Reliably focus + select the URL input when the link editor opens (more robust
  // than the autoFocus prop alone).
  useEffect(() => {
    if (linkOpen && linkInputRef.current) {
      linkInputRef.current.focus();
      linkInputRef.current.select();
    }
  }, [linkOpen]);

  const toggleMark = (name: string) => (e: MouseEvent) => {
    e.preventDefault();
    if (!editor) return;
    editor.chain().focus().toggleMark(name).run();
  };

  const openLinkEditor = (e: MouseEvent) => {
    e.preventDefault();
    if (!editor) return;
    const existing = editor.isActive('link') ? (editor.getAttributes('link').href as string) : '';
    setLinkUrl(existing ?? '');
    setLinkOpen(true);
  };

  const applyLink = () => {
    if (!editor) return;
    const url = linkUrl.trim();
    if (!url) {
      // Empty URL on an existing link removes it; otherwise just cancel.
      if (editor.isActive('link')) {
        editor.chain().focus().extendMarkRange('link').unsetLink().run();
      }
    } else if (editor.state.selection.empty && !editor.isActive('link')) {
      // No selection and not in a link: insert the URL as linked text.
      editor.chain().focus().insertContent({ type: 'text', text: url, marks: [{ type: 'link', attrs: { href: url } }] }).run();
    } else {
      editor.chain().focus().extendMarkRange('link').setLink({ href: url }).run();
    }
    setLinkOpen(false);
  };

  const removeLink = () => {
    if (!editor) return;
    editor.chain().focus().extendMarkRange('link').unsetLink().run();
    setLinkOpen(false);
  };

  const cancelLink = () => {
    setLinkOpen(false);
    editor?.chain().focus().run();
  };

  return (
    <div
      ref={toolbarRef}
      className={`q2-rt-toolbar${placeBelow ? ' q2-rt-toolbar-below' : ''}`}
      contentEditable={false}
    >
      {richSupported && <ModeToggle />}
      {/* Divider: sets the mode toggle apart from the marks (only when both show). */}
      {richSupported && editor && <span className="q2-rt-tb-sep" />}
      {editor && (!linkOpen ? (
        <>
          {MARKS.map((m) => (
            <button
              key={m.name}
              type="button"
              title={m.title}
              aria-pressed={editor.isActive(m.name)}
              className={`q2-rt-tb-btn q2-rt-tb-${m.name}${editor.isActive(m.name) ? ' q2-rt-tb-active' : ''}`}
              onMouseDown={toggleMark(m.name)}
            >
              {m.label}
            </button>
          ))}
          <span className="q2-rt-tb-sep" />
          <button
            type="button"
            title="Link"
            aria-pressed={editor.isActive('link')}
            className={`q2-rt-tb-btn q2-rt-tb-link${editor.isActive('link') ? ' q2-rt-tb-active' : ''}`}
            onMouseDown={openLinkEditor}
          >
            🔗
          </button>
        </>
      ) : (
        <div className="q2-rt-link-editor">
          <input
            ref={linkInputRef}
            type="url"
            className="q2-rt-link-input"
            placeholder="https://…"
            value={linkUrl}
            onChange={(e) => setLinkUrl(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                applyLink();
              } else if (e.key === 'Escape') {
                e.preventDefault();
                cancelLink();
              }
            }}
          />
          <button type="button" className="q2-rt-tb-btn" title="Apply" onMouseDown={(e) => { e.preventDefault(); applyLink(); }}>✓</button>
          {editor.isActive('link') && (
            <button type="button" className="q2-rt-tb-btn" title="Remove link" onMouseDown={(e) => { e.preventDefault(); removeLink(); }}>✕</button>
          )}
        </div>
      ))}
      {/* Type/nesting indicator (always). Leading separator only when a toggle
          and/or marks precede it, so a bare code-chunk toolbar has no leading rule. */}
      {(richSupported || editor) && <span className="q2-rt-tb-sep" />}
      <EditTypeIndicator />
    </div>
  );
}
