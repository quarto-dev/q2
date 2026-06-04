# Plan 5 — JSON wire format extension for Generated

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
`Generated { by, from }` variant introduced by Plan 4. In the same
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

## Inherited failure that must close on Plan 5's first reader change (bd-3odjm)

Plan 3's idempotence gate already ships a live reproduction of this
bug as a failing test on the integration branch. Plan 5 *inherits*
it as the canonical first-iteration target.

- Test: `cargo nextest run -p quarto-core --test idempotence lua_shortcode_lipsum_fixed`
  (orchestrator mode only; `SingleFile` passes — the pipeline itself
  is idempotent).
- Beads issue: **bd-3odjm**.
- Symptom: `MalformedSourceInfoPool` from
  `pampa::readers::json::read` re-parsing the orchestrator's AST JSON
  for a lipsum-shortcode-bearing document.
- Pre-Plan-5 cause: code-3 collision (writer emits FilterProvenance
  `[filter_path, line]`; reader decodes as legacy Transformed
  `[parent_id, ...]`).

**The contract:** the very first time Plan 5 runs the idempotence
suite after a reader change lands, `lua_shortcode_lipsum_fixed` must
go green. The full chain is:

  1. Plan 5 lands the legacy code-3 reader change (per §"Code 3 —
     Legacy reader only" below) — recognize FilterProvenance's
     string-array payload, produce
     `Generated { by: filter, from: vec![] }`, fall through to
     legacy Transformed for the numeric-array payload.
  2. `cargo nextest run -p quarto-core --test idempotence
     lua_shortcode_lipsum_fixed` passes.
  3. The full Plan-3 idempotence suite is green (27/27).

**If step 2 fails after the reader change**, the Plan-5 author has a
real signal: either the reader's discrimination between the two
code-3 shapes is wrong, or the lipsum path produces a code-3 shape
that neither arm handles. In that case, do not move on to other
Plan-5 work — the failing test on the integration branch is the
canonical reproduction and must be the focus until green.

This is also a positive: bd-3odjm is the most realistic Plan-5
regression test available — a real fixture, a real pipeline, a real
round-trip — so it doubles as the smoke check before any of the
hand-constructed tests in §"Test plan" run.

## Scope

### In scope

- Add wire format code `4` for `Generated { by, from }`. Payload
  encoding:
  ```json
  {
    "by": { "kind": "...", "data": <object|null> },
    "from": [
      { "role": "<role-string>", "si_id": <pool_id> },
      ...
    ]
  }
  ```
  Outer `from` mirrors the Rust field name (`Generated.from`). Inner
  `si_id` is the source-info pool reference — it points to another
  entry in the pool, typically an `Original` covering the source bytes
  the anchor describes. The name is deliberately distinct from
  `Substring`'s `parent_id`: a Substring genuinely *has* a parent in
  the chain (the slice's ancestor), but an anchor's reference is a
  sideways pointer, not a containment relationship. `si_id` reads as
  "source-info pool index" with no tree-structure overclaim. Multiple
  anchors share an `si_id` naturally (multi-inline shortcode: every
  resolved inline's `Invocation` anchor references the same token's
  pool entry).
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
    string then number): decode as `Generated { by: By::filter(filter_path, line), from: smallvec![] }`.
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
- **Typed anchor list at the wire level.** Each entry in the `from`
  array carries a `role` string and an `si_id` pool reference. This
  keeps the source-info chain typed even at the wire boundary —
  `si_id` refers to another pool entry, never an inlined object.
- **Code 3 stays as a legacy reader** — fixes the latent bug AND
  retires `FilterProvenance` in one step. The reader recognizes both
  old shapes (legacy Transformed array of numbers; FilterProvenance
  `[filter_path, line]`) and dispatches accordingly. Post-Plan 5,
  writers never emit code 3.
- **`from` is one name across three layers, with different inner types
  at each layer.** Worth knowing before reading any one layer in
  isolation:
  - **User-facing (`quarto-source-map`):** `SourceInfo::Generated.from:
    SmallVec<[Anchor; 2]>` where `Anchor { role, source_info: Arc<SourceInfo> }`.
    Carries actual `Arc<SourceInfo>` references.
  - **Writer-internal (`writers/json.rs`):** `SerializableSourceMapping::Generated.from:
    Vec<(AnchorRole, usize)>` where the `usize` is the pool ID returned
    by `intern` for the anchor's source_info. Same semantic concept,
    flattened to pool IDs.
  - **On the wire (JSON):** `"from": [{ "role": "...", "si_id": <pool_id> }, ...]`,
    omitted when empty. Same data, JSON-shaped.
  The name `from` is preserved at every layer so the implementer can
  read top-down without renames; the inner type changes are
  deliberate (Arc → ID → JSON) and follow the pattern already
  established by `Substring.parent` → `parent_id`.
- **Verbose keys (`kind`, `data`, `by`, `from`, `role`, `si_id`)**
  at the payload level for self-documentation. The wire format's outer
  fields (`t`, `r`, `d` at the SourceInfoJson level) stay compact for
  consistency with existing code. The asymmetry is intentional: outer
  fields appear once per pool entry across the whole pool (N×K bytes
  for K outer fields, repeated for each of N entries — the compact
  names amortize across thousands of entries), while the inner payload
  keys appear only inside Generated entries (a minority of pool entries
  — most are Substring/Original from parsing). Document-level overhead
  from the verbose payload keys is empirically small; clarity at the
  new boundary outweighs it. Pool JSON is also gzipped on the wire in
  the orchestrator and hub-client transports, which collapses the
  repeated short keys further.

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
`skip_serializing_if` on `By`. The `"from"` field is omitted when the
list is empty.)

The source-info pool entry for a `Generated` value with **one
Invocation anchor** (shortcode resolution):

```json
{
  "t": 4,
  "r": [0, 0],
  "d": {
    "by": { "kind": "shortcode", "data": { "name": "meta" } },
    "from": [
      { "role": "invocation", "si_id": 7 }
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
    "from": [
      { "role": "invocation",   "si_id": 7 },
      { "role": "value-source", "si_id": 12 }
    ]
  }
}
```

The pool entry's `r: [0, 0]` because Generated doesn't carry its own
offsets — ranges are obtained via the `resolve_byte_range` /
`preimage_in` chain-walk through the `Invocation` anchor.

### Code 3 — Legacy reader only

