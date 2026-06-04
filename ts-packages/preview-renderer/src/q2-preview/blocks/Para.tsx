import { useContext, useRef, useState } from 'react';
import { renderChildren } from '../../framework';
import type { NodeArgs, ParaBlock } from '../../framework';
import { PreviewContext } from '..';

export const Para = (args: NodeArgs<ParaBlock>) => {
    const ctx = useContext(PreviewContext);
    // pool id from the block's source_info reference (may be absent)
    const poolId = (args.node as any).s as string | number | undefined;
    const isEditable = ctx?.commitEdit !== undefined && poolId !== undefined;

    const [editing, setEditing] = useState(false);
    const textRef = useRef<HTMLParagraphElement>(null);

    if (!isEditable) {
        return <p>{renderChildren(args)}</p>;
    }

    const commitCurrentText = () => {
        const el = textRef.current;
        if (!el) return;
        // Normalize   (non-breaking space) that contentEditable inserts
        // at text boundaries to prevent whitespace collapsing.
        const newText = el.innerText.trim().replace(/ /g, ' ');
        if (newText) {
            ctx!.commitEdit!(poolId!, newText + '\n');
        }
        setEditing(false);
    };

    return (
        <p
            ref={textRef}
            contentEditable={editing}
            suppressContentEditableWarning
            onClick={() => setEditing(true)}
            onBlur={commitCurrentText}
            onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    commitCurrentText();
                }
                if (e.key === 'Escape') {
                    setEditing(false);
                }
            }}
            style={editing ? { outline: '2px solid #4a9eff', cursor: 'text' } : { cursor: 'pointer' }}
            title={editing ? undefined : 'Click to edit'}
        >
            {renderChildren(args)}
        </p>
    );
};
