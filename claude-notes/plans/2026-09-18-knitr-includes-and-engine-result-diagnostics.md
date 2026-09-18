# knitr HTML dependencies fail the render; engine-result errors need real diagnostics (GH #683)

**Strand:** bd-gy2ozix3
**Branch:** `braid/bd-gy2ozix3-knitr-includes-engine-diagnostics`
**Phase C strand:** bd-yd94iyq9 (separate; decided 2026-09-18)
**GitHub:** https://github.com/quarto-dev/q2/issues/683
**Date:** 2026-09-18
**Status:** Phases A and B complete and committed 2026-09-18; awaiting push approval. Phase C is bd-yd94iyq9.

## Overview

`q2 render` fails on any knitr document whose R output attaches an HTML
dependency. The reporter saw it with `reactable` and `DT` (both are
htmlwidgets); `gt` "works" only because it inlines its CSS and attaches
no dependency. The failure is not in the markdown knitr produces — that
markdown is well-formed, exactly as the reporter's Q1 `keep-md` output
shows. It is a **wire-shape mismatch between q2's own R script and q2's
own Rust struct**, and the error message that mismatch produces is the
second, larger problem this plan addresses.

Two deliverables:

1. **Phase A — fix the bug.** Make `KnitrIncludes` accept what
   `execute.R` actually sends (an array of paths per include slot,
   which is also Quarto 1's declared type).
2. **Phase B — make engine-result failures a real diagnostic.** A
   result q2 cannot read should produce a coded, documented diagnostic
   that names the engine, the offending field, and a preserved copy of
   the raw result to attach to a bug report — instead of an uncoded
   `Error:` with 500 bytes of JSON.

Phase C (mapping the *rest* of `ExecutionError` onto coded diagnostics
with source locations) is scoped below as candidate follow-up work; the
recommendation is to file it as its own strand.

## Reproduction

```bash
cargo run --bin q2 -- render q2-issue-683.qmd
```

```
---
format: html
engine: knitr
---

```{r}
#| eval: true
reactable::reactable(mtcars[1:2, 1:2])
```
```

Observed (2026-09-18, `main` at a9475a57):

```
processing file: q2-issue-683.rmarkdown
output file: q2-issue-683.knit.md

error: while rendering .../q2-issue-683.qmd
Error: Failed to parse R results: invalid type: sequence, expected path string at line 1 column 5752
JSON: {"engine":"knitr","markdown":"---\nengine: knitr\nformat: html\nquarto:\n  language:\n    appendix-attribution-bibtex: ...
1 error
```

A widget-free fixture reproduces it identically (only `htmltools`,
which `rmarkdown` already requires — this is the regression-test
fixture in Phase 0):

```
---
format: html
engine: knitr
---

```{r}
htmltools::attachDependencies(
  htmltools::div("hi"),
  htmltools::htmlDependency("q2dep", "1.0", src = "deps", script = "q2dep.js")
)
```
```

with `deps/q2dep.js` next to the document. (An `href`-only dependency
does **not** work as a fixture: rmarkdown rejects it with `Dependency
q2dep 1.0 is not disk-based` — which incidentally surfaces as the
equally generic `Error: Execution failed in knitr: R process failed`.)

## Diagnosis

### Root cause: `include-in-header` is an array on the wire, a single path in Rust

The R side, `crates/quarto-core/src/engine/knitr/resources/rmd/execute.R:593-607`
(`create_pandoc_includes`), writes each include slot to a temp file and
stores the path wrapped in `I()`:

```r
pandoc[[to]] <<- I(path)
```

`rmd.R:291` serializes the result with `jsonlite::toJSON(auto_unbox = TRUE, result)`.
`I()` marks the value `AsIs`, which is precisely the jsonlite idiom for
"do not unbox this" — so the slot is **always** emitted as a
one-element array. This is deliberate and matches Quarto 1's declared
type, `external-sources/quarto-cli/src/execute/types.ts:197`:

```ts
[kIncludeInHeader]?: string[];
```

The raw results file captured from the failing render (markdown field
elided) confirms it:

```json
{
  "engine": "knitr",
  "supporting": [".../q2-issue-683_files"],
  "filters": ["rmarkdown/pagebreak.lua"],
  "includes": {
    "include-in-header": ["/var/folders/.../quarto-pipeline_46gJNU/file65026e45f832"]
  },
  "engineDependencies": {},
  "preserve": {},
  "postProcess": true
}
```

and that include file is the htmlwidgets dependency block:

```html
<script src="q2-issue-683_files/core-js-2.5.3/shim.min.js"></script>
<script src="q2-issue-683_files/react-18.2.0/react.min.js"></script>
...
<link href="q2-issue-683_files/reactable-0.4.5/reactable.css" rel="stylesheet" />
<script src="q2-issue-683_files/reactable-binding-0.4.5/reactable.js"></script>
```

The Rust side, `crates/quarto-core/src/engine/knitr/types.rs:142-155`,
declares every slot as a single path:

```rust
pub struct KnitrIncludes {
    pub include_in_header: Option<PathBuf>,
    pub include_before_body: Option<PathBuf>,
    pub include_after_body: Option<PathBuf>,
}
```

So the first document that ever produces an include fails
deserialization. The struct dates from the initial engine work
(`748856f5`, 2026-01-07); the module doc comment at `types.rs:39` and
the unit tests at `types.rs:327-362` all encode the assumed shape
(`"include-in-header": "/tmp/header.html"`), which R never sends — a
textbook case of tests verifying the contract the author had in mind
rather than the one the engine relies on. The only real-R coverage in
the tree (`knitr_display_fence.rs`, `knitr_inline_expressions.rs`,
`nested_cell_mask_render.rs`) uses fixtures without dependencies, so
the mismatch never fired in CI.

`convert_includes` (`knitr/mod.rs:330-358`) then reads each path into
`PandocIncludes.header_includes: Vec<String>` — already a vector, so
the downstream consumer needs no change beyond iterating.

### Experiment: the fix is sufficient for the widget to render

A throwaway local patch that unwrapped one-element arrays in
`deserialize_includes` (reverted; not committed) rendered the repro
cleanly:

```
$ cargo run -q --bin q2 -- render q2-issue-683.qmd
$ grep -o 'reactable[^"]*\.js\|class="reactable[^"]*"' q2-issue-683.html
reactable-binding-0.4.5/reactable.js
class="reactable html-widget html-fill-item"
$ ls q2-issue-683_files
bootstrap.bundle.min.js  core-js-2.5.3  htmltools-fill-0.5.9  htmlwidgets-1.6.4
react-18.2.0  reactable-0.4.5  reactable-binding-0.4.5  reactwidget-2.0.0  ...
```

The `<script src="q2-issue-683_files/...">` references resolve against
the `supporting` directory that the existing resource-report path
already copies. The rendered HTML was inspected via grep and directory
listing, not in a browser; Phase A's end-to-end step opens it.

### The error message: five separate deficiencies

Path of the failure today:

1. `knitr/subprocess.rs:427-433` — `serde_json::from_str` fails; the
   error is wrapped as `ExecutionError::other("Failed to parse R results: {e}\nJSON: {first 500 bytes}")`.
2. `stage/stages/engine_execution.rs:511` — `.map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))`:
   the variant is flattened to a string.
3. `stage/error.rs:122-127` — `stage_error` builds `DiagnosticMessage::error(message)` with **no code**.
4. `quarto/src/commands/render.rs:1421-1426` — prints `error: while rendering <path>` and the message.

Deficiencies:

- **No code, no docs page, no hint.** Every other user-facing failure
  in q2 carries a `Q-*` code with a page under `docs/errors/`; engine
  failures carry nothing. There is no `engine` subsystem in
  `crates/quarto-error-catalog/error_catalog.json` at all (the two
  engine-adjacent codes, `Q-2-40` and `Q-16-10..12`, live under
  `markdown` and `extension`).
- **The evidence shown is the least useful field.** `truncate_for_error`
  dumps the first 500 bytes of the results file, and `markdown` is the
  first (and by far largest) field — so the user sees the merged
  front-matter language table and never the `includes` object at
  column 5752 that serde is complaining about. serde reports only a
  line/column, not a field path.
- **The evidence is destroyed.** The results file is a `NamedTempFile`
  and is deleted when `call_r` returns. The enclosing pipeline temp
  dir (`stage/context.rs:389`, created with `.into_path()`) actually
  *survives* the render, so keeping the file is a one-line `.keep()`
  — but today nothing does, and the user cannot attach it to a report.
- **Latent panic.** `truncate_for_error` does `&s[..max_len]` on bytes;
  if byte 500 falls inside a multi-byte character (the merged metadata
  routinely contains curly quotes and non-ASCII language strings) the
  error path itself panics.
- **`--quiet` swallows R's stderr on this path.** With `quiet: true`
  stderr is piped and only consulted on non-zero exit
  (`subprocess.rs:397-408`). A parse failure after a *successful* R
  exit reports nothing R printed.

The message also misattributes: it reads as though the user's document
produced bad output, when the problem is entirely between q2's R script
and q2's Rust struct. The diagnostic should say so, and say what to do
(report it, attach the preserved file).

### Related, not in scope

- **bd-5oyk1xce** — `q2 preview` drops engine `include-in-header`.
  After Phase A, `q2 render` hydrates widgets; `q2 preview` still won't
  until that strand lands. Link as `related`.
- **bd-14rer** — `ExecuteResult.filters` from knitr is never consumed
  (`rmarkdown/pagebreak.lua` in the capture above). Unchanged here.
- **bd-8hrjqcx0** — engine source-map drift; relevant to Phase C's
  "point `Quitting from lines N-M` at the user's file", not to A/B.

## Design decisions (all approved 2026-09-18)

1. **New `engine` subsystem (`Q-18`).** There is no home for engine
   diagnostics today. Reusing `internal` (`Q-0`) for the protocol error
   would be defensible for Phase B alone, but Phase C's codes (runtime
   missing, package missing, cell failed at lines) are user-facing and
   engine-specific, so I recommend opening the subsystem now:
   `docs/errors/engine/` + a `- section: "engine"` block in the errors
   sidebar (both lints require catalog entry, page, and sidebar entry
   in the same commit).