Post-Plan-5 writers never emit code 3. The arm exists only to read
pre-Plan-5 JSON. Two shapes are possible and the dispatch order is
**numeric-first, then string-headed** — JSON `Number` and `String` are
disjoint types, so the order is unambiguous; numeric goes first because
legacy `Transformed` is the historically larger producer.

```rust
3 => {
    // Legacy code-3 reader. Writers no longer emit code 3.
    //   - Legacy Transformed:        data = [parent_id, ...]   (number-headed)
    //   - Latent FilterProvenance:   data = [filter_path, line] (string-headed)
    // Both shapes are read strictly — `MalformedSourceInfoPool` on any
    // length/type mismatch (same convention as the Substring / Concat
    // arms above).
    let array = data.as_array().ok_or(MalformedSourceInfoPool)?;
    if array.is_empty() { return Err(MalformedSourceInfoPool); }

    if let Some(parent_id) = array[0].as_u64() {
        // Legacy Transformed path. Approximate as Substring pointing to parent.
        // (existing behavior — kept for back-compat)
        let parent_id = parent_id as usize;
        // ...current logic...
        SourceInfo::Substring { parent: ..., start_offset, end_offset }
    } else if let Some(filter_path) = array[0].as_str() {
        // Latent FilterProvenance shape: must be exactly [path, line].
        if array.len() != 2 { return Err(MalformedSourceInfoPool); }
        let line = array[1].as_u64().ok_or(MalformedSourceInfoPool)? as usize;
        SourceInfo::Generated {
            by: By::filter(filter_path.to_string(), line),
            from: smallvec![],
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
    // Generated { by, from }. The outer `r` field is parsed by the
    // caller and *ignored here* — Generated entries don't carry their
    // own offsets; ranges come from chain-walking the Invocation anchor
    // via `resolve_byte_range` / `preimage_in`. The writer hard-codes
    // `r: [0, 0]` for code-4 entries; downstream code that reads `r`
    // directly will see zeros — that's the signal to walk the anchor
    // chain instead. A code-4 entry with `r != [0, 0]` from an
    // older/future writer is silently accepted (precedent: today's
    // Concat arm also parses `r` but doesn't use it).
    //
    // Strict on every other shape: missing `by`, `by.kind`, `from` entry
    // missing `role`/`si_id`, `from` present but not an array, or an
    // `Other("")` role string → `MalformedSourceInfoPool`. Same
    // convention as the Substring/Concat arms above.
    let obj = data.as_object().ok_or(MalformedSourceInfoPool)?;
    let by_obj = obj.get("by").and_then(|v| v.as_object())
        .ok_or(MalformedSourceInfoPool)?;
    let kind = by_obj.get("kind").and_then(|v| v.as_str())
        .ok_or(MalformedSourceInfoPool)?.to_string();
    let by_data = by_obj.get("data").cloned().unwrap_or(Value::Null);
    let by = By { kind, data: by_data };

    let mut from = SmallVec::<[Anchor; 2]>::new();
    match obj.get("from") {
        None => {} // absent ≡ empty (writer skips empty `from`)
        Some(v) => {
            let from_arr = v.as_array().ok_or(MalformedSourceInfoPool)?;
            for entry in from_arr {
                let entry_obj = entry.as_object()
                    .ok_or(MalformedSourceInfoPool)?;
                let role_str = entry_obj.get("role").and_then(|v| v.as_str())
                    .ok_or(MalformedSourceInfoPool)?;
                let role = parse_anchor_role(role_str)?;
                let si_id = entry_obj.get("si_id").and_then(|v| v.as_u64())
                    .ok_or(MalformedSourceInfoPool)? as usize;
                if si_id >= current_index {
                    return Err(CircularSourceInfoReference(si_id));
                }
                let si = pool.get(si_id).cloned()
                    .ok_or(InvalidSourceInfoRef(si_id))?;
                from.push(Anchor { role, source_info: Arc::new(si) });
            }
        }
    }

    SourceInfo::Generated { by, from }
}

fn parse_anchor_role(s: &str) -> Result<AnchorRole, MalformedSourceInfoPool> {
    match s {
        "invocation"   => Ok(AnchorRole::Invocation),
        "value-source" => Ok(AnchorRole::ValueSource),
        _ => {
            let name = s.strip_prefix("other:")
                .ok_or(MalformedSourceInfoPool)?;
            if name.is_empty() { return Err(MalformedSourceInfoPool); }
            Ok(AnchorRole::Other(name.to_string()))
        }
    }
}
```

Writer:

```rust
SerializableSourceMapping::Generated { by, from } => {
    let mut by_json = json!({ "kind": by.kind });
    if !by.data.is_null() { by_json["data"] = by.data.clone(); }

    let mut d = json!({ "by": by_json });
    if !from.is_empty() {
        let arr: Vec<Value> = from.iter()
            .map(|(role, si_id)| json!({
                "role": serialize_anchor_role(role),
                "si_id": si_id,
            }))
            .collect();
        d["from"] = Value::Array(arr);
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

### TypeScript wire-format definitions

`ts-packages/preview-renderer/src/types/sourceInfo.ts` is a hand-mirror
of the Rust wire format. Earlier provenance-epic churn (during the
2026-05-20 design discussion) left it carrying a stale forward-declared
split: code 4 = `Synthetic { d: By }`, code 5 = `Derived { d: { from, by } }`.
That split never shipped. Plan 5 reconciles the file with the unified
Generated design:

**Before Plan 5 (current file):**

```ts
export type SourceInfoEntry =
    | { t: 0; r: [number, number]; d: number }
    | { t: 1; r: [number, number]; d: number }
    | { t: 2; r: [number, number]; d: Array<[number, number, number]> }
    | { t: 3; r: [number, number]; d: [string, number] }
    | { t: 4; r: [0, 0]; d: By }                                // Synthetic — never shipped
    | { t: 5; r: [0, 0]; d: { from: number; by: By } };         // Derived — never shipped
```

**After Plan 5:**

```ts
export interface AnchorRef {
    role: string;          // "invocation" | "value-source" | "other:<name>"
    si_id: number;         // index into the source-info pool
}

export type SourceInfoEntry =
    | { t: 0; r: [number, number]; d: number }                              // Original
    | { t: 1; r: [number, number]; d: number }                              // Substring
    | { t: 2; r: [number, number]; d: Array<[number, number, number]> }    // Concat
    | { t: 3; r: [number, number]; d: [string, number] | [number, ...number[]] } // legacy reader only (no new writes)
    | { t: 4; r: [0, 0]; d: { by: By; from?: AnchorRef[] } };               // Generated
