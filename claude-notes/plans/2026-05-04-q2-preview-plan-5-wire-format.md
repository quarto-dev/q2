# Plan 5 — JSON wire format extension for Generated + anchors

**Date:** 2026-05-04 (revised 2026-05-20)
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** none directly — fixes a latent bug, prepares wire for
  the rest of the provenance epic

## Epic context

Part of the **provenance epic** (Plans 3–8). Plan 5 carries the wire
format adjustments needed so the typed provenance Plan 4 introduces can
cross the WASM/JSON boundary and round-trip without information loss.
The file name keeps its q2-preview-plan-N form for continuity with
earlier discussion notes.

## Goal

Extend the source-info pool's JSON wire format to encode the
`Generated { by, anchors }` variant introduced by Plan 4. In the same
change, fix a latent bug: today's writer emits `FilterProvenance` as
type code `3` with payload `[filter_path, line]`, but today's reader
interprets code `3` as the long-removed `Transformed` variant and tries
to parse it as `[parent_id, ...]` — resulting in a hard
`MalformedSourceInfoPool` error on any AST that crosses the JSON
boundary with a FilterProvenance value in it.

The latent bug doesn't surface in current main because `parse_qmd_to_ast`
doesn't run filters that produce FilterProvenance. **But the q2-preview
pipeline (already shipped via Plans 1–2) does run filters and
shortcodes**, and the latent bug becomes reachable as soon as a
built-in or user filter constructs a node whose JSON-serialized
source_info crosses the WASM boundary. Plan 5 is therefore higher
priority than the original "prepares wire for downstream plans"
framing suggested — it fixes a bug that's no longer latent in design,
only in reach.

## Scope

### In scope

