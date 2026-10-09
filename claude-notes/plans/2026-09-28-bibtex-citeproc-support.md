---
title: 'pampa citeproc loads CSL-JSON bibliographies only — no BibTeX (.bib) support (bd-l6eh1635)'
date: 2026-09-28
description: 'Adds BibTeX `.bib` reading to pampa''s citeproc bibliography loader, translating entries into the same canonical references that CSL-JSON produces, so the citeproc filter and book merges accept `.bib` files.'
status: done  # All four phases landed 2026-09-28 (see per-phase results below). Workspace green throughout; ready for review/push.
braid:
  strand: bd-l6eh1635
  priority: P2
---

**Worktree:** `.worktrees/workspace-6` (branch `braid/bd-l6eh1635-bibtex-citeproc`, based on `main` @ `e8379cfe1fa68a99c29b45093b87f35d02bb5d0c`)

## Triage verdict

**Ready to implement.** This plan is the product of a full research/design session (not a
skeleton with open questions) — every design question raised during that session is
resolved below. TDD each phase per the workspace's standard gates (clippy `-D warnings` +
`cargo nextest run -p <crate>` per phase; `cargo nextest run --workspace` at phase
boundaries and before any push, delta reported against the current live baseline).

## Issue context

> pampa's `load_bibliography` (`crates/pampa/src/citeproc_filter.rs`) parses bibliography
> files exclusively as CSL-JSON via `serde_json::from_str` into `Vec<Reference>`. A `.bib`
> file — the format Quarto 1 users overwhelmingly declare — fails with
> `BibliographyParseError`. Found while writing book-projects P2 item 82/83 tests, which
> had to use CSL-JSON fixtures (`REFS_JSON`) even though real books say
> `bibliography: refs.bib`. Note the Typst output path is unaffected (pandoc's Typst
> writer emits native citations resolved by hayagriva, which reads BibTeX); this gap only
> bites the `filters: [citeproc]` / book-single-file-merge path where pampa's own citeproc
> runs.

Status: open, priority 2, filed 2026-09-24 by Gordon Woodhull.

## Dependency graph

- **discovered-from: bd-oqoozmtr** (closed) — "bibliography/csl paths resolve against
  process CWD, not doc or project dir." That fix (book-projects P2, 2026-09-24) landed
  the declaration-site base-dir threading that `load_bibliography`/`load_csl_style` now
  use; while writing its own regression tests (`quarto-core/tests/integration/book_citations.rs`),
  this BibTeX gap was found and filed separately rather than folded in.
- No incoming `blocks` edges — nothing is waiting on this strand; it isn't gating any
  other open work.

## Architecture recap (context for whoever picks this up)

Two separate concerns sit in the citation pipeline:

- **Rendering** — `quarto-citeproc` (implements the CSL algorithm) + `quarto-csl` (parses
  `.csl` style files). Ported natively to Rust specifically so q2 doesn't need to shell
  out to Pandoc's Haskell citeproc for rendering. Operates entirely on
  `quarto_citeproc::Reference`, an in-memory struct modeled on the CSL-JSON schema. Does
  not care what file format the data originally came from.
- **Ingestion** — `load_bibliography` in `crates/pampa/src/citeproc_filter.rs` is the
  single choke point, used identically by ordinary single-document citeproc and by book
  multi-file merge (`ChapterCitationManifest`). Today it has exactly one front-end: a
  literal CSL-JSON array via `serde_json`. This plan adds a second front-end for BibTeX/
  BibLaTeX's very different serialization (entry types like `@article`, field names like
  `journaltitle`, name lists, `@string` macros, `crossref` inheritance), translating into
  the same canonical `Reference` objects so the rest of the pipeline never knows which
  source format was used.

