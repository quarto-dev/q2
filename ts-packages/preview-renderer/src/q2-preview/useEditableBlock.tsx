import React from 'react';
import { useContext } from 'react';
import { PreviewContext } from './PreviewContext';
import type { ResolvedSource } from './sourceIndex';
import { sliceBytes } from '../utils/sliceSource';

/**
 * Shared editor hook (Plan 2b, P1 + P2).
 *
 * When the given `poolId` matches `ctx.editTarget.poolId`, returns a
 * `<textarea>` sized and fonted per:
 *   - P1: `editTarget.rect` dimensions for zero-document-reflow sizing
 *   - P2: monospace at 0.9× computed body font-size
 *
 * Returns `null` when this block is not the active edit target.
 *
 * Usage (in any editable block component):
 * ```tsx
 * const editor = useEditableBlock({ poolId, resolved });
 * if (editor) return editor;
 * ```
 */
export function useEditableBlock({
    poolId,
    resolved,
}: {
    poolId: string | number | undefined;
    resolved: ResolvedSource | null;
}): React.ReactNode | null {
    const ctx = useContext(PreviewContext);

    const isEditable = resolved?.reachabilityClass === 'TopLevel'
        && ctx?.commitTextEdit !== undefined
        && ctx.content != null
        && poolId !== undefined;

    const isEditTarget = isEditable && ctx!.editTarget?.poolId === poolId;

    if (!isEditTarget || !ctx || !resolved || poolId === undefined) return null;

    const rect = ctx.editTarget!.rect;
    const initialText = sliceBytes(
        ctx.content!,
        resolved.sourceEntry.r[0],
        resolved.sourceEntry.r[1],
    ).trimEnd();

    const commit = (el: HTMLTextAreaElement) => {
        const text = el.value;
        if (!text.trim()) {
            ctx.setEditTarget!(null);
            return;
        }
        ctx.commitTextEdit!(JSON.stringify(resolved.sourceEntry), text);
        ctx.setEditTarget!(null);
    };

    return (
        <textarea
            autoFocus
            defaultValue={initialText}
            style={{
                fontFamily: 'monospace',
                fontSize: '0.9em',
                width: rect.width,
                height: rect.height,
                boxSizing: 'border-box',
                resize: 'vertical',
            }}
            onBlur={(e) => commit(e.currentTarget)}
            onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                    e.preventDefault();
                    commit(e.currentTarget);
                } else if (e.key === 'Escape') {
                    e.preventDefault();
                    ctx.setEditTarget!(null);
                }
            }}
        />
    );
}
