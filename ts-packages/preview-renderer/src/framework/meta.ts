/**
 * Pandoc Meta-value coercion helpers (framework-tier).
 *
 * Pure shape concerns: every consumer (slide-renderer slide
 * title/author, q2-preview body container + title block, future
 * format renderers) reads `meta.{title, author, ...}` from the same
 * Pandoc-AST shape, so the coercion logic belongs alongside the
 * AST types it walks. The `MetaInlines` / `MetaBlocks` branches
 * delegate to `framework/plainText.ts`'s walks.
 */

import type { BlockNode, InlineNode } from './types';
import { inlinesToPlainText, blocksToPlainText } from './plainText';

/**
 * Extract a string from a Pandoc Meta value. Handles MetaString,
 * MetaInlines, and MetaBlocks. Returns undefined for missing keys,
 * MetaBool, MetaList, or MetaMap (which can't reasonably be coerced
 * to a string).
 *
 * The MetaBlocks branch matches Rust's `config_value_to_template_value`
 * fallthrough to `blocks_to_text` (template.rs:610-614) — needed for
 * `abstract: |` block-scalar YAML, which parses as MetaBlocks.
 */
export function extractMetaString(meta: unknown): string | undefined {
    if (!meta || typeof meta !== 'object') return undefined;
    const m = meta as { t?: string; c?: unknown };
    if (m.t === 'MetaString' && typeof m.c === 'string') return m.c;
    if (m.t === 'MetaInlines' && Array.isArray(m.c)) {
        return inlinesToPlainText(m.c as InlineNode[]);
    }
    if (m.t === 'MetaBlocks' && Array.isArray(m.c)) {
        // Match Rust's blocks_to_text fallthrough — block boundaries
        // collapse to a single space, same fidelity loss as Rust today.
        return blocksToPlainText(m.c as BlockNode[]);
    }
    return undefined;
}

/**
 * Extract a boolean from a Pandoc Meta value. Treats both MetaBool
 * and `MetaString("true" | "false")` as valid — the YAML parser
 * produces one or the other depending on quoting (`minimal: true`
 * vs `minimal: "true"`). Other MetaString values do NOT coerce.
 */
export function extractMetaBool(meta: unknown): boolean | undefined {
    if (!meta || typeof meta !== 'object') return undefined;
    const m = meta as { t?: string; c?: unknown };
    if (m.t === 'MetaBool' && typeof m.c === 'boolean') return m.c;
    if (m.t === 'MetaString' && (m.c === 'true' || m.c === 'false')) {
        return m.c === 'true';
    }
    return undefined;
}

/**
 * Walk a dotted path through a nested Pandoc Meta value and return
 * the leaf node. `meta` is the *top-level* meta object (a plain
 * `Record<string, MetaValue>`); subsequent steps drop into
 * `MetaMap.c` arrays (`{key, key_source, value}` entries — see
 * `crates/pampa/src/writers/json.rs::write_config_value`).
 *
 * Returns `undefined` when any segment is missing or the shape
 * doesn't match. The caller is expected to coerce the leaf via
 * [`extractMetaString`] / [`extractMetaStringList`] etc.
 *
 * Example: `getMetaPath(meta, ['rendered', 'navigation', 'navbar'])`
 * walks `meta.rendered` (MetaMap) → `navigation` (MetaMap) →
 * `navbar` (MetaString).
 */
export function getMetaPath(
    meta: unknown,
    path: readonly string[],
): unknown {
    if (path.length === 0) return meta;
    let cursor: unknown = meta;
    for (let i = 0; i < path.length; i++) {
        if (!cursor || typeof cursor !== 'object') return undefined;
        const segment = path[i];
        if (i === 0) {
            // First step: top-level meta is a plain object.
            cursor = (cursor as Record<string, unknown>)[segment];
        } else {
            // Subsequent steps: cursor is a MetaMap whose `c` is the
            // entries array. (`MetaMap` is the only nested-object
            // variant emitted by the JSON writer.)
            const m = cursor as { t?: string; c?: unknown };
            if (m.t !== 'MetaMap' || !Array.isArray(m.c)) return undefined;
            const entries = m.c as Array<{ key: string; value: unknown }>;
            const found = entries.find((e) => e.key === segment);
            if (!found) return undefined;
            cursor = found.value;
        }
    }
    return cursor;
}

/**
 * Extract a string list from a Pandoc MetaList. Each list entry is
 * coerced via the same MetaString / MetaInlines / MetaBlocks logic
 * as `extractMetaString`. Returns an empty array for missing keys,
 * `MetaString` (use `extractMetaString` for that single-value shape),
 * or any other wrong shape.
 */
export function extractMetaStringList(meta: unknown): string[] {
    if (!meta || typeof meta !== 'object') return [];
    const m = meta as { t?: string; c?: unknown };
    if (m.t !== 'MetaList' || !Array.isArray(m.c)) return [];
    const out: string[] = [];
    for (const entry of m.c) {
        const s = extractMetaString(entry);
        if (s !== undefined) out.push(s);
    }
    return out;
}
