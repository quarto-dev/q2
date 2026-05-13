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
