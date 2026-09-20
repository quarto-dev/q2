# Accepted divergences from the Q1 golden

Per-fixture ledger of every `pandoc-goldens` snapshot pair where Q2's
Pandoc-hybrid output is **known and accepted** to differ from the real Q1
golden captured by `cargo xtask capture-pandoc-goldens` (P7 Task 10), rather
than compared byte-for-byte by
`crates/quarto-core/tests/integration/pandoc_goldens.rs` (P7 Task 11).

This file is checked against the test's own
`ACCEPTED_DIVERGENT_SNAPSHOTS` list by
`test_divergence_ledger_is_complete_and_accurate` (T11.5): every entry here
must name a snapshot that list also names, and vice versa. **Never** "fix" a
persistent divergence by teaching `quarto-ooxml-extract` to skip the
differing surface — that would make every future regression in that area
invisible while this file (and T11.5) keeps passing. Delete an entry (and
its corresponding line in `ACCEPTED_DIVERGENT_SNAPSHOTS`) only when the
underlying capability actually lands and a fresh capture confirms parity.

## `smoke-all/mermaid/backticks.qmd`

- **Snapshot(s):** `smoke_all_mermaid_backticks__docx`, `smoke_all_mermaid_backticks__pptx`
- **What differs:** Q1 rasterizes the fixture's `{mermaid}` cell via a real
  headless-browser pipeline (`chrome-headless-shell`) into an embedded
  raster image (the captured golden's `media`/`image_relationships` list an
  entry, `drawing_count: 1`). Q2's Pandoc-hybrid tail has no mermaid
  renderer — it leaves the cell's echoed source text
  (`This would have been a problem.`, per the fixture's `%%| echo: true`) as
  a plain code paragraph and embeds no image.
- **Design reference:** `claude-notes/designs/pandoc-hybrid-architecture.md`
  §12 ("no warning ships in v1" — the accepted gap this fixture makes
  reviewable rather than silent). Tracked as `bd-h1ub8f8z`.
- **Accepted:** 2026-09-20/21, as part of P7 Task 10's real capture run
  (see `claude-notes/plans/2026-08-20-pandoc-hybrid-P7-format-tail.md`,
  Finding 4).
- **What Task 11 asserts instead:** `test_mermaid_fixture_preserves_diagram_source_text`
  binds the narrower shape that survives — the diagram's source text stays
  present as body content — rather than asserting equality against the Q1
  golden. See that test's doc comment for the acknowledged limits of what
  this actually guards (labeling, not mermaid behavior).
