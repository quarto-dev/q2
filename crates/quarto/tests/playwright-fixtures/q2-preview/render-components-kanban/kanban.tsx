const React = window.React;
const { renderChildren, usePreviewEdit } = window.__Q2_PREVIEW_RENDERER__;

const inlineText = (inlines) => inlines.map(inline => {
    if (inline.t === 'Str') return inline.c;
    if (inline.t === 'Space') return ' ';
    return '';
}).join('');

const isSection = (block) =>
    block.t === 'Div' && block.c[0][1].includes('section');

export const Div = (args) => {
    const edit = usePreviewEdit();
    const { node: div } = args;

    const [[id, classes]] = div.c;

    if (!classes.includes('kanban')) {
        return <div id={id} className={classes.join(' ')}>{renderChildren(args)}</div>;
    }

    // Parse kanban structure from the *transformed* node, for display.
    //
    // Each column arrives as a section Div, not a bare Header:
    // `SectionizeTransform` wraps every heading and the content that
    // follows it, including headings nested inside a Div like this one.
    //
    //   Div(.kanban)
    //     Div(#backlog .section .level2)[ Header(2) "backlog", BulletList ]
    //     Div(#doing   .section .level2)[ Header(2) "doing",   BulletList ]
    //
    // Reading the transformed AST means reading it as the pipeline
    // leaves it. The grouping is a convenience here — a column's heading
    // and its cards arrive together instead of having to be stitched
    // back up from a flat run of siblings.
    const blocks = div.c[1];
    const columns = [];

    for (const block of blocks) {
        if (!isSection(block)) continue;
        const children = block.c[1];
        const header = children.find(b => b.t === 'Header' && b.c[0] === 2);
        if (!header) continue;

        const column = { title: inlineText(header.c[2]), items: [] };
        for (const child of children) {
            if (child.t !== 'BulletList') continue;
            column.items.push(...child.c.map(listItem =>
                listItem.map(b =>
                    (b.t === 'Plain' || b.t === 'Para') ? inlineText(b.c) : ''
                ).join('')
            ));
        }
        columns.push(column);
    }

    const onMove = (newColumns) => {
        const resolved = edit.resolveSource(div);
        if (!resolved) return;

        // The write path is deliberately *not* symmetric with the read
        // path above. `resolveSource` hands back the node as it appears
        // in the source document, which is pre-transform: flat Headers,
        // no sections. Emitting sections here would write them into the
        // user's .qmd.
        const newBlocks = [];
        for (const col of newColumns) {
            newBlocks.push({
                t: 'Header',
                c: [2, ['', [], []], col.title.split(' ').flatMap(word => [{ t: 'Str', c: word }, { t: 'Space' }])]
            });
            if (col.items.length > 0) {
                newBlocks.push({
                    t: 'BulletList',
                    c: col.items.map(itemText => [{ t: 'Plain', c: [{ t: 'Str', c: itemText }] }])
                });
            }
        }

        const modified = structuredClone(resolved.sourceNode);
        modified.c[1] = newBlocks;
        edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), modified);
    };

    return <KanbanBoard columns={columns} onMove={onMove} />;
};

const KanbanBoard = ({ columns, onMove }) => {
    const [draggedItem, setDraggedItem] = React.useState(null);

    const handleDragStart = (colIndex, itemIndex) => {
        setDraggedItem({ colIndex, itemIndex });
    };

    const handleDrop = (targetColIndex) => {
        if (!draggedItem) return;

        const { colIndex: srcColIndex, itemIndex: srcItemIndex } = draggedItem;
        if (srcColIndex === targetColIndex) {
            setDraggedItem(null);
            return;
        }

        const newColumns = columns.map((col) => ({ ...col, items: [...col.items] }));
        const [movedItem] = newColumns[srcColIndex].items.splice(srcItemIndex, 1);
        newColumns[targetColIndex].items.push(movedItem);

        onMove(newColumns);
        setDraggedItem(null);
    };

    return (
        <div style={{
            display: 'flex',
            gap: '16px',
            padding: '16px',
            backgroundColor: '#f5f5f5',
            borderRadius: '8px',
            overflowX: 'auto'
        }}>
            {columns.map((col, colIndex) => (
                <div
                    key={colIndex}
                    onDragOver={(e) => e.preventDefault()}
                    onDrop={() => handleDrop(colIndex)}
                    style={{
                        minWidth: '150px',
                        backgroundColor: '#fff',
                        borderRadius: '8px',
                        padding: '12px',
                        boxShadow: '0 2px 4px rgba(0,0,0,0.1)'
                    }}
                >
                    <h3 style={{ margin: '0 0 12px 0', fontSize: '1rem', fontWeight: 'bold' }}>
                        {col.title}
                    </h3>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                        {col.items.map((item, itemIndex) => (
                            <div
                                key={itemIndex}
                                draggable
                                onDragStart={() => handleDragStart(colIndex, itemIndex)}
                                style={{
                                    padding: '8px',
                                    backgroundColor: '#fafafa',
                                    border: '1px solid #e0e0e0',
                                    borderRadius: '4px',
                                    cursor: 'move',
                                    fontSize: '0.875rem'
                                }}
                            >
                                {item}
                            </div>
                        ))}
                    </div>
                </div>
            ))}
        </div>
    );
};
