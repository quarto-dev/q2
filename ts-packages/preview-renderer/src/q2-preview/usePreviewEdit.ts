import { useContext } from 'react';
import type { BlockNode } from '../framework/types';
import { PreviewContext } from './PreviewContext';
import type { ResolvedSource } from './sourceIndex';

/**
 * Public hook for render-component authors.
 *
 * Returns { resolveSource, commitSubtreeEdit, commitTextEdit } sourced from
 * `PreviewContext`. When the context is absent (q2-debug, q2-slides — which
 * never provide `PreviewContext`), all three functions are no-ops so
 * components can call `edit.commitSubtreeEdit?.()` and degrade silently.
 *
 * Access via `window.__Q2_PREVIEW_RENDERER__.usePreviewEdit`.
 */
export function usePreviewEdit(): {
    resolveSource: (node: BlockNode) => ResolvedSource | null;
    commitSubtreeEdit: (destinationSourceInfoJson: string, modifiedBlock: BlockNode) => void;
    commitTextEdit: (destinationSourceInfoJson: string, newText: string) => void;
} {
    const ctx = useContext(PreviewContext);
    return {
        resolveSource: ctx?.resolveSource ?? (() => null),
        commitSubtreeEdit: ctx?.commitSubtreeEdit ?? (() => undefined),
        commitTextEdit: ctx?.commitTextEdit ?? (() => undefined),
    };
}
