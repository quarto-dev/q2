import { Node } from '../../framework';
import type { BlockNode, InlineNode, NodeArgs, TableBlock } from '../../framework';

/**
 * Table → `<table>` with optional `<caption>` / `<thead>` / `<tbody>` /
 * `<tfoot>`. Matches Pandoc's HTML writer output structurally.
 *
 * Pandoc Table AST shape:
 *   c[0] = Attr
 *   c[1] = Caption: (shortCaption|null, blocks)
 *   c[2] = ColSpec[]: (Alignment, ColWidth)
 *   c[3] = TableHead: (Attr, Row[])
 *   c[4] = TableBody[]: (Attr, RowHeadColumns, Row[], Row[])[]
 *   c[5] = TableFoot: (Attr, Row[])
 *
 * Row = (Attr, Cell[])
 * Cell = (Attr, Alignment, RowSpan, ColSpan, BlockNode[])
 *
 * v1 q2-preview targets structural parity. setLocalAst integration for
 * cell content is skipped — table cells aren't editable in q2-preview's
 * v1 (per plan §"Out of scope: Edit affordances"). The component emits
 * a NOOP into <Node> for cells so the framework's atomic gate stays
 * the source of truth on read-only behavior.
 */

type Alignment = { t: 'AlignDefault' | 'AlignLeft' | 'AlignRight' | 'AlignCenter' };
type Row = [Attr, Cell[]];
type Cell = [Attr, Alignment, number, number, BlockNode[]];
type Attr = [string, string[], [string, string][]];

const NOOP: (n: BlockNode | InlineNode) => void = () => {};

const alignClass = (a: Alignment | undefined): string | undefined => {
    switch (a?.t) {
        case 'AlignLeft':
            return 'text-left';
        case 'AlignRight':
            return 'text-right';
        case 'AlignCenter':
            return 'text-center';
        default:
            return undefined;
    }
};

function attrToProps(attr: Attr | undefined): Record<string, string> {
    const props: Record<string, string> = {};
    if (!attr) return props;
    const [id, classes, kvs] = attr;
    if (id) props.id = id;
    if (classes && classes.length) props.className = classes.join(' ');
    if (kvs) {
        for (const [k, v] of kvs) {
            if (k.startsWith('data-')) props[k] = v;
        }
    }
    return props;
}

function CellNode({
    cell,
    asHeader,
    onNavigateToDocument,
}: {
    cell: Cell;
    asHeader: boolean;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) {
    const [attr, align, rowSpan, colSpan, blocks] = cell;
    const props = attrToProps(attr);
    const cls = alignClass(align);
    if (cls) props.className = props.className ? `${props.className} ${cls}` : cls;
    const Tag = (asHeader ? 'th' : 'td') as 'th' | 'td';
    const spanProps: Record<string, number> = {};
    if (rowSpan && rowSpan > 1) spanProps.rowSpan = rowSpan;
    if (colSpan && colSpan > 1) spanProps.colSpan = colSpan;
    return (
        <Tag {...props} {...spanProps}>
            {blocks.map((b, i) => (
                <Node key={i} node={b} setLocalAst={NOOP} onNavigateToDocument={onNavigateToDocument} />
            ))}
        </Tag>
    );
}

function RowNode({
    row,
    asHeader,
    onNavigateToDocument,
}: {
    row: Row;
    asHeader: boolean;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) {
    const [attr, cells] = row;
    return (
        <tr {...attrToProps(attr)}>
            {cells.map((cell, i) => (
                <CellNode
                    key={i}
                    cell={cell as Cell}
                    asHeader={asHeader}
                    onNavigateToDocument={onNavigateToDocument}
                />
            ))}
        </tr>
    );
}

export const Table = (args: NodeArgs<TableBlock>) => {
    const { node, onNavigateToDocument } = args;
    const c = node.c as unknown as [
        Attr,
        [InlineNode[] | null, BlockNode[]],
        unknown[],
        [Attr, Row[]],
        [Attr, number, Row[], Row[]][],
        [Attr, Row[]],
    ];
    const [attr, [, captionBlocks], , [, headRows], bodies, [, footRows]] = c;

    return (
        <table {...attrToProps(attr)}>
            {captionBlocks && captionBlocks.length > 0 && (
                <caption>
                    {captionBlocks.map((b, i) => (
                        <Node
                            key={i}
                            node={b}
                            setLocalAst={NOOP}
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    ))}
                </caption>
            )}
            {headRows && headRows.length > 0 && (
                <thead>
                    {headRows.map((r, i) => (
                        <RowNode
                            key={i}
                            row={r}
                            asHeader={true}
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    ))}
                </thead>
            )}
            {bodies?.map(([bodyAttr, , bodyHead, bodyRows], bi) => (
                <tbody key={bi} {...attrToProps(bodyAttr)}>
                    {bodyHead.map((r, i) => (
                        <RowNode
                            key={`bh-${i}`}
                            row={r}
                            asHeader={true}
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    ))}
                    {bodyRows.map((r, i) => (
                        <RowNode
                            key={`br-${i}`}
                            row={r}
                            asHeader={false}
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    ))}
                </tbody>
            ))}
            {footRows && footRows.length > 0 && (
                <tfoot>
                    {footRows.map((r, i) => (
                        <RowNode
                            key={i}
                            row={r}
                            asHeader={false}
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    ))}
                </tfoot>
            )}
        </table>
    );
};