// code 5 — unassigned, free for future reservation
```

Changes vs. current file:

- Code 4's `d` shape narrows from bare `By` to `{ by: By; from?: AnchorRef[] }`.
- Code 5's entry is removed entirely. It was never emitted by any
  shipping writer; no on-disk artifact carries it. Removing the variant
  is safe.
- Code 3's `d` shape widens to a union to reflect the dual-shape legacy
  reader (string-headed = FilterProvenance, numeric-headed = old
  Transformed). New writers don't emit code 3 either way, so this is a
  read-side typing only.
- `from?` is absent (not `[]`) when empty — writer skips the field via
  `if !from.is_empty()`. TS consumers use `entry.d.from ?? []` as the
  canonical access pattern; absent and `[]` are treated equivalently.
- The file's header doc-comment (lines 10–19 of the current file)
  references `Synthetic` and `Derived` by name and says "Plan 5 wires
  this up." Rewrite it to describe Generated instead and drop the
  Synthetic/Derived nomenclature.

**`utils/sourceInfo.ts` reconciliation** (full enumeration of the
"audit" called for in Phase 5):

- `entryFor(node, pool)` — unchanged.
- `isDerived(node, pool)` — **delete entirely.** It checks `entry?.t === 5`,
  which after Plan 5 is unreachable (code 5 unassigned). Any caller
  still using it migrates to `isAtomicSourceInfo`.
- `isAtomicSourceInfo(node, pool, atomicKinds)` — rewrite. The current
  body branches on `entry.t === 5` (always atomic) OR
  `entry.t === 4 && atomicKinds.has(entry.d.kind)`. After Plan 5: only
  `entry.t === 4 && atomicKinds.has(entry.d.by.kind)` — the `kind`
  field moves from `entry.d.kind` to `entry.d.by.kind`, and the code-5
  branch is removed.
- `ATOMIC_SYNTHETIC_KINDS` constant (currently empty) — **rename to
  `ATOMIC_KINDS`** to match the Rust canonical name `By::is_atomic_kind`,
  and populate with the Plan-4 atomic set:
  `new Set(["filter", "shortcode", "title-block", "tree-sitter-postprocess"])`.
  The accompanying doc-comment ("mirrors `By::is_atomic_synthesizer()`")
  is updated to "mirrors `By::is_atomic_kind()`."

The TS type and the Rust serializer must agree byte-for-byte; the
header doc-comment cites the Rust file as the source of truth, same
convention as for the atomic-CustomNodes registry.

## Work items

Phase-ordered. Each phase compiles cleanly **and leaves the workspace
fully green** before the next begins. Phase 1 lands on its own as the
bd-3odjm fix even if the rest of Plan 5 stalls.

**Ordering note.** The naive 1 → 2 → 3 → 4 order would break round-trip
between Phase 2 (writer emits code 4) and Phase 4 (reader decodes code
4) — every fixture containing a filter or shortcode would fail with
`MalformedSourceInfoPool` on code 4 in that window. The order below
puts the code-4 reader (renumbered Phase 2) before the writer change
so each phase leaves the workspace green. Phases 3 (writer) and 4
(streaming writer) **must land atomically** as a single commit/squash
because Phase 3 removes `SerializableSourceMapping::FilterProvenance`,
which the streaming writer references — splitting them produces a
build break.

### Phase 0 — Start gate

- [x] Confirm Plan 4 (Generated + By + Anchor + AnchorRole) has merged
      into `feature/provenance`. If not, stop — Plan 5 cannot build.
      Verify with `git grep -n "enum SourceInfo" crates/quarto-source-map/src/source_info.rs`
      and confirm a `Generated` arm exists.
- [x] Confirm the Plan-4 interim writer state is present in
      `crates/pampa/src/writers/json.rs`: a `SourceInfo::Generated { by, .. }`
      arm in `SourceInfoSerializer::intern` that recovers
      `(filter_path, line)` via `by.as_filter().expect(...)` and emits
      `SerializableSourceMapping::FilterProvenance`. This is the arm
      Phase 3 rewrites. As of Plan 4's commit, the arm lives around
      `writers/json.rs:314-331`; refresh before implementing. Verify
      with `git grep -n "Plan 5's wire-code 4 emitter" crates/pampa/src/writers/json.rs`
      — exactly one hit (Plan 4's `expect` message).
- [x] Confirm `SerializableSourceMapping::FilterProvenance` still
      exists as a variant in `writers/json.rs` (it does post-Plan-4 —
      Plan 4 deliberately kept the *serializable* enum variant even
      though the source-map variant is gone, because the interim
      writer arm above still emits it). Verify with
      `git grep -n "SerializableSourceMapping::FilterProvenance" crates/pampa/`
      — expect ~4 hits (writer's `to_json` arm, the interim `intern`
      arm above, the streaming writer's two arms in
      `stream_write_source_info_pool`). All four go away in Phase 3+4.
- [x] Confirm no on-disk JSON snapshots carry code-3 entries that the
      new dual-shape reader would need to decode. Verified at planning
      time: `grep -rn '"t":3\|"t": 3' crates/ tests/ hub-client/`
      returns zero hits and `grep -rln 'FilterProvenance' crates/pampa/snapshots
      crates/pampa/tests/snapshots crates/quarto-core/tests/snapshots`
      is also empty. Re-run before starting Phase 1 to confirm nothing
      has been added in the interim. **No fixture migration needed.**

### Phase 1 — Legacy code-3 dual-shape reader (closes bd-3odjm)

- [x] Add `parse_anchor_role` helper in `crates/pampa/src/readers/json.rs`
      (used by Phase 2 too — landing it here is a no-op until then).
- [x] Rewrite the code-3 arm in `SourceInfoDeserializer::new` (currently
      `crates/pampa/src/readers/json.rs:252-283`) per §"Code 3 — Legacy
      reader only": dispatch on `data[0]` numeric → legacy Substring;
      string → strict `[path, line]` decode to `Generated { by:
      By::filter(path, line), from: smallvec![] }`; otherwise
      `MalformedSourceInfoPool`. No silent `unwrap_or(0)` — line must
      be a number or the entry is malformed.
- [x] Rewrite the code-3 reader's doc-comment to:
      "Legacy reader for code 3 — accepts both old Transformed
      numeric-array and buggy FilterProvenance string-array; writes
      never emit code 3."
- [x] Run `cargo nextest run -p quarto-core --test idempotence lua_shortcode_lipsum_fixed`
      → green (closes bd-3odjm).
- [x] Run the full Plan-3 idempotence suite → 27/27 green.
- [x] **Per-phase verification gate:** `cargo nextest run --workspace`
      → all green. bd-3odjm closed; no regressions. Phase 1 is
      independently revertible (the reader change is purely additive
      — restoring the prior arm removes only the new FilterProvenance
      recovery branch).
- [x] **Rollback signal:** the Phase-1 reader change only touches the
      code-3 arm; other code-paths and other pool entries are
      unaffected. If a Plan-3 idempotence case *other than*
      `lua_shortcode_lipsum_fixed` regresses (or a workspace test
      outside the idempotence suite regresses), that is a real signal
      — either the dual-shape discriminator misclassifies a payload
      shape that *isn't* the buggy FilterProvenance, or the new
      `Generated` recovery loses information a downstream test
      depended on. Do not paper over it by relaxing the strict
      rejection rules. Investigate the failing case's pool entries
      with `jq '.astContext.sourceInfoPool'` on the offending fixture's
      JSON, identify which code-3 entries are present, and decide
      whether the discriminator needs an additional case or the failing
      test had a buggy pre-existing expectation. Either way, file a
      beads issue.

### Phase 2 — Code-4 reader

Lands before any writer change so the reader is forward-compatible
when Phase 3 starts emitting code 4. Phase 2 alone leaves the workspace
green: no production code emits code 4 yet, so the new arm is exercised
only by hand-constructed tests.

- [x] Add a `4 => { … }` arm in `SourceInfoDeserializer::new`
      (`readers/json.rs:154-287`) per §"Code 4 — Reader / writer":
      decode `by` (kind + optional data), decode `from` array entries
      via `parse_anchor_role` + `si_id`, with the `si_id < current_index`
      circular-ref guard.
- [x] Reject malformed code-4 payloads with `MalformedSourceInfoPool`:
      missing `by`; missing `by.kind`; `from` present but not an array;
      `from` entry not an object; `from` entry missing `role`; `from`
      entry missing `si_id`; unrecognized role string; `Other("")` with
      empty suffix. See §"Code 4 — Reader / writer" for the full
      snippet — same strictness as the Substring/Concat arms.
- [x] Silently accept code-4 entries with `r != [0, 0]` (one-line
      comment in the arm; precedent: today's Concat arm).
- [x] Add the forward-compat unit tests in `readers/json.rs::tests` —
      see Phase 6 for the full list of tests landing here.

### Phase 3 — Writer code-4 emit (`SerializableSourceMapping` + intern + `to_json`) **+ Phase 4 streaming-writer parity, landed atomically**

Phases 3 and 4 (below) must land in one commit / squash: Phase 3
removes `SerializableSourceMapping::FilterProvenance`, which Phase 4's
streaming writer references — splitting them produces a build break.

Starting state from Plan 4: `SourceInfo::FilterProvenance` is gone, but
`SerializableSourceMapping::FilterProvenance` survives because Plan 4's
interim writer arm (see Plan 4 §"Migrations", `pampa/src/writers/json.rs:314`)
converts `SourceInfo::Generated { by: filter, .. }` into the legacy
shape via `by.as_filter().expect(...)`. That arm panics for non-filter
Generated kinds, so the workspace only stays buildable as long as no
non-filter Generated is constructed — Plan 6 doesn't ship shortcode
stamping until later, so Plan 4's expect is safe in the interim.
Phase 3 removes both the interim arm and the `SerializableSourceMapping::FilterProvenance`
variant at once.

- [x] Add `Generated { by: By, from: Vec<(AnchorRole, usize)> }` to
      `SerializableSourceMapping` in `crates/pampa/src/writers/json.rs`.
- [x] Replace Plan 4's interim `SourceInfo::Generated { by, .. } => …
      SerializableSourceMapping::FilterProvenance` arm with a real
      `SerializableSourceMapping::Generated { … }` construction (no more
      `by.as_filter().expect(...)`); supports all `by.kind` values
      uniformly.
- [x] Remove `SerializableSourceMapping::FilterProvenance` (no longer
      reachable after the interim arm above is rewritten).
- [x] Update `SourceInfoSerializer::intern` (`writers/json.rs:260-333`):
      - Recognize `SourceInfo::Generated { by, from }`.
      - **Recursively intern each anchor's `source_info` BEFORE pushing
        the parent pool entry** (same pattern as today's `Concat` and
        `Substring` arms), so anchor `si_id`s are strictly less than
        the Generated's own id. The reader's `si_id < current_index`
        guard requires this invariant.
      - **Reuse the existing `arc_parent_ids` cache** (keyed by
        `Arc::as_ptr(&anchor.source_info)`) for anchor dedup. Same cache,
        same key shape as `Substring.parent`. Multi-inline shortcode
        resolutions (every resolved inline shares one `Arc` for the
        token's `Original`) hit the cache and produce a single pool
        entry for the shared target — exactly the dedup behavior the
        "Anchor dedup test" in Phase 6 verifies.
      - Build the **`intern`-match-arm return tuple** as
        `(0, 0, SerializableSourceMapping::Generated { by, from: from_ids })`
        — `intern` returns `(start_offset, end_offset, mapping)`; the
        `r: [0, 0]` rule is enforced by hard-coding the first two
        components to zero, exactly as today's FilterProvenance arm at
        lines 314-322 does.
- [x] Update `SerializableSourceInfo::to_json` (`writers/json.rs:169-190`)
      with the code-4 arm per §"Code 4 — Reader / writer".
- [x] Add `serialize_anchor_role` helper.
- [x] Update the `SourceInfoJson.t` legend comment at
      `writers/json.rs:115` from
      `"0=Original, 1=Substring, 2=Concat, 3=FilterProvenance"` to
      `"0=Original, 1=Substring, 2=Concat, 3=Legacy (read-only), 4=Generated"`.

### Phase 4 — Streaming writer parity (atomic with Phase 3)

- [x] Add the code-4 arm in `stream_write_source_info_pool`
      (`writers/json.rs:3482-3532` as of `eb06c4cf`; refresh before
      implementing); mirror the `to_json` shape exactly.
- [x] Remove the FilterProvenance arms (lines 3509-3514 emit, line 3526
      tag as of `eb06c4cf`). They become unreachable once
      `SerializableSourceMapping::FilterProvenance` is gone from Phase 3.

### Phase 5 — TypeScript types

- [x] Rewrite `ts-packages/preview-renderer/src/types/sourceInfo.ts`
      per §"TypeScript wire-format definitions":
      - Add `AnchorRef` interface.
      - Code 4's `d` becomes `{ by: By; from?: AnchorRef[] }`.
      - Code 3's `d` becomes `[string, number] | [number, ...number[]]`.
      - Remove the code-5 entry.
      - Rewrite the header doc-comment to describe Generated, not
        Synthetic/Derived. The current header cites
        `crates/pampa/src/writers/json.rs:54-91`, which is stale (the
        wire-format types now live at ~lines 109-207 of that file). The
        new doc-comment should cite **two** sources of truth: the Rust
        enum `SourceInfo` in
        `crates/quarto-source-map/src/source_info.rs` (canonical
        producer-side definition) and the JSON wire mirror in
        `crates/pampa/src/writers/json.rs` (`SerializableSourceMapping`
        ~lines 193-207, `SourceInfoJson` ~lines 109-116, code-4
        serializer in `to_json` ~lines 167-190). Do not bake in exact
        line numbers — cite the type names; they will outlast line
        drift.
- [x] Update `ts-packages/preview-renderer/src/utils/sourceInfo.ts` per
      §"TypeScript wire-format definitions" → `utils/sourceInfo.ts`
      reconciliation:
      - Delete `isDerived` entirely.
      - Rewrite `isAtomicSourceInfo` to read `entry.d.by.kind` (was
        `entry.d.kind`) and drop the code-5 branch.
      - **Rename** `ATOMIC_SYNTHETIC_KINDS` → `ATOMIC_KINDS` to match
        the Rust canonical `By::is_atomic_kind`.
      - Populate `ATOMIC_KINDS` with `new Set(["filter", "shortcode",
        "title-block", "tree-sitter-postprocess"])` (mirrors Plan 4's
        `By::is_atomic_kind`).
      - Update the file's doc-comment from "mirrors
        `By::is_atomic_synthesizer()`" to "mirrors `By::is_atomic_kind()`."
      - Migrate any remaining `isDerived` callers (`grep -rn isDerived ts-packages/`)
        to the new `isAtomicSourceInfo` shape.
- [x] Update `ts-packages/preview-renderer/src/utils/sourceInfo.test.ts`
      — the existing tests will not compile after the changes above.
      Specifically:
      - Drop the `import { isDerived, ATOMIC_SYNTHETIC_KINDS }` lines
        and the entire `describe('isDerived', …)` block. `isDerived` is
        gone; `ATOMIC_SYNTHETIC_KINDS` is renamed `ATOMIC_KINDS` and now
        populated (the existing `is empty in 2A` assertion no longer
        holds).
      - Rewrite `samplePool`:
        - Drop the code-5 entry entirely (codes 5 unassigned post-Plan-5).
        - Reshape the code-4 entry from `d: { kind: 'IncludeShortcode' }`
          (bare `By`) to `d: { by: { kind: 'shortcode', data: { name: 'meta' } } }`
          (no `from` — absent is the canonical empty form). Add a second
          code-4 entry with `from: [{ role: 'invocation', si_id: 0 }]`
          so the `entry.d.from ?? []` access pattern is exercised.
        - Reshape the code-3 entry: keep one with `d: ['filter.lua', 42]`
          (string-headed legacy FilterProvenance) and add a sibling with
          `d: [0]` (numeric-headed legacy Transformed) to exercise the
          new dual-shape `d` type.
      - Rewrite the `isAtomicSourceInfo` describe block: the
        "Synthetic vs Derived" framing is dead. Drive new assertions
        against `ATOMIC_KINDS` populated with the Plan-4 atomic set,
        using a code-4 entry whose `by.kind` is `"shortcode"` (atomic)
        and another whose `by.kind` is `"sectionize"` (non-atomic).
      - Add an `ATOMIC_KINDS` describe block asserting the four
        Plan-4 atomic kinds are members and at least one non-atomic kind
        (`"sectionize"`) is not. Replaces the deleted
        `ATOMIC_SYNTHETIC_KINDS` block.
      - Run `cd hub-client && npm run build:all` after the rewrite — the
        production build (`tsc -b && vite build`) is stricter than
        `tsc --noEmit` / vitest and catches type-narrowing errors that
        unit tests miss.

### Phase 6 — Tests

**Test placement.** All tests are hand-written (no proptest in this
file; the repo doesn't use it heavily). Unit tests extend the existing
test modules; the end-to-end integration test extends the existing
integration crate:

- Writer-side unit tests → `crates/pampa/src/writers/json.rs::tests`
  (joins the existing `test_source_info_pool_*` cluster at
  `writers/json.rs:3688+`).
- Reader-side unit tests → `crates/pampa/src/readers/json.rs::tests`
  (joins the existing `test_deserialize_source_info_pool_*` cluster at
  `readers/json.rs:2479+`).
- End-to-end integration test → `crates/pampa/tests/json_reader_smoke_tests.rs`
  (existing integration crate that drives file fixtures through
  `pampa::readers::json::read`).

Per-phase landing: forward-compat tests for the code-4 reader and the
legacy code-3 recovery test land with Phase 1/2 (reader-only); writer
round-trips, dedup, and the end-to-end test land with Phases 3+4 once
the writer emits code 4.

**Tests:**

- [x] Round-trip property test for every `SourceInfo` variant (Original,
      Substring, Concat, Generated with various By kinds and `from`
      configurations). Hand-written cases (one per shape). See §Test
      plan.
- [x] Concat-of-Generated round-trip case: a `Concat { pieces }` whose
      pieces' `source_info` is `Generated`. Serialize → deserialize →
      assert structural equality. Closes a coverage gap — current
      production paths emit this shape (e.g. coalesced filter-emitted
      spans). Sits in the writer-side test module since it exercises
      the recursive intern of mixed-variant pieces.
- [x] Substring-of-Generated round-trip case: a
      `Substring { parent: Arc::new(Generated { … }), … }` — e.g. a
      filter-emitted span whose substring is later coalesced. The
      writer's existing `intern` recursion routes
      `Substring.parent: Arc<SourceInfo>` through the new code-4 path
      with no extra logic, and the reader's existing Substring arm
      reads the parent_id back as a code-4 pool entry. The test serves
      as a regression guard for that path: confirm pool ordering
      (parent Generated entry interns strictly before the Substring
      child) and assert structural equality across serialize →
      deserialize. Co-located with the Concat-of-Generated case in
      the writer-side test module.
- [x] Filter-provenance recovery test (hand-constructed code-3 with
      string-array payload → `Generated { by: filter, from: smallvec![] }`).
- [x] Legacy Transformed back-compat test (hand-constructed code-3 with
      numeric-array payload → `Substring`).
- [x] Strict code-3 rejection tests: `[path]` (missing line) and
      `[path, "not-a-number"]` (non-numeric line) both
      → `MalformedSourceInfoPool`. Guards the no-`unwrap_or(0)` rule.
- [x] Forward-compat test (code-4 with unknown `by.kind`, arbitrary
      `data` → preserved round-trip).
- [x] Strict code-4 rejection tests: missing `by`, missing `by.kind`,
      `from` present but not an array, `from` entry not an object,
      `from` entry missing `role`/`si_id`, role string `"other:"`
      (empty suffix) → all `MalformedSourceInfoPool`.
- [x] **Anchor dedup test (writer-side only).** Hand-construct an AST
      with N inlines, each carrying
      `Generated { by: By::shortcode("meta"), from: smallvec![Anchor::invocation(Arc::clone(&shared))] }`.
      Serialize. Assert: the pool contains the shared target exactly
      once and every Generated entry's `from[0].si_id` references that
      single ID. **Read-side note:** deserialization rebuilds each anchor
      with a fresh `Arc`, so a subsequent re-serialization produces N
      copies — this test verifies the *write-time* optimization keyed
      on `Arc::as_ptr`. See [[anchor-dedup-invariant]] in §"Risk areas"
      for the broader contract. Test passes Plan-5-alone (no shortcode
      resolver needed — Arc sharing is hand-wired).
- [x] Streaming-writer parity test. Helper shape:
      `roundtrip_via_stream(ast) -> ast` that calls `stream_write_pandoc`
      into a `Vec<u8>`, reads back via `pampa::readers::json::read`,
      and asserts SourceInfo equality at chosen Generated nodes. The
      streaming writer's match arms are independent of `to_json`'s;
      without this coverage, a Phase-4 regression in
      `stream_write_source_info_pool` could slip through.
- [x] AnchorRole round-trip test: build a `Generated` with each role
      (`Invocation`, `ValueSource`, `Other("ext/foo/bar")`) wrapped in
      anchors; serialize through JSON via the writer's code-4 path;
      deserialize via the reader's code-4 path; assert the role survives.
- [x] End-to-end production reachability test (kbd-shortcode fixture →
      `render_qmd_to_preview_ast` → JSON → `pampa::readers::json::read`
      → assert success and recovered shape). Lives in
      `crates/pampa/tests/json_reader_smoke_tests.rs`.
- [x] TypeScript-side type round-trip (parse a JSON pool with Generated
      entries; confirm `SourceInfoEntry` shape matches; confirm
      `entry.d.from ?? []` access pattern works for both absent and
      present `from`).

### Phase 7 — Verification gate

- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace --no-fail-fast` all green
      (bd-3odjm closed in Phase 1; no other regressions). Use
      `--no-fail-fast` so a single regression doesn't hide downstream
      green tests — same convention used to close Plan 4.
