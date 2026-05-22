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
    // Generated { by, from }
    let obj = data.as_object().ok_or(MalformedSourceInfoPool)?;
    let by_obj = obj.get("by").and_then(|v| v.as_object())
        .ok_or(MalformedSourceInfoPool)?;
    let kind = by_obj.get("kind").and_then(|v| v.as_str())
        .ok_or(MalformedSourceInfoPool)?.to_string();
    let by_data = by_obj.get("data").cloned().unwrap_or(Value::Null);
    let by = By { kind, data: by_data };

    let mut from = SmallVec::<[Anchor; 2]>::new();
    if let Some(from_arr) = obj.get("from").and_then(|v| v.as_array()) {
        for entry in from_arr {
            let role_str = entry.get("role").and_then(|v| v.as_str())
                .ok_or(MalformedSourceInfoPool)?;
            let role = parse_anchor_role(role_str)?;
            let si_id = entry.get("si_id").and_then(|v| v.as_u64())
                .ok_or(MalformedSourceInfoPool)? as usize;
            if si_id >= current_index {
                return Err(CircularSourceInfoReference(si_id));
            }
            let si = pool.get(si_id).cloned()
                .ok_or(InvalidSourceInfoRef(si_id))?;
            from.push(Anchor { role, source_info: Arc::new(si) });
        }
    }

    SourceInfo::Generated { by, from }
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
- The file's header doc-comment (lines 10–19 of the current file)
  references `Synthetic` and `Derived` by name and says "Plan 5 wires
  this up." Rewrite it to describe Generated instead and drop the
  Synthetic/Derived nomenclature.

The TS type and the Rust serializer must agree byte-for-byte; the
header doc-comment cites the Rust file as the source of truth, same
convention as for the atomic-CustomNodes registry.

## Work items

Phase-ordered. Each phase compiles cleanly and runs its own slice of
the test suite before the next begins. Phase 1 lands on its own as the
bd-3odjm fix even if the rest of Plan 5 stalls.

### Phase 0 — Start gate

- [ ] Confirm Plan 4 (Generated + By + Anchor + AnchorRole) has merged
      into `feature/provenance`. If not, stop — Plan 5 cannot build.
      Verify with `git grep -n "enum SourceInfo" crates/quarto-source-map/src/source_info.rs`
      and confirm a `Generated` arm exists.

### Phase 1 — Legacy code-3 dual-shape reader (closes bd-3odjm)

- [ ] Add `parse_anchor_role` helper in `crates/pampa/src/readers/json.rs`
      (used by Phase 4 too — landing it here is a no-op until then).
