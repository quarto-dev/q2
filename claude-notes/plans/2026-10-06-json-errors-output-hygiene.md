# `q2 render --json-errors`: pure, attributable NDJSON on stderr

**Strand:** bd-gnw9asuo (bug, P2). Child: bd-ckbqmupi. Discovered from bd-uk8zgkha (claude-notes website).
**Branch:** `braid/bd-gnw9asuo-q2-render-json-errors`, off `origin/main` @ `7f70632cc`.
**Status:** DRAFT. Waiting for review before execution.

## Overview

`--json-errors` exists so tools like `jq` can filter and attribute diagnostics. Running
`q2 render --json-errors` inside `claude-notes/` (on branch `braid/bd-uk8zgkha-claude-notes-website`,
q2 `0.33.0-nightly.20261006`) shows the stream falls short in two ways:

1. **Non-JSON lines on stderr.** Two plain-text status lines are mixed into the NDJSON.
2. **Records with a location but no file.** No top-level `json-diagnostic` record carries
   `source_file`, so their `start_line`/`start_column` cannot be attributed to a file in a
   multi-file render.

## Reproduction (done 2026-10-06)

```bash
cd claude-notes            # on braid/bd-uk8zgkha-claude-notes-website
q2 render --json-errors >out.txt 2>err.txt   # exit 1, 4.7 s wall
```

- stdout: empty. stderr: 715 lines, of which 713 are JSON.
- The two non-JSON lines:
  ```
  Rendering project: /Users/cscheid/rooms/room-4/q2/claude-notes (type: website)
  Rendered 1146 of 1436 files to /Users/cscheid/rooms/room-4/q2/claude-notes/_site
  ```
- Records by schema: 290 `json-pass1-failure.json` and 423 `json-diagnostic.json`.
- **All 423 top-level `json-diagnostic` records lack `source_file`.** The issue is the whole
  record class, not one line. 422 of them have `start_line`/`start_column`. The only way to
  find the file is to scrape the OSC-8 hyperlink out of the ANSI `rendered` blob.
  The last line reported was Q-2-49 at `120:223`, which is really
  `yaml-with-source-info-lifetime-approach.md`.
  Breakdown: Q-2-49 ×203, Q-2-9 ×99, Q-16-3 ×51, Q-16-5 ×48, Q-2-45/46 ×4 each, Q-13-4 ×2,
  Q-0-99 ×2, Q-2-50 ×1, Q-5-6 ×1, Q-5-31 ×1 (no span, project-scope), uncoded ×7.
- The nested diagnostics inside pass-1 failure records *do* carry `source_file`, which is correct.

## Root causes (from code reading on `origin/main`)

- **Status lines.** `crates/quarto/src/commands/render.rs:1227` (`Rendering project: …`) and
  `:865` (`Rendered N of M …`, from `render_summary_line` at `:538`) go through
  `quarto_util::user_status!(args.quiet, …)`. That prints to stderr unless `--quiet` is set.
  JSON mode only drops the "— N errors, M warnings" suffix (`:859`). The same gate covers
  `:877` (the counts clause) and `:1046` (`Rendering single file: …`).
  The existing test helper `parse_ndjson_lines` (`tests/integration/json_errors.rs:63`)
  skips non-`{` lines, which is why no test caught this.
- **Missing `source_file`.** `print_render_diagnostics_json` (`render.rs:1938`) has four emission
  loops. The pass-1 and pass-2 failure loops wrap each record with `with_source_file(…)`.
  The **per-page diagnostics loop** (`:2028`) and the **project diagnostics loop** (`:2014`) do not.
  The comment on the per-page loop says `RenderToFileResult` "does not carry the input path".
  That is out of date: `input_path` was added in bd-mg3ckvp7 (`render_to_file.rs:144`), and the
  text branch already uses it (`render.rs:1788`).
  `quarto_error_reporting::json::diagnostic_to_json` always sets `source_file: None`
  (external crate 0.3.2, `json.rs:310`). Callers must tag the file.

## Decisions to make before execution

- **D1: status lines under `--json-errors`.** Options:
  (a) treat `--json-errors` as implying `--quiet` for status lines. This is the smallest change.
  (b) also emit one final structured summary record (e.g. a new `json-render-summary` schema
  with rendered/total/failed counts and output dir), so the information isn't lost.
  (b) needs a new schema in the external `quarto-error-reporting` crate, or a q2-local schema.
  **Recommendation: (a) now, and file (b) as a follow-up strand if wanted.**
- **D2: `source_file` for project diagnostics.** These are usually span-less (Q-5-31) or anchored in
  `_quarto.yml` (Q-5-13/14/15). Recommendation: when the diagnostic has a location, set
  `source_file` to the config file it resolves into. Leave it absent when there is no span.
