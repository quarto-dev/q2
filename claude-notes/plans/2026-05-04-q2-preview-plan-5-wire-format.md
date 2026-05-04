# Plan 5 — JSON wire format extension + code-3 fix

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** none directly — fixes a latent bug, prepares wire for Plans 6/7/8

## Goal

Extend the source-info pool's JSON wire format to encode two new variants
introduced by Plan 4: `Synthetic { by: By }` and `Derived { from: SourceInfo,
by: By }`. In the same change, fix a latent bug: today's writer emits
`FilterProvenance` as type code `3` with payload `[filter_path, line]`, but
today's reader interprets code `3` as the long-removed `Transformed` variant
and tries to parse it as `[parent_id, ...]` — resulting in a hard
`MalformedSourceInfoPool` error on any AST that crosses the JSON boundary
with a FilterProvenance value in it.

The latent bug doesn't surface today because `parse_qmd_to_ast` doesn't run
the transforms that produce `FilterProvenance`. The instant Plan 1 enables
the q2-preview pipeline (which runs filters and shortcodes), we'd hit it.

## Scope

### In scope

- Add wire format code `4` for `Synthetic { by: By }`. Payload encoding:
  `d` carries `{"kind": "...", "data": ...}` (or `{"kind": "..."}` if
  `by.data` is null).
- Add wire format code `5` for `Derived { from, by }`. Payload encoding:
  `d` carries `{"from": <pool_id>, "by": {"kind": "...", "data": ...}}`.
  The `from` is interned in the source-info pool just like `Substring.parent`.
- Fix the code-3 reader. Today's reader interprets code 3 as Transformed and
  tries to read a parent_id out of `data[0]`. Make it accept *both* shapes:
  - **Legacy Transformed** (`data` is `[parent_id, ...]` of numbers): map to
    `Substring` (current behavior), preserving back-compat for old JSON.
  - **Latent FilterProvenance** (`data` is `[filter_path, line]` — string
    then number): decode as `Synthetic { by: By::filter(filter_path, line) }`.
    This recovers the FilterProvenance shape that was being silently corrupted.
- After the fix, the writer no longer emits code 3 for new content (codes 4
  and 5 cover everything). Code 3 becomes a read-only legacy compat path.
- Round-trip tests: every `SourceInfo` variant survives Rust → JSON → Rust
  unchanged.

### Out of scope

- Lua serde changes (Plan 4 covers those — the Lua format is independent of
  the JSON pool wire format).
- The wire format for `By.data` itself is just `serde_json::Value` (already
  handled by serde derives on `By`).

## Design decisions (settled in conversation)

- **Two new wire codes (4 and 5)**: Synthetic and Derived. The `Derived`
  variant came back in the conversation after we saw that pure-provenance
  alone couldn't distinguish "shortcode resolution" (atomic; user edits
  prohibited at the writer level) from "filter mutation" (non-atomic; user
  edits flow to source). Derived gives the type-level distinction.
- **Code 3 stays as a legacy reader** — fixes the latent bug AND retires
  `FilterProvenance` in one step. The reader recognizes both old shapes
  (legacy Transformed array of numbers; FilterProvenance `[filter_path, line]`)
  and dispatches accordingly. Post-Plan 5, writers never emit code 3.
- **Verbose keys (`kind`, `data`, `from`, `by`) over compact ones** at the
  payload level for self-documentation. The wire format's outer fields
  (`t`, `r`, `d` at the SourceInfoJson level) stay compact for consistency
  with existing code.

## Concrete wire format

### Code 4 — Synthetic

The source-info pool entry for a `Synthetic` value:

```json
{
  "t": 4,
  "r": [0, 0],
  "d": { "kind": "filter", "data": { "filter_path": "/path/to/f.lua", "line": 42 } }
}
```

For kinds without per-instance data:

```json
{ "t": 4, "r": [0, 0], "d": { "kind": "sectionize" } }
```

(`"data"` field omitted when the inner `By.data` is null, per the serde
`skip_serializing_if` on the `By` struct from Plan 4.)

### Code 5 — Derived

The source-info pool entry for a `Derived` value:

```json
{
  "t": 5,
  "r": [0, 0],
  "d": {
    "from": 7,
    "by": { "kind": "shortcode", "data": { "name": "meta" } }
  }
}
```