- [ ] Rewrite the code-3 arm in `SourceInfoDeserializer::new` (currently
      `crates/pampa/src/readers/json.rs:252-283`) to dispatch on `data[0]`:
      numeric → legacy Substring (today's behavior); string → `Generated
      { by: By::filter(path, line), from: smallvec![] }`; neither → `MalformedSourceInfoPool`.
- [ ] Run `cargo nextest run -p quarto-core --test idempotence lua_shortcode_lipsum_fixed`
      → green (closes bd-3odjm).
- [ ] Run the full Plan-3 idempotence suite → 27/27 green.

### Phase 2 — Writer code-4 emit (`SerializableSourceMapping` + intern + `to_json`)

Starting state from Plan 4: `SourceInfo::FilterProvenance` is gone, but
`SerializableSourceMapping::FilterProvenance` survives because Plan 4's
interim writer arm (see Plan 4 §"Migrations", `pampa/src/writers/json.rs:314`)
converts `SourceInfo::Generated { by: filter, .. }` into the legacy
shape via `by.as_filter().expect(...)`. That arm panics for non-filter
Generated kinds, so the workspace only stays buildable as long as no
non-filter Generated is constructed — Plan 6 doesn't ship shortcode
stamping until later, so Plan 4's expect is safe in the interim.
Phase 2 removes both the interim arm and the `SerializableSourceMapping::FilterProvenance`
variant at once.

- [ ] Add `Generated { by: By, from: Vec<(AnchorRole, usize)> }` to
      `SerializableSourceMapping` in `crates/pampa/src/writers/json.rs`.
- [ ] Replace Plan 4's interim `SourceInfo::Generated { by, .. } => …
      SerializableSourceMapping::FilterProvenance` arm with a real
      `SerializableSourceMapping::Generated { … }` construction (no more
      `by.as_filter().expect(...)`); supports all `by.kind` values
      uniformly.
- [ ] Remove `SerializableSourceMapping::FilterProvenance` (no longer
      reachable after the interim arm above is rewritten).
- [ ] Update `SourceInfoSerializer::intern` (`writers/json.rs:260-333`):
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
      - Build the `(start_offset, end_offset, mapping)` return tuple as
        `(0, 0, SerializableSourceMapping::Generated { by, from: from_ids })`
        — the `r: [0, 0]` rule is enforced at this tuple, just like
        today's FilterProvenance arm at lines 314-322.
- [ ] Update `SerializableSourceInfo::to_json` (`writers/json.rs:169-190`)
      with the code-4 arm per §"Code 4 — Reader / writer".
- [ ] Add `serialize_anchor_role` helper.

### Phase 3 — Streaming writer parity

- [ ] Add the code-4 arm in `stream_write_source_info_pool`
      (`writers/json.rs:3472-3522`); mirror the `to_json` shape exactly.
- [ ] Remove the FilterProvenance arms (lines 3499-3504 emit, line 3516
      tag). They become unreachable once `SerializableSourceMapping::FilterProvenance`
      is gone from Phase 2.

### Phase 4 — Code-4 reader

- [ ] Add a `4 => { … }` arm in `SourceInfoDeserializer::new`
      (`readers/json.rs:154-287`) per §"Code 4 — Reader / writer":
      decode `by` (kind + optional data), decode `from` array entries
      via `parse_anchor_role` + `si_id`, with the `si_id < current_index`
      circular-ref guard.
- [ ] Reject malformed code-4 payloads with `MalformedSourceInfoPool`:
      missing `by`; missing `by.kind`; `from` entry missing `role`;
      `from` entry missing `si_id`; unrecognized role string. (See
      §"Open questions for implementation" — settled here.)
- [ ] Prefer `s.strip_prefix("other:").map(...)` over `s[6..]` in
      `parse_anchor_role`. Cosmetic — both are UTF-8-safe here because
      `"other:"` is 6 ASCII bytes, but `strip_prefix` is clearer and
      avoids the magic number.

### Phase 5 — TypeScript types

- [ ] Rewrite `ts-packages/preview-renderer/src/types/sourceInfo.ts`
      per §"TypeScript wire-format definitions":
      - Add `AnchorRef` interface.
      - Code 4's `d` becomes `{ by: By; from?: AnchorRef[] }`.
      - Code 3's `d` becomes `[string, number] | [number, ...number[]]`.
      - Remove the code-5 entry.
      - Rewrite the header doc-comment to describe Generated, not
        Synthetic/Derived.
- [ ] Audit `ts-packages/preview-renderer/src/utils/sourceInfo.ts`
      (`isAtomicSourceInfo` and helpers) for code-paths that match on
      the old code-4 = bare-By shape; adjust per the new shape.

### Phase 6 — Tests

- [ ] Round-trip property test for every `SourceInfo` variant (Original,
      Substring, Concat, Generated with various By kinds and `from`
      configurations). See §Test plan.
- [ ] Filter-provenance recovery test (hand-constructed code-3 with
      string-array payload → `Generated { by: filter, from: smallvec![] }`).
- [ ] Legacy Transformed back-compat test (hand-constructed code-3 with
      numeric-array payload → `Substring`).
- [ ] Forward-compat test (code-4 with unknown `by.kind`, arbitrary
      `data` → preserved round-trip).
- [ ] Anchor dedup test (multi-inline shortcode → one `Original` pool
      entry, N Generated entries each `from[0].si_id` referencing it).
      **Hand-construct the AST with `Arc::clone(&shared)` directly** —
      do not drive through the shortcode resolver, which doesn't ship
      until Plan 6. The test exercises the serializer's `arc_parent_ids`
      cache against the same Arc-sharing contract Plan 6 will later
      satisfy in production. Test passes Plan-5-alone.
- [ ] Ensure at least one Phase 6 test routes through
      `stream_write_pandoc` (not just the non-streaming `write_pandoc`)
      with Generated entries in the pool. The streaming writer's match
      arms are independent of `to_json`'s; without explicit coverage,
      a Phase-3 regression in `stream_write_source_info_pool` could
      slip through.
- [ ] AnchorRole round-trip test (Invocation / ValueSource / Other).
- [ ] End-to-end production reachability test (kbd-shortcode fixture →
      `render_qmd_to_preview_ast` → JSON → `pampa::readers::json::read`
      → assert success and recovered shape).
- [ ] TypeScript-side type round-trip (parse a JSON pool with Generated
      entries; confirm `SourceInfoEntry` shape matches).

### Phase 7 — Verification gate

- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace` all green (bd-3odjm closed in
      Phase 1; no other regressions).
- [ ] `cargo xtask verify` (full — `quarto-core`/`pampa` are WASM
      consumers; hub-build leg matters).
- [ ] `git grep "FilterProvenance"` returns only legacy-reader / legacy
      doc references (no writer emissions, no `SerializableSourceMapping`
      variant).
- [ ] Update bd-3odjm: close as duplicate of Plan 5 PR. Refresh its
      description to use `from:` not `anchors:` if reopened for any
      reason. **If Phase 2 or 3 introduces a *new* failure mode in the
      lipsum fixture, file a fresh beads issue** rather than reopening
      bd-3odjm — that issue is specifically the code-3 collision and
      should stay scoped to it.

## Open questions for implementation

- **Eventually retiring code 3**: at some point, no JSON files in the
  wild contain code 3 (the buggy FilterProvenance shape never
  round-tripped before Plan 5; the legacy Transformed shape predates a
  transition we made earlier). Could remove the legacy reader. Don't
  need to decide now.
- **Detecting malformed code 4 payloads**: settled in Phase 4 of
  §"Work items" — `MalformedSourceInfoPool` for missing `by`, missing
  `by.kind`, `from` entry missing `role`/`si_id`, or unrecognized role
  string.
- **Streaming writer parity** (`stream_write_source_info_pool`): settled
  in Phase 3 of §"Work items". Both the non-streaming `to_json` path
  and the streaming writer iterate `SerializableSourceMapping`, so the
  Phase 2 enum change drives both — Phase 3 only needs the inline
  match-arm update in the streaming writer.
- **Pool deduplication of anchor `si_id` references**: when many
  Generated entries share the same anchor target (multi-inline
  shortcode), the writer interns once and reuses the ID. The existing
  `arc_parent_ids` HashMap pattern (already used for `Substring.parent`)
  handles this — same interning mechanism, different reader-side name
  (`si_id` for anchors, `parent_id` for substrings).
- **TypeScript hand-mirror updates**: see §"TypeScript wire-format
  definitions" above. Settled — code 4's `d` becomes `{ by; from? }`,
  code 5 is removed, code 3's `d` becomes a union for the dual-shape
  legacy reader.

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
  enum (Original/Substring/Concat/FilterProvenance arms). Phase 2 adds
  a `Generated` arm and removes `FilterProvenance`.
- `crates/pampa/src/writers/json.rs:260-333` — `SourceInfoSerializer::intern`;
  Phase 2 adds a `SourceInfo::Generated` arm with topologically-ordered
  anchor recursion.
- `crates/pampa/src/writers/json.rs:3472-3522` — `stream_write_source_info_pool`;
  Phase 3 mirrors the to_json changes here (lines 3499-3504 emit, line
  3516 tag).
- `crates/pampa/src/readers/json.rs:99-293` — `SourceInfoDeserializer::new`
  (the pool reader). Code-3 arm at lines 252-283 (Phase 1 rewrites);
  Phase 4 adds a code-4 arm.
- `crates/quarto-source-map/src/source_info.rs:21-55` — `SourceInfo` enum
  (extended by Plan 4 — confirm Generated/By/Anchor/AnchorRole present
  before Plan 5 starts; see Phase 0).
- `ts-packages/preview-renderer/src/types/sourceInfo.ts` — JS-side
  `SourceInfoEntry`. See §"TypeScript wire-format definitions" for the
  full before/after.
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
  Assert the reader produces `Generated { by: filter, from: smallvec![] }`
  with the right path/line via `by.as_filter()`.
- **Legacy Transformed back-compat test**: hand-construct a JSON pool
  entry with code-3-with-numeric-array-payload (the legacy Transformed
  shape). Assert the reader still produces a `Substring` (preserving
  today's back-compat behavior).
- **Forward-compat test**: hand-construct a JSON pool entry with code 4
  and an unknown kind (`"kind": "ext/future/foo"`, arbitrary data).
  Assert it decodes as `Generated { by: By { kind: "ext/future/foo",
  data: ... }, from: smallvec![] }`. Round-trips unchanged.
- **Anchor dedup test**: build an AST where multiple inlines have
  Generated source_info each carrying an `Invocation` anchor that
  references the same `Original` (multi-inline shortcode resolution).
  Serialize. Confirm the pool contains the `Original` exactly once and
  each Generated entry's `from[0].si_id` references it by ID.
- **AnchorRole round-trip test**: round-trip a Generated with each role
  (Invocation, ValueSource, Other(String)) through JSON; assert the
  role survives.
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
  arms. Compiler exhaustiveness catches missed arms after Phase 2's
  enum change — a deliberate safety property. The named-but-unrelated
  pair `write_custom_block` / `stream_write_custom_block` handles
  `CustomNode` blocks, not the pool; don't confuse them.
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

**`r: [0, 0]` for Generated entries during the Plan-5↔Plan-7 window.**
After Plan 5 ships, all `Generated` pool entries carry `r: [0, 0]` —
the per-entry range field is no longer the right accessor for
Generated; use `resolve_byte_range` (via the Invocation anchor) for
chain-resolved ranges. Any diagnostic UI (q2-debug, hub-client devtools)
that reads `r` directly will see uninformative zeros for these entries.
This is a long-lived integration branch and the same developer is
implementing all of Plans 5–7, so the surprise window is local; once
Plan 7's `preimage_in` lands, the standard accessor pattern reaches
through Generated correctly. No external consumers need warning.
