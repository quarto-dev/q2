---
title: 'Write search.json only when Algolia is configured (bd-cdehspam)'
date: 2026-11-04
description: 'Stop writing the Quarto 1-shaped search.json by default; write it only with a complete Algolia config or an explicit website.search.search-json override, and skip record extraction when it is not needed.'
status: in-progress
braid:
  strand: bd-cdehspam
---

**Branch:** `braid/bd-cdehspam-search-json-algolia-gate` (off `main` @ `94e1f8afe`).
The user gave the go-ahead on 2026-11-04.

## Triage verdict

**Ready to implement.** The strand records the behavior the user chose
on 2026-11-04, and the code it names exists in the shape it describes.
The remaining design questions were settled with the user the same
day (see "Design decisions").

## Issue context

bd-cdehspam (feature, P2, labels `perf`, `search`). It was filed today
by Carlos Scheidegger and is still `open`. In summary:

- **Default:** write `<output>/search.json` only when
  `SearchOptions.algolia` is `Some` after resolution.
- **Override:** a new boolean key `website.search.search-json`.
  `true` always writes the file and `false` never does. If the key is
  absent, the default applies.
- `website.search: false` keeps its meaning (no search outputs).
- When the effective setting is off, remove a `search.json` that a
  previous run of q2 wrote, using the `WriterState.search_json`
  mechanism. Never delete one that q2 didn't write.
- When the file isn't needed, skip `extract_records` entirely,
  including the scoped thread added in PR #1054.
- **Why:** q2's search client never reads `search.json`. On
  claude-notes the file is 30.8 MB with 13,732 records, larger than
  the 24 MB pagefind bundle, and building it costs about 2% of serial
  render CPU. In parallel renders it is now on the critical path.

## Dependency graph

- **parent-child → bd-eip0naf4** (epic, in progress): the website
  search port. It introduced `search.json` to keep Quarto 1's shape,
  so that Algolia users and external tools could still read it.
- **related → bd-w0xmxkhn** (in progress): parallel pagefind
  indexing. Its latest comment says: "our search.json extraction is on
  the critical path". With the parallel fossick fork, pagefind
  finishes sooner, and the `extract_records` thread in `writer.rs`
  becomes the longest step. This strand removes that work for the
  common non-Algolia site, so the two strands complement each other.
- Nothing blocks this strand, and it blocks nothing.

## What the code looks like today

The investigation confirmed everything at HEAD (`94e1f8afe`):

- `crates/quarto-core/src/project/search/writer.rs`
  - `write_search_index` always extracts records. When
    `worker_count() > 1` it does so on a `thread::scope` thread next
    to `build_bundle`; otherwise it runs afterwards. It always writes
    `search.json` and then writes `WriterState { search_json: true }`.
  - `remove_outputs` (the `website.search: false` path) deletes the
    lib subdir, `search.json` if the state says q2 wrote it, and
    **the whole `.quarto/search/` dir**, including the per-page
    capture cache. This new feature turns off only `search.json`, so
    it can't reuse `remove_outputs` as-is. Instead it has to delete
    the file and **rewrite** the state with `search_json: false`,
    leaving the cache alone.
  - The `pages.is_empty()` early return already removes a
    `search.json` that q2 wrote, based on the state. That path stays
    the same.
- `crates/quarto-core/src/project/search/config.rs`
  - `resolve_website_search` is a `match` over keys. Unknown keys fall
    through to `unknown_key` (Q-5-38), and bad values go to
    `invalid_value` (Q-5-39). `SearchOptions` has no `search-json`
    field yet. The module-doc YAML sketch needs one line.
  - Order: `algolia` is resolved inside the loop, so the effective
    value (`explicit.unwrap_or(algolia.is_some())`) has to be computed
    **after** the loop. Otherwise the result would depend on whether
    `search-json` comes before or after `algolia`.
- `crates/quarto-core/src/project/search/records.rs`: the module doc
  explains why the file exists. Per the strand, it should also say
  when the file is written.
- Tests that rely on the file today:
  - `crates/quarto-core/tests/integration/website_search.rs`: the
    `records()` helper (line 179) reads `search.json`, and several
    tests use it (shape, incremental, book). Line 284 asserts the file
    is absent for `search: false`. Line 624 asserts it is present with
    `navbar.search: false`. Line 768 asserts "search.json still
    written" under Algolia.
  - `crates/quarto/tests/integration/format_resolution_cli.rs:62-83`
    filters `search.json` out of its output listing, with a comment
    saying a book always writes it. After this change, that filter
    and its comment are stale.
  - `crates/quarto-preview/src/static_mode/embedded_site.rs:103` uses
    `search.json` only as a router fixture. No change is needed.
- **Schema / completions:** q2 has no live YAML schema for
  `website.search`. The only copy is the Quarto 1 fixture
  `crates/pampa/test-fixtures/schemas/definitions.yml`, which
  shouldn't be edited. So the strand's "schema or completion entry"
  item has nothing to change.