- [x] `cargo xtask verify` (full — `quarto-core`/`pampa` are WASM
      consumers; hub-build leg matters). The WASM rebuild leg will
      modify `crates/wasm-quarto-hub-client/Cargo.lock` as a side
      effect (separate lockfile from the workspace one); include it
      in the commit. Plan 4 hit this and committed it without issue.
- [x] `git grep "FilterProvenance"` returns only legacy-reader / legacy
      doc references (no writer emissions, no `SerializableSourceMapping`
      variant).
- [x] Update bd-3odjm: close at the Phase-1 commit (the reader change
      that turns `lua_shortcode_lipsum_fixed` green). The close trigger
      is the commit itself, not a downstream PR or merge — Plan 5 lands
      on the `feature/provenance` integration branch via merge commits,
      not a standalone PR, so tying the close to the commit gives the
      issue a concrete reference. Refresh its description to use `from:`
      not `anchors:` if reopened for any reason. **If Phase 3 or 4
      introduces a *new* failure mode in the lipsum fixture, file a
      fresh beads issue** rather than reopening bd-3odjm — that issue is
      specifically the code-3 collision and should stay scoped to it.

## Implementation guidance carried over from Plan 4

A few small things came up during Plan 4 that are worth knowing before
starting Plan 5:

- **`SmallVec::new()` is the construction pattern, not `smallvec![]`.**
  Plan 4 uniformly used `SmallVec::<[Anchor; 2]>::new()` for empty
  lists, never the `smallvec!` macro. The reader file
  `crates/pampa/src/readers/json.rs` does not currently import
  `smallvec::smallvec`. Code samples in this plan that show
  `smallvec![]` are pseudocode — when implementing, write
  `SmallVec::new()` (matches Plan 4's convention, avoids a needless
  import). The `SmallVec` type itself needs
  `use smallvec::SmallVec;` at the top of the file — Plan 4 added
  this to every consumer it touched (`pampa/src/lua/diagnostics.rs`,
  `pampa/src/lua/types.rs`); `readers/json.rs` and the writer's
  Generated arm (Phase 3) will need it too.

