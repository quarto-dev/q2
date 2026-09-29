/**
 * Comment chrome for SPANS (prototype).
 *
 * A comment written inside a span associates with the span rather than
 * the whole block, mirroring the block comment container:
 *
 *     Some prose [this is my span [>> this is a comment on the span]] tail.
 *
 * The qmd reader yields a `Span` whose content ends in a
 * `quarto-edit-comment` span; the qmd writer emits exactly this syntax
 * back (`[…]` for an attribute-less span, `[>> …]` for the comment).
 *
 * This component is the registry's `Inline` entry. It wraps the raw
 * `dispatchers.tsx` `Inline` dispatcher and, for a `Span` holding
 * comment spans as direct children, strips them from the rendered copy
 * and shows them in the shared bubble chrome (`CommentWrapper`),
 * anchored to the span's own `<span>` element (`Span.tsx` registers it
 * via `useCommentAnchorRef`) but placed in the RIGHT MARGIN — beside the
 * span's line, just past the content edge — never over the text. The commented span gets a dotted
 * underline so the association reads without hovering; hovering the
 * span anywhere reveals the chrome (blocks use their right half only).
 *
 * Adds and resolves commit through the ENCLOSING BLOCK: inlines are not
 * in the source index, so the span resolves the block it sits in
 * (`EnclosingBlockContext`) to its source node, then finds itself
 * inside that node by source-range value (the same by-value
 * correspondence `resolveSource` uses for blocks), edits that span's
 * content, and commits the whole block.
 *
 * Out of scope for the prototype: comments nested in `Emph` / `Strong`
 * / `Link` (they stay inline in the text as before), and synthesized
 * `quarto-*` spans (note references, math-with-attribute, …).
 */
import React from 'react';
import type { BlockNode, InlineNode, NodeArgs, SpanInline } from '../../framework';
import { Inline as I } from '../dispatchers';
import { PreviewContext } from '../PreviewContext';
import type { CommentsMode } from '../PreviewContext';
import { usePreviewEdit } from '../usePreviewEdit';
import { claimPendingCommentOpen } from '../commentPending';
import {
    CommentWrapper,
    EDITORIAL_MARK_CLASSES,
    EnclosingBlockContext,
    isComment,
    sameCommentableKind,
} from './CommentBlock';

/** Class the rendered copy of a commented span carries (styled in CommentBlock's injected rules). */
export const COMMENTED_SPAN_CLASS = 'q2-commented-span';

type PoolEntry = { t: number; r: [number, number] };

function sameRange(a: PoolEntry | undefined, b: PoolEntry | undefined): boolean {
    return !!a && !!b && a.t === 0 && b.t === 0 && a.r[0] === b.r[0] && a.r[1] === b.r[1];
}

/**
 * Depth-first search of `root` (any AST subtree) for a `Span` whose
 * source-pool entry covers `range`. Generic over the JSON shape so it
 * sees spans wherever they nest (list items, div children, emphasis…).
 */
function findSpanByRange(root: unknown, pool: PoolEntry[], range: PoolEntry): SpanInline | null {
    if (Array.isArray(root)) {
        for (const item of root) {
            const hit = findSpanByRange(item, pool, range);
            if (hit) return hit;
        }
        return null;
    }
    if (root && typeof root === 'object') {
        const obj = root as { t?: unknown; s?: unknown; c?: unknown };
        if (obj.t === 'Span' && typeof obj.s === 'number' && sameRange(pool[obj.s], range)) {
            return root as SpanInline;
        }
        return findSpanByRange(obj.c, pool, range);
    }
    return null;
}

/** Plain-text projection of inlines (Str/Space/Code, recursing into containers), for matching. */
function inlinesText(items: InlineNode[]): string {
    let out = '';
    for (const n of items) {
        const node = n as { t: string; c?: unknown };
        if (node.t === 'Str') out += node.c as string;
        else if (node.t === 'Space' || node.t === 'SoftBreak' || node.t === 'LineBreak') out += ' ';
        else if (node.t === 'Code') out += (node.c as [unknown, string])[1];
        else if (node.t === 'Span' || node.t === 'Link') out += inlinesText((node.c as [unknown, InlineNode[]])[1]);
        else if (node.t === 'Quoted') out += inlinesText((node.c as [unknown, InlineNode[]])[1]);
        else if (Array.isArray(node.c)) out += inlinesText(node.c as InlineNode[]);
    }
    return out;
}

