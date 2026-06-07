/**
 * SourceInfo-value index for q2-preview's structural editability gate (Plan 2a).
 *
 * Builds a `Map<string, SourceIndexEntry>` from the untransformed AST JSON,
 * keyed by the serialized SourceInfo value of each Original, active-file block.
 *
 * Key format: `"${t}:${r[0]}-${r[1]}:${d}"` — deterministic, no JSON parse
 * overhead, unambiguous (e.g. `"0:42-87:0"`).
 *
 * Correspondence is by value, so it survives transforms that move blocks
 * (appendix-structure, footnotes) or insert structure (sectionize → Generated
 * Div → absent from index).
 */

import type { BlockNode } from '../framework/types';

export type ReachabilityClass = 'TopLevel' | 'Descendable' | 'Opaque';

export interface SourceIndexEntry {
    sourceNode: BlockNode;
    reachabilityClass: ReachabilityClass;
}

export interface ResolvedSource {
    sourceNode: BlockNode;
    reachabilityClass: ReachabilityClass;
    sourceEntry: { t: 0; r: [number, number]; d: number };
}

export function serializeSourceEntry(entry: {
    t: 0;
    r: [number, number];
    d: number;
}): string {
    return `${entry.t}:${entry.r[0]}-${entry.r[1]}:${entry.d}`;
}

type RawPool = Array<{ t: number; r: [number, number]; d: unknown }>;

function childClass(
    parent: ReachabilityClass,
    kind: 'Descendable' | 'Opaque',
): ReachabilityClass {
    return parent === 'Opaque' ? 'Opaque' : kind;
}

function indexBlock(
    block: BlockNode,
    pool: RawPool,
    reachability: ReachabilityClass,
    index: Map<string, SourceIndexEntry>,
): void {
    const s = (block as unknown as { s?: unknown }).s;
    if (s !== undefined) {
        const entry = pool[Number(s)];
        if (entry?.t === 0 && entry.d === 0) {
            const key = serializeSourceEntry(
                entry as { t: 0; r: [number, number]; d: number },
            );
            index.set(key, { sourceNode: block, reachabilityClass: reachability });
        }
    }
    descendBlock(block, pool, reachability, index);
}

function walkBlocks(
    blocks: BlockNode[],
    pool: RawPool,
    reachability: ReachabilityClass,
    index: Map<string, SourceIndexEntry>,
): void {
    if (!Array.isArray(blocks)) return;
    for (const block of blocks) {
        indexBlock(block, pool, reachability, index);
    }
}

function walkTableRows(
    rows: unknown[],
    pool: RawPool,
    index: Map<string, SourceIndexEntry>,
): void {
    if (!Array.isArray(rows)) return;
    for (const row of rows) {
        // row = [Attr, Cell[]] where Cell = [Attr, Align, RowSpan, ColSpan, BlockNode[]]
        const cells = (row as unknown[])[1];
        if (!Array.isArray(cells)) continue;
        for (const cell of cells as unknown[][]) {
            const blocks = cell[4] as BlockNode[];
            walkBlocks(blocks, pool, 'Opaque', index);
        }
    }
}

function descendBlock(
    block: BlockNode,
    pool: RawPool,
    reachability: ReachabilityClass,
    index: Map<string, SourceIndexEntry>,
): void {
    const c = (block as unknown as { c?: unknown }).c;
    switch (block.t) {
        case 'Div': {
            const next = childClass(reachability, 'Descendable');
            walkBlocks((c as [unknown, BlockNode[]])[1], pool, next, index);
            break;
        }
        case 'BlockQuote': {
            const next = childClass(reachability, 'Descendable');
            walkBlocks(c as BlockNode[], pool, next, index);
            break;
        }
        case 'BulletList': {
            const next = childClass(reachability, 'Descendable');
            for (const item of c as BlockNode[][]) {
                walkBlocks(item, pool, next, index);
            }
            break;
        }
        case 'OrderedList': {
            const next = childClass(reachability, 'Descendable');
            for (const item of (c as [unknown, BlockNode[][]])[1]) {
                walkBlocks(item, pool, next, index);
            }
            break;
        }
        case 'DefinitionList': {
            const next = childClass(reachability, 'Descendable');
            for (const [, defBlocks] of c as [unknown, BlockNode[][]][]) {
                for (const defs of defBlocks) {
                    walkBlocks(defs, pool, next, index);
                }
            }
            break;
        }
        case 'Figure': {
            // c[2] = body blocks → Descendable
            // c[1] = [shortCaption, longCaptionBlocks] → Opaque
            const fc = c as [unknown, [unknown, BlockNode[]], BlockNode[]];
            const bodyClass = childClass(reachability, 'Descendable');
            walkBlocks(fc[2], pool, bodyClass, index);
            walkBlocks(fc[1][1], pool, 'Opaque', index);
            break;
        }
        case 'Table': {
            // c[1] = [shortCaption, captionBlocks] → Opaque
            // c[3] = TableHead: [Attr, Row[]]
            // c[4] = TableBody[]: [Attr, RowHeadColumns, Row[], Row[]][]
            // c[5] = TableFoot: [Attr, Row[]]
            const tc = c as [
                unknown,
                [unknown, BlockNode[]],
                unknown,
                [unknown, unknown[]],
                unknown[][],
                [unknown, unknown[]],
            ];
            walkBlocks(tc[1][1], pool, 'Opaque', index);
            walkTableRows(tc[3][1] as unknown[], pool, index);
            for (const body of tc[4] as unknown[][]) {
                walkTableRows(body[2] as unknown[], pool, index); // head rows
                walkTableRows(body[3] as unknown[], pool, index); // body rows
            }
            walkTableRows(tc[5][1] as unknown[], pool, index);
            break;
        }
        default:
            break;
    }
}

/**
 * Build a SourceInfo-value index from the untransformed AST JSON.
 *
 * Returns `null` when input is absent or unparseable, or when the AST
 * carries no pool (`astContext.p`).
 */
export function buildSourceIndex(
    untransformedAstJson: string | null | undefined,
): Map<string, SourceIndexEntry> | null {
    if (!untransformedAstJson) return null;

    let ast: { blocks?: unknown; astContext?: { p?: unknown } };
    try {
        ast = JSON.parse(untransformedAstJson);
    } catch {
        return null;
    }

    const pool = ast.astContext?.p as RawPool | undefined;
    if (!pool) return null;

    const index = new Map<string, SourceIndexEntry>();
    walkBlocks(ast.blocks as BlockNode[], pool, 'TopLevel', index);
    return index;
}
