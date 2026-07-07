// Phase 1a (bd-sjb4pzx8) — the WYSIWYG block editor.
//
// Drop-in alternative to `EditTextarea`: rendered inside the SAME measured box,
// seeded from the block's untransformed AST subtree (astToDoc), committing
// markdown through the UNCHANGED `commitTextEdit` path. Because it lives in the
// preview iframe — which already has the Bootstrap + Quarto theme CSS loaded —
// the editor's semantic tags (<p>, <em>, <strong>, <a>) are styled by the theme
// automatically, so editing looks like the rendered page.
//
// Scope (1a): single paragraph, inline editing only. Enter is intercepted (no
// structural splits yet — that arrives in 1c). Dirtiness is read from
// ProseMirror's own change signal (`doc.eq`), NOT from comparing serialized text,
// so an unedited open-and-close is a true no-op (C3) and never reformats.

import { useEffect, useMemo, useRef } from 'react';
import { useEditor, EditorContent } from '@tiptap/react';
import { Extension } from '@tiptap/core';
import type { Editor } from '@tiptap/core';
import type { Node as PMNode } from '@tiptap/pm/model';
import type { PreviewContextValue, ResolvedSource } from './../PreviewContext';
import { buildNestingCommitDestination, buildAncestorPath } from './../nestingNav';
import { BreadcrumbCrumbs } from './../BreadcrumbCrumbs';
import { astToDoc } from './astToProseMirror';
import { docToMarkdown } from './serializer';
import { buildRichTextExtensions } from './editorConfig';
import { RichTextToolbar } from './RichTextToolbar';
import type { AstNode, PoolEntry } from './ast';
import { ensureRichTextStyles } from './styles';
import { placeCaretFromClick } from './caretFromClick';

function commitDestination(ctx: PreviewContextValue, resolved: ResolvedSource): string | null {
  if (ctx.editTargetRef !== undefined) {
    return buildNestingCommitDestination(ctx.editTargetRef.current);
  }
  return JSON.stringify(resolved.sourceEntry);
}

/** True when this editor is still the active target (guards stale-unmount blur). */
function isStillActive(ctx: PreviewContextValue, resolved: ResolvedSource): boolean {
  if (ctx.editTargetRef === undefined) return true;
  const cur = ctx.editTargetRef.current;
  return !!cur && cur.anchorR0 === resolved.sourceEntry.r[0];
}

