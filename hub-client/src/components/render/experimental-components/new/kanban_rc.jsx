const React = window.React;
const { renderChildren } = window.__REACT_AST_DEBUG_RENDERER__;

export const Div = (args) => {
    const { node: div, setLocalAst } = args;

    // Check if this is a kanban div
    const [[id, classes, attrs]] = div.c;

    if (!classes.includes('kanban')) {
        return <div id={id} className={classes.join(' ')}>{renderChildren(args)}</div>;
    }

    // Parse kanban structure
    const blocks = div.c[1];
    const columns = [];
    let currentColumn = null;

    for (const block of blocks) {
        if (block.t === 'Header' && block.c[0] === 2) {
            // New column header
            const title = block.c[2].map(inline => {
                if (inline.t === 'Str') return inline.c;
                if (inline.t === 'Space') return ' ';
                return '';
            }).join('');

            currentColumn = { title, items: [] };
            columns.push(currentColumn);
        } else if (block.t === 'BulletList' && currentColumn) {
            // Items for current column
            const items = block.c.map(listItem => {
                // Each listItem is [Block] - an array of blocks
                return listItem.map(b => {
                    if (b.t === 'Plain' || b.t === 'Para') {
                        return b.c.map(inline => {
                            if (inline.t === 'Str') return inline.c;
                            if (inline.t === 'Space') return ' ';
                            return '';
                        }).join('');
                    }
                    return '';
                }).join('');
            });
            currentColumn.items.push(...items);
        }
    }

    return <KanbanBoard columns={columns} div={div} setLocalAst={setLocalAst} />;
};

const KanbanBoard = ({ columns, div, setLocalAst }) => {
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

        // Build new AST
        const newColumns = columns.map((col) => ({
            ...col,
            items: [...col.items]
        }));

        const [movedItem] = newColumns[srcColIndex].items.splice(srcItemIndex, 1);
        newColumns[targetColIndex].items.push(movedItem);

        // Reconstruct div blocks
        const newBlocks = [];
        for (const col of newColumns) {
            // Add header
            newBlocks.push({
                t: 'Header',
                c: [2, ['', [], []], col.title.split(' ').map(word => ({ t: 'Str', c: word }))]
            });

            // Add bullet list if items exist
            if (col.items.length > 0) {
                newBlocks.push({
                    t: 'BulletList',
                    c: col.items.map(itemText => [{
                        t: 'Plain',
                        c: [{ t: 'Str', c: itemText }]
                    }])
                });
            }
        }

        const newDiv = structuredClone(div);
        newDiv.c[1] = newBlocks;
        setLocalAst(newDiv);
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
                        minWidth: '250px',
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

function parseStyleString(styleStr) {
    const style = {};
    styleStr.split(';').forEach(rule => {
        const [prop, value] = rule.split(':').map(s => s.trim());
        if (prop && value) {
            // Convert CSS property names to camelCase (e.g., 'background-color' -> 'backgroundColor')
            const camelProp = prop.replace(/-([a-z])/g, (g) => g[1].toUpperCase());
            style[camelProp] = value;
        }
    });
    return style;
}
