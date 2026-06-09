const React = window.React;
const { renderChildren, usePreviewEdit } = window.__Q2_PREVIEW_RENDERER__;

export const Div = (args) => {
    const edit = usePreviewEdit();
    const { node: div } = args;

    const [[id, classes]] = div.c;

    if (!classes.includes('kanban')) {
        return <div id={id} className={classes.join(' ')}>{renderChildren(args)}</div>;
    }

    // Parse kanban structure from the transformed node (for display)
    const blocks = div.c[1];
    const columns = [];
    let currentColumn = null;

    for (const block of blocks) {
        if (block.t === 'Header' && block.c[0] === 2) {
            const title = block.c[2].map(inline => {
                if (inline.t === 'Str') return inline.c;
                if (inline.t === 'Space') return ' ';
                return '';
            }).join('');
            currentColumn = { title, items: [] };
            columns.push(currentColumn);
        } else if (block.t === 'BulletList' && currentColumn) {
            const items = block.c.map(listItem =>
                listItem.map(b => {
                    if (b.t === 'Plain' || b.t === 'Para') {
                        return b.c.map(inline => {
                            if (inline.t === 'Str') return inline.c;
                            if (inline.t === 'Space') return ' ';
                            return '';
                        }).join('');
                    }
                    return '';
                }).join('')
            );
            currentColumn.items.push(...items);
        }
    }

    const onMove = (newColumns) => {
        const resolved = edit.resolveSource(div);
        if (!resolved) return;

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
