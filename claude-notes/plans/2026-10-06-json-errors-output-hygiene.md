# `q2 render --json-errors`: pure, attributable NDJSON on stderr

**Strand:** bd-gnw9asuo (bug, P2). Child: bd-ckbqmupi. Discovered from bd-uk8zgkha (claude-notes website).
**Branch:** `braid/bd-gnw9asuo-q2-render-json-errors`, off `origin/main` @ `7f70632cc`.
**Status:** APPROVED 2026-10-06 (D1=a, D2=yes, D3=absolute, D4=dropped; R1–R6 deferred). Executing.

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

## Decisions (resolved 2026-10-06)

Resolutions: **D1 → (a) only**; no summary record (users can derive one with jq).
**D2 → yes**; afterwards, re-check claude-notes for remaining records with no `source_file`
and file follow-up strands as appropriate. **D3 → absolute.** **D4 → dropped**: keep JSON
complete and uncoalesced; downstream tooling can coalesce. **R1–R6: no action in this strand.**

Original options, kept for the record:

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

## Audit findings during execution (2026-10-06)

The Phase 1 audit found more non-JSON writers than the two status lines. These are in scope
because they are the same defect (non-JSON text on the `--json-errors` stream, emitted by the
render command itself):

- **CLI-emitted diagnostics printed as ariadne text unconditionally.** Q-20-8 (multi-format
  `format:` reduced to one; `render.rs` `render_once`) and the project-config diagnostics
  (Q-5-11 `pre_render` typo, project-kind diagnostics, `config_diagnostics`; `render_project`)
  call `eprintln!("{}", diagnostic.to_text(None))` regardless of `--json-errors`.