- **Docs:** `docs/guides/projects/website-search.qmd` says in three
  places that `search.json` is always written: the outputs list
  (~line 109), the Algolia section (~line 179: "Quarto still renders
  `search.json`"), and "Differences from Quarto 1" (~line 205). That
  last section is the natural home for the migration note.
- No other code reads `search.json`. The only references are in the
  hub-client, ts-packages, the preview SPA, and smoke-all specs, none
  of which consume the file.

## Proposed phases (draft)

- **Phase 0: Tests first.** In `website_search.rs`:
  - Default without Algolia: no file.
  - Default with Algolia: file present.
  - `search-json: true` without Algolia: present.
  - `search-json: false` with Algolia: absent.
  - Flip-off: render with the file, change the config, re-render, and
    check that the file is gone and the capture cache survives.
  - A `search.json` that q2 didn't write survives, and so does one
    that q2 wrote but the user has since edited (hash mismatch).
  - Book: `book.search.search-json: true` writes the file, and a book
    with no setting doesn't (decision 3).
  - A non-boolean `search-json` value gives Q-5-39 and the default.
  - Unit tests in `config.rs` for key order and for an incomplete
    Algolia block (Q-5-40) giving the default `false`.
  - Make the existing record-shape tests opt in with
    `search-json: true`.
- **Phase 1: Config.** Add a `search_json: bool` field to
  `SearchOptions`, or `Option<bool>` plus an accessor. Resolve the key
  and compute the effective value after the loop. Update the module
  doc.
- **Phase 2: Writer.** Gate the extraction thread and the serial
  fallback on the effective flag, and write the file only when it is
  on. When it is off, remove the file only if its hash matches the
  one in the state (decision 2), then persist a state with no hash.
  The `website.search: false` path (`remove_outputs`) and the
  empty-pages path use the same hash check. Update the module docs in `writer.rs` and
  `records.rs`.
- **Phase 3: Other tests.** Fix the `format_resolution_cli.rs` filter
  and its comment, and the Algolia test's "still written" assertion.
- **Phase 4: Docs.** Document the key and its default in the guide,
  rewrite the three passages listed above, and add the migration
  note. Re-render the docs with q2.
- **Phase 5: Measure.** Optionally, re-time the parallel claude-notes
  render to record the gain alongside bd-w0xmxkhn's numbers.

## Design decisions (settled with the user, 2026-11-04)

1. **Key name:** `website.search.search-json`, as the strand
   proposes.
2. **Ownership check:** `WriterState` stores a SHA-256 of the bytes q2
   wrote, and q2 deletes `search.json` only if the file still matches.
   This is a courtesy, not a contract. q2 can't reason precisely about
   every file it generates, so the code comments, module docs, and
   user docs must not present this as a general promise that q2 never
   touches user files. Word it as: "if the file is unchanged since q2
   wrote it, q2 removes it". The state file grows from the boolean to
   `search_json_sha256: Option<String>`, so bump `STATE_VERSION`.
   **Implemented choice:** a v1 state with `search_json: true` (no
   hash) falls back to the old boolean behavior once, and the file is
   removed. Leaving it would keep a stale file, possibly 30 MB, on
   every site already rendered by q2, which defeats the point. v1
   states only exist from q2 builds of the last few weeks.
3. **Books:** they already behave the same, by construction. The book
   project's `pre_render` (`project/book/project_type.rs:104`) calls
   `copy_book_keys_to_website` on `project.config.metadata` before
   anything resolves search. `"search"` is in `BOOK_TO_SITE_KEYS`
   (`project/book/config.rs:71`), so the whole `book.search` map,
   including `search-json`, replaces `website.search`. The writer,
   `SearchCaptureStage`, and `search_ui` then all resolve the same
   `website.search`. So the book form needs no code, just one test:
   a book with `book.search.search-json: true` writes the file, and a
   book with no setting doesn't. (Existing behavior, unchanged here:
   when both `book.search` and `website.search` are set, the book's
   map wins wholesale rather than being merged.)
4. **Migration note:** goes in "Differences from Quarto 1" in
   `docs/guides/projects/website-search.qmd`. Nothing goes on a
   central migration page.

The plan has no open questions left.

## Progress

- [x] Config: `SearchOptions::search_json`, resolved after the key
  loop; Q-5-38 hint and Q-5-39 for non-booleans; unit tests in
  `config.rs`.
- [x] Writer: extraction (thread and serial) and write gated on the
  flag. `WriterState` v2 stores a hash, and `remove_own_search_json`
  is used by the off path, the empty-pages path, and
  `website.search: false`.
- [x] Integration tests: `records_site` fixture for the record-shape
  tests, plus new cases (default absent, `true`, `false` with
  Algolia, flip off and on, edited file survives, foreign file
  survives, v1 state, book default).
- [x] `format_resolution_cli.rs` filter removed.
- [x] Docs: guide (options table, output files, Algolia section,
  migration note), Q-5-38 and Q-5-39 option lists.
- [x] `cargo xtask verify --skip-hub-build` green. `q2 render docs`
  is clean in the edited pages (`--strict` reports 34 errors that
  were already there, in figures/brand/lua-filters), and the docs
  site no longer writes `search.json`.
- [x] Windows cross-clippy attempted. It can't build `aws-lc-sys` from
  macOS; the change has no platform-specific code.
- [ ] PR, CI.

## Risks / tradeoffs (draft)

- **Silent behavior change** for anyone already using q2 who reads
  `search.json` without Algolia. The file just stops appearing, with
  no diagnostic. That is acceptable under the strand's decision, but
  it makes the docs and the migration note the only signal.
- **An incomplete Algolia block** (Q-5-40) now also means "no
  `search.json`". A user part-way through Algolia setup who needs the
  file to populate the index will see Q-5-40 first, which points them
  at the problem. The docs should mention `search-json: true` for the
  bootstrap case: upload first, then configure.
- **Test churn.** Many record-shape tests go through the `records()`
  helper. Opting them in through the shared `search_site` fixture
  keeps the diff small.
