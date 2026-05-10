import { useContext } from 'react';
import type { NodeArgs, NoteInline } from '../../framework';
import { NoteNumberingContext } from '../NoteNumberingContext';
import { FOOTNOTE_REF } from '../quartoClasses';
import { blocksToPlainText } from '../utils';

const TOOLTIP_BODY_CAP = 250;

/**
 * Defensive JS-side fallback for raw `Note` inlines that survive into
 * the AST under `reference-location: block` or `section` (where
 * `FootnotesTransform` no-ops upstream — see plan §"FootnotesTransform"
 * and bd-1kly).
 *
 * Renders `<sup class="footnote-ref" title="<body>">{N}</sup>` where:
 *  - `N` is looked up by object identity in `NoteNumberingContext`.
 *    A miss renders `?` so the unhandled case is visible (defensive
 *    — shouldn't happen in normal flow).
 *  - The `title=` carries `blocksToPlainText(node.c)` capped at
 *    250 chars + ellipsis.
 *
 * Class taxonomy matches the document-mode transform's `footnote-ref`
 * so the eventual tippy.js popup integration can target both paths
 * uniformly when it lands.
 *
 * Position-correct rendering (per-block / per-section footnote section)
 * is bd-1kly's job, not q2-preview's.
 */
export const Note = ({ node }: NodeArgs<NoteInline>) => {
    const numbering = useContext(NoteNumberingContext);
    const number = numbering.get(node);
    const display = number !== undefined ? String(number) : '?';

    let body = blocksToPlainText(node.c);
    if (body.length > TOOLTIP_BODY_CAP) {
        body = body.substring(0, TOOLTIP_BODY_CAP) + '…';
    }

    const props: { className: string; title?: string } = { className: FOOTNOTE_REF };
    if (body) props.title = body;

    return <sup {...props}>{display}</sup>;
};