- Add wire format code `4` for `Generated { by, anchors }`. Payload
  encoding:
  ```json
  {
    "by": { "kind": "...", "data": <object|null> },
    "anchors": [
      { "role": "<role-string>", "from": <pool_id> },
      ...
    ]
  }
  ```
  `from` is a pool ID referencing another entry in the source-info pool
  — typically an `Original` covering the source bytes the anchor points
  at. Multiple anchors share their `from` pool IDs naturally
  (multi-inline shortcode: every resolved inline's `Invocation` anchor
  references the same token's pool entry).
- Anchor role encoding: `"invocation"`, `"value-source"`, or
  `"other:<extension-defined-name>"` for `AnchorRole::Other(String)`.
  Kebab-case throughout.
- Fix the code-3 reader. Today's reader interprets code 3 as
  Transformed and tries to read a parent_id out of `data[0]`. Make it
  accept *both* shapes:
  - **Legacy Transformed** (`data` is `[parent_id, ...]` of numbers):
    map to `Substring` (current behavior), preserving back-compat for
    old JSON.
  - **Latent FilterProvenance** (`data` is `[filter_path, line]` —
    string then number): decode as `Generated { by: By::filter(filter_path, line), anchors: vec![] }`.
    This recovers the FilterProvenance shape that was being silently
    corrupted.
- After the fix, the writer no longer emits code 3 for new content (code
  4 covers everything). Code 3 becomes a read-only legacy compat path.
- **Code 5 is unassigned.** Earlier drafts proposed code 5 for a
  separate `Derived` variant; that variant was unified into `Generated`
  during the 2026-05-20 design discussion and never shipped. Code 5
  remains free for future reservation.
- Round-trip tests: every `SourceInfo` variant survives Rust → JSON →
  Rust unchanged.

### Out of scope

- Lua serde changes (Plan 4 covers those — the Lua format is
  independent of the JSON pool wire format).
- The wire format for `By.data` itself is just `serde_json::Value`
  (already handled by serde derives on `By`).
- The metadata-loader changes that would populate `ValueSource` anchors
  (separate follow-up; the wire format is forward-compatible — anchor
  arrays simply gain entries when the resolver starts attaching them).
- Lua-file-registration that would convert `Dispatch` anchor data from
  `by.data` into typed `Original`-backed anchors (separate follow-up;
  wire-format forward-compatible the same way).

## Design decisions (settled in conversation)

- **One new wire code (4)**, not two. The original Plan 4 / 5 drafts
  split `Synthetic` (code 4) and `Derived` (code 5). The unified
  `Generated` variant collapses these. Code 5 remains unassigned.
- **Typed anchor list at the wire level.** Each entry in the `anchors`
  array carries a `role` string and a `from` pool ID. This keeps the
  source-info chain typed even at the wire boundary — `from` refers to
  another pool entry, never an inlined object.
- **Code 3 stays as a legacy reader** — fixes the latent bug AND
  retires `FilterProvenance` in one step. The reader recognizes both
  old shapes (legacy Transformed array of numbers; FilterProvenance
  `[filter_path, line]`) and dispatches accordingly. Post-Plan 5,
  writers never emit code 3.
- **Verbose keys (`kind`, `data`, `by`, `anchors`, `role`, `from`)** at
  the payload level for self-documentation. The wire format's outer
  fields (`t`, `r`, `d` at the SourceInfoJson level) stay compact for
  consistency with existing code.

## Concrete wire format

### Code 4 — Generated

The source-info pool entry for a `Generated` value with **no anchors**
(pure synthesis — sectionize, filter, title-block, footnotes, appendix,
tree-sitter-postprocess, user-edit):

```json
{ "t": 4, "r": [0, 0], "d": { "by": { "kind": "sectionize" } } }
```

```json
{ "t": 4, "r": [0, 0], "d": { "by": { "kind": "filter", "data": { "filter_path": "/path/to/f.lua", "line": 42 } } } }
```

(The `"data"` field is omitted when `By.data` is `null`, per the serde
`skip_serializing_if` on `By`. The `"anchors"` field is omitted when the
vec is empty.)

The source-info pool entry for a `Generated` value with **one
Invocation anchor** (shortcode resolution):

```json
{
  "t": 4,
  "r": [0, 0],
  "d": {
    "by": { "kind": "shortcode", "data": { "name": "meta" } },
    "anchors": [
      { "role": "invocation", "from": 7 }
    ]
  }
}
```

The source-info pool entry for a `Generated` value with **multiple
anchors** (future: a shortcode resolution that also records its value
source after the metadata-loader follow-up lands):

```json
{
  "t": 4,
  "r": [0, 0],
  "d": {
    "by": { "kind": "shortcode", "data": { "name": "meta" } },
    "anchors": [
      { "role": "invocation",   "from": 7 },
      { "role": "value-source", "from": 12 }
    ]
  }
}
```

The pool entry's `r: [0, 0]` because Generated doesn't carry its own
offsets — ranges are obtained via the `resolve_byte_range` /
`preimage_in` chain-walk through the `Invocation` anchor.

### Code 3 — Legacy reader only

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
        // Latent FilterProvenance shape. Decode to Generated.
        let line = array.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        SourceInfo::Generated {
            by: By::filter(filter_path.to_string(), line),
            anchors: vec![],
        }
    } else {
        return Err(MalformedSourceInfoPool);
    }
}
```

Future writers don't emit code 3. Eventually code 3 can be retired
entirely (once we're confident no on-disk JSON files contain it), but
for now it's a no-cost read-only compat shim.

### Code 4 — Reader / writer

```rust
4 => {
    // Generated { by, anchors }
    let obj = data.as_object().ok_or(MalformedSourceInfoPool)?;
    let by_obj = obj.get("by").and_then(|v| v.as_object())
        .ok_or(MalformedSourceInfoPool)?;
    let kind = by_obj.get("kind").and_then(|v| v.as_str())
        .ok_or(MalformedSourceInfoPool)?.to_string();
    let by_data = by_obj.get("data").cloned().unwrap_or(Value::Null);
    let by = By { kind, data: by_data };

    let mut anchors = Vec::new();
    if let Some(anchors_arr) = obj.get("anchors").and_then(|v| v.as_array()) {
        for entry in anchors_arr {
            let role_str = entry.get("role").and_then(|v| v.as_str())
                .ok_or(MalformedSourceInfoPool)?;
            let role = parse_anchor_role(role_str)?;
            let from_id = entry.get("from").and_then(|v| v.as_u64())
                .ok_or(MalformedSourceInfoPool)? as usize;
            if from_id >= current_index {
                return Err(CircularSourceInfoReference(from_id));
            }
            let from = pool.get(from_id).cloned()
                .ok_or(InvalidSourceInfoRef(from_id))?;
            anchors.push(Anchor { role, source_info: Arc::new(from) });
        }
    }

    SourceInfo::Generated { by, anchors }
}

fn parse_anchor_role(s: &str) -> Result<AnchorRole, MalformedSourceInfoPool> {
    match s {
        "invocation"   => Ok(AnchorRole::Invocation),
        "value-source" => Ok(AnchorRole::ValueSource),
        other if other.starts_with("other:") =>
            Ok(AnchorRole::Other(other[6..].to_string())),
        _ => Err(MalformedSourceInfoPool),
    }
}
```

Writer:

```rust
SerializableSourceMapping::Generated { by, anchor_pool_ids } => {
    let mut by_json = json!({ "kind": by.kind });
    if !by.data.is_null() { by_json["data"] = by.data.clone(); }

    let mut d = json!({ "by": by_json });
    if !anchor_pool_ids.is_empty() {
        let arr: Vec<Value> = anchor_pool_ids.iter()
            .map(|(role, from_id)| json!({
                "role": serialize_anchor_role(role),
                "from": from_id,
            }))
            .collect();
        d["anchors"] = Value::Array(arr);
    }

    (4, d)
}

