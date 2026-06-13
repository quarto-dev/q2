/**
 * r[0] uniqueness regression test (Phase 2.1).
 *
 * Verifies assumption B from the block-editing plan:
 *   In real Q2 pool output, every Original block's r[0] is distinct.
 *   A container and its first child do NOT share a start offset.
 *
 * This test loads committed AST JSON produced by the real pampa parser,
 * walks the *block* tree collecting each block node's pool entry, filters
 * to Original entries (t === 0, d === 0), and asserts all r[0] values are
 * distinct (no duplicates).
 *
 * The uniqueness check uses an explicit duplicate-detection loop (not a
 * deduplicating Map keyed by the entry) so a real collision is surfaced as a
 * failing assertion, not silently hidden.
 *
 * If this test fails with a real collision, stop and report — that contradicts
 * assumption B and matters for the anchorR0 identity scheme.
 */

import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));

// ── Types ────────────────────────────────────────────────────────────────────

type OriginalEntry = { t: 0; r: [number, number]; d: 0 };
type PoolEntry = { t: number; r: [number, number]; d: unknown };
type RawPool = PoolEntry[];

// A minimal block-node shape: any object with a `t` string and optional `c`/`s`.
type RawBlock = { t: string; s?: number; c?: unknown };

// ── Block tree walker ────────────────────────────────────────────────────────

/**
 * Walk a block list, collecting (block.t, pool entry) pairs for every block
 * node that carries an `s` field whose pool entry is an Original (t === 0, d === 0).
 *
 * Mirrors the descent in `sourceIndex.ts::descendBlock`, covering:
 * - Div           c = [Attr, BlockNode[]]
 * - BlockQuote    c = BlockNode[]
 * - BulletList    c = BlockNode[][]
 * - OrderedList   c = [ListAttrs, BlockNode[][]]
 * - DefinitionList c = [Term, BlockNode[][]][]
 * - Figure        c = [Attr, Caption, BlockNode[]]
 *
 * Inline nodes (Str, Space, Link, etc.) are skipped — they are NOT blocks.
 */
function collectBlockEntries(
    blocks: RawBlock[],
    pool: RawPool,
    out: OriginalEntry[],
): void {
    if (!Array.isArray(blocks)) return;
    for (const block of blocks) {
        if (!block || typeof block.t !== 'string') continue;
        if (block.s !== undefined) {
            const entry = pool[block.s];
            if (entry && entry.t === 0 && entry.d === 0) {
                out.push(entry as OriginalEntry);
            }
        }
        descendBlock(block, pool, out);
    }
}

function descendBlock(
    block: RawBlock,
    pool: RawPool,
    out: OriginalEntry[],
): void {
    const c = block.c;
    switch (block.t) {
        case 'Div': {
            // c = [Attr, BlockNode[]]
            const children = (c as [unknown, RawBlock[]])[1];
            collectBlockEntries(children, pool, out);
            break;
        }
        case 'BlockQuote': {
            // c = BlockNode[]
            collectBlockEntries(c as RawBlock[], pool, out);
            break;
        }
        case 'BulletList': {
            // c = BlockNode[][]
            for (const item of c as RawBlock[][]) {
                collectBlockEntries(item, pool, out);
            }
            break;
        }
        case 'OrderedList': {
            // c = [ListAttrs, BlockNode[][]]
            for (const item of (c as [unknown, RawBlock[][]])[1]) {
                collectBlockEntries(item, pool, out);
            }
            break;
        }
        case 'DefinitionList': {
            // c = [Term, BlockNode[][]][]
            for (const [, defBlocks] of c as [unknown, RawBlock[][]][]) {
                for (const defs of defBlocks) {
                    collectBlockEntries(defs, pool, out);
                }
            }
            break;
        }
        case 'Figure': {
            // c = [Attr, [shortCaption, captionBlocks], bodyBlocks]
            const fc = c as [unknown, [unknown, RawBlock[]], RawBlock[]];
            collectBlockEntries(fc[2], pool, out);
            collectBlockEntries(fc[1][1], pool, out);
            break;
        }
        default:
            break;
    }
}

