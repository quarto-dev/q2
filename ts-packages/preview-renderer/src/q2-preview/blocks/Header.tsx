import React, { useContext, useRef, useState } from 'react';
import { renderChildren } from '../../framework';
import type { HeaderBlock, NodeArgs } from '../../framework';
import { PreviewContext } from '..';

const headerTags = ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'] as const;

export const Header = (args: NodeArgs<HeaderBlock>) => {
    const [level, [id, classes, kvs]] = args.node.c;
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const isEditable = ctx?.commitEdit !== undefined && poolId !== undefined;

    const [editing, setEditing] = useState(false);
    const textRef = useRef<HTMLHeadingElement>(null);

    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'role') props[k] = v;
    }
    const Tag = headerTags[Math.min(Math.max(level, 1), 6) - 1];

    if (!isEditable) {
        return <Tag {...props}>{renderChildren(args)}</Tag>;
    }

    const commitCurrentText = () => {
        const el = textRef.current;
        if (!el) return;
        // Normalize non-breaking spaces that contentEditable inserts.
        const newText = el.innerText.trim().replace(/ /g, ' ');
        if (newText) {
            const hashes = '#'.repeat(Math.min(Math.max(level, 1), 6));
            ctx!.commitEdit!(poolId!, `${hashes} ${newText}\n`);
        }
        setEditing(false);
    };

    const editStyle = editing
        ? { outline: '2px solid #4a9eff', cursor: 'text' }
        : { cursor: 'pointer' };

    return (
        <Tag
            {...props}
            ref={textRef as React.RefObject<HTMLHeadingElement>}
            contentEditable={editing}
            suppressContentEditableWarning
            onClick={() => setEditing(true)}
            onBlur={commitCurrentText}
            onKeyDown={(e: React.KeyboardEvent) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    commitCurrentText();
                }
                if (e.key === 'Escape') {
                    setEditing(false);
                }
            }}
            style={editStyle}
            title={editing ? undefined : 'Click to edit'}
        >
            {renderChildren(args)}
        </Tag>
    );
};