- **Don't name a local `gen`.** Rust 2024 makes `gen` a reserved
  keyword. Plan 4's test code had to rename a `let gen = ...` to
  `let generated = ...`. None of Plan 5's code samples currently use
  `gen` as an identifier — keep it that way. (Some writer prose uses
  `gen.invocation_anchor()` as shorthand; that's pseudocode, not
  literal Rust to type.)

- **Phase boundary "compiles cleanly" semantics.** Plan 4 found that
  "each phase compiles cleanly" really means "the directly-touched
  crate compiles cleanly" — adding a new `SourceInfo` variant
  immediately broke `match` exhaustiveness across ~10 crates, and the
  workspace stayed red between Plan-4 Phase 1 and Phase 5. Plan 5's
  Phase 1 → 2 → 3+4 ordering above explicitly avoids this trap (each
  phase leaves the workspace green); the *atomic* Phase 3+4 squash is
  the only place where you have to land more than one commit's worth
  of code in a single push.

- **`cargo xtask verify --skip-rust-tests` is a useful intermediate.**
  Plan 4 ran `cargo nextest run --workspace --no-fail-fast` first
  (confirms only bd-3odjm is red), then `cargo xtask verify
  --skip-rust-tests` (confirms the WASM/hub-client legs are green
  without re-running the same Rust tests). Plan 5 should follow the
  same split for the final verification gate.

