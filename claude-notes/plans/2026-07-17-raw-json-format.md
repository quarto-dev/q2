# Pampa-native "raw JSON" reader/writer (GH issue #11)

**Status:** Draft v2 — iterating with Carlos before implementation.
**GitHub issue:** https://github.com/quarto-dev/q2/issues/11
**Braid strand:** bd-en2hvrwn
**Related strands:** k-42 (ASTContext serialization — see "k-42 status" below),
k-gv05 (nondeterministic sourceInfoPool IDs).
**Related docs:** `claude-notes/designs/wire-format-source-info-codes.md`
(source-info pool wire codes — raw-json **shares** this wire format, see below).

## Overview

`pampa -t json` / `-f json` speak a Pandoc-compatible superset: real Pandoc
`t`/`c` tags plus q2 sidecars (`s` pool ids, `a` attr sources, `astContext`).
Because the node vocabulary is Pandoc's, the writer **cannot represent
pampa's AST extensions** — it desugars or hard-errors on them:

| Construct | Today's behavior |
|---|---|
| `Inline::Attr` (standalone attribute) | **Error Q-3-32** (the issue #11 repro) |
| `Inline::NoteReference` | Error Q-3-31, desugared to `Span.footnote-ref` |
| `Inline::Insert/Delete/Highlight/EditComment` (CriticMarkup) | Errors Q-3-33..36, desugared to `Span.critic-*` |
| `Inline::Shortcode` | Silently desugared to a `Span` |
| `Block::CaptionBlock` (orphaned) | Error Q-3-21, rendered as `Plain` |
| `ConfigValue` metadata | Partially lossy (see "Metadata fidelity" below) |
| `Block::BlockMetadata`, note definitions, `Custom` nodes | Representable, roundtrip today |

**Goal:** a new, pampa-specific format (working name: `raw-json`) whose
contract is:

> `write(ast) |> read` is the identity (structural equality) on the pampa
> AST — blocks, meta, and the roundtrip-relevant parts of ASTContext — for
> **every** AST the rest of pampa can produce, including all extensions
> above.

Secondary goals:

1. **Unmistakably not `pandoc -t json` output**, to humans and machines.
2. **Freedom to drift** from Pandoc's schema when the AST needs it.

Non-goals (v1): not a replacement for `-t json` (which stays the
Pandoc-superset / filter-facing format); not the filter wire format; no
TS/hub-client reader; no cross-version stability promise (the version field
is a fail-fast guard, not a compat contract); no WASM / quarto-core pipeline
changes.

## Design: extend the existing JSON machinery with a "raw" mode

**(Decision v2, 2026-07-17.)** Draft v1 recommended a serde-derived shape
(the AST types all derive `Serialize`/`Deserialize`). Carlos flagged the
fatal flaw: derived serde inlines every `SourceInfo` tree per node, and
**read-back cannot reconstruct the Arc-shared DAG** — sharing that q2's
source-location machinery depends on. A code study confirmed the existing
`-t json` machinery already solves exactly this, bidirectionally, and should
be reused:

### What the existing code already provides (reuse inventory)

- **Writer pool** — `SourceInfoSerializer` (`writers/json.rs:271-447`):
  interns `SourceInfo` into a flat topologically-ordered pool (`astContext.p`),
  dedups shared `Substring` parents / `Generated` anchors by `Arc::as_ptr`
  (~93% size reduction), `perf.intern` gauge under `QUARTO_PERF_STATS=1`.
- **Reader pool** — `SourceInfoDeserializer` (`readers/json.rs:102-480`):
  rebuilds each pool entry exactly once (forward-reference guard), children
  clone earlier entries — the clone shares the entry's *inner* Arcs, so the
  reconstructed graph preserves structural sharing with no blowup. (Each
  edge gets a fresh outer `Arc` wrapper; see "Roundtrip contract" for the
  consequence.)
- **ASTContext** — writer emits `astContext.files` (name, line-break table,
  total length) + `metaTopLevelKeySources`; reader `read_ast_context`
  (`readers/json.rs:1191-1253`) rebuilds `SourceContext` + `filenames`.
- **Node arms** — exhaustive `match` over every `Block`/`Inline` variant in
  writer and reader, including the q2-only representable ones
  (`BlockMetadata`, note definitions, `Custom` via wrapper nodes).

### The raw mode

Rather than forking the ~8600 lines of writer+reader (or building a parallel
serde format), **add a mode to the existing code path**:

- **Writer**: a `raw` flag on `JsonConfig` (or a sibling entry point
  `writers/raw_json.rs` delegating to shared internals). In raw mode:
  - emit the envelope marker first, omit `pandoc-api-version`;
  - the extension arms emit **native tags** (`t: "Attr"`, `t:
    "NoteReference"`, `t: "Insert"`, `t: "Delete"`, `t: "Highlight"`,
    `t: "EditComment"`, `t: "Shortcode"`, `t: "CaptionBlock"`) instead of
    desugaring or erroring — these arms already exist as the Q-3-3X
    defensive branches; raw mode is the *easy* branch of each;
  - metadata is written faithfully (below).
  - v1 implements raw mode on the **streaming** writer only (the production
    path); the `Value`-building `write_pandoc` twin exists for the HTML
    writer's source map and doesn't need raw mode.
- **Reader**: dispatch on the envelope marker. Raw mode accepts the native
  extension tags (new arms next to the existing ones, sharing all node
  helpers and the pool deserializer). The Pandoc-mode reader continues to
  reject them, keeping the two formats honest.
- **Sharing property**: because the raw arms live inside the same exhaustive
  `match`es, **adding a new AST variant fails compilation until its raw
  behavior is decided** — the roundtrip guarantee is enforced by the
  compiler at the arm level and by the roundtrip corpus at the semantic
  level. (This was the main advantage claimed for the serde option; the
  shared-arms design keeps most of it.)

Maintenance cost vs. v1's serde option: every new variant needs a writer arm
and reader arm — but it needs those for `-t json` anyway; the raw arm is the
trivial one. In exchange we keep pool interning, Arc sharing on read-back,
compact output, and one battle-tested code path for both formats.

### Metadata fidelity (raw mode)

Current `-t json` meta encoding (`write_config_value`, `writers/json.rs:1662-1716`)
is lossy in specific, now-catalogued ways; raw mode fixes each:

| Loss in `-t json` | raw mode |
|---|---|
| `Path`/`Glob`/`Expr` → `MetaInlines` (tag dropped) | tagged natively (`t: "Path"` etc.) |
| `merge_op` dropped | carried |
| Scalar `Integer`/`Real`/`Bool`/`Null` → `MetaString`/`MetaBool` (YAML type collapsed) | scalar type preserved |
| Top-level map keys **sorted** (insertion order lost) | entry order preserved (`ConfigValue` maps are `Vec`-backed) |
| `key_source` | already carried today; kept |

Reader side already reconstructs `ConfigValue` directly
(`read_config_value_top_level`), so raw arms slot in naturally.

## Self-identification (the "not pandoc JSON" marker)

Top-level envelope, marker emitted first:

```json
{
  "pampa-json-format": {"version": 1},
  "astContext": { "files": [...], "p": [...] },
  "meta": { ... },
  "blocks": [ ... ]
}
```

- The marker is a **top-level key, not a document-metadata entry** — it
  describes the file, not the document. First position makes it visible in
  truncated previews / first line of output.
- **`pandoc-api-version` is deliberately absent** — machine consumers of
  Pandoc JSON fail fast instead of half-parsing.
- The raw reader **requires** the marker: missing marker + present
  `pandoc-api-version` → targeted diagnostic ("this looks like Pandoc-style
  JSON; use `-f json`"); missing both → "not a pampa raw JSON document";
  wrong `version` → version-mismatch error. New Q-3-XX codes for each.
- Open bikeshed: also carry `producer: "pampa <semver>"` for debuggability
  (leaning yes — cheap).

## Format name

Working name **`raw-json`** (`-f raw-json`, `-t raw-json`), matching issue
#11. Alternative: `pampa-json`. Keep the name in one constant either way.

## Roundtrip contract, precisely

- `read(write(doc)) == doc` — structural equality (`PartialEq`) on
  (`Pandoc`, ASTContext projection), for any `doc` pampa can produce.
- **Byte-stability of `write ∘ read ∘ write` is NOT promised.** The reader
  rebuilds pool entries with fresh outer `Arc` wrappers per edge, so a
  re-serialize can intern a handful of extra pool entries / shift ids (the
  writer's dedup is by pointer identity, by design — see
  `provenance-contract.md` and k-gv05's pointer-reuse lesson). The contract
  is semantic identity, not canonical bytes. If canonical bytes ever matter
  (caching), that's a follow-up strand (content-hash interning).
- ASTContext roundtrip scope: `files` (names + line-break tables — **no
  embedded file contents**, matching today), and the source-info pool.
  Known non-roundtripped fields, documented as out of scope v1:
  `example_list_counter` (reader resets to 1 today — carry it in the raw
  envelope? cheap, leaning yes), `parent_source_info` (parse-time-only
  state, not meaningful post-parse).

## k-42 status (checked 2026-07-17)

k-42 asks "investigate how to serialize ASTContext properly in JSON
readers/writers" (motivated by the `filenames`/`source_context` duplication).
The serialization question is **already implemented and shipping** — both
directions, via `astContext.files` + `read_ast_context`. What remains live in
k-42 is only the duplication cleanup (store names once internally). This plan
adds a second consumer of that code but doesn't change the duplication
either way. Action: comment findings on k-42; keep it open for the cleanup;
`related` link to bd-en2hvrwn already in place.

## Phases

### Phase 0 — decisions (this document)

- [x] Wire shape: reuse existing pooled machinery via raw mode (v2 decision,
      supersedes v1's serde-derived Option A)
- [x] Format name: **`raw-json`** (matches issue #11)
- [x] Marker shape: **`"pampa-json-format": {"version": 1}`** — no
      `producer` field; the key name itself identifies the producer, and a
      version-bearing string would churn snapshots on every release
- [x] Meta fidelity table above; native tags: `Attr`, `NoteReference`,
      `Insert`, `Delete`, `Highlight`, `EditComment`, `Shortcode`,
      `CaptionBlock`; meta kind tags: `Path`, `Glob`, `Expr`
- [x] `example_list_counter` carried in the raw envelope's `astContext`
- [x] Raw-reader rejections: new **`JsonReadError` variants** (targeted
      messages), NOT new Q-3-XX codes — the entire JSON reader path reports
      plain `JsonReadError` without catalog codes today (`main.rs:314-317`),
      and a one-off catalog integration for three new errors would be
      inconsistent; reader-wide Q-code integration is a separate,
      pre-existing gap (file a low-priority strand). The error-corpus item
      previously listed under Phase 2 is dropped — that corpus is for merr
      parse errors, not reader errors.

### Phase 1 — tests first (TDD) — DONE 2026-07-17

- [x] `tests/integration/test_raw_json_roundtrip.rs`: AST-level identity
      roundtrip covering **every** extension: standalone `Attr`,
      `NoteReference`, all four CriticMarkup inlines, `Shortcode`,
      `CaptionBlock`, `BlockMetadata`, both note-definition blocks, block +
      inline `Custom` nodes; `ConfigValue` metadata with `Path`/`Glob`/
      `Expr`, `merge_op`, non-string scalars, map entry order; source-info
      preservation incl. shared `Substring` parents (assert sharing survives:
      structural equality of parent chains) and `Concat`/`Generated`
- [x] Issue #11's exact repro: `test_raw_json_roundtrip_issue_11_document`
      parses the exact document with the qmd reader and roundtrips the
      parser-produced AST. (Note learned en route: a *trailing* paragraph
      `Attr` is representable in Pandoc-superset JSON via the para-`attr`
      hoist, bd-aeyss6p5 — only a mid-paragraph Attr exercises Q-3-32.)
- [x] Marker tests: raw reader rejects Pandoc-style JSON with the targeted
      diagnostic; rejects wrong version; `-f json` rejects raw-json input
      (and vice versa)
- [x] Meta fidelity regression tests pinned to the table above
- [x] ~~Fixture dirs `tests/writers/raw-json/` + `tests/readers/raw-json/`~~
      Replaced by `test_raw_json_roundtrip_writer_fixture_corpus`: sweeps
      the existing `tests/writers/json/*.md` corpus through a raw-json
      identity roundtrip. Identity assertions subsume snapshots for this
      format, and reusing the existing corpus avoids a parallel fixture
      tree.
- [x] Ran tests before implementing; failed for the expected reasons
      (unresolved `writers::raw_json`, missing `UnexpectedRawJsonMarker`)

### Phase 2 — implementation — DONE 2026-07-17

- [x] Raw mode through `JsonConfig::raw` + streaming writer (marker-first
      envelope; native tags in the eight extension arms via
      `stream_write_span_like_raw` / `stream_write_shortcode_body`;
      faithful `stream_write_config_value` raw branches incl. `m` merge-op
      key and Q-3-57 for non-scalar YAML in `Scalar`)
- [x] Reader: marker validation (`read_raw_pandoc`) + raw arms guarded by
      `SourceInfoDeserializer.raw` + faithful meta read-back;
      pool/`read_ast_context` shared untouched (astContext gains
      `exampleListCounter`, read leniently in both modes)
- [x] `writers/raw_json.rs` / `readers/raw_json.rs` thin public entry
      points; registered in `readers/mod.rs`, `writers/mod.rs`, `main.rs`
      reader+writer arms, `options.rs` format tables
- [x] **Bonus fidelity fixes found by the corpus sweep** (shared reader,
      improves `-f json` too): `Link`/`Image` `target_source` (`targetS`)
      and `Citation.id_source` (`citationIdS`) were written but dropped
      on read-back; now restored (`read_target_source`,
      `read_opt_source_ref`) with a dedicated regression test
      (`test_raw_json_roundtrip_sidecar_source_infos`).
- [x] All 21 raw-json tests green; full workspace suite green
      (10099 passed)

### Phase 3 — end-to-end + docs

- [x] End-to-end through the real binary (output inspected):

      ```
      $ echo 'Hello. {#free-floating-attribute} Here?' | pampa -t raw-json
      {"pampa-json-format":{"version":1},"blocks":[{"c":[{"c":"Hello.","s":1,"t":"Str"},
      {"s":2,"t":"Space"},{"a":{...},"c":["free-floating-attribute",[],[]],"s":3,"t":"Attr"},
      {"s":5,"t":"Space"},{"c":"Here?","s":6,"t":"Str"}],"s":0,"t":"Para"}],
      "meta":{"c":[],"s":7,"t":"MetaMap"},"astContext":{"files":[...],
      "exampleListCounter":1,"p":[...]}}
      ```

      Piped back through `-f raw-json -t raw-json` twice: gen1 == gen2 ==
      gen3 **byte-identical** (stronger than the contract requires for
      this document), and `-f raw-json -t qmd` reproduces
      `Hello. {#free-floating-attribute} Here?` exactly. The old
      `-t json` path still errors with Q-3-32 on the same input
      (asserted in tests).
- [x] Full `cargo xtask verify` (WASM leg required — pampa is in the
      hub-client dependency chain). First run caught the classic
      out-of-workspace trap: `wasm-quarto-hub-client/src/lib.rs` had an
      exhaustive `JsonConfig` initializer (fixed with struct-update
      syntax). Second run: all legs green except the 20 "live"
      `hub-mcp.test.ts` tests, which require `wss://sync.automerge.org` —
      down on 2026-07-17 (unrelated to this change; they are connection
      timeouts in quarto-hub-mcp, which has no pampa dependency).
- [ ] Design doc `claude-notes/designs/raw-json-format.md`: contract,
      envelope, versioning policy, native tag vocabulary; cross-link from
      `wire-format-source-info-codes.md` (raw-json is a second consumer of
      the pool codes — same allocation policy applies)
- [ ] File low-priority strand: JSON reader errors lack Q-codes
      (pre-existing; noted in Phase 0 decisions)
- [x] Comment findings on k-42 (done at design time)
- [ ] Close the loop on GH issue #11 (after review/merge)

## Notes / references (2026-07-17 code study)

- Writer: `crates/pampa/src/writers/json.rs` (~5200 lines; streaming
  `stream_write_pandoc` :3926 is the production path; Q-3-32 emit :922-934;
  pool `SourceInfoSerializer` :271; meta `write_config_value` :1662).
- Reader: `crates/pampa/src/readers/json.rs` (strict `read` :1267 / lenient
  `read_completing_source_info` :1293; pool `SourceInfoDeserializer` :102;
  `read_ast_context` :1191; no `pandoc-api-version` check :1310; unknown
  `t` tags error, unknown top-level keys silently dropped).
- `config_json.rs` is a deliberately lossy projection for `q2 get-config` —
  not reusable here.
- Filters exchange the current JSON superset (`json_filter.rs:172-227`) —
  untouched by this plan.
- Dispatch sites: `options.rs:240-272`, `main.rs:245`/`:488`,
  `readers/mod.rs`, `writers/mod.rs`; WASM + quarto-core hardcode the
  current json module and are out of scope.
- Serde derives exist across the AST types but are test-only today; v1 of
  this plan (see git history of this file) explored building on them and was
  rejected for losing source-info sharing on read-back.
