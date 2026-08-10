# Named HTML entity references silently dropped in prose (bd-named-entities-w6xbfftj)

**Date:** 2026-08-10
**Braid:** bd-named-entities-w6xbfftj (bug, P1, labels `pampa`, `parity`)
**Checkout:** main @ `0cb8abce` (investigated in place; no worktree created)
**Status:** Implemented 2026-08-10; all phases complete, full verify green. Discovered follow-ups: bd-v8qc9zyc (grammar regex), bd-96fswwce (combining-mark parse failure).

## Triage verdict

**Ready to design.** The strand's root-cause analysis is accurate at HEAD, the fix
surface is small and well-localized (one new match arm in pampa + one exported
constant in tree-sitter-qmd's Rust bindings), and the open questions are about
mechanism choices, not missing context.

## Issue context

Filed 2026-08-10 by Carlos (same day as this investigation — no staleness risk).
Named entity references in prose (`&gt;`, `&lt;`, `&amp;`, `&quot;`, `&nbsp;`,
`&copy;`, …) are dropped from output with no diagnostic; numeric references
(`&#62;`, `&#x40;`) work. Q1/Pandoc/CommonMark resolve named references to their
characters. Real-world hit: Posit Connect docs use named entities in 13 files —
UI breadcrumbs like `**APIs & Services** &gt; **Credentials**` lose their `>`
separators. Origin strand in the connect-docs porting skein:
br-named-entities-zu8jp8pw.

## Dependency graph

**Empty** — no edges in this skein (`braid dep tree` / `dep list` show only the
strand itself). The filing context lives in the connect-docs skein
(br-named-entities-zu8jp8pw), quoted in the description. No incoming `blocks`
pressure; the P1 comes from the docs-porting impact.

## What the code looks like today

All paths in the description check out at `0cb8abce`:

- **Grammar side (produces the node):**
  `crates/tree-sitter-qmd/common/common.js:41` defines
  `entity_reference: $ => html_entity_regex()`; the regex builder
  (`common.js:173-181`) is generated from `common/html_entities.json` — the
  WHATWG entities table (2,231 keys, `name → {codepoints, characters}`,
  includes multi-codepoint entities like `&NotEqualTilde;` → `"≂̸"`).
  `entity_reference` appears in prose, table cells, link labels/destinations, etc.
- **Converter side (drops the node):**
  `crates/pampa/src/pandoc/treesitter.rs:845` has a match arm only for
  `"numeric_character_reference"` (→ `process_numeric_character_reference` in
  `treesitter_utils/numeric_character_reference.rs`). `"entity_reference"` has
  **no arm** and falls to the default at `treesitter.rs:1695`, which writes an
  "Unhandled node kind" warning to the *verbose* buffer only and returns
  `IntermediateUnknown` — which is dropped. Hence "silent."
- **Sharing precedent:** `crates/tree-sitter-qmd/bindings/rust/lib.rs` already
  exports `include_str!` constants (`HIGHLIGHT_QUERY`, `INJECTION_QUERY`,
  `NODE_TYPES`), and `pampa` already depends on `tree-sitter-qmd`. Exposing
  `HTML_ENTITIES_JSON` the same way gives both sides one source of truth.

Repro captured at
`claude-notes/plans/named-entity-references-dropped-investigation/repro.qmd`
(copied from the strand's external repro dir; Q1-verified expected output in the
strand description). Reproduction at HEAD: see `investigation-notes.md` in the
same directory.

### Discovered while investigating: grammar regex mangles legacy entity names

`html_entity_regex()` builds alternatives with
`name.substring(1, name.length - 1)` — correct for the 2,125 semicolon-
terminated keys (strips `&` and `;`), but the WHATWG table also carries 106
legacy no-semicolon keys (`&AMP`, `&AElig`, …) whose **last letter** gets
stripped instead, producing bogus alternatives (`AM`, `AEli`, …). Net effect:
`&AM;` parses as `entity_reference` (then would miss any name→char lookup),
while the actual legacy forms (`&AMP` bare) don't match — the latter is
*correct* per CommonMark (only semicolon-terminated entities are recognized in
markdown), the former is a real but minor grammar bug. Filed as
**bd-v8qc9zyc** (discovered-from this strand); the converter must handle
lookup misses gracefully regardless.

## Work items

### Phase 0 — Tests first (TDD; run and verify each fails before implementing)

- [x] Coverage tests in
      `crates/pampa/tests/integration/test_treesitter_coverage.rs` (new
      "Entity reference tests" section): `&gt;` → `Str ">"`; `&nbsp;` →
      `Str "\u{00A0}"` (not a plain space); `&copy;` → `©`; multi-codepoint
      `&NotEqualTilde;` → `"≂̸"`; lookup-miss `&AM;` → literal `"&AM;"`;
      `&quot;` → straight `"` (no smart typography); entity inside emphasis.
- [x] Native snapshot fixture
      `crates/pampa/tests/snapshots/native/entity-references.qmd`
      (insta snapshot lands in `crates/pampa/snapshots/native/`).
- [x] Roundtrip fixture
      `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/named_entities.qmd`
      (driver is a fixpoint check — JSON-after-reparse equality, so `&gt;`
      re-emitting as `\>` is fine).
- [x] Smoke-all fixture under `crates/quarto/tests/smoke-all/` asserting the
      rendered HTML contains the decoded characters (end-to-end through the
      real render path).
- [x] Run the new tests; confirm each fails for the expected reason
      (entities dropped). Verified 2026-08-10: all 7 coverage tests fail
      (e.g. got `"A  B"`, want `"A > B"`; `&AM;` confirmed parsing as
      entity_reference and dropped); native snapshot `.snap.new` showed the
      dropped-entity AST (deleted, to be accepted post-fix); smoke-all fails
      all four named-entity patterns and trips the illegal pattern
      `Named: A  B`; roundtrip consistency also fails pre-fix (doubled
      Spaces are not reparse-stable). Tests committed together with the fix
      so main never carries a red suite.

### Phase 1 — Expose the table

- [x] `pub const HTML_ENTITIES_JSON: &str =
      include_str!("../../common/html_entities.json")` in
      `crates/tree-sitter-qmd/bindings/rust/lib.rs` (mirrors `NODE_TYPES`).

### Phase 2 — Converter arm

- [x] New `crates/pampa/src/pandoc/treesitter_utils/entity_reference.rs`:
      lazy `OnceLock<HashMap<String, String>>` parsed from
      `HTML_ENTITIES_JSON` (key → `characters`); `process_entity_reference`
      mirroring the numeric handler; miss → emit original text verbatim,
      no warning.
- [x] Match arm `"entity_reference"` in
      `crates/pampa/src/pandoc/treesitter.rs`.
- [x] New tests pass; snapshot accepted after review
      (`crates/pampa/snapshots/native/entity-references.snap`, new file —
      shows decoded entities, straight quotes, `&AM;` verbatim).
- [x] **Discovered: qmd writer must escape `"`.** The roundtrip fixture
      exposed that a straight `"` in a `Str` (reachable via `&quot;` /
      `&#34;` / programmatic ASTs) was written bare and re-read as
      `Quoted DoubleQuote`. Fixed in `escape_markdown`
      (`crates/pampa/src/writers/qmd.rs`): `"` → `\"`, which re-reads as
      the literal straight quote. Fixture extended with a numeric `&#34;`
      line to pin the pre-existing exposure.
- [x] **Discovered: parser rejects decomposed Unicode (combining marks) in
      prose** — filed as **bd-96fswwce**. `x ≂̸ y` (U+2242 U+0338 — exactly
      what `&NotEqualTilde;` decodes to) and `e` + U+0301 both fail to
      parse, while precomposed `é` is fine. Entity decoding to the AST works
      (unit test + native snapshot cover it), but written-out qmd containing
      the combining sequence is un-reparseable, so the roundtrip fixture
      deliberately omits multi-codepoint entities; bd-96fswwce's fix should
      re-add `&NotEqualTilde;` there.

### Phase 3 — Verification

- [x] `cargo nextest run --workspace`: 11231 tests run, 11231 passed
      (2 leaky), 197 skipped — no regressions.
- [x] End-to-end: `cargo run --bin q2 -- render
      claude-notes/plans/named-entity-references-dropped-investigation/repro.qmd`,
      output inspected. Rendered HTML now reads
      `Named: A &gt; B &lt; C &amp; D &quot;E&quot; F<NBSP>G © H.` — with
      the NBSP verified byte-level (`46 c2 a0 47`) and © present; the
      numeric-reference line unchanged. (Render artifacts deleted after
      inspection; the fixture stays.)
- [x] Full `cargo xtask verify` (pampa flows into the WASM leg): all steps passed.

### Phase 4 — Bookkeeping

- [x] Update this plan; commit (document snapshot changes per CLAUDE.md).
- [x] Close bd-named-entities-w6xbfftj (grammar-regex cleanup stays in
      bd-v8qc9zyc).

## Design decisions (settled with user, 2026-08-10)

1. **Table sharing mechanism:** expose `HTML_ENTITIES_JSON` from
   `tree-sitter-qmd`'s Rust bindings via `include_str!` (same pattern as
   `NODE_TYPES`; single source of truth).