- **`quiet` flows further than `user_status!`.** `RenderArgs.quiet` also feeds
  `RenderToFileOptions.quiet` (engine progress output) and the render-script context
  (`"Running pre-render script: …"`, plus a script's inherited stdout/stderr). Under
  `--json-errors` all of these should behave as under `--quiet`.
- **More located records without a file.** `emit_parse_error_json` is called with
  `input: None` for discovery and render-script errors, so e.g. Q-5-17 (unknown
  `project.type`, located at `_quarto.yml:2:9`) has no `source_file` either.

**Attribution rule (refines D2/D3).** `source_file` names *the file `start_line`/`start_column`
refer to*. Derive it from the diagnostic's own location, resolved through the same
`SourceContext` that produced the coordinates:
1. location resolves into a virtual file with a `FileOrigin::NotebookCell` origin → its
   `notebook_path` (the documented "file reported in structured output"; `origin` carries the cell);
2. location resolves into a file that exists on disk → that path;
3. location resolves but the file is not on disk → no `source_file` (honest; counted in the re-check);
4. no location, or it does not resolve → the caller's known file (page input, config path), if any.
All emitted paths are made absolute (`std::path::absolute`, which stays in plain form on Windows).
This mirrors `quarto-error-reporting`'s own `hyperlink_target` rule for the text path. It lives in
q2 because the crate's `diagnostic_to_json` leaves attribution to callers by design.
Rule 1 matters: page renders produce diagnostics whose coordinates are in `_quarto.yml`
(e.g. raw HTML in `website.page-footer`, Q-2-9). Tagging those with the page path would make the
record point at the wrong file.

**Out of scope, to file as follow-ups** (non-JSON writers outside the CLI's emission layer):
- `tracing::warn!` from `quarto*` crates prints at the default filter (`quarto=warn`) as plain text.
- pampa `eprintln!`s: `mediabag.fetch failed …` (`lua/mediabag.rs:190`), "shouldn't happen"
  parser warnings (`treesitter.rs:1221`, `language_specifier.rs:125`).
- `quarto.log.output` from user Lua filters (`lua/quarto_api.rs:74`). This is user content, so it is
  arguably correct as-is.
- A failed render script's captured stderr is replayed raw before its Q-5-10 diagnostic.
- `render_diagnostic_guarded` / `emit_json_line` internal-failure fallbacks print plain text.
- `_quarto.yml` YAML *syntax* errors at discovery surface as span-less Q-7-8
  (`DispatchError::Discover(String)`), losing their location in every mode.

## Checklist

### Phase 0: tests first (TDD)
- [x] In `crates/quarto/tests/integration/json_errors.rs`: add a project-render test with one
      page that renders successfully but has a warning (e.g. a bare `[x]` → Q-2-49) and one page
      that fails pass 1. Assert the warning record has `source_file` ending in that page's name.
      → `project_page_warning_json_carries_source_file`
- [x] Add an assertion that **every** stderr line under `--json-errors` parses as JSON, for both
      project and single-file renders. Tighten (or add a strict sibling to) `parse_ndjson_lines`
      so it fails on non-JSON lines instead of skipping them.
      → `parse_ndjson_strict`; `project_/single_doc_json_errors_stderr_is_pure_ndjson`
- [x] (D2) Add a test for a `_quarto.yml`-anchored project diagnostic carrying `source_file`.
      → `project_config_diagnostic_json_carries_source_file` (Q-5-13)
- [x] Tests for the audit findings: `multi_format_warning_is_json_with_source_file` (Q-20-8),
      `config_typo_warning_is_json_with_source_file` (Q-5-11),
      `config_anchored_page_warning_names_config_file` (Q-2-9 from `page-footer`),
      `discovery_parse_error_json_carries_source_file` (Q-5-17),
      `render_scripts_cli::json_errors_keeps_script_output_off_stderr`.
- [x] Run the tests and confirm they fail for the expected reasons. 9/9 failed as predicted:
      5 on a non-JSON line (status lines, Q-20-8/Q-5-11 text, script chatter), 4 on a missing
      `source_file`. The original discovery fixture (YAML syntax error) turned out to produce a
      span-less Q-7-8, so it was retargeted to Q-5-17 and the gap was noted above.

### Phase 1: silence status lines in JSON mode
- [x] One `RenderArgs` predicate for the effective console-quiet (`quiet || json_errors`), used at
      every site `args.quiet` flows: the `user_status!` calls, `RenderToFileOptions.quiet`, and
      both render-script contexts. → `RenderArgs::console_quiet()`; the CLI help for
      `--json-errors` now says it implies `--quiet`.
- [x] Audit the render path for any other unconditional `eprintln!`/`user_status!` (see findings above).
- [x] Emit Q-20-8 and the project-config diagnostics as JSON under `--json-errors`.
      → `emit_cli_diagnostic` (text form unchanged; JSON form binds the config source context).

### Phase 2: `source_file` on per-page and project diagnostics
- [x] One helper implementing the attribution rule; route every JSON conversion in `render.rs`
      through it (pass-1 nested, pass-2, project, per-page, parse-error, CLI-emitted).
      → `diagnostic_json` / `json_source_file` / `wire_path`. No direct `diagnostic_to_json`
      call remains outside it.
- [x] Per-page loop: fall back to `result.input_path`. Delete the out-of-date comment.
- [x] Project loop: per D2 (falls out of the rule via the config source context).
- [x] Fix the out-of-date schema-path comments (`render.rs:82, 1928`). They point at the in-tree
      `crates/quarto-error-reporting/schemas/`, which is now external.

### Phase 3: verify
- [x] `cargo nextest run --workspace`: 15492/15493 passed. The one failure is `smoke_all`, and
      it is environmental: typst fixtures need knitr (R cannot load its cairo DLL on this machine)
      and the Python `great_tables` module (not installed). Neither involves `--json-errors`.
- [x] `cargo xtask verify --skip-hub-build --skip-rust-tests --skip-css-lint --skip-hub-tests`:
      steps 1–4 green (custom lints, clippy `-D warnings`, rustfmt, warnings-denied workspace
      build, tree-sitter). Step 6 (ts-packages) is green after an `npm install` in the worktree.
      Not verifiable here, and none of them touched by this Rust-CLI-only change:
      - `lint:css` fails on `hub-client/vscode-sync-experiment` (no `lint:css` script) with or
        without this branch's changes;
      - hub-client tests (step 8) and the shared preview-* tests (step 11) need the
        `wasm-quarto-hub-client` artifact, which `--skip-hub-build` does not produce.
      `crates/quarto` is not a dependency of the WASM client.
- [x] End-to-end on the real project (recorded below).
- [x] Re-check claude-notes for records still lacking `source_file`; file follow-up strands (D2).

#### End-to-end record (2026-10-06)

Invocation, from `claude-notes/` on `braid/bd-uk8zgkha-claude-notes-website`, using this
branch's debug binary:

```bash
.worktrees/bd-gnw9asuo-q2-render-json-errors/target/debug/q2 render --json-errors 2>err2.txt
```

Inspected output:

- exit 1 (290 pass-1 failures, as before). stderr has 713 lines, and `grep -vc '^{'` gives **0**
  (before: 715 lines, 2 of them non-JSON).
- Located top-level records without `source_file`: **0** (before: 422).
  Nested pass-1 diagnostics without it: 0. Non-absolute `source_file`: 0.
- The one record without `source_file` is Q-5-31 ("Skipped 7 nested projects"). It has no span
  and is project-scoped, so this is correct per D2.
- With `source_file` stripped, the records are identical to the nightly's (same 713, same
  content), so the change is purely additive on the wire.
- The record originally reported now reads:
  `{"code":"Q-2-49","source_file":"/Users/cscheid/rooms/room-4/q2/claude-notes/yaml-with-source-info-lifetime-approach.md","start_line":120,"start_column":223}`