export function RichTextEditor({
  ctx,
  resolved,
}: {
  ctx: PreviewContextValue;
  resolved: ResolvedSource;
}) {
  ensureRichTextStyles();

  // Seed the document from the AST subtree once, at mount.
  const seedJSON = useMemo(() => {
    const pool = (ctx.pool ?? []) as PoolEntry[];
    const src = ctx.content ?? '';
    const { doc } = astToDoc([resolved.sourceNode as unknown as AstNode], pool, src);
    return doc.toJSON();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The seeded doc, captured post-normalization in onCreate — the dirty baseline.
  const initialDocRef = useRef<PMNode | null>(null);
  // The original markdown the textarea seeds from (so reverting an unedited rich
  // doc restores the exact source rather than a re-serialized form).
  const originalMarkdownRef = useRef<string | null>(null);
  // Latch so a commit fires at most once (blur can follow a key-commit).
  const committedRef = useRef(false);

  // Keyboard handlers for the commit keymap (bd-hafs0qho). Populated each render
  // (below, after `commit`/`cancel` are defined) so the keymap always calls the
  // current closures without going stale.
  const keymapHandlersRef = useRef<{
    escape: () => void;
    modEnter: () => void;
    enter: () => void;
  } | null>(null);

  // Commit/cancel/plain-Enter live in tiptap's keymap — NOT a DOM keydown
  // listener (bd-hafs0qho). A DOM listener runs AFTER ProseMirror's keymap
  // plugins, so tiptap's HardBreak `Mod-Enter` (now disabled in editorConfig,
  // belt-and-suspenders) or any other binding would win the race and mutate the
  // doc before we could `preventDefault`. A high-priority keymap extension runs
  // first and returns `true`, so `Mod-Enter` deterministically commits with no
  // hard break inserted, `Escape` cancels, and plain `Enter` is swallowed
  // (no structural split; `Shift-Enter` still inserts a hard break).
  const commitKeymap = useMemo(
    () =>
      Extension.create({
        name: 'q2CommitKeymap',
        priority: 1000,
        addKeyboardShortcuts() {
          return {
            Escape: () => {
              keymapHandlersRef.current?.escape();
              return true;
            },
            'Mod-Enter': () => {
              keymapHandlersRef.current?.modEnter();
              return true;
            },
            Enter: () => {
              keymapHandlersRef.current?.enter();
              return true;
            },
          };
        },
      }),
    [],
  );

  const extensions = useMemo(
    () => [...buildRichTextExtensions(), commitKeymap],
    [commitKeymap],
  );

  const editor = useEditor({
    extensions,
    content: seedJSON,
    // Initial caret placement is owned entirely by the mount effect below (click
    // position for a mouse-open, else end-of-block) — see bd-q9lyghv2. tiptap's
    // own `autofocus` applies its end-selection in a requestAnimationFrame, which
    // RACES our placement (browser-verified: it lands on the same frame and wins,
    // pinning the caret to end). Disabling it removes the competitor so there is a
    // single source of truth for the opening selection.
    autofocus: false,
    // 1b: edit existing structure only — no markdown auto-conversion (e.g. typing
    // "## " must not turn a paragraph into a heading, or change a heading's level).
    // Structural edits are a later phase; bold/italic via Cmd-B/I still work.
    enableInputRules: false,
    enablePasteRules: false,
    onCreate({ editor: ed }) {
      initialDocRef.current = ed.state.doc;
      // editDraftRef was seeded with the original markdown at activation; remember
      // it so we can restore it verbatim if the user reverts their rich edits.
      originalMarkdownRef.current = ctx.editDraftRef?.current ?? null;
    },
    onUpdate({ editor: ed }) {
      // Keep the shared markdown draft current so a switch to plain text carries
      // the rich edits across (dirty-aware: when unchanged, restore the verbatim
      // original so an untouched toggle doesn't reformat the block — C3).
      if (!ctx.editDraftRef) return;
      const base = initialDocRef.current;
      ctx.editDraftRef.current =
        base && !ed.state.doc.eq(base)
          ? docToMarkdown(ed.state.doc)
          : originalMarkdownRef.current ?? docToMarkdown(ed.state.doc);
    },
  });

  const commit = (ed: Editor) => {
    if (committedRef.current) return;
    // A rich/plain surface swap is not a commit — the content is preserved in
    // editDraftRef; the swap must not close the edit session.
    if (ctx.editorModeSwitchRef?.current) return;
    // Stale-unmount guard (mirrors EditTextarea): a dropped/re-anchored editor's
    // blur must not write to a byte range it no longer owns.
    if (!isStillActive(ctx, resolved)) return;
    committedRef.current = true;

    const base = initialDocRef.current;
    const changed = base ? !ed.state.doc.eq(base) : false;
    if (!changed) {
      // True no-op: unedited open-and-close never reformats (C3).
      ctx.setEditTarget?.(null);
      return;
    }
    const dest = commitDestination(ctx, resolved);
    if (dest === null) {
      ctx.setEditTarget?.(null);
      return;
    }
    ctx.commitTextEdit?.(dest, docToMarkdown(ed.state.doc));
    ctx.setEditTarget?.(null);
  };

  const cancel = () => {
    committedRef.current = true;
    ctx.requestFocusRestore?.(resolved.sourceEntry.r[0]);
    ctx.setEditTarget?.(null);
  };

  // Keep the commit keymap's handlers pointed at the current closures. Assigned
  // during render (idempotent, no external effect) so a keypress after any
  // render calls fresh `commit`/`cancel` — no stale-closure window.
  keymapHandlersRef.current = {
    escape: cancel,
    modEnter: () => {
      if (!editor) return;
      ctx.requestFocusRestore?.(resolved.sourceEntry.r[0]);
      commit(editor);
    },
    // Swallow plain Enter — no structural split in this single-block editor.
    // (Shift-Enter is a hard break, handled by the HardBreak extension.)
    enter: () => {},
  };

  // The whole edit box (editor + toolbar + link input) is one focus scope: we
  // commit only when focus leaves it entirely, so focusing the toolbar's link
  // input keeps the session open.
  const rootRef = useRef<HTMLDivElement | null>(null);

  // bd-q9lyghv2 caret-at-click: own the editor's opening caret. Consume the
  // viewport coords of the mouse click that opened this editor (stashed by
  // useBlockEditHover) and place the caret there via posAtCoords — so the FIRST
  // click lands the cursor where the user clicked. With no coords (keyboard/touch
  // open, or a posAtCoords miss) we focus at end-of-block — the historical
  // default that `autofocus:'end'` used to provide before we disabled it.
  //
  // Read-and-CLEAR the coords exactly once: a self-heal re-anchor can remount this
  // editor, but by then the document has reflowed and the captured coordinates are
  // stale (the block moved on screen), so the remount falls back to end-of-block
  // rather than re-replaying a now-wrong click. Nulling the ref here guarantees that.
  //
  // The placement runs in a requestAnimationFrame so the swapped-in editor box is
  // laid out before posAtCoords reads geometry, and (with autofocus disabled)
  // nothing competes to reset the selection afterward.
  useEffect(() => {
    if (!editor) return;
    const coords = ctx.pendingClickCoordsRef?.current ?? null;
    if (ctx.pendingClickCoordsRef) ctx.pendingClickCoordsRef.current = null;
    const raf = requestAnimationFrame(() => {
      if (editor.isDestroyed) return;
      if (coords && placeCaretFromClick(editor, coords)) return;
      editor.commands.focus('end');
    });
    return () => cancelAnimationFrame(raf);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor]);

  // Esc/Mod-Enter/plain-Enter are handled by the commit keymap (bd-hafs0qho);
  // see `commitKeymap` above. This effect only wires the focusout commit: a
  // focus move OUT of the edit box (not into the toolbar/link input) commits.
  useEffect(() => {
    if (!editor) return;
    const root = rootRef.current;
    const onFocusOut = (e: FocusEvent) => {
      // A surface swap fires this as the editor unmounts — not a commit.
      if (ctx.editorModeSwitchRef?.current) return;
      const next = e.relatedTarget as Node | null;
      // Focus stayed within the edit box (toolbar button / link input) — not a commit.
      if (next && root && root.contains(next)) return;
      ctx.requestFocusRestore?.(resolved.sourceEntry.r[0]);
      commit(editor);
    };
    root?.addEventListener('focusout', onFocusOut);
    return () => {
      root?.removeEventListener('focusout', onFocusOut);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor]);

  // Inline nesting breadcrumb (bd-9x3zbuj8 Task 2): when the nesting cursor is
  // on, the hierarchy navigator renders to the RIGHT of the formatting buttons in
  // the SAME toolbar row, instead of as a separate floating chip that would
  // overlap the toolbar. The standalone BreadcrumbChip self-suppresses for this
  // exact case (rich editor active for the target). Built here from the live edit
  // target so it tracks nesting in/out moves.
  const et = ctx.editTarget;
  const inlineBreadcrumb =
    ctx.unlockNestingCursor && et
      ? (() => {
          const crumbs = buildAncestorPath(ctx.sourceIndex, et.anchorR0, et.anchorR1);
          if (crumbs.length === 0) return null;
          return (
            <BreadcrumbCrumbs
              layout="inline"
              displayItems={crumbs.map((crumb) => ({ kind: 'crumb' as const, crumb }))}
            />
          );
        })()
      : null;

  // The left-margin affordance (Editing… + rich/plain toggle) is rendered by
  // renderMeasuredEdit so it is shared with the plain-text surface. The
  // formatting toolbar is rich-only and lives here (it needs the editor).
  return (
    <div className="q2-richtext-editor" ref={rootRef}>
      {editor && <RichTextToolbar editor={editor} trailing={inlineBreadcrumb} />}
      {editor && <EditorContent editor={editor} />}
    </div>
  );
}