2. **Parse strategy:** lazy `OnceLock<HashMap>` + serde_json at first use.
3. **Lookup-miss behavior:** emit the original text verbatim, **no warning**.
   Rationale: the `entity_reference` regex is generated from the same table
   the converter looks up in, so every node the grammar produces is a known
   name — the only reachable misses are the ~106 bogus truncated alternatives
   from the bd-v8qc9zyc regex bug (`&AM;`, `&AEli;`), which essentially never
   occur in real prose and become unreachable once that strand lands.
   Genuinely unknown references (`&foo;`) never match the regex and are
   already literal text, matching CommonMark/Pandoc. (If we ever want to
   catch typo'd entity names, that's a separate prose lint, not this arm.)
4. **Grammar regex cleanup:** stays in the separate strand bd-v8qc9zyc;
   this strand lands the converter fix alone.
5. **Smart typography:** decoded entities bypass `apply_smart_typography`
   (`&quot;` stays a straight quote), matching the numeric handler and Pandoc.

## Risks / tradeoffs

- **qmd-writer re-escaping — verified NOT an issue.** The qmd writer already
  backslash-escapes markdown-significant characters in `Str` content:
  `&#62; not a blockquote` round-trips as `\> not a blockquote`, and mid-line
  `A &#62; B` as `A \> B` (verified at `0cb8abce` via
  `pampa -t qmd`). The roundtrip test pins this behavior for named entities.
- **Snapshot churn.** Any existing snapshots containing dropped entities will
  change (correctly). Per CLAUDE.md, count and summarize them in the commit.
- **WASM leg.** pampa changes flow into `wasm-quarto-hub-client`; full
  `cargo xtask verify` (not just `--skip-hub-build`) before push.