fn serialize_anchor_role(role: &AnchorRole) -> String {
    match role {
        AnchorRole::Invocation => "invocation".to_string(),
        AnchorRole::ValueSource => "value-source".to_string(),
        AnchorRole::Other(s) => format!("other:{}", s),
    }
}
```

The serializer interns each anchor's `source_info` into the pool when
first encountered and reuses the ID on later references — the same
`arc_parent_ids` HashMap pattern already used for `Substring.parent`.
Multi-inline shortcode resolution thus produces N `Generated` entries,
each with one `Invocation` anchor, all referencing the same pool ID for
the shortcode token's `Original` entry.

## Open questions for implementation

- **Eventually retiring code 3**: at some point, no JSON files in the
  wild contain code 3 (the buggy FilterProvenance shape never
  round-tripped before Plan 5; the legacy Transformed shape predates a
  transition we made earlier). Could remove the legacy reader. Don't
  need to decide now.
- **Detecting malformed code 4 payloads**: if shape doesn't match
  expectation, error with `MalformedSourceInfoPool`. Confirm the exact
  error variant for each malformation.
- **Streaming writer parity** (`stream_write_custom_block` and the
  streaming source-info-pool writer): both writer paths need updating.
  Today both have the same code-3 → FilterProvenance shape — the bug
  applies to both. Update both to emit code 4 for Generated.
- **Pool deduplication of anchor `from` references**: when many
  Generated entries share the same anchor target (multi-inline
  shortcode), the writer interns once and reuses the ID. The existing
  `arc_parent_ids` HashMap pattern handles this.
- **TypeScript hand-mirror updates**: `ts-packages/preview-renderer/src/types/sourceInfo.ts`
  defines `SourceInfoEntry` for JS consumers. After Plan 5, the entry
  type for code 4 grows an optional `anchors` field with `{ role,
  from }` shape. The TS type and the Rust serializer must agree
  byte-for-byte; sync via doc-comment convention as we do for the
  atomic-CustomNodes registry.

## References

- `crates/pampa/src/writers/json.rs:80` — type code comment.
- `crates/pampa/src/writers/json.rs:115` — `SourceInfoJson` struct
  (the type code comment lives here in current main).
- `crates/pampa/src/writers/json.rs:132-155` — `SerializableSourceInfo::to_json`.
- `crates/pampa/src/writers/json.rs:145-148` — current FilterProvenance →
  code 3 emit (the buggy line).
- `crates/pampa/src/writers/json.rs:225-298` — full SerializableSourceInfo
  enum and conversion.
- `crates/pampa/src/readers/json.rs:155-290` — pool reader; the code-3
  branch is at line 252.
- `crates/quarto-source-map/src/source_info.rs:22-55` — SourceInfo enum
  (extended by Plan 4).
- `ts-packages/preview-renderer/src/types/sourceInfo.ts` — JS-side
  `SourceInfoEntry` type definition (needs the `anchors` field).
- `ts-packages/preview-renderer/src/utils/sourceInfo.ts` — JS-side
  helpers (`isAtomicSourceInfo`, etc.); needs adjustment for the new
  shape per Plan 4 / Plan 7.

## Test plan

- **Round-trip property test**: for each variant (Original, Substring,
  Concat, Generated with various By kinds and anchor configurations),
  build a `SourceInfo`, serialize to JSON, deserialize, assert
  equality. Cover the full enum.
- **Filter-provenance recovery test**: hand-construct a JSON pool entry
  with the buggy code-3-with-string-array-payload shape. Read it.
  Assert the reader produces `Generated { by: filter, anchors: vec![] }`
  with the right path/line via `by.as_filter()`.
- **Legacy Transformed back-compat test**: hand-construct a JSON pool
  entry with code-3-with-numeric-array-payload (the legacy Transformed
  shape). Assert the reader still produces a `Substring` (preserving
  today's back-compat behavior).
- **Forward-compat test**: hand-construct a JSON pool entry with code 4
  and an unknown kind (`"kind": "ext/future/foo"`, arbitrary data).
  Assert it decodes as `Generated { by: By { kind: "ext/future/foo",
  data: ... }, anchors: vec![] }`. Round-trips unchanged.
- **Anchor dedup test**: build an AST where multiple inlines have
  Generated source_info each carrying an `Invocation` anchor that
  references the same `Original` (multi-inline shortcode resolution).
  Serialize. Confirm the pool contains the `Original` exactly once and
  each Generated entry's anchors[0].from references it by ID.
- **AnchorRole round-trip test**: round-trip a Generated with each role
  (Invocation, ValueSource, Other(String)) through JSON; assert the
  role survives.
- **End-to-end production reachability test** (regression guard for
  the bug Plan 5 fixes — current main would fail this test as soon as
  the JSON round-trip is exercised on a Lua-shortcode-bearing
  document):
  1. Build a fixture using `{{< kbd Ctrl+C >}}` (the kbd extension's
     `kbd.lua` calls `pandoc.Span(...)`, which the Lua machinery's
     `filter_source_info` auto-attach tags with FilterProvenance /
     post-Plan-4 `Generated { by: filter, ... }`).
  2. Run it through `render_qmd_to_preview_ast` (or the equivalent
     production path that drives the JSON writer with
     filter-constructed nodes in the AST).
  3. Take the resulting JSON, feed it back through
     `pampa::readers::json::read`.
  4. Assert the round-trip succeeds (no `MalformedSourceInfoPool`
     error) AND the recovered source_info is `Generated { by:
     shortcode, anchors: [Invocation -> ...] }` after Plan 6's
     post-walk has stamped it. (If running Plan 5 alone — before
     Plan 6 lands — the recovered shape is `Generated { by: filter,
     anchors: [] }` with `(filter_path, line)` in `by.data`; the
     round-trip still succeeds.)

  This is distinct from the hand-constructed "Filter-provenance
  recovery test" above. That test exercises the legacy code-3 reader
  in isolation; this one drives a real pipeline + JSON writer + reader
  to verify the bug-fix holds end-to-end against a production-shaped
  path. Without Plan 5, the round-trip on step 3 errors out
  (`MalformedSourceInfoPool` from the code-3-as-Transformed
  misinterpretation) on any document whose shortcode-resolution path
  hits a Lua handler.
- **End-to-end with Plan 4**: build an AST containing both
  no-anchor and with-anchor Generated nodes, serialize to JSON via the
  existing JSON writer, deserialize via the reader, assert structural
  equality.
- **TypeScript-side type round-trip**: hub-client / preview-renderer
  test parses a JSON pool with Generated entries and confirms its
  `SourceInfoEntry` shape matches.

## Dependencies

- Depends on: Plan 4 (Generated variant + By + Anchor + AnchorRole).
- Blocks: Plans 6, 7, 8 (they all rely on Generated round-tripping
  through JSON).

## Risk areas

- **Streaming writer code path**: there are two writer paths in
  `json.rs` (`write_custom_block` non-streaming and
  `stream_write_custom_block` streaming). Both have the same
  source-info-pool emission logic. Both need updating. Easy to forget
  the streaming variant.
- **Pool ID stability**: changing the format of pool entries shouldn't
  affect their IDs (which are sequential by intern order). Verify.
- **Old JSON files**: anyone with on-disk JSON snapshots of ASTs (test
  fixtures, debug exports) generated by current writers will have code
  3 with the buggy shape. Plan 5's reader handles them. New writes emit
  code 4.
- **Coexistence with attribution wire fields in the same file**: the
  attribution work (already shipped) added `astContext.attribution`
  and `attributionActors` near the source-info pool emission in
  `crates/pampa/src/writers/json.rs`. Plan 5 touches different
  conditional branches of the same writer file but no semantic
  conflict — `astContext.attribution` records reference source-info
  pool IDs unchanged; new code-4 entries are valid `s` targets just as
  Original entries are.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Code 4 writer (with anchor interning) | ~80 |
| Code 4 reader (with anchor decoding) | ~70 |
| Code 3 dual-shape legacy reader | ~30 |
| `AnchorRole` ↔ string serialization | ~20 |
| Streaming writer parity | ~40 |
| TypeScript type definition update | ~20 |
| Tests | ~220 |
| **Total** | **~480** |

One focused session.

## Notes

The bug-fix opportunity is real and now reachable in production: this
change makes things work that have been silently latent. Worth a clear
callout in the implementation commit message:

> This change fixes a latent bug where `FilterProvenance` values written
> by the JSON writer could not be read back. Production code never
> tripped this in current main because no production path produced
> FilterProvenance in an AST that crossed the JSON boundary — but
> Plans 1–2 shipped the q2-preview pipeline that runs filters whose
> output does cross that boundary. Plan 5's reader recovers the
> `Generated { by: filter, ... }` shape from the buggy code-3 payload,
> closing the gap.

The single-code-4 design (no separate code 5) is the result of
unifying `Synthetic` + `Derived` into `Generated` during the 2026-05-20
design discussion. Code 5 is left unassigned, free for future
reservation.
