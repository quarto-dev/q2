/**
 * Strip q2's source-map "sidecar" keys from an AST subtree before it is
 * sent over the subtree edit channel (`commitSubtreeEdit` in
 * PreviewRoot.tsx).
 *
 * Sidecar values are indices into the emitting document's SourceInfo
 * pool (`astContext`). A subtree cloned out of the untransformed AST
 * still carries them, but the wrapped replacement doc we send to Rust's
 * `apply_node_edit` has no pool — any surviving index makes the reader
 * throw `InvalidSourceInfoRef` and the whole edit fails.
 *
 * Keys stripped (see `crates/pampa/src/writers/json.rs` and the
 * normalization rule in `crates/pampa/tests/integration/lua_differential.rs`):
 * - `s`           — node SourceInfo (every node)
 * - `a`           — AttrSourceInfo (attr-bearing nodes; `id`/`classes`/`kvs`
 *                   values are pool indices)
 * - `targetS`     — Link/Image `[urlRef, titleRef]`
 * - `captionS`    — Table/Figure caption ref (a bare pool index)
 * - `citationIdS` — citation id ref on Cite citation objects (bare index)
 *
 * Table-internal sidecars (`headS`, `footS`, `bodiesS`, `rowsS`,
 * `cellsS`, `bodyS`) are nested objects whose only pool-carrying leaves
 * are `s`/`a` keys, which the recursive replacer already removes; the
 * leftover skeletons read back as empty. Dropping them wholesale is
 * deferred until table editing is actually supported.
 */

const STRIPPED_KEYS = new Set(['s', 'a', 'targetS', 'captionS', 'citationIdS']);

export function stripSourceInfoFields<T>(block: T): T {
    return JSON.parse(JSON.stringify(block, (key, value) =>
        STRIPPED_KEYS.has(key) ? undefined : value,
    )) as T;
}
