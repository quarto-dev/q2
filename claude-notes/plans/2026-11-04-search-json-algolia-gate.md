---
title: 'Write search.json only when Algolia is configured (bd-cdehspam)'
date: 2026-11-04
description: 'Stop writing the Quarto 1-shaped search.json by default; write it only with a complete Algolia config or an explicit website.search.search-json override, and skip record extraction when it is not needed.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-cdehspam
---

**Checkout:** main checkout, branch `main` @ `94e1f8afe`.
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The strand records the behavior the user chose on
2026-11-04, and the code it names exists in the shape it describes. A
few small decisions remain: the key name, how ownership is tracked,
and how to word the migration note.

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
  - A `search.json` that q2 didn't write survives.
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
  on. When it is off, remove a file that q2 wrote and persist
  `search_json: false`. Update the module docs in `writer.rs` and
  `records.rs`.
- **Phase 3: Other tests.** Fix the `format_resolution_cli.rs` filter
  and its comment, and the Algolia test's "still written" assertion.
- **Phase 4: Docs.** Document the key and its default in the guide,
  rewrite the three passages listed above, and add the migration
  note. Re-render the docs with q2.
- **Phase 5: Measure.** Optionally, re-time the parallel claude-notes
  render to record the gain alongside bd-w0xmxkhn's numbers.

## Open design questions for the user

1. **Key name.** The strand proposes `website.search.search-json`.
   Keep it, or pick something that names the purpose, such as
   `records-json`? I recommend keeping `search-json`, because it names
   the file the user sees.
2. **How strict is "q2 wrote it"?** Today the state records only a
   boolean. Suppose q2 wrote the file in an earlier run, and the user
   later replaces it with their own file, for example as a project
   resource. When the setting flips off, the boolean would delete the
   user's file. Is the boolean good enough, or should the state store
   a SHA-256 of the bytes q2 wrote, and q2 delete the file only if it
   still matches? I lean towards the hash: it is cheap and makes
   "never delete what q2 didn't write" true.
3. **Books.** Does `book:` config reach `website.search` (so that
   `book.search.search-json` works the same way)? Or should the plan
   confirm and test the book spelling explicitly? The existing book
   test reads `records()` and will need the override either way.
4. **Migration note.** Is the "Differences from Quarto 1" section of
   `website-search.qmd` enough, or is there a central Quarto 1 → 2
   migration page this should also go on?

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