## Open questions for implementation

- **Eventually retiring code 3**: at some point, no JSON files in the
  wild contain code 3 (the buggy FilterProvenance shape never
  round-tripped before Plan 5; the legacy Transformed shape predates a
  transition we made earlier). Could remove the legacy reader. Don't
  need to decide now.
- **Detecting malformed code 4 payloads**: settled in Phase 2 of
  §"Work items" — `MalformedSourceInfoPool` for missing `by`, missing
  `by.kind`, `from` not an array, `from` entry not an object, `from`
  entry missing `role`/`si_id`, unrecognized role string, and empty
  `Other("")` suffix.
- **Streaming writer parity** (`stream_write_source_info_pool`): settled
  in Phase 4 of §"Work items" — atomic with Phase 3 (writer code-4 emit).
- **Pool deduplication of anchor `si_id` references**: when many
  Generated entries share the same anchor target (multi-inline
  shortcode), the writer interns once and reuses the ID. The existing
  `arc_parent_ids` HashMap pattern (already used for `Substring.parent`)
  handles this — same interning mechanism, different reader-side name
  (`si_id` for anchors, `parent_id` for substrings). This is a
  **writer-side optimization only** — deserialization rebuilds each
  anchor with a fresh `Arc`, so pool-size is not stable over
  read-write-read. AST content and Plan-3 hashes (which exclude
  `source_info`) are stable. See [[anchor-dedup-invariant]] in §"Risk
  areas".
