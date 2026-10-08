# Frontmatter YAML parse error: Q-0-99, internal path in title, whole-block span (bd-x30aq7ae)

**Date:** 2026-10-08
**Braid:** bd-x30aq7ae
**Branch:** `braid/bd-x30aq7ae-frontmatter-yaml-error-code` (topic branch in the main checkout, based on `main` @ `3d3360ab6`)
**Status:** Investigation is done and the design needs the user's input. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design; Phase 2 is blocked on an upstream quarto-yaml release** (`qy-scan-error-location-f9r4yvxq`). All three defects reproduce at HEAD at one call site. Defects (1) and (2) can be fixed locally in pampa. Defect (3) needs one decision: fix `quarto-yaml` upstream (cleanest, but needs a crate release and a bump) or recover the position inside pampa.

## Issue context

This is R2 from `claude-notes/plans/2026-10-06-json-errors-output-hygiene.md`, found while rendering `claude-notes/` with `q2 render --json-errors`. The strand was filed on 2026-10-07 as an open P2 bug. A malformed frontmatter block produces:

- code **Q-0-99**. The catalog says this code "should never appear in production, and is a bug in Quarto". The strand proposes **Q-1-1** "YAML Syntax Error" instead.
- a title that contains the internal location `(at crates/pampa/src/utils/diagnostic_collector.rs:45)`. That is the line inside `error_at`, not even the meaningful caller.
- a span that covers the whole YAML block, even though the YAML error knows a precise line and column.

## Dependency graph

