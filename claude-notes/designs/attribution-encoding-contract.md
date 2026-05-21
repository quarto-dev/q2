# Attribution encoding contract

**Status:** Active (Phase 5 of the attribution pipeline, see
`claude-notes/plans/2026-05-06-attribution-pipeline.md`).
**Conversion site:** `buildAttributionPayload` in
`hub-client/src/hooks/useAttribution.ts`.
**Key types:** `AttributionRun` (TS) in
`hub-client/src/services/attribution-runs.ts`, `AttributionRun` (Rust)
in `crates/quarto-core/src/attribution/types.rs`.

## Summary

Attribution intentionally uses **two coordinate spaces** for run
boundaries, joined at exactly one site. The JS side speaks UTF-16
code units because that is what Automerge text patches, JS string
indexing, and Monaco editor offsets all natively produce. The Rust
side speaks UTF-8 byte offsets because that is what `&str` indexing
and `SourceInfo` ranges use. The conversion happens at the WASM
wire, immediately before the JSON payload is shipped to the Rust
pipeline.

This is not a bug to be "harmonized." Both sides are correct for
their own domain; collapsing to a single encoding would force one
side to fight its primitives on every offset.

## The two spaces

| Side | Space | Why |
|---|---|---|
| Automerge (Rust crate, compiled with `utf16-indexing`) | UTF-16 code units | Crate default; verified at `crates/quarto-hub/src/automerge_api_tests.rs::test_text_encoding_is_utf16` |
| Automerge (JS via `@automerge/automerge`) | UTF-16 code units | Native to JS strings; `patch.value.length` and `patch.length` on `diff()` output are UTF-16 |
| Monaco editor (presence cursors) | UTF-16 code units | Native to `model.getOffsetAt` / `model.getPositionAt`; passes through `A.getCursorPosition` unchanged |
| Run-list replay (`attribution-runs.ts`) | UTF-16 code units | Direct passthrough of Automerge patches |
| Rust attribution (`AttributionRun`, `query_byte_range`, `GitBlameProvider`) | UTF-8 byte offsets | Aligns with `&str` indexing and the rest of `SourceInfo` |

## Single conversion site

`buildAttributionPayload(state, sourceText, identities)` in
`hub-client/src/hooks/useAttribution.ts` calls
`buildCharToByteMap(text)` (iterates `text.charCodeAt(i)` with
explicit surrogate-pair handling, returning a `Uint32Array` of byte
offsets keyed by UTF-16 index), then `runsCharToByteOffsets(runs, map)`
to translate each run's `start`/`end` from char offsets to byte
offsets before `JSON.stringify`.

All Automerge-driven JS code upstream of `buildAttributionPayload`
stays in UTF-16. All Rust code downstream of the JSON wire stays in
UTF-8 bytes. The wire format itself carries bytes.

## Surrogate-pair handling

`buildCharToByteMap` walks UTF-16 code units, not code points. A
surrogate pair (e.g. an emoji or non-BMP CJK character) occupies two
consecutive UTF-16 indices, both of which receive a map entry: the
high-surrogate index maps to the byte offset *before* the 4-byte
UTF-8 sequence, the low-surrogate index to the offset *after*.
Automerge does not emit splice positions that land mid-surrogate, so
a run boundary on the low-surrogate index should not occur in
practice; if it does, the map keeps the translation well-defined.

Test coverage: `hub-client/src/services/attribution-runs.test.ts`
exercises ASCII (identity), 2-byte (Latin-1 supplement), 3-byte
(CJK), and 4-byte (surrogate-pair) cases.

## Failure modes if "harmonized"

The encoding split is not a stylistic preference on either side.
Each side's encoding is forced by the substrate immediately beneath
it; collapsing to a single encoding doesn't simplify the design, it
relocates the translation cost to a worse place.

**The encodings are inherited, not chosen.** On the Rust side, every
caller of `query_byte_range` already has `start` and `end` in byte
form because the layers below produce byte offsets natively:
tree-sitter returns node positions as bytes (`Range.start_byte` /
`end_byte`), `SourceInfo` carries those bytes through every AST
transform, `&str` indexing requires bytes, and `GitBlameProvider`
consumes line-based blame output which is byte-positional. On the
JS side, `string[i]`, `charCodeAt(i)`, `string.length`, Automerge
text splice positions, Monaco's `getOffsetAt`, and the DOM Selection
API all return UTF-16 code units natively. Attribution is the
*consumer* of coordinate systems chosen many layers below it on
both sides — there is no UTF-16 plane to ask tree-sitter for, and
no UTF-8 plane to ask Monaco for.

**Force UTF-8 on the JS side.** Every Automerge patch position,
every Monaco cursor, and every `string[i]` would need byte
translation per use. Cost moves from one conversion per payload
(debounced at ~500 ms) to one conversion per editor interaction —
many orders of magnitude more work, in the editor hot path rather
than the producer's cold debounced path.

**Force UTF-16 on the Rust side, option (a) — translate at every
query.** Every `query_byte_range` call becomes a
`byte_range → utf16_range → query` chain. For a document with
thousands of AST nodes, every render triggers thousands of
conversions. Translation cost moves from O(payloads) to
O(AST nodes × renders), into the rendering hot path.

**Force UTF-16 on the Rust side, option (b) — translate the
substrate.** Rewrite tree-sitter integration, `SourceInfo`, every
AST transform, citeproc, link rewriting, diagnostics, and
serialization to track UTF-16 code units rather than bytes. Massive
ripple for no win — `&str` still indexes by bytes, so a byte ↔
code-unit map would have to travel alongside every range anyway.

The current design picks the WASM wire as the conversion point
because it is the smallest, coldest, most auditable surface: one
function, debounced, off the rendering hot path.

## Soft floor on async desync

`runsCharToByteOffsets` uses `charToByte[r.start] ?? r.start` rather
than asserting in-bounds. This is deliberate: between the moment a
run list is computed (via `buildRunListAttribution` /
`updateRunListAttribution`) and the moment `buildAttributionPayload`
reads `sourceTextRef.current`, the document can receive a remote
Automerge change. A deletion-shaped race produces runs whose `end`
exceeds the new `sourceText.length`. The `??` falls back to the raw
UTF-16 offset for that one frame; the next debounced update heals
it. Converting to a hard assertion would null the payload on benign
races for no correctness gain.

## Related

- `claude-notes/plans/2026-05-06-attribution-pipeline.md` — original Phase 5 plan
- `crates/quarto-hub/src/automerge_api_tests.rs` — UTF-16 feature check + emoji splice tests
- `crates/quarto-core/src/attribution/types.rs::AttributionRun` — Rust-side byte-offset types
- `crates/quarto-lsp-core/src/types.rs::Position` — a separate UTF-16 surface (LSP spec); not connected to attribution
