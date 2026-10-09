---
title: 'YAML provenance desync after a block scalar with non-ASCII content (bd-e0e9kd4a)'
date: 2026-10-08
description: 'Records the fix for a YAML provenance desync after a block scalar with non-ASCII text, caused by a byte-counting bug in the upstream parser, fixed by upgrading `quarto-yaml` and adding a regression test.'
status: done  # Resolved (2026-10-08). The fix shipped upstream; q2 adds a regression test. See § Resolution.
braid:
  strand: bd-e0e9kd4a
  priority: P2
---

**Branch:** `braid/bd-e0e9kd4a-folded-scalar-provenance-desync` (topic branch in the main checkout, based on `main` @ `8ae461f1b`)

## Resolution

We went with option (d) from the quarto-yaml strand, a variant of Q1 below. The bug was filed in the quarto-yaml skein as
`qy-block-scalar-utf8-drift-7dccrmto`. quarto-yaml fixed it by switching its parser from yaml-rust2 to
saphyr, whose scanner already counts characters (posit-dev/quarto-yaml#22, released as 0.5.0). q2 picked up
0.5.0 in `feca201b9`. That commit changes only `Cargo.toml` and `Cargo.lock`, so the remaining q2 work was:

- **Assessment against 0.5.0.** A scratch probe printed spans and content provenance for these cases:
  - the original repro (`beads` is now at bytes 21..26; 0.4.0 gave 23..28);
  - `|`;
  - a multi-line `>`;
  - two block scalars in a row, one on a line longer than the lookahead buffer;
  - a block scalar inside a sequence.

  Every case was correct. `q2 render` of the repro and of the two mermaid plans where the bug was found now
  produces no "no content provenance" warning.
- **Regression test.** `pampa::pandoc::meta::tests::non_ascii_block_scalars_keep_later_spans_and_provenance`
  puts a folded and a literal non-ASCII scalar before a key whose value contains raw HTML. It asserts two
  things: no desync warning, and Q-2-9 carets that underline exactly `<b>` and `</b>`. The test parses real
  YAML; the older desync tests used hand-built nodes. Re-pinning quarto-yaml to `=0.4.0` turns it red: two
  desync warnings, and both carets fall back to a zero-width span at end of input.

The design questions below are moot now that the upstream switch landed. They are kept for the record.

## Triage verdict

**Ready to design.** The root cause is a byte-vs-char counting bug in
**yaml-rust2's** scanner (not in quarto-yaml's provenance walk, and not in q2).
The fix is a one-liner upstream. The open question is where we fix it:
upstream only, a quarto-yaml workaround, or both.

## Issue context

Filed today (2026-10-08) by Carlos, P2 bug. Front matter

```yaml
---
status: >
  v2 — x
beads: y
---
```

emits two `YAML string scalar has no content provenance` warnings. The
warnings come from the desync self-checks in `crates/quarto-config/src/convert.rs`
(`content_provenance_desync_warning`) and in `crates/pampa/src/pandoc/meta.rs`.
Both are reported at the *next* key. Two controls render clean: the same YAML
with an ASCII dash, and a double-quoted non-ASCII scalar. The strand left
literal (`|`) scalars unchecked.

## Dependency graph

- **discovered-from**: bd-uk8zgkha (*Render claude-notes as a q2 website*,
  in_progress). Two plans in that work use a `status: >` header that contains
  an em dash: `2026-05-28-mermaidjs-engine-design.md` and
  `2026-07-20-mermaid-regular-rendering.md`. This bug is a side finding from
  that work. Nothing depends on it.
- There are no blocks or related edges.

Related history (not braid edges):
- posit-dev/quarto-yaml#9 (closed): *Spans are built from character indices but
  consumed as byte offsets*. The fix for #9 added the char→byte cursor in
  `byte_offset_of_char`. That cursor assumes `Marker::index()` counts chars
  **everywhere**. This bug breaks that assumption.
- Ethiraric/yaml-rust2#79 (closed 2026-08-18, filed by hadley): the
  `Marker::index()` docs said bytes but the value counts chars. Upstream fixed
  only the docs and confirmed that chars is the contract. The bug below
  violates that contract.

## What the code looks like today

The bug reproduces at HEAD. See
`2026-10-08-folded-scalar-provenance-desync-investigation/marker-probe/`, a
standalone crate that prints raw yaml-rust2 0.11.1 event markers and the
quarto-yaml 0.4.0 tree. Its output is saved in `…-investigation/probe-output.txt`.

**Root cause.** The bug is in
`yaml-rust2-0.11.1/src/scanner.rs:1746` (`scan_block_scalar_content_line`).
This function handles both literal and folded block scalars. It reads the part
of a content line that is past the 16-char lookahead buffer with
`raw_read_ch()`, then advances the marker by