- **discovered-from** bd-gnw9asuo (closed, PR #795), "q2 render --json-errors: make stderr pure, attributable NDJSON". That work audited every record in the `--json-errors` stream and listed this one as a leftover hygiene item (R2). Sibling items from the same review: R3 (uncoded diagnostics), R4 (temp path and Lua traceback leaking), R5 (Q-13-4 quoting).
- No blocks, related, or child edges.
- Incidental: pre-flight verify found main red because of the CSS lint step. That was filed as **bd-verify-css-lint-nested-ws-r0dd5vdk** (discovered-from this strand) and is unrelated to this work.

## What the code looks like today

The emitting site still exists as the strand describes it, in `crates/pampa/src/pandoc/meta.rs`, `rawblock_to_config_value` (around L598–606):

```rust
Err(e) => {
    diagnostics.error_at(
        format!("Failed to parse YAML frontmatter: {}", e),
        yaml_parent,
    );
```

How each defect arises:

1. **Code and (2) internal path.** `DiagnosticCollector::error_at` (`crates/pampa/src/utils/diagnostic_collector.rs:40-49`) builds its diagnostic with `generic_error!`. In quarto-error-reporting 0.4.0, `generic_error!` expands to `DiagnosticMessageBuilder::generic_error(msg, file!(), line!())`, which produces code Q-0-99 and appends `(at file:line)` to the title. Q-1-1 exists in `crates/quarto-error-catalog/error_catalog.json`, but **no Rust code emits it yet**. This would be its first use, so check whether `cargo xtask lint` requires a `docs/errors/yaml/Q-1-1` page or a sidebar entry for codes in use (rules `error-docs-page-missing` and `error-docs-sidebar-unlisted`).
2. **The precise position is discarded upstream, not in pampa.** In `quarto-yaml` 0.3.0 (published crate, repo `posit-dev/quarto-yaml`), `parser.rs:153` does `parser.load(...).map_err(Error::from)?`. Its `impl From<yaml_rust2::ScanError> for Error` (`error.rs`) builds `Error::ParseError { message: err.to_string(), location: None }`. As a result:
   - the `location` field always arrives as `None` for scan errors, even though `parse_with_parent` has the parent `SourceInfo` and the scanner `Marker` in hand;
   - the only remaining position is text inside `message`. `ScanError`'s Display renders `"{info} at byte {index} line {line} column {col+1}"`, where `index` counts **chars**, not bytes, despite the label. quarto-yaml already has `byte_offset_of_char` to convert. The line is relative to the YAML body, not to the file, so the "line N" a user sees today is wrong by the frontmatter's offset;
   - `quarto_yaml::Error`'s `Display` also has a TODO saying the location cannot be displayed.

   The local `external-sources/quarto-yaml` checkout is stale (0.1.2) and cannot be used as a reference for 0.3.0. Read the registry copy instead.
3. Another consumer already expects `location` to be populated: `quarto-core/src/cell_options/mod.rs` `CellOptionsError::location()` pattern-matches `quarto_yaml::Error` for it. An upstream fix would therefore also make cell-option YAML errors precise.

**Reproduced at HEAD** (`claude-notes/plans/frontmatter-yaml-error-code-investigation/`). The fixture has an unclosed `author: [a, b` on line 3, and the scanner fails at the `:` of `format: html` (file line **4**, column 7). Captured record:

```
code:  Q-0-99
title: Failed to parse YAML frontmatter: Parse error: illegal placement of ':' indicator
       at byte 35 line 3 column 7 (at crates/pampa/src/utils/diagnostic_collector.rs:45)
span:  2:1–5:1
```

All three defects show up. The embedded "line 3" is relative to the YAML body, so in file terms it is one line too high.

### Bug-class audit (strand asked for this)

Production emitters of Q-0-99 in pampa (through `DiagnosticCollector::{error,warn,error_at,warn_at}` or a direct `generic_error!`/`generic_warning!`):

| Site | Message | User-reachable? |
|---|---|---|
| `pandoc/meta.rs:603` | "Failed to parse YAML frontmatter: …" | **Yes** (this strand) |
| `pandoc/treesitter_utils/postprocess.rs:1963` | "Caption found without a preceding table" (fallback path when the caption's source text can't be recovered) | **Yes** (needs an uncontiguous or generated source) |
| `pandoc/treesitter.rs:1873` | "The input document is too deeply nested (more than N levels)." | **Yes** (pathological input) and has no location |
| `pandoc/treesitter.rs:1883` | "Failed to parse document: top-level parse error" | Arguably internal |
| `pandoc/treesitter_utils/postprocess.rs:1831` | "Found attr in postprocess … should have been removed" | Internal invariant; Q-0-99 is honest here |

`FilterContext::{warn,error}{,_at}` (`pampa/src/filter_context.rs`) forward to the same helpers, but no production caller uses them today. `quarto-doctemplate` has its **own** `DiagnosticCollector` that uses `DiagnosticMessageBuilder::error/warning`. Those diagnostics are uncoded but carry no Q-0-99 and no path, so they belong to a different bug class (R3, uncoded diagnostics).

## Proposed phases (draft)

- **Phase 0: failing tests first.** A pampa unit or integration test that parses the fixture and asserts code `Q-1-1`, a title without `(at `, and a span start at the offending line and column (file-relative). Possibly a `q2 render --json-errors` snapshot as well.
- **Phase 1: code and title (pampa only).** Emit through `DiagnosticMessageBuilder::error(...).with_code("Q-1-1").with_location(..)`, with the scanner's `info` as the problem text, instead of `error_at`. Decide what happens to `error_at`/`warn_at`; see Q2.
- **Phase 2: precise span.** Either (a) fix `quarto-yaml` so that `parse_with_parent` maps the `ScanError` marker through `parent` into `location` and `Display` stops embedding position text, then release, bump, and consume `location` here; or (b) recover the marker inside pampa. See Q1.
- **Phase 3: bug-class follow-ups.** Fix the other user-reachable rows in the audit table, or file strands for them.
- **Phase 4: docs.** Q-1-1 error page or sidebar if the lint requires it.

## Open design questions for the user

1. **Where should the precise span come from?** The options:
   - (a) Fix `quarto-yaml` upstream: populate `location` from `ScanError::marker()` mapped through `parent`, and drop the position from the message. This needs a quarto-yaml release (0.3.1 or 0.4.0) and a workspace bump. It also benefits cell options and every other `parse_with_parent` caller.
   - (b) Recover the position in pampa without touching quarto-yaml, for example by re-scanning `block.text` with `yaml_rust2` (pampa already depends on it) to get the `Marker` and mapping it through `yaml_parent`.
   - (c) Do (b) now and (a) later.

   I lean toward (a). Do you want an upstream quarto-yaml change in scope here?

   **Decided (user, 2026-10-08): (a).** Filed upstream as `qy-scan-error-location-f9r4yvxq` in the quarto-yaml skein (repo `posit-dev/quarto-yaml`, local clone `~/repos/github/posit-dev/quarto-yaml`). A separate agent fixes it and publishes a new crate version. The q2 side of this plan resumes after that release: bump the workspace `quarto-yaml` dep, then consume `Error::ParseError { location, .. }` in `rawblock_to_config_value`.
2. **Should the collector helpers change for every caller?** One option is to give `DiagnosticCollector::error_at/warn_at` a required `code` argument (or remove them), so the Q-0-99-with-path pattern can't recur. The other is to fix only this call site and leave the helpers for genuine internal invariants.
3. **Should the other Q-0-99 sites be in this PR?** The caption-without-table fallback and the too-deeply-nested error are both user-reachable. Fold them in, or file separate strands? The depth-limit case would need its own new code.
4. **Message shape.** Keep the title "YAML Syntax Error" from the Q-1-1 catalog, put the scanner info (for example "did not find expected ',' or ']'") in `problem`, and add a hint naming the frontmatter. Or keep a custom title such as "Failed to parse YAML frontmatter"?

## Risks / tradeoffs (draft)

- Option (a) adds a cross-repo release step. The provenance work shows quarto-yaml and quarto-yaml-validation sometimes have to bump together (`2026-08-20-provenance-1-foundations.md`).
- Char-versus-byte offsets: `Marker::index` is a char index. Getting this wrong breaks carets on non-ASCII frontmatter, so include a test with a non-ASCII title before the error.
- Using Q-1-1 for the first time may trigger the error-docs lints.
- Nothing outside `meta.rs` matches the text "Failed to parse YAML frontmatter" (grepped crates, hub-client/src and ts-packages), so changing the title breaks no known snapshot.