**Why this gap exists at all:** In Quarto 1, BibTeX ingestion was never Quarto's own code.
Q1 shells out to the `pandoc` binary, which bundles its own mature Haskell BibTeX reader;
Q1's own `bibliography-formats.lua` Lua filter (confirmed by reading its actual source)
only reshapes `bibliography:` metadata around `pandoc.utils.references(doc)` — it never
parses files itself. Porting citeproc's *rendering* engine to Rust (the right call, to
avoid a Pandoc subprocess) didn't bring along Pandoc's *reading* side, since that lived in
the same external binary. Nobody has separately reimplemented it — CSL-JSON's own reader
is trivial (`serde_json`), so nothing forced the BibTeX case until real book projects
(which almost universally declare `.bib`) hit it.

The Typst output path already has BibTeX support today, via a *different* rendering
engine entirely: Typst's own writer emits native citations resolved at compile time by
`hayagriva` (Typst's own CSL-equivalent engine), which depends on the `biblatex` crate to
read `.bib` files. That's why `biblatex` is already present in this workspace's dependency
tree today, transitively — this plan promotes it from transitive to direct, in a
different, non-Typst code path.

`quarto-citeproc`\'s renderer already implements CSL's `nocase`-span text-case-protection
machinery (confirmed in `crates/quarto-citeproc/src/output.rs`) and already parses
embedded `<span class="nocase">` markup inside CSL-JSON string fields (the standard
CSL-JSON embedded-richtext convention). This matters for the title case-folding fix below:
our new ingestion mapper's job is to *produce* input in that already-supported canonical
form, not to invent new rendering behavior.

## Chosen design

### Dependency: direct `biblatex = "0.12"` (latest; API verified; `hayagriva` rejected)

Updated 2026-09-28 after confirming 0.12.0 is the latest crates.io release. Its API is
compatible with the spike: `Bibliography::parse`, `Entry::get`, generated `Entry::author`,
`Chunk::{Normal, Verbatim, Math}`, `ChunksExt::format_sentence`/`format_verbatim`, and the
`EntryType` variants used below all remain available. `cargo check -p pampa --lib` passed
against the resolved 0.12.0 dependency.

The spike's 0.11 API claim was not the only issue found during implementation. CSL
`sentence_case` lowercases the *entire* string (see `quarto-citeproc/src/output.rs`), so
wrapping only verbatim chunks would still incorrectly lowercase unbraced ALL-CAPS and
internally mixed-case words. Phase 1 now translates eligible normal text by word, preserving
non-sentence-case words, and emits `nocase` markup around braced chunks; this is covered by
regression tests below. The specialized BibTeX casing mapper is applied to title-shaped
fields (`title`, `shorttitle`, `journaltitle`/`journal`, `booktitle`, `shortjournal`, and
`series`). `publisher`, publisher-place, and other fields where capitalization is
semantically significant retain their source spelling rather than being sentence-cased.

Investigated reusing `hayagriva::io::from_biblatex_str`/`from_biblatex()` (already
transitively present via Typst). Rejected: it produces `hayagriva::Entry`/`Library` —
hayagriva's own internal schema, not CSL-JSON — so using it would still require
hand-writing a `hayagriva::Entry → quarto_citeproc::Reference` mapper, no less bespoke
than mapping directly from `biblatex::Entry`, while pulling in hayagriva's much heavier
dependency tree (citationberg, icu, etc.) for zero conversion benefit. The existing
spike's direct `biblatex` dependency is confirmed as the right choice.

`biblatex`\'s own dependencies (`paste`, `roman-numerals-rs`, `strum`,
`unicode-normalization`, `unscanny`) are pure Rust with no libc/getrandom/IO — low wasm32
risk by inspection, not yet empirically confirmed by an actual build (Phase 3 gate,
below).

### Extension/format dispatch

Keep the spike's existing extension match: `.bib`, `.bibtex`, `.biblatex` — no change.

### Correctness fixes required in the existing spike

