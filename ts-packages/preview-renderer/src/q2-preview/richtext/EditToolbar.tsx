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
//
// Comment-on-selection (💬, rich surface only — span comments prototype, see
// custom/CommentSpan.tsx): applies the editable `span` mark to the selection
// (serializes as `[selected text]`), commits the block, and opens the span's
// add-comment bubble in the rendered view.

import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from 'react';
import { getMarkRange, type Editor } from '@tiptap/core';
import { shouldPlaceChromeBelow } from '../editChromeGeometry';
import { ensureRichTextStyles } from './styles';
import type { EditorialKind } from './schema';
import { editorialAvailability } from './editorialSelection';
import { requestOpenCommentOnSpan } from '../commentPending';
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
  onCommit,
}: {
  /** Commit the edit session now (rich surface); used by comment-on-selection. */
  onCommit?: () => void;
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

  // ---- comment on selection ------------------------------------------------
  // Wrap the selection (within one text block) in a plain span chip, commit the
  // block, and ask the span comment chrome to open its add-comment bubble on
  // the span once it renders (see `commentPending.ts`). The comment itself is
  // typed in the bubble — the same UI as the `+` on a block — with the span
  // highlighted, so what is being commented on stays visible.
  // Transient feedback in the toolbar when the selection can't be commented
  // on (there is no toast facility inside the preview iframe).
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 2500);
    return () => clearTimeout(t);
  }, [notice]);

  const startComment = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (!editor) return;
    const { doc, schema, selection } = editor.state;
    const { from, to, $from, $to } = selection;
    if (from === to) {
      setNotice('Select some text to comment on');
      return;
    }
    if (!$from.sameParent($to)) {
      setNotice('Select text within one paragraph');
      return;
    }
    const spanType = schema.marks.span;
    // Existing spans are never altered here (no extending, merging, or
    // nesting). Entirely inside one span: just open that span's bubble.
    // Partly overlapping one: refuse.
    if (editor.isActive('span')) {
      const range = getMarkRange($from, spanType);
      if (!range) return;
      requestOpenCommentOnSpan(doc.textBetween(range.from, range.to, ' '));
      setTimeout(() => onCommit?.(), 0);
      return;
    }
    if (doc.rangeHasMark(from, to, spanType)) {
      setNotice('Selection overlaps an existing span');
      return;
    }
    const text = doc.textBetween(from, to, ' ');
    if (!text.trim()) {
      setNotice('Select some text to comment on');
      return;
    }
    editor.chain().setMark('span', { attr: ['', [], []], comments: [] }).run();
    requestOpenCommentOnSpan(text);
    // Commit once the press has fully completed: the trailing `click` must be
    // delivered while the toolbar still exists, or it lands on whatever sits
    // under the pointer after the editor closes and can re-open the block.
    setTimeout(() => onCommit?.(), 0);
  };

  // ---- editorial marks (!! highlight, -- delete, ++ insert) ------------------
  // Enabled only with text selected. Adds the editable `span` mark with an
  // editorial `kind` (serializes as `[!! text]` etc.); when the selection is
  // exactly an editorial span, only that kind is enabled and it removes the
  // mark (see editorialSelection.ts). Existing spans are never otherwise altered.
  const avail = editor
    ? editorialAvailability(editor.state.doc, editor.state.selection.from, editor.state.selection.to)
    : ({ mode: 'none' } as const);

  const applyEditorialMark = (kind: EditorialKind) => (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (!editor) return;
    if (avail.mode === 'add') {
      editor.chain().focus().setMark('span', { attr: ['', [], []], comments: [], kind }).run();
    } else if (avail.mode === 'remove' && avail.kind === kind) {
      const { mark, from, to } = avail;
      const attr = mark.attrs.attr as [string, string[], unknown[]];
      const bare = !attr[0] && attr[1].length === 0 && attr[2].length === 0 && mark.attrs.comments.length === 0;
      editor
        .chain()
        .focus()
        .command(({ tr }) => {
          tr.removeMark(from, to, mark);
          // A span carrying attrs/comments stays a plain span; a bare one disappears.
          if (!bare) tr.addMark(from, to, mark.type.create({ ...mark.attrs, kind: '' }));
          return true;
        })
        .run();
    }
  };

  const editorialButtons = (
    [
      { sigil: '!!', kind: 'highlight', title: 'Highlight selection', label: '!!' },
      { sigil: '--', kind: 'delete', title: 'Mark selection as deleted', label: '--' },
      { sigil: '++', kind: 'insert', title: 'Mark selection as inserted', label: '++' },
    ] as const
  ).map((b) => {
    const removing = avail.mode === 'remove' && avail.kind === b.kind;
    const enabled = avail.mode === 'add' || removing;
    return (
      <button
        key={b.sigil}
        type="button"
        title={removing ? `Remove ${b.kind} mark` : b.title}
        disabled={!enabled}
        aria-pressed={removing}
        className={`q2-rt-tb-btn q2-rt-tb-${b.kind}${removing ? ' q2-rt-tb-active' : ''}`}
        onMouseDown={(e) => e.preventDefault()}
        onMouseUp={applyEditorialMark(b.kind)}
      >
        {b.label}
      </button>
    );
  });

  const commentButton = (
    <button
      type="button"
      title="Comment on selection"
      className="q2-rt-tb-btn q2-rt-tb-comment"
      // mousedown only keeps the editor's focus + selection (as every toolbar
      // button does); the action itself runs on mouse-up, so the wrap + commit
      // happen once the press completes rather than mid-press.
      onMouseDown={(e) => e.preventDefault()}
      onMouseUp={startComment}
    >
      💬
    </button>
  );

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
          {commentButton}
          {editorialButtons}
          {notice && <span className="q2-rt-tb-notice">{notice}</span>}
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