The `from` field is a pool ID referencing another entry in the source-info
pool — typically an `Original` entry covering the shortcode token's bytes.
The `by` carries the same shape as Synthetic's `d` (`{kind, data}` with
`data` optional).

The pool entry's `r: [0, 0]` because Derived doesn't carry its own offsets
— ranges are obtained via the `preimage_in` walk through the `from` chain.

## The dual-shape code-3 reader

```rust
3 => {
    // Legacy code-3: either old `Transformed` (data is [parent_id, ...])
    // or the buggy FilterProvenance writer (data is [filter_path, line]).
    let array = data.as_array().ok_or(MalformedSourceInfoPool)?;
    if array.is_empty() { return Err(MalformedSourceInfoPool); }

    if let Some(parent_id) = array[0].as_u64() {
        // Legacy Transformed path. Approximate as Substring pointing to parent.
        // (existing behavior — kept for back-compat)
        let parent_id = parent_id as usize;
        // ...current logic...
        SourceInfo::Substring { parent: ..., start_offset, end_offset }
    } else if let Some(filter_path) = array[0].as_str() {
        // Latent FilterProvenance shape. Decode to Synthetic.
        let line = array.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        SourceInfo::Synthetic {
            by: By::filter(filter_path.to_string(), line),
        }
    } else {
        return Err(MalformedSourceInfoPool);
    }
}
```