- **D3: path form.** Pass-1 records use absolute paths. Keep per-page records consistent with
  that (absolute). Project-relative paths would be a separate, cross-cutting change.
- **D4: per-page coalescing / config-source repair.** The text branch runs `coalesce_by_source` and
  `attach_config_source` on per-page diagnostics. The JSON branch does neither. Recommendation:
  route the JSON per-page loop through the same preparation, so both modes report the same
  diagnostic set (a warning anchored in `_quarto.yml` should name `_quarto.yml`, not the page).
  This needs a check of what coalescing does to the per-file attribution.

## Broader review: other findings in the claude-notes stream

These are out of scope for the core fix. Each is listed so we can decide whether to fold it in
or file it separately.

- **R1. ANSI in `rendered` / pass-1 `error` (bd-ckbqmupi item 2).** Always ariadne-colored with
  OSC-8 links. Ariadne `Config::default()` has color on, and there is no plain option. Fixing it
  properly needs an option in external `quarto-error-reporting`. Note that
  `ipynb_diagnostic_hyperlinks_real_notebook` relies on the OSC-8 links. Also, the pass-1 `error`
  blob duplicates (a subset of) `diagnostics[].rendered`.
- **R2. Internal source paths leaking into titles.** Q-0-99:
  `"Failed to parse YAML frontmatter: Parse error: … (at crates/pampa/src/utils/diagnostic_collector.rs:45)"`.
  Q-0-99 is the generic/uncatalogued code, and its span covers the whole frontmatter
  (`2:1–8:1`, `103:1–291:1`), even though the YAML error has a precise line/column.
  This is a candidate for its own strand.
- **R3. Uncoded diagnostics.** 329 nested pass-1 "Parse error / unexpected character or token here"
  (no code). Top level: 5× "Missing shortcode argument" (`meta` with no key), 1× "Shortcode error",
  and 1× "YAML string scalar has no content provenance". The last one is an internal-invariant
  message (`quarto-yaml` provenance desync) surfacing as a user warning.
- **R4. Shortcode error leaks a temp path + Lua traceback** (`/var/folders/…` in `problem`).
- **R5. Q-13-4 problem text** `'plans/Q-X-Y'.qmd'` (the quote lands inside the path). This is a
  cosmetic formatting bug.
- **R6. Ordering.** All pass-1 failure records are emitted before every page warning, so per-file
  output is not grouped. Once `source_file` is present this matters less, because consumers can
  `group_by(.source_file)`.

## Checklist

### Phase 0: tests first (TDD)
- [ ] In `crates/quarto/tests/integration/json_errors.rs`: add a project-render test with one
      page that renders successfully but has a warning (e.g. a bare `[x]` → Q-2-49) and one page
      that fails pass 1. Assert the warning record has `source_file` ending in that page's name.
- [ ] Add an assertion that **every** stderr line under `--json-errors` parses as JSON, for both
      project and single-file renders. Tighten (or add a strict sibling to) `parse_ndjson_lines`
      so it fails on non-JSON lines instead of skipping them.
- [ ] (D2) Add a test for a `_quarto.yml`-anchored project diagnostic carrying `source_file`.
- [ ] Run the tests and confirm they fail for the expected reasons.

### Phase 1: silence status lines in JSON mode
- [ ] Gate `user_status!` calls at `render.rs:865, 877, 1046, 1227` on `args.quiet || args.json_errors`.
      Prefer one helper (e.g. `args.status_quiet()`) over repeating the expression.
- [ ] Audit the render path for any other unconditional `eprintln!`/`user_status!`: render scripts,
      engines, freeze, resources.

### Phase 2: `source_file` on per-page and project diagnostics
- [ ] Per-page loop: `with_source_file(…, result.input_path)`. Delete the out-of-date comment.
- [ ] Project loop: per D2.
- [ ] Per D4: share per-page preparation with the text branch.
- [ ] Fix the out-of-date schema-path comments (`render.rs:82, 1928`). They point at the in-tree
      `crates/quarto-error-reporting/schemas/`, which is now external.

### Phase 3: verify
- [ ] `cargo nextest run --workspace`, then `cargo xtask verify --skip-hub-build`.
- [ ] End-to-end: rebuild `q2`, re-run in `claude-notes/`, and check that
      `grep -vc '^{' err.txt` is 0 and that
      `jq -s '[.[] | select(."$schema"|test("json-diagnostic")) | select(.start_line and (.source_file|not))] | length'`
      is 0. Record the invocation and output here.
- [ ] Decide the fate of R1–R6 (fold in, or file strands linked `discovered-from:bd-gnw9asuo`).