2. **Accept both shapes.** `KnitrIncludes` slots become `Vec<PathBuf>`
   deserialized from either a string or an array (Q1's TS is `string[]`,
   R always sends an array; accepting a bare string costs nothing and
   keeps the tests' single-path fixtures meaningful).
3. **Preserve the raw result on parse failure only.** `NamedTempFile::keep()`
   in the error branch; the diagnostic names the path. No new
   directory convention; the pipeline temp dir already outlives the
   render. (Whether that dir *should* outlive the render is a separate
   question — noted, not touched.)
4. **Use `serde_path_to_error` for the field path.** Already in
   `Cargo.lock` (0.1.20) as a transitive dep; add it as a direct dep of
   `quarto-core`. Turns `invalid type: sequence, expected path string at
   line 1 column 5752` into `includes.include-in-header: invalid type:
   sequence, expected path string`. **Follow-up filed as bd-rfug8fxo:** a
   `quarto-json` crate analogous to `quarto-yaml`, so JSON diagnostics
   can eventually carry a source-mapped (intermediate-file) location.
   Out of scope here — too large a change for one internal error.
5. **Drop the JSON dump from the message entirely.** The preserved file
   replaces it; this also removes the `truncate_for_error` panic.
6. **Missing include file becomes a warning.** `convert_includes`
   currently `if let Ok(content)`-ignores an unreadable include path,
   which would silently ship a widget with no dependencies. Propose a
   `Q-18-2` warning ("engine named an include file Quarto could not
   read"). Small, but it closes the next silent failure in the same
   spot. Approved — in scope for Phase A/B.

## Proposed diagnostic (Phase B)

```
error: while rendering /path/to/doc.qmd
Error (Q-18-1): knitr returned a result Quarto could not read
  ✖ includes.include-in-header: invalid type: sequence, expected path string
  ℹ The raw result was preserved at /var/folders/.../quarto-pipeline_XXXX/fileYYYY
  ℹ This is a bug in Quarto's knitr integration, not in your document.
    Please report it at https://github.com/quarto-dev/q2/issues and attach the file above.
  → https://quarto.org/docs/errors/engine/Q-18-1
```

(Exact glyphs/layout follow whatever `DiagnosticMessageBuilder` already
renders; the content is what matters.)

## Work items

### Phase 0 — tests first (all must fail before Phase A/B code)

- [x] **T1** `types.rs` unit test: the verbatim captured results JSON
      (markdown elided) deserializes; `include_in_header == vec![<path>]`.
      Fails today with `invalid type: sequence`.
- [x] **T2** `types.rs` unit tests: a slot given as a bare string, as a
      two-element array, and as `[]`; top-level `includes: []` and
      `includes: {}` still yield `None`. Rewrite the existing tests at
      `types.rs:327-362` and the module doc at `:39` to the real shape.
- [x] **T3** `knitr/mod.rs` unit test: `convert_includes` with two
      header paths yields two `header_includes` entries in order.
- [x] **T4** New R-gated e2e test
      `crates/quarto-core/tests/integration/knitr_html_dependency.rs`
      (register in `main.rs`, alphabetized; gate copied from
      `nested_cell_mask_render.rs:63-85`): render the htmltools fixture
      through `render_document_to_file`; assert the render succeeds, the
      HTML contains `<script src="htmldep_files/q2dep-1.0/q2dep.js">`
      (confirm the exact `_files/<name>-<version>/` layout against real
      output when writing the test), and that file exists on disk.
      Fails today with the parse error.
- [x] **T5** Phase B unit test on the new parse helper (see A/B below):
      malformed JSON → `ExecutionError::MalformedResult` carrying engine
      name, field path `includes.include-in-header`, serde message, and
      the preserved path; the preserved file exists and equals the input.
- [x] **T6** Phase B diagnostic test: the `ExecutionError → DiagnosticMessage`
      conversion yields code `Q-18-1`, error severity, the field path in
      the problem line, the preserved path in a detail, the report hint.
- [x] **T7** `convert_includes` with an unreadable path returns a
      `Q-18-2` warning and drops the entry (plus: readable siblings in
      the same slot still land, one warning per bad file).
- [x] **T9** (added) `engine_execution.rs`: warnings on
      `ExecuteResult::warnings` drain into `ctx.diagnostics` exactly once.
- [x] **T8** Moot: `truncate_for_error` and its test were deleted with
      the JSON dump (decision 5); no byte-slicing of the results file
      remains.

### Phase A — fix the wire type

- [x] `KnitrIncludes` slots → `Vec<PathBuf>` with a
      `deserialize_string_or_seq` helper; `deserialize_includes` keeps
      the `[]`/`{}`/`null` → `None` handling and returns `None` when all
      slots are empty.
- [ ] `convert_includes` iterates each slot (and, per decision 6,
      collects warnings — signature grows a `&mut Vec<DiagnosticMessage>`
      or returns them; pick whichever the stage's existing
      `ctx.add_diagnostics` seam prefers).
- [x] Update the module doc example in `types.rs`.
- [x] Run T1–T4 green (358 knitr/include-filtered tests, both R-gated e2e tests ran on this machine); workspace gate via `cargo xtask verify --skip-hub-build`.
- [x] **End-to-end (2026-09-18):**
      `cargo run --bin q2 -- render <scratch>/repro/q2-issue-683.qmd`
      exits 0 with only knitr's `processing file:` / `output file:` lines.
      Output inspected: the HTML carries
      `<script src="q2-issue-683_files/reactable-binding-0.4.5/reactable.js">`,
      `<div class="reactable html-widget html-fill-item">`, and the
      `application/json` payload; `q2-issue-683_files/` holds
      `core-js-2.5.3 htmltools-fill-0.5.9 htmlwidgets-1.6.4 react-18.2.0
      reactable-0.4.5 reactable-binding-0.4.5 reactwidget-2.0.0`. Opened
      in Chrome via file://: the widget hydrated — header cells
      `["", "mpg", "cyl"]`, body cells `Mazda RX4 / 21 / 6`,
      `Mazda RX4 Wag / 21 / 6`, three sortable column headers, no JS
      errors; screenshot showed the rendered table.

### Phase B — malformed-result diagnostic

- [x] Added `ExecutionError::MalformedResult { engine, field_path: String, detail, preserved: Option<PathBuf> }`
      (`field_path` is never absent: `serde_path_to_error` renders the
      root as `.`).
- [x] `call_r` ends in `parse_results::<R>(results_file, "knitr")`:
      `serde_path_to_error` names the field (down to the element index,
      e.g. `includes.include-in-header[0]`), `NamedTempFile::keep()` on
      failure, trailing-garbage caught via `Deserializer::end()`.
      `truncate_for_error` deleted.
- [x] `serde_path_to_error = "0.1.20"` as a workspace dep, used by `quarto-core`.
- [x] Catalog entries `Q-18-1` and `Q-18-2` (`engine` subsystem,
      `99.9.9`); pages `docs/errors/engine/Q-18-{1,2}.qmd` (status
      `stub`); `- section: "engine"` appended to the errors sidebar.
- [x] New module `crates/quarto-core/src/engine/diagnostics.rs` with
      `engine_error_diagnostic` (the seam; `MalformedResult` → `Q-18-1`,
      everything else falls through to `DiagnosticMessage::error(e.to_string())`)
      and `unreadable_include_diagnostic` (`Q-18-2`). The stage's
      `map_err` now goes through the seam. `ExecuteResult` gained
      `warnings: Vec<DiagnosticMessage>` (`#[serde(default)]`, so stored
      captures still load; a replayed trace re-raises them), drained into
      `ctx.diagnostics` right after the capture emit.
- [x] Full `cargo xtask verify` green (2026-09-18): lints + clippy,
      workspace build, Rust tests, ts-packages, WASM + hub-client build
      and tests. (Local prerequisites that bit along the way: the shell's
      Node must come from fnm, and a stale `node_modules`/WASM artifact
      after main's automerge upgrade needed `npm install` + the full
      hub-build leg.)
- [x] **End-to-end (2026-09-18), via throwaway local patches, reverted,
      not committed** — neither code can be reached from a real document
      any more, which is the point. `cargo run --bin q2 -- render <repro>`:

      Patch A (slot visitor rejects arrays) → exit 1:
      ```
      error: while rendering <scratch>/repro/q2-issue-683.qmd
      Error [Q-18-1]: Engine Returned an Unreadable Result
      the `knitr` engine ran, but the result it returned is not in the shape Quarto expects — at `includes.include-in-header`: invalid type: sequence, expected a path string or a list of path strings at line 1 column 5789.
      ✖ The engine's raw result was preserved at /var/folders/.../quarto-pipeline_De6x8V/.tmpTOm78b.
      ℹ This is a bug in Quarto's integration with the `knitr` engine, not a problem in your document. Please report it at https://github.com/quarto-dev/q2/issues, including this message and the preserved file.
      1 error
      ```
      The preserved file existed and began with the engine's JSON.

      Patch B (a nonexistent path injected into `include-in-header`) → exit 0,
      widget still rendered:
      ```
      Warning [Q-18-2]: Engine Include File Could Not Be Read
      the `knitr` engine asked for the contents of /nonexistent/q2-dep-header.html to be placed in `include-in-header`, but the file could not be read: No such file or directory (os error 2).
      ✖ The page was rendered without it, so content the engine expected there (typically an HTML dependency's scripts and stylesheets) is missing.
      ℹ Re-run the render; if it recurs, check that the path exists and is readable, or report it at https://github.com/quarto-dev/q2/issues. Use `--strict` to make this stop the render.
      1 warning
      ```
      With `--strict` the same run exits 1 (`1 error`).

### Wrap-up

- [x] `braid dep add bd-gy2ozix3 bd-5oyk1xce --type related`.
- [x] File the `quarto-json` follow-up (bd-rfug8fxo, `discovered-from`).
- [x] Phase C filed as its own strand (bd-yd94iyq9).
- [x] Phase A committed (7b456302); Phase B committed (see git log).
- [ ] Push + PR — awaiting explicit approval.

## Phase C — candidate follow-up: the rest of `ExecutionError`

Everything below goes through the same `e.to_string()` flattening at
`engine_execution.rs:511` today. With Phase B's seam in place, each is
a match arm plus a catalog entry plus a page:

| Variant | Today | Proposed |
| --- | --- | --- |
| `RuntimeNotFound` | `Engine runtime not found: knitr requires Rscript (install R from …)` | `Q-18-3`, hint with install URL and `QUARTO_R` |
| `MissingPackage` / `PackageVersionTooOld` | `Missing package: knitr` (the `suggestion` field is dropped) | `Q-18-4`/`Q-18-5`, suggestion becomes the hint |
| `ExecutionFailedAtLines` | `Execution failed in knitr at lines 6-8: …` — lines are knitr's lines in the `.rmarkdown` intermediate, not the user's file | `Q-18-6` **with a source location on the `.qmd`**, via the engine source map (Q1 does this in `rmd.ts:300-315`); the highest-value item, and it touches bd-8hrjqcx0 |
| `ExecutionFailed` (generic `R process failed`) | The actual R error is only visible because stderr is inherited; under `--quiet` it is lost | `Q-18-7`: first `Error in …` line from `parse_r_error` as the problem, stderr tail as detail |
| `ProcessCrashed` / `Timeout` | uncoded | `Q-18-8`/`Q-18-9` |

Recommendation: keep Phase C out of this strand. A and B are a
contained bug fix plus the seam; C is a design pass over engine
diagnostics as a whole (source mapping, quiet-mode capture, and how
much R stderr to echo) and deserves its own plan. If you'd rather do C
here, the seam from Phase B is where it plugs in and the table above is
the checklist.