```rust
self.mark.col   += line_buffer.len();   // bytes!
self.mark.index += line_buffer.len();   // bytes!
```

`line_buffer` is a `String`, so `.len()` returns a byte count. All other paths
add one per char. As a result, **every marker after a block scalar that holds
multi-byte characters is shifted forward** by the extra bytes in that portion
of the line. The drift adds up across block scalars and lasts until the end of
the document. In the probe, `beads` reports idx=21. The true char index is 19,
and 21 is the byte offset. quarto-yaml then converts 21 as a char index and
lands on byte 23.

The bug is still present on yaml-rust2 `master` (same line, checked via the
GitHub API today). The latest release is 0.13.0, and we are on 0.11.1.

**Consequences (wider than the warning):**
1. All **raw spans** after the block scalar are wrong, not just content
   provenance. In the probe, the `beads` key's span is 23..28 (`ads: `) and the
   `y` value's span is 30..30. Every schema-validation diagnostic, LSP range, or
   caret after such a scalar points at the wrong bytes. The desync warning is
   just the only check that notices.
2. **Literal (`|`) scalars are affected the same way.** They share the same
   scanner function, and the probe confirms it.
3. Plain and quoted scalars with non-ASCII text are fine, because those paths
   count per char.
4. `marker.col()` is wrong too on block-content lines, but it resets at each
   newline. Every marker that starts a token *after* the block scalar (on a
   later line) therefore has a correct `(line, col)`.

**Where things live.** quarto-yaml is an external crate (crates.io 0.4.0,
posit-dev/quarto-yaml), not in this workspace. Its `byte_offset_of_char`
(`src/parser.rs:270`) and `compute_scalar_provenance` (`src/parser.rs:415`)
work correctly given correct markers. The two q2 self-checks are working as
intended: they caught a real bug.

## Proposed phases (draft)

- Phase 0 — Failing tests.
  - **yaml-rust2:** a marker-index test for a folded and a literal scalar with
    multi-byte content, followed by a key.
  - **quarto-yaml:** a span test and a content-provenance test for the key and
    value after such a scalar (`content_provenance_tests.rs`), covering both
    `>` and `|`, and two block scalars in a row (accumulated drift).
  - **q2:** a pampa or quarto-config test confirming that the front matter
    above produces no desync warning and that the `beads` key has span 21..26.
- Phase 1 — Upstream fix in yaml-rust2: `line_buffer.chars().count()` for both
  `col` and `index`. Open the PR or issue against Ethiraric/yaml-rust2 and link
  it to #79.
- Phase 2 — quarto-yaml (depends on Q1): either pick up the fixed yaml-rust2
  release, or work around it locally (options in Q1). Release quarto-yaml 0.4.1.
- Phase 3 — Bump quarto-yaml in q2 and add the q2 regression test. Re-render
  the two mermaid plans to confirm the warning is gone.

## Open design questions for the user

1. **Where to fix.** Options:
   - (a) **Upstream only.** Send a PR to yaml-rust2 and wait for the release.
     yaml-rust2 releases are slow, and we would also need to move from 0.11 to
     0.13 or later.
   - (b) **Upstream PR plus a `[patch.crates-io]` git pin** in q2 and
     quarto-yaml until the release ships.
   - (c) **Upstream PR plus a quarto-yaml workaround** that doesn't depend on
     the release. One way: compute byte offsets from `(line, col)` instead of
     `index`. Line is always correct, and col is correct at every token start
     after a block scalar (see consequence 4). Another way: re-sync the cursor
     after each block scalar using the scalar's known byte span.

   My recommendation is (c) with the `(line, col)` mapping, because it also
   protects against any other undercount we haven't found yet. Do you want
   that, or a lighter option?
2. **Moving to yaml-rust2 0.13.** Should the upstream bump happen in the same
   quarto-yaml release, or stay separate? The 0.11→0.13 changes haven't been
   surveyed.
3. **Who files upstream.** Should I open the yaml-rust2 issue/PR, or do you
   (or hadley, who filed #79) want to?
4. **Should the q2 self-check get more tests?** The warning did its job. Do we
   want a q2 integration test that feeds a corpus of non-ASCII block scalars
   (literal, folded, chomping indicators, nested in sequences) through
   `config_value_from_yaml` and asserts that no desync warning appears?

## Risks / tradeoffs (draft)

- A `(line, col)` mapping in quarto-yaml needs a line-start table, which adds a
  little memory per parse. It must also handle `\r\n`: check how yaml-rust2
  counts col after a CR.
- Any q2 snapshot tests that encoded the *wrong* spans after a non-ASCII block
  scalar would change when the fix lands. That is probably rare, but grep for
  them in Phase 0.
- The marker probe in the investigation dir is a standalone crate (it has its
  own `[workspace]`), following the precedent in
  `claude-notes/research/2026-09-21-quarto-math-probes/`.