- **TypeScript hand-mirror updates**: see §"TypeScript wire-format
  definitions" above. Settled — code 4's `d` becomes `{ by; from? }`,
  code 5 is removed, code 3's `d` becomes a union for the dual-shape
  legacy reader, `ATOMIC_SYNTHETIC_KINDS` renames to `ATOMIC_KINDS`
  with the Plan-4 atomic set populated. The companion test file
  `utils/sourceInfo.test.ts` is rewritten in lockstep — see Phase 5.
- **Writer JSON-build style**: hand-build via `json!` macro, matching
  the existing convention throughout `writers/json.rs`. Not derive-based.
  Settled.
- **`By::kind` canonical enumeration**: see Plan 4's `By::` builders
  (`filter`, `sectionize`, `user_edit`, `shortcode`, `include`,
  `title_block`, `footnotes`, `appendix`, `tree_sitter_postprocess`,
  `raw`) for the full set. Plan 5 emits whatever `by.kind` string is
  present, kebab-case throughout. Atomic-kind list mirrors
  `By::is_atomic_kind` (`filter | shortcode | title-block |
  tree-sitter-postprocess`). Cross-plan invariant — no Plan-5-owned
  decision here.

## References

(Line numbers as of `feature/provenance` @ 4c465768. Plan 4's migration
will shift these; refresh before implementing.)

- `crates/pampa/src/writers/json.rs:115` — `SourceInfoJson.t` field
  comment, currently `"0=Original, 1=Substring, 2=Concat, 3=FilterProvenance"`.
  Plan 5 extends the legend to include `4=Generated` and notes code 3
  as legacy reader only.
- `crates/pampa/src/writers/json.rs:160-190` — `SerializableSourceInfo`
  struct and `to_json` method. Code-3 emit at lines 180-182 (the bug).
- `crates/pampa/src/writers/json.rs:193-207` — `SerializableSourceMapping`
  enum (Original/Substring/Concat/FilterProvenance arms). Phase 3 adds
  a `Generated` arm and removes `FilterProvenance`.
- `crates/pampa/src/writers/json.rs:260-333` — `SourceInfoSerializer::intern`;
  Phase 3 adds a `SourceInfo::Generated` arm with topologically-ordered
  anchor recursion.
- `crates/pampa/src/writers/json.rs:3482-3532` — `stream_write_source_info_pool`;
  Phase 4 mirrors the to_json changes here (lines 3509-3514 emit, line
  3526 tag).
- `crates/pampa/src/readers/json.rs:99-293` — `SourceInfoDeserializer::new`
  (the pool reader). Code-3 arm at lines 252-283 (Phase 1 rewrites);
  Phase 2 adds a code-4 arm.
- `crates/quarto-source-map/src/source_info.rs:21-55` — `SourceInfo` enum
  (extended by Plan 4 — confirm Generated/By/Anchor/AnchorRole present
  before Plan 5 starts; see Phase 0).
- `ts-packages/preview-renderer/src/types/sourceInfo.ts` — JS-side
  `SourceInfoEntry`. See §"TypeScript wire-format definitions" for the
  full before/after.
- `ts-packages/preview-renderer/src/utils/sourceInfo.ts` — JS-side
  helpers (`isAtomicSourceInfo`, etc.); needs adjustment for the new
  shape per Plan 4.

## Test plan

(Hand-written tests; the repo doesn't use proptest in this area. See
Phase 6 for test-file placement and per-phase landing.)

- **Round-trip property test**: for each variant (Original, Substring,
  Concat, Generated with various By kinds and anchor configurations),
  build a `SourceInfo`, serialize to JSON, deserialize, assert
  equality. Cover the full enum.
- **Concat-of-Generated round-trip**: a `Concat { pieces }` whose
  pieces' `source_info` is `Generated` (the shape produced by coalesced
  filter-emitted spans). Serialize → deserialize → assert structural
  equality. Closes a coverage gap not exercised by the per-variant
  property test above.
- **Substring-of-Generated round-trip**: a
  `Substring { parent: Arc::new(Generated { … }), … }`.
  `Substring.parent: Arc<SourceInfo>` is structurally unrestricted, so
  this shape can arise whenever a transform produces a span and a
  downstream coalesce or slice carves a substring out of it. The
  serializer's `Substring` arm interns the parent recursively, which
  routes through the new code-4 arm; the reader's `Substring` arm then
  reads the parent_id back. Round-trip the construction and assert
  structural equality.
- **Filter-provenance recovery test**: hand-construct a JSON pool entry
  with the buggy code-3-with-string-array-payload shape. Read it.
  Assert the reader produces `Generated { by: filter, from: smallvec![] }`
  with the right path/line via `by.as_filter()`.
- **Strict code-3 rejection**: hand-construct `[path]` (missing line)
  and `[path, "not-a-number"]` (non-numeric line); assert both
  → `MalformedSourceInfoPool`. Guards the no-`unwrap_or(0)` rule.