The spike (`crates/pampa/src/citeproc_filter.rs`\'s current uncommitted diff) is a
reasonable starting shape but has real gaps beyond "needs more tests":

1. **Institutional/corporate authors.** Confirmed via an isolated probe (not committed;
   see the investigation notes) that `biblatex::Person::parse` collapses a
   doubly-braced institutional name (`{{World Health Organization}}`) to the exact same
   shape as a lone-mononym individual (`Person { name: "...", given_name: "" }`) — the
   distinguishing signal (was this field's chunk a `Chunk::Verbatim`, i.e. explicitly
   protected) exists only in the raw `Entry::get(field) -> ChunksRef` data, which
   `Entry::author()` (and the spike's current mapper) discards.

   **Fix:** for each parsed `Person` with empty `given_name`, check whether the raw field
   also contains a `Chunk::Verbatim` whose `.get()` text exactly matches that
   `Person.name` — if so, populate `Name.literal` instead of `Name.family`/`given`. This
   is a translation-layer fix; `biblatex` itself behaves per its own documented,
   intentional two-convention name-parsing design (classic BibTeX splitting, or
   BibLaTiX's newer extended key=value format) — neither of which models "single
   protected span = institution," so there's nothing to report upstream.

2. **Title case-folding / `nocase`-span protection.** The spike currently uses
   `biblatex::ChunksExt::format_sentence` for titles, which lowercases every non-first,
   non-`Chunk::Verbatim` character with no per-word shape awareness. This diverges from
   Pandoc/citeproc's actual behavior — traced to the real Haskell source
   (`citeproc`\'s `Citeproc.CaseTransform.withSentenceCase`): it only lowercases a word
   matching `isCapitalized` (exactly one leading capital, rest lowercase), leaving
   ALL-CAPS/internally-mixed-case words untouched as a side effect of that narrow
   predicate — not a deliberate acronym-detection rule. Separately, explicit
   `{braced}` spans in the source `.bib` are a deliberate, spec-documented protection
   mechanism (Pandoc's `protectCase`/`nocase`-span machinery), independent of word shape.

   **Fix:** write a small per-word classifier reimplementing the `isCapitalized`
   predicate for `Chunk::Normal` (unbraced) text, and wrap `Chunk::Verbatim` (braced)
   spans in `<span class="nocase">...</span>` — the CSL-JSON embedded-richtext
   convention `quarto-citeproc/src/output.rs` already parses and protects at render
   time. Applies to `title`/`container-title`-shaped fields specifically, where
   BibTeX's Title-Case authoring convention diverges from CSL-JSON's sentence-case
   convention; verbatim-only fields (DOI, URL, ISBN, etc.) are unaffected.

3. **`genre` for thesis entries.** `@phdthesis`/`@mastersthesis` currently both collapse
   to `ref_type: "thesis"` with no further distinction, unlike Pandoc, which preserves
   the specific kind via a `genre` field (CSL's own sub-typing mechanism for a coarse
   `ref_type`, consumed by many styles when rendering thesis entries). Confirmed cheap:
   `quarto_citeproc::Reference` has no dedicated field for it, but its variable lookup
   already falls through generically to `self.other.get(name)` (the same mechanism
   backing `citation-label`) — so this is `reference.other.insert("genre", ...)`, no
   core-model change.

### Regression-test scope for v1

**Must-have** — each a new or extended test in `crates/pampa/src/citeproc_filter.rs`\'s
existing test module unless noted:

- `@string` macro resolution (common in real `.bib` files, e.g. journal abbreviations).
- `crossref` inheritance (e.g. an `@inproceedings` inheriting `booktitle`/`editor` from
  a referenced `@proceedings`).
- Institutional/corporate author → `Name.literal` (fix #1 above).
- Page-range dash normalization — already covered by the existing Knuth84 fixture;
  confirm it stays covered, no new work needed.
- Title case-folding / `nocase`-span protection (fix #2 above) — at least: one ALL-CAPS
  acronym staying unlowered, one ordinary Title-Case word getting lowered, one
  explicitly `{braced}` lowercase word staying protected.
- `genre` for `@phdthesis` vs `@mastersthesis` (fix #3 above).
- Malformed/unparseable `.bib` → clean `CiteprocFilterError::BibliographyParseError`,
  not a panic — parity with the existing CSL-JSON malformed-file error test.
- Entry-type mapping breadth — at least one test per family (a book-like type, a
  thesis-like type, a report-like type, `Misc`/`Unknown` fallback), not just `article`.
- **Book-merge integration test with a `.bib` bibliography**, in
  `crates/quarto-core/tests/integration/book_citations.rs`. That file's own doc comment
  currently states it uses CSL-JSON fixtures specifically because "pampa's citeproc does
  not parse BibTeX at all — separate gap, tracked independently." This strand *is* that
  gap; the comment and a real `.bib`-based test both need to land together, so the
  `ChapterCitationManifest`/multi-file-merge path is exercised end-to-end with real
  `.bib` input, not just the lower-level unit tests.

**Nice-to-have / explicitly deferred** — documented as known limitations, not silently
dropped:

- Arbitrary embedded LaTeX commands inside field text (e.g. `\textit{...}`, hand-typed
  accent commands) — Pandoc itself handles this unreliably (directly tested:
  `\textit{Biology}` leaked through as mangled literal text `\ntextitBiology`), so
  there's no working reference behavior to match.
- Non-Byzantine/CJK name-ordering edge cases — `biblatex`\'s `Person` parser is
  fundamentally Western-name-shaped (family/given/prefix/suffix); no realistic way to do
  meaningfully better in v1 without inventing something upstream doesn't have either.
- BibLaTeX's newer extended name-key-value syntax (`family={...}, given={...}`) —
  already works via `biblatex`\'s existing support (confirmed via probe); not worth a
  dedicated fixture since real exported `.bib` files rarely use it.

## Phases

- [x] **Phase 0 — Design.** This session's research + discussion; captured above. No
  code changes.
- [x] **Phase 1 — Fix correctness gaps + unit tests in `citeproc_filter.rs`.**
  Institutional-author `literal` mapping, title case-folding/`nocase`-span protection,
  `genre` field, plus all must-have unit-level regression tests (macro resolution,
  crossref inheritance, malformed-file error, entry-type breadth). Gate:
  `cargo clippy -p pampa --all-targets -- -D warnings`, `cargo nextest run -p pampa`.
  **Done 2026-09-28.** `biblatex` bumped to `0.12` (latest; API verified compatible —
  `Bibliography::parse`, `Entry::get`, generated `Entry::author`,
  `Chunk::{Normal,Verbatim,Math}`, `ChunksExt::format_sentence`/`format_verbatim`,
  `EntryType` variants all unchanged from 0.11). Clippy `-D warnings` clean;
  `cargo nextest run -p pampa`: **4855 passed / 2 skipped** (unchanged skip count; the
  4 new/fixed BibTeX tests are additive). Phase-boundary `cargo nextest run --workspace`
  (per the user's stricter global gating rule, run at this phase boundary rather than
  only at the end): **15232 passed / 201 skipped / 0 failed**, vs. the live baseline
  **15224 passed / 201 skipped @ `88f2d94a8`** (measured directly via `git stash`/re-run
  on this exact HEAD, not copied from this document) = **+8 passed, skips unchanged**.
  Accounted for exactly: 4 new `citeproc_filter.rs` unit tests
  (`test_load_bibtex_bibliography`, `test_load_bibtex_macros_crossrefs_and_types`,
  `test_load_bibtex_title_case_and_protected_spans`,
  `test_load_bibtex_malformed_file_returns_parse_error`), each counted twice because
  pampa's test suite compiles for both the `pampa` lib target and the `pampa::bin/pampa`
  binary target — 4 × 2 = 8. No other crate's count moved.
- [x] **Phase 2 — Book-merge coverage.** Add a `.bib`-bibliography variant to
  `crates/quarto-core/tests/integration/book_citations.rs`; update its doc comment (the
  "separate gap, tracked independently" note is stale once this lands). Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings`,
  `cargo nextest run -p quarto-core`.
  **Done 2026-09-28.** New test `book_merge_supports_bibtex_bibliography` mirrors
  `numeric_citation_numbers_are_book_wide` with a `.bib` bibliography (`REFS_BIB`)
  instead of CSL-JSON, exercising the same book-wide deferred-citeproc merge path
  end-to-end. Doc comment updated to drop the stale "separate gap, tracked
  independently" note. Clippy `-D warnings` clean; `cargo nextest run -p quarto-core`:
  **5283 passed / 32 skipped**. Phase-boundary `cargo nextest run --workspace`:
  **15233 passed / 201 skipped / 0 failed**, vs. Phase 1's own live baseline
  (15232 passed / 201 skipped) = **+1 passed, skips unchanged** — exactly the one new
  integration test (single binary; unlike Phase 1's pampa unit tests, quarto-core's
  integration tests compile into one `integration` binary per the workspace's
  `tests/integration/<name>.rs` convention, so no ×2 multiplier here).
- [x] **Phase 3 — WASM verification.** Confirm `biblatex` actually compiles for
  `wasm32-unknown-unknown` under pampa's hub-client feature set (a smoke build, not the
  full `wasm_lua.rs` suite — `citeproc_filter.rs` has no
  `#[cfg(target_arch = "wasm32")]` blocks, so it is not in `.claude/rules/wasm.md`'s
  tracked-files list, but this needs confirming rather than assuming). A failure here is
  a blocking finding requiring a design revisit, not a silent skip.
  **Done 2026-09-28.** Ran (per `dev-docs/wasm.md`'s documented local setup, on the
  pinned nightly + Homebrew LLVM clang):
  ```
  CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang \
  CFLAGS_wasm32_unknown_unknown="-isystem $PWD/crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
  cargo build -p pampa --lib --target wasm32-unknown-unknown \
    --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
  ```
  Exit code 0 — `biblatex 0.12.0` compiled cleanly for `wasm32-unknown-unknown`, and
  `pampa`\'s lib compiled successfully with it under the hub-client feature set. No
  errors; only pre-existing warnings unrelated to this change (duplicate-crate lint
  noise, an unused import in `quarto-system-runtime::wasm`). No blocking finding — the
  low-wasm32-risk inference from Phase 0 is confirmed, not just assumed.
- [x] **Phase 4 — Workspace gate + wrap-up.** `cargo nextest run --workspace`, delta
  reported against the current live baseline. Reconcile this plan's checklist against
  what actually landed before handing off, per the standard "finishing a plan" step.
  **Done 2026-09-28.** Final `cargo nextest run --workspace`: **15233 passed / 201
  skipped / 0 failed** — unchanged from Phase 2's boundary (Phase 3 added no tests),
  and consistent with the cumulative math: 15224 (original live baseline @
  `88f2d94a8`) + 8 (Phase 1) + 1 (Phase 2) + 0 (Phase 3) = 15233. This checklist was
  re-read against the actual landed commits (`17cc60f4a` Phase 1, `5ed7c153d` Phase 2,
  `fcaefbc38` Phase 3) rather than trusted as-written before marking phases done.

**Out of scope for this plan (separate decision, not bundled in):**
`quarto-project-create`\'s book template currently ships `references.json` specifically
because there was no BibTeX parser (its own doc comment says so explicitly). Whether to
switch the template to `.bib` once this lands is a distinct scaffolding decision, not
citeproc correctness — flag as a possible follow-up rather than folding into this plan's
checklist.

## Risks / tradeoffs

- The title case-folding fix requires writing new logic (a small word-classifier)
  rather than trusting `biblatex::ChunksExt::format_sentence` outright — well-understood
  and traced to primary source, but genuinely new code, not a call-through.
- WASM compilation of `biblatex` was inferred low-risk from its dependency tree;
  **confirmed** by Phase 3's smoke build (exit 0, no errors) — sequencing it before
  the final workspace gate meant a real blocker would have surfaced with enough
  runway to revisit the design, but none did.
- `book_citations.rs`\'s doc comment was effectively documentation-as-tracking for this
  exact gap; Phase 2 updated it alongside the new `.bib` test, so it no longer claims
  the gap is unaddressed.