// ── Test ─────────────────────────────────────────────────────────────────────

describe('r[0] uniqueness in real parser output', () => {
    it('every Original block r[0] is distinct (no container/first-child collision)', () => {
        // Load the committed AST JSON produced by:
        //   cargo run --bin pampa -- r0-uniqueness.qmd -t json
        const astPath = join(__dirname, 'r0-uniqueness.ast.json');
        const astJson = readFileSync(astPath, 'utf-8');
        const ast = JSON.parse(astJson) as {
            blocks: RawBlock[];
            astContext?: { p?: RawPool };
        };

        const pool = ast.astContext?.p;
        expect(pool, 'astContext.p (pool) must be present in the AST JSON').toBeTruthy();
        expect(Array.isArray(pool), 'pool must be an array').toBe(true);

        const entries: OriginalEntry[] = [];
        collectBlockEntries(ast.blocks, pool!, entries);

        // Must have found at least some entries in the fixture document.
        expect(
            entries.length,
            'expected at least 5 Original block entries in the fixture',
        ).toBeGreaterThanOrEqual(5);

        // Explicit duplicate check: collect all r[0] values, then verify each
        // appears exactly once. A deduplicating Map would hide a collision;
        // we use a seen-Set + a separate duplicates collection so real
        // collisions are surfaced as failing assertions, not silently merged.
        const r0Values = entries.map(e => e.r[0]);

        const seen = new Set<number>();
        const duplicates: Array<{ r0: number }> = [];
        for (const r0 of r0Values) {
            if (seen.has(r0)) {
                if (!duplicates.some(d => d.r0 === r0)) {
                    duplicates.push({ r0 });
                }
            } else {
                seen.add(r0);
            }
        }

        expect(
            duplicates,
            `Duplicate r[0] values found among Original block entries: ${JSON.stringify(duplicates)}. ` +
            `This contradicts assumption B of the block-editing plan — stop and report.`,
        ).toHaveLength(0);
    });

    it('fixture covers container + first-child adjacency (Div, BlockQuote, BulletList, nested list)', () => {
        // Sanity-check that the fixture actually exercises the container/first-child
        // pairs we care about, by confirming we collected entries for all the expected
        // block types.
        const astPath = join(__dirname, 'r0-uniqueness.ast.json');
        const astJson = readFileSync(astPath, 'utf-8');
        const ast = JSON.parse(astJson) as {
            blocks: RawBlock[];
            astContext?: { p?: RawPool };
        };

        const pool = ast.astContext!.p!;
        const entries: OriginalEntry[] = [];
        collectBlockEntries(ast.blocks, pool, entries);

        // Collect block types from the fixture's block tree (not the pool).
        const types = new Set<string>();
        function collectTypes(blocks: RawBlock[]): void {
            for (const b of blocks) {
                if (!b || typeof b.t !== 'string') continue;
                types.add(b.t);
                const c = b.c;
                if (b.t === 'Div' && Array.isArray(c)) {
                    collectTypes((c as [unknown, RawBlock[]])[1]);
                } else if (b.t === 'BlockQuote' && Array.isArray(c)) {
                    collectTypes(c as RawBlock[]);
                } else if (b.t === 'BulletList' && Array.isArray(c)) {
                    for (const item of c as RawBlock[][]) collectTypes(item);
                } else if (b.t === 'OrderedList' && Array.isArray(c)) {
                    for (const item of (c as [unknown, RawBlock[][]])[1]) collectTypes(item);
                }
            }
        }
        collectTypes(ast.blocks);

        expect(types.has('Div'), 'fixture must contain a Div').toBe(true);
        expect(types.has('BlockQuote'), 'fixture must contain a BlockQuote').toBe(true);
        expect(types.has('BulletList'), 'fixture must contain a BulletList').toBe(true);
    });
});