- **Legacy Transformed back-compat test**: hand-construct a JSON pool
  entry with code-3-with-numeric-array-payload (the legacy Transformed
  shape). Assert the reader still produces a `Substring` (preserving
  today's back-compat behavior).
- **Forward-compat test**: hand-construct a JSON pool entry with code 4
  and an unknown kind (`"kind": "ext/future/foo"`, arbitrary data).
  Assert it decodes as `Generated { by: By { kind: "ext/future/foo",
  data: ... }, from: smallvec![] }`. Round-trips unchanged.
- **Strict code-4 rejection**: missing `by`, missing `by.kind`, `from`
  present but not an array, `from` entry not an object, `from` entry
  missing `role`/`si_id`, unrecognized role string, and role string
  `"other:"` (empty `Other` suffix) → all `MalformedSourceInfoPool`.
- **Anchor dedup test (writer-side only)**: build an AST where N
  inlines carry Generated source_info each with an `Invocation` anchor
  wrapping `Arc::clone(&shared)`. Serialize. Confirm the pool contains
  the shared target exactly once and each Generated entry's
  `from[0].si_id` references it by ID. *Read-side note:* deserialization
  rebuilds each anchor with a fresh `Arc`; this test only verifies the
  write-time optimization (see [[anchor-dedup-invariant]] in §"Risk
  areas"). Test passes Plan-5-alone (no shortcode resolver needed).
- **Streaming-writer parity test**: implement helper
  `roundtrip_via_stream(ast) -> ast` that streams the AST via
  `stream_write_pandoc` into a `Vec<u8>` and reads back through
  `pampa::readers::json::read`. Run a representative Generated-bearing
  AST through it; assert equality. The streaming writer's match arms
  are independent of `to_json`'s, so a Phase-4 regression could
  otherwise slip through.
- **AnchorRole round-trip test**: build a `Generated` with each role
  (`Invocation`, `ValueSource`, `Other("ext/foo/bar")`) wrapped in
  anchors; serialize through JSON via the writer's code-4 path;
  deserialize via the reader's code-4 path; assert the role survives.
- **Live regression test already on the integration branch:**
  `cargo nextest run -p quarto-core --test idempotence lua_shortcode_lipsum_fixed`
  (filed as **bd-3odjm**; see §"Inherited failure that must close on
  Plan 5's first reader change (bd-3odjm)" above). This is the
  fastest first-iteration smoke check: it drives a real pipeline + a
  real shortcode + a real JSON round-trip + the existing Plan-3
  hashing harness, and goes red until Plan 5 fixes the code-3
  collision. Run it before the hand-constructed tests below.
- **End-to-end production reachability test** (additional regression
  guard for the bug Plan 5 fixes — current main would fail this test
  as soon as the JSON round-trip is exercised on a Lua-shortcode-bearing
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
     shortcode, from: [Invocation -> ...] }` after Plan 6's
     post-walk has stamped it. (If running Plan 5 alone — before
     Plan 6 lands — the recovered shape is `Generated { by: filter,
     from: [] }` with `(filter_path, line)` in `by.data`; the
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

- **Streaming writer code path**: source-info-pool emission lives in
  two functions in `crates/pampa/src/writers/json.rs`:
  `SerializableSourceInfo::to_json` (used by the non-streaming
  `write_pandoc` at line 1657) and `stream_write_source_info_pool`
  (called from `stream_write_pandoc` at line 3530). Both consume the
  same `SerializableSourceMapping` enum but inline their own match
  arms. Compiler exhaustiveness catches missed arms after Phase 3's
  enum change — a deliberate safety property, and the reason Phases 3
  and 4 must land atomically. The named-but-unrelated pair
  `write_custom_block` / `stream_write_custom_block` handles
  `CustomNode` blocks, not the pool; don't confuse them.
- **Pool ID stability**: changing the format of pool entries shouldn't
  affect their IDs (which are sequential by intern order). Verify.
- **<a id="anchor-dedup-invariant"></a>Anchor dedup is a writer-side
  optimization, not a round-trip-stable property.** The writer's
  `arc_parent_ids` HashMap is keyed by `Arc::as_ptr`; multiple anchors
  pointing to the same `Arc<SourceInfo>` collapse to one pool entry.
  After deserialization, each anchor gets a freshly-allocated `Arc`
  carrying a `clone` of the pool target, so a subsequent re-serialize
  materializes N copies. **Pool-size is not stable over read-write-read;
  AST content and Plan-3 hashes are.** Plan-3's idempotence harness
  hashes `doc.ast.blocks` / `doc.ast.meta` via `compute_block_hash_fresh`
  / `compute_meta_hash_fresh_excluding_rendered`, both of which
  explicitly skip `source_info` (see
  `claude-notes/plans/2026-05-04-q2-preview-plan-3-builtin-filter-idempotence.md`
  §"Goal" — *"skips `source_info` and `key_source`"*). Same contract as
  today's `Substring.parent` reads. The reader-side `Arc::new(si)`
  pattern in the new code-4 arm matches the existing Substring arm at
  `readers/json.rs:196-200`, which also calls `Arc::new(pool.get(parent_id).cloned()?)`
  on every read — no sharing on the read side, by design.
- **Acyclic-by-construction assumption.** `SourceInfo` graphs are
  acyclic by construction — transforms build bottom-up, `Arc<SourceInfo>`
  is immutable post-construction. The writer's recursive interning
  relies on this invariant — same precondition as today's
  Substring/Concat arms. No cycle detection in the reader either.
- **Recursion depth.** Anchor interning adds a third recursion path on
  top of Substring chains and Concat pieces. Production depth is
  bounded by AST depth (shallow in practice); no separate guard.
  Adversarial input could blow the stack, but that's no different from
  the existing Substring-chain recursion — out of scope for Plan 5.
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
| Code 3 dual-shape legacy reader | ~35 |
| `AnchorRole` ↔ string serialization | ~20 |
| Streaming writer parity | ~40 |
| TypeScript type + utils updates | ~30 |
| Tests (incl. strict-rejection + stream helper + Concat-of-Generated) | ~290 |
| **Total** | **~565** |

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

**`r: [0, 0]` for Generated entries during the Plan-5↔Plan-7 window.**
After Plan 5 ships, all `Generated` pool entries carry `r: [0, 0]` —
the per-entry range field is no longer the right accessor for
Generated; use `resolve_byte_range` (via the Invocation anchor) for
chain-resolved ranges. Any diagnostic UI (q2-debug, hub-client devtools)
that reads `r` directly will see uninformative zeros for these entries.
This is a long-lived integration branch and the same developer is
implementing the provenance plans, so the surprise window is local;
the writer's `preimage_in` accessor reaches through Generated
correctly. No external consumers need warning.