Future writers don't emit code 3. Eventually code 3 can be retired entirely
(once we're confident no on-disk JSON files contain it), but for now it's a
no-cost read-only compat shim.

## The new code-4 reader

```rust
4 => {
    // Synthetic { by: By }
    let by_obj = data.as_object().ok_or(MalformedSourceInfoPool)?;
    let kind = by_obj.get("kind")
        .and_then(|v| v.as_str())
        .ok_or(MalformedSourceInfoPool)?
        .to_string();
    let data = by_obj.get("data").cloned().unwrap_or(Value::Null);
    SourceInfo::Synthetic { by: By { kind, data } }
}
```

The new code-4 writer:

```rust
SerializableSourceMapping::Synthetic { by } => {
    let mut by_json = json!({ "kind": by.kind });
    if !by.data.is_null() {
        by_json["data"] = by.data.clone();
    }
    (4, by_json)
}
```

(start_offset and end_offset for Synthetic are both 0 — there's no source
range. The writer continues to emit `r: [0, 0]`.)

## The new code-5 reader/writer

```rust
5 => {
    // Derived { from: Arc<SourceInfo>, by: By }
    let obj = data.as_object().ok_or(MalformedSourceInfoPool)?;
    let from_id = obj.get("from")
        .and_then(|v| v.as_u64())
        .ok_or(MalformedSourceInfoPool)? as usize;
    if from_id >= current_index {
        return Err(CircularSourceInfoReference(from_id));
    }
    let from = pool.get(from_id).cloned().ok_or(InvalidSourceInfoRef(from_id))?;
    let by_obj = obj.get("by").and_then(|v| v.as_object())
        .ok_or(MalformedSourceInfoPool)?;
    let kind = by_obj.get("kind").and_then(|v| v.as_str())
        .ok_or(MalformedSourceInfoPool)?.to_string();
    let by_data = by_obj.get("data").cloned().unwrap_or(Value::Null);
    SourceInfo::Derived { from: Arc::new(from), by: By { kind, data: by_data } }
}
```

Writer:

```rust
SerializableSourceMapping::Derived { from_id, by } => {
    let mut by_json = json!({ "kind": by.kind });
    if !by.data.is_null() { by_json["data"] = by.data.clone(); }
    (5, json!({ "from": from_id, "by": by_json }))
}
```

`from_id` is an interned pool ID, the same way `Substring.parent_id` works.
The serializer interns the `from` SourceInfo when first encountered and
reuses the ID on later references — natural deduplication for shortcode
resolutions where many resolved nodes share the same `from`.

`r: [0, 0]` for Derived too — offsets are recovered through the chain via
`preimage_in` (Plan 7), not stored on the Derived entry itself.

## Open questions for implementation

- **Eventually retiring code 3**: at some point, no JSON files in the wild
  contain code 3 (the buggy FilterProvenance shape never round-tripped before
  Plan 5; the legacy Transformed shape predates a transition we made earlier).
  Could remove the legacy reader. Don't need to decide now.
- **Detecting malformed code 4/5 payloads**: if shape doesn't match
  expectation, error with `MalformedSourceInfoPool`. Confirm the exact
  error variant for each malformation.
- **Streaming writer parity** (`stream_write_custom_block` and the streaming
  source-info-pool writer): both writer paths need updating. Today both have
  the same code-3 → FilterProvenance shape — the bug applies to both.
  Update both to emit code 4 for Synthetic and code 5 for Derived.
- **Pool deduplication of Derived `from` references**: when many Derived
  source_infos share the same `from` (e.g., a multi-inline shortcode
  resolution where every resolved inline points at the same shortcode
  token), the writer should intern `from` once and reuse the ID. The
  existing `arc_parent_ids` HashMap pattern (used for `Substring.parent`)
  applies here.

## References

- `crates/pampa/src/writers/json.rs:80` — type code comment.
- `crates/pampa/src/writers/json.rs:132-155` — `SerializableSourceInfo::to_json`.
- `crates/pampa/src/writers/json.rs:145-148` — current FilterProvenance →
  code 3 emit (the buggy line).
- `crates/pampa/src/writers/json.rs:225-298` — full SerializableSourceInfo
  enum and conversion.
- `crates/pampa/src/readers/json.rs:155-290` — pool reader; the code-3
  branch is at line 252.
- `crates/quarto-source-map/src/source_info.rs:22-55` — SourceInfo enum
  (extended by Plan 4).

## Test plan

- **Round-trip property test**: for each variant (Original, Substring,
  Concat, Synthetic, Derived with various By kinds), build a `SourceInfo`,
  serialize to JSON, deserialize, assert equality. Cover the full enum.
- **Filter-provenance recovery test**: hand-construct a JSON pool entry with
  the buggy code-3-with-string-array-payload shape. Read it. Assert the
  reader produces `Synthetic { by: By::filter(...) }` with the right path/line.
- **Legacy Transformed back-compat test**: hand-construct a JSON pool entry
  with code-3-with-numeric-array-payload (the legacy Transformed shape).
  Assert the reader still produces a `Substring` (preserving today's
  back-compat behavior).
- **Forward-compat test**: hand-construct a JSON pool entry with code 4 and
  an unknown kind (`"kind": "ext/future/foo"`, arbitrary data). Assert it
  decodes as `Synthetic { by: By { kind: "ext/future/foo", data: ... } }`.
  Round-trips unchanged. Same test for code 5.
- **Derived dedup test**: build an AST where multiple inlines have Derived
  source_info sharing the same `from`. Serialize. Confirm the pool contains
  the `from` Original entry exactly once and each Derived entry references
  it by ID (rather than re-encoding the Original each time).
- **End-to-end with Plan 4**: build an AST containing Synthetic-tagged AND
  Derived-tagged nodes, serialize to JSON via the existing JSON writer,
  deserialize via the reader, assert structural equality.

## Dependencies

- Depends on: Plan 4 (Synthetic + Derived variants + By struct).
- Blocks: Plans 6, 7, 8 (they all rely on the new variants round-tripping
  through JSON).

## Risk areas

- **Streaming writer code path**: there are two writer paths in `json.rs`
  (`write_custom_block` non-streaming and `stream_write_custom_block`
  streaming). Both have the same source-info-pool emission logic. Both need
  updating. Easy to forget the streaming variant.
- **Pool ID stability**: changing the format of pool entries shouldn't
  affect their IDs (which are sequential by intern order). Verify.
- **Old JSON files**: anyone with on-disk JSON snapshots of ASTs (test
  fixtures, debug exports) generated by current writers will have code 3
  with the buggy shape. Plan 5's reader handles them. New writes emit code 4.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Code 4 writer + reader | ~50 |
| Code 5 writer + reader (with `from` interning) | ~60 |
| Code 3 dual-shape reader | ~30 |
| Streaming writer parity | ~30 |
| Tests | ~180 |
| **Total** | **~350** |

One focused session.

## Notes

The bug-fix opportunity is real: this plan makes things work that have been
silently latent. Worth a clear callout in the implementation commit message:
"This change fixes a latent bug where FilterProvenance values written by
the JSON writer could not be read back. Production code never tripped this
because no production path produced FilterProvenance in the AST that crossed
the JSON boundary."