/** A span the prototype leaves alone: editorial marks and synthesized `quarto-*` spans. */
function isPlainAuthoredSpan(span: SpanInline): boolean {
    const classes = span.c[0][1] ?? [];
    return !classes.some((c) => EDITORIAL_MARK_CLASSES.has(c) || c.startsWith('quarto-'));
}

export const CommentInline = (args: NodeArgs<InlineNode>) => {
    const edit = usePreviewEdit();
    const previewCtx = React.useContext(PreviewContext);
    const enclosing = React.useContext(EnclosingBlockContext);
    const mode: CommentsMode = previewCtx?.commentsMode ?? 'show';
    const { node, onNavigateToDocument, setLocalAst } = args;
    // Decided once per mount: did the block editor just wrap this span for
    // commenting? (`null` = not yet checked.)
    const initialOpenRef = React.useRef<boolean | null>(null);

    const passthrough = <I node={node} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />;
    if (node.t !== 'Span' || !isPlainAuthoredSpan(node as SpanInline)) return passthrough;
    const span = node as SpanInline;
    const comments = span.c[1].filter(isComment);

    /**
     * Resolve the enclosing block to a committable source node and check
     * this span is present in it. Null when the span cannot be edited
     * safely: no enclosing block, no pool ids, the block is Opaque (table
     * cell) or a lossy transform product, or the span is not found in the
     * source (it was synthesized by a transform). Read-only — no clone —
     * since this runs on every render of every plain span.
     */
    const resolveTarget = () => {
        const pool = previewCtx?.pool as PoolEntry[] | undefined;
        const s = (span as { s?: unknown }).s;
        if (!enclosing || !pool || typeof s !== 'number') return null;
        const range = pool[s];
        if (!range || range.t !== 0) return null;
        const resolved = edit.resolveSource(enclosing);
        if (!resolved || !resolved.sourceNode || !resolved.sourcePool) return null;
        if (resolved.reachabilityClass === 'Opaque') return null;
        if (!sameCommentableKind(enclosing, resolved.sourceNode)) return null;
        const sourcePool = resolved.sourcePool as PoolEntry[];
        if (!findSpanByRange(resolved.sourceNode, sourcePool, range)) return null;
        return { resolved, sourcePool, range };
    };

    /** Clone the source block for editing and find this span in the clone. */
    const locate = (): { sourceEntry: object; modified: BlockNode; target: SpanInline } | null => {
        const hit = resolveTarget();
        if (!hit) return null;
        const modified = structuredClone(hit.resolved.sourceNode);
        const target = findSpanByRange(modified, hit.sourcePool, hit.range);
        if (!target) return null;
        return { sourceEntry: hit.resolved.sourceEntry, modified, target };
    };

    // A comment-less span only gets the hover `+` affordance when a
    // comment could actually be committed to it.
    if (comments.length === 0 && resolveTarget() === null) return passthrough;
    if (initialOpenRef.current === null) {
        initialOpenRef.current =
            comments.length === 0 && claimPendingCommentOpen(inlinesText(span.c[1]));
    }

    // Rendered copy: comment spans stripped; commented spans marked.
    const rendered = structuredClone(span);
    rendered.c[1] = rendered.c[1].filter((n) => !isComment(n));
    if (comments.length > 0) {
        rendered.c[0] = [rendered.c[0][0], [...rendered.c[0][1], COMMENTED_SPAN_CLASS], rendered.c[0][2]];
    }
    const content = <I node={rendered} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />;
    if (mode === 'hide') return content;

    const addComment = (text: string) => {
        const found = locate();
        if (!found) return;
        found.target.c[1].push({
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: text }]],
        });
        edit.commitSubtreeEdit(JSON.stringify(found.sourceEntry), found.modified);
    };

    const resolveCommentAtIndex = (index: number) => {
        const found = locate();
        if (!found) return;
        const arr = found.target.c[1];
        let seen = -1;
        for (let i = 0; i < arr.length; i++) {
            if (!isComment(arr[i])) continue;
            seen++;
            if (seen === index) {
                arr.splice(i, 1);
                break;
            }
        }
        edit.commitSubtreeEdit(JSON.stringify(found.sourceEntry), found.modified);
    };

    return (
        <CommentWrapper
            comments={comments}
            anchorNode={rendered}
            hoverWholeAnchor
            placement="margin"
            initialOpen={initialOpenRef.current}
            addComment={addComment}
            resolveCommentAtIndex={resolveCommentAtIndex}
            mode={mode}
            onNavigateToDocument={onNavigateToDocument}
        >
            {content}
        </CommentWrapper>
    );
};
