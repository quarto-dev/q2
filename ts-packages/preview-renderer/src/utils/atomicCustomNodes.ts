/**
 * Hand-mirror of the Rust-side `ATOMIC_CUSTOM_NODES` const that Plan 7
 * will ship in `crates/quarto-core`. Lists CustomNode `type_name` strings
 * whose subtrees are atomic — Plan 2B's atomic-aware dispatcher gate (in
 * `framework/dispatch.tsx`'s `Node`) consumes this set to no-op
 * `setLocalAst` for atomic content.
 *
 * The CustomNode `type_name` is recovered from the `data-custom-type`
 * attribute the JSON writer attaches to wrapper Div (block) / Span
 * (inline) nodes — see `crates/pampa/src/writers/json.rs:1297-1325`
 * (block) and `:1381+` (inline). Strings here must match the writer's
 * emitted `type_name` byte-for-byte.
 *
 * Sync convention: when the Rust `ATOMIC_CUSTOM_NODES` const changes
 * (Plan 7 introduces it; Plan 8 adds `"IncludeExpansion"`), update this
 * file and re-run hub-client tests. Matches the
 * `types/diagnostic.ts` ↔ `DiagnosticMessage` and
 * `types/intelligence.ts` ↔ `quarto-lsp-core` patterns.
 */

export const ATOMIC_CUSTOM_NODES: ReadonlySet<string> = new Set<string>([
    'CrossrefResolvedRef',
]);

export function isAtomicCustomNode(typeName: string): boolean {
    return ATOMIC_CUSTOM_NODES.has(typeName);
}
