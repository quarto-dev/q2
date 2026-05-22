/**
 * Wire-format types for the source-info pool. Hand-mirror of the Rust
 * producers — keep this file aligned with two sources of truth:
 *
 *   - `SourceInfo` enum (canonical producer):
 *     `crates/quarto-source-map/src/source_info.rs`
 *   - JSON wire mirror:
 *     `crates/pampa/src/writers/json.rs`
 *       - `SerializableSourceMapping` (writer-side enum)
 *       - `SourceInfoJson` (wire entry shape)
 *       - `SerializableSourceInfo::to_json` (code-4 serializer)
 *
 * The pool is an array of entries indexed by `node.s` (the `s` field on
 * each Pandoc node in the serialized AST). Each entry has a type code
 * `t`, a [start, end] offset range `r`, and a code-specific data payload
 * `d`.
 *
 * Type codes:
 *  - 0: Original — `d` is the file id (FileId.0).
 *  - 1: Substring — `d` is a parent_id into the pool.
 *  - 2: Concat — `d` is an array of [source_info_id, offset_in_concat, length].
 *  - 3: Legacy — read-only compat for two old shapes; no new writes:
 *       `[parent_id, ...]` (numeric-headed legacy `Transformed`)
 *       `[filter_path, line]` (string-headed buggy `FilterProvenance`).
 *  - 4: Generated — `d` is `{ by: By, from?: AnchorRef[] }`. `r` is `[0, 0]`;
 *       ranges come from the chain-walk via the `invocation` anchor.
 *
 * Code 5 is unassigned and reserved for future use.
 */

/**
 * A `By` marker identifies the producer (transform) responsible for a
 * `Generated` entry. Mirrors the Rust `By` struct: a kebab-case `kind`
 * tag plus an optional per-kind JSON `data` payload.
 *
 * Known kinds at the time of writing: `"filter"`, `"shortcode"`,
 * `"sectionize"`, `"user-edit"`, `"include"`, `"title-block"`,
 * `"footnotes"`, `"appendix"`, `"tree-sitter-postprocess"`, `"raw"`.
 * Third-party extensions namespace as `"ext/<extension>/<kind>"`.
 */
export interface By {
    kind: string;
    data?: unknown;
}

/**
 * A typed, role-labeled pointer into the source-info pool, attached to
 * a `Generated` entry via its `from` array. Mirrors the Rust `Anchor`
 * struct flattened to its writer-internal `(role, si_id)` shape.
 *
 * `role` is one of:
 *   - `"invocation"` — the user-written construct that triggered the
 *     producer (e.g. the `{{< meta foo >}}` token).
 *   - `"value-source"` — where the value carried by this node was
 *     defined, when distinct from the invocation site.
 *   - `"other:<name>"` — extension-defined or future role we haven't
 *     enumerated. `<name>` is kebab-case, namespaced as
 *     `ext/<extension>/<role>`. The bare `"other:"` form (empty
 *     suffix) is rejected by the reader.
 *
 * `si_id` is the pool index of the anchor's target (typically an
 * `Original` covering the source bytes the anchor describes).
 */
export interface AnchorRef {
    role: string;
    si_id: number;
}

export type SourceInfoEntry =
    | { t: 0; r: [number, number]; d: number }                              // Original
    | { t: 1; r: [number, number]; d: number }                              // Substring
    | { t: 2; r: [number, number]; d: Array<[number, number, number]> }    // Concat
    | { t: 3; r: [number, number]; d: [string, number] | [number, ...number[]] } // Legacy (read-only)
    | { t: 4; r: [0, 0]; d: { by: By; from?: AnchorRef[] } };               // Generated
// code 5 — unassigned, reserved for future use

export type SourceInfoPool = readonly SourceInfoEntry[];

/**
 * The `astContext` field of a serialized Pandoc AST. Mirrors
 * `AstContextJson` in the JSON writer.
 */
export interface AstContext {
    files: Array<{ name: string; lineBreaks?: number[]; totalLength?: number }>;
    metaTopLevelKeySources?: unknown;
    sourceInfoPool?: SourceInfoPool;
}
