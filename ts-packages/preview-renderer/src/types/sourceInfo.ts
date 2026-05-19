/**
 * Wire-format types for the source-info pool, mirroring
 * `crates/pampa/src/writers/json.rs:54-91`.
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
 *  - 3: FilterProvenance — `d` is [filter_path, line].
 *  - 4: Synthetic — `d` is a By marker. Dormant; Plan 5 wires this up.
 *  - 5: Derived — `d` is { from: parent_id, by: By }. Dormant; Plan 5 wires this up.
 *
 * Codes 4 and 5 are forward-declared so 2A's accessor module doesn't need
 * amending when Plan 5 ships writer support for them.
 */

/**
 * A `By` marker identifies the synthesizer responsible for a Synthetic or
 * Derived source-info entry. The shape is intentionally coarse — Plan 4
 * introduces specific kinds with structured `data`. Once consumers branch
 * on `kind`, this can be narrowed to a discriminated union.
 */
export interface By {
    kind: string;
    data?: unknown;
}

export type SourceInfoEntry =
    | { t: 0; r: [number, number]; d: number }
    | { t: 1; r: [number, number]; d: number }
    | { t: 2; r: [number, number]; d: Array<[number, number, number]> }
    | { t: 3; r: [number, number]; d: [string, number] }
    | { t: 4; r: [0, 0]; d: By }
    | { t: 5; r: [0, 0]; d: { from: number; by: By } };

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
