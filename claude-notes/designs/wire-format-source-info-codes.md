# Source-info wire-format code allocation

**Status:** Active. Codes 0–3 in production; codes 4–5 reserved for q2-preview plan 5.
**Source of truth:** `crates/pampa/src/writers/json.rs:54-91` (writer types
`SourceInfoJson`, `AstContextJson`, `NodeJson`).
**Rust enum:** `quarto_source_map::SourceInfo` at `crates/quarto-source-map/src/source_info.rs`.
**Plans:** [q2-preview plan 2a](../plans/2026-05-04-q2-preview-plan-2a-iframe-foundation.md)
(first formal TS reader); [q2-preview plan 5](../plans/2026-05-04-q2-preview-plan-5-wire-format.md)
(`Synthetic` / `Derived` variants).

> **Second consumer:** the pampa `raw-json` format (bd-en2hvrwn, GH #11;
> see [raw-json-format.md](raw-json-format.md)) emits the same
> `astContext.p` pool through the same writer code. Code allocations
> here govern both formats.

## Purpose

The source-info pool is a JSON-encoded structure emitted by the Pandoc
JSON writer alongside every AST. Each AST node carries an integer
`s` field that indexes into the pool. Pool entries discriminate on a
small integer code in the `t` field. This document is the policy for
allocating those codes.

The reason it needs a written policy: the codes cross language
boundaries (Rust writer → TS iframe reader, with future cross-process
readers possible) and they are encoded as integers, so semantic drift
produces silent wrong behavior, not type errors.

## Allocation policy

1. **Codes are append-only.** Once a number has been emitted by a
   released writer, the variant shape at that number is frozen.
   New variants get new numbers. This is standard wire-format
   hygiene; it's the policy a reader can rely on without
   negotiating versions.

2. **Variant removal burns the number.** If a variant is dropped
   from the Rust enum, the wire number is documented as burnt in
   this file rather than recycled. A future variant that "feels
   like" the removed one still gets a fresh number.

3. **Forward-reservations are explicit.** A plan that reserves codes
   ahead of the writer (e.g., q2-preview plan 2a pre-declaring TS
   types for codes 4 and 5) records the reservation here.
   Pre-declared reader types are inert until the writer emits.

4. **Number-to-shape changes are synchronized.** Any change to the
   shape at an existing code requires writer + every reader (Rust
   reader, hub-client iframe TS reader, any future reader) updated
   in a single PR or coordinated series. This is the highest-coupling
   cross-language change in the stack.

5. **The writer types are the source of truth.** TS mirrors lag,
   never lead — same convention as `hub-client/src/types/diagnostic.ts`
   ↔ `DiagnosticMessage` and `hub-client/src/utils/pipelineKind.ts` ↔
   `PipelineKind`.

## Current allocations

| Code | Variant | `d` shape | Writer | Native reader | TS reader |
|------|---------|-----------|--------|---------------|-----------|
| 0 | `Original` | `file_id: number` | Emits | Reads | Reads (q2-preview plan 2a) |
| 1 | `Substring` | `parent_id: number` | Emits | Reads | Reads (q2-preview plan 2a) |
| 2 | `Concat` | `Array<[piece_id, offset_in_concat, length]>` | Emits | Reads | Reads (q2-preview plan 2a) |
| 3 | `FilterProvenance` | `[filter_path: string, line: number]` | Emits | Reads | Reads (q2-preview plan 2a) |
| 4 | `Synthetic` (reserved) | `By` (`{ kind: string; data?: unknown }`) | Reserved for q2-preview plan 5 | Reserved for q2-preview plan 5 | Forward-declared (q2-preview plan 2a) |
| 5 | `Derived` (reserved) | `{ from: number; by: By }` | Reserved for q2-preview plan 5 | Reserved for q2-preview plan 5 | Forward-declared (q2-preview plan 2a) |

The `r` field on every entry is `[start_offset, end_offset]` —
constant across all codes. Codes 4 and 5 have `r: [0, 0]` because
their variants are not anchored to a file range.

## Burnt numbers

| Code | Removed variant | Original shape | Removed in | Status |
|------|-----------------|----------------|------------|--------|
| (3) | `Transformed` | `[parent_id, ...]` | Pre-`Substring`-pool refactor | **Grandfathered exception.** Code 3 was recycled into `FilterProvenance`. This predates the policy and shipped without bite because no AST JSON is persisted anywhere. Counts as one-off, not precedent. |

There are no other burnt numbers as of this writing.

## Procedure for adding a new code

1. **Reserve the number** in this file (add a row to "Current
   allocations" with status "Reserved for `<plan>`"). Mention the
   intended `d` shape.
2. **Forward-declare the TS type** in `hub-client/src/types/sourceInfo.ts`
   so reader code can compile against the shape before the writer
   emits.
3. **Implement the writer** in `crates/pampa/src/writers/json.rs` —
   add the variant to `SerializableSourceMapping` and the match arm
   in `to_json`.
4. **Update the Rust enum** in `crates/quarto-source-map/src/source_info.rs`
   if the variant is new to the Rust side too.
5. **Update consumers** — native reader, TS accessors in
   `hub-client/src/utils/sourceInfo.ts`, and any other readers.
6. **Update this doc** — promote the row from "Reserved" to live,
   adjust the writer / reader columns, and link the plan PR.

## Synchronization invariant

For any released writer version V, every reader that processes
output from V must agree on the shape at every emitted code. Drift
between Rust and TS readers is a silent wrong-behavior bug, not a
type error. Reviewers of any PR that touches `json.rs`'s wire types
must verify the corresponding TS types in `sourceInfo.ts` move in
lockstep.
