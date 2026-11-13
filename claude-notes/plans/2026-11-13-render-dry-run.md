# `q2 render --dry-run`: report Pass-1 diagnostics without rendering

**Strand:** bd-render-dry-run-dyiq7d08
**Status:** decisions recorded 2026-11-13; awaiting go-ahead to execute

## Goal

Add a flag to `q2 render` that runs the render only through Pass 1,
prints every problem found up to that point, and exits. It does not
execute engines, run Pass 2, or write output. CI jobs, pre-commit
hooks, and agents get a quick answer to "does every file in this
project parse, and does the project config hold together?" without
paying for engine execution or a full pipeline.

It cannot report what only Pass 2 finds: engine errors, Lua filter
errors, crossref resolution, template errors, unresolved links. The
help text and docs must say so.

## What the code does today (findings)

### Where Pass 1 lives

- `ProjectPipeline` in `crates/quarto-core/src/project/orchestrator.rs`
  drives both passes. Every CLI render goes through it, including
  single-file renders (`render_single_doc` uses `RenderMode::ActivePage`).
  So one seam covers single documents, projects, subsets, and books.
- `pass_one()` (orchestrator.rs:2500) dispatches each file through
  `pass1_profile_with_cache` → `pass1_profile_single_file_live`, which
  runs `build_head_stages()` (pipeline.rs:346):
  `SourceConversion → ParseDocument → MetadataMerge → LanguageResolve →
  IncludeExpansion → IncludeResolve → ListingItemInfo →
  DocumentProfile → LinkResolution`. Engines are not executed here.
- `run_head()` (orchestrator.rs:2060) wraps `pass_one` with the
  project-level checks that already run once per render: the empty
  render set, unmatched `project.render` patterns, nested projects,
  missing project `css:`, the Pass-1 engine-resolution warning, the
  fail-fast barrier, and then `pre_render`.
- `--fail-fast` already stops right after Pass 1 when Pass 1 fails
  (`RunHead::Stopped`, `stopped_early: true`). A dry run is close to
  "always stop at that barrier", except that it should also run
  `pre_render`. See below.

### Gap 1: Pass 1 drops warnings

`pass1_profile_single_file_live` throws away the non-fatal
diagnostics:

```rust
let (output, _diagnostics) =
    run_pipeline(source_bytes, &source_name, &mut ctx, runtime, stages).await?;
```

Only a hard `Err` becomes a `FileFailure` in `pass1_failures`. Parse
warnings, metadata or schema warnings from the merge, include
warnings, and similar diagnostics are lost. A normal render does not
notice this because Pass 2 runs the head stages again and reports
them there. A dry run has no Pass 2, so it must collect them.

### Gap 2: the profile cache skips parsing

`pass1_profile_with_cache` returns a cached `DocumentProfile` when the
key matches, so the file is never parsed. Cached profiles do not
store diagnostics. On a warm `.quarto/cache/profiles`, a dry run that
uses the cache would report nothing for unchanged files. That would
still be correct for hard errors, since a file that failed to parse
was never cached, but it would miss their warnings.

### Gap 3: book dispatch bypasses `run_head`

`run_with_book_support` sends books down several paths:
`run_book_scoped`, `run_book_single_file_merge`,
`run_book_multi_file_html` and `run_book_named_separately`. Some of
them call `pass_one()` directly instead of going through `run_head`,
which is why each one repeats the fail-fast early return. A dry run
must stop before any of them start Pass 2.

### Other side effects on the path to Pass 1

- **CLI, `render_project`**: `--clean-cache` wipes caches; `pre-render`
  scripts run and the project is discovered again. Neither belongs in a
  dry run by default (see Q3).
- **Pass 1**: saves the profile cache (`.quarto/cache/profiles/*`).
- **Source conversion** may start TS engine extension subprocesses
  (Node) to convert non-`.qmd` inputs (`.ipynb`, engine-claimed
  extensions). This is conversion, not execution, and a dry run needs
  it: an unconvertible file is a real Pass-1 error ("Can't determine
  execution engine"). `run_with_book_support` already calls
  `registry.shutdown_all()` at the end, so those subprocesses are
  cleaned up.
- **Book `pre_render`** (`book/project_type.rs:81`) works in memory.
  It builds the chapter list and reports Q-5-34, Q-5-35 and Q-5-36
  (nested part, missing chapter file, no home page) as hard errors.
  Those are exactly the project-structure errors a dry run should
  catch, so the dry run should run `pre_render`. Website projects use
  the default no-op.

### Naming precedent

`q2 add` / `q2 use brand` already have `--dry-run`, meaning "show what
would happen; write nothing" (bd-cjqhcaz6). `q2 check` exists but is
`NotImplemented`. In Quarto 1, `quarto check` checks the installation,
so reusing that name for this feature would confuse Q1 users.

## Proposed design

### CLI surface

```text
q2 render [inputs…] --dry-run
```

Help text (draft):

> Check the render without producing output: discover the project,
> resolve formats, parse every file and merge its metadata, then report
> all diagnostics and exit. Code is not executed and nothing is
> written. Problems that only a full render can find (engine, filter,
> template and cross-reference errors) are not reported.

What it does with the other flags:

| Flag | Behavior under `--dry-run` |
| --- | --- |
| `--to`, `--profile`, `-M`, `inputs` | Honored: they change format resolution and the metadata merge, which is what we are checking. |
| `--strict` | Honored: warnings become errors and the exit code is non-zero. |
| `--json-errors` | Honored: the same NDJSON records as a real render. |
| `--fail-fast` | Honored: Pass 1 stops at the first failure. |
| `--quiet` | Honored. |
| `--no-render-scripts` | Implied (see Q3). |
| `--execute` / `--no-execute`, `--execute-*`, `--cache*`, `--replay`, `--use-freezer`, `--attribution`, `-o`, `--output-dir`, `--no-clean`, `--debug`, pandoc args | Accepted and ignored, because nothing executes or writes. `-o` and `--output-dir` could be validated, but probably should not be. |
| `--clean-cache` | Rejected with a clap `conflicts_with`, since a dry run writes nothing (see Q2). |

Exit code: the same rule as a real render (`should_exit_nonzero`):
non-zero if there are any errors, or any warnings under `--strict`.

Summary line, replacing "Rendered N of M files to DIR":

```text
Checked 42 files (html): 2 errors, 3 warnings
```

Under `-v`, the run also lists which files would render and in which
format (decision 5). That list is what `--dry-run` means to most
users, and the orchestrator already computes it (the augmented render
set, and the book render list after `pre_render`). Under
`--json-errors`, the list goes in the stdout summary instead (see
"Machine-readable output").

### Orchestrator (`quarto-core`)

1. **`ProjectPipeline::with_dry_run(bool)`**, a builder like
   `with_fail_fast`, stored as a field.
2. **Collect Pass-1 diagnostics.** Let `pass1_profile_single_file_live`
   return `(DocumentProfile, Vec<DiagnosticMessage>, SourceContext)`
   instead of dropping them. `pass_one` returns them next to the
   profiles. Only the dry run reads them; a normal render discards
   them as it does today, because Pass 2 reports them again. Changing
   the internal return type is cheap: `pass_one` has four callers and
   `__pass_one_for_test_only` has one.
3. **Bypass the cache read under dry run.** Parse every file live, so
   every warning comes back. Keep or skip the cache save (see Q2).
   Parsing everything stays cheap next to a render: there is no
   engine, no filter and no pandoc.
4. **One early exit.** In `run_with_book_support`, after scope
   resolution and `decide_member_format` (FR6 and Q-5-33-style errors
   are checks we want), and before the book dispatch branches, divert
   to a new `run_dry(&mut self)`:
   - the `run_head` prologue (initial diagnostics), extracted into a
     helper so `run_head` and `run_dry` share it;
   - `pass_one` (live), the engine-resolution warning, `build_index`;
   - `pre_render`. A hook error becomes a summary diagnostic instead of
     an `Err` abort, so it is reported together with the per-file
     problems. `QuartoError::Parse` is already handled this way at the
     CLI boundary, but returning `Err` would lose the Pass-1 failures
     collected so far;
   - optionally `compute_augmented_render_set` to produce the
     would-render list;
   - `drain_registry_diagnostics()`, then return the summary.
   `run()` (non-book) gets the same branch. Both paths already call
   `registry.shutdown_all()`.
5. **Where Pass-1 warnings go.** Add to `ProjectRenderSummary`:
   ```rust
   /// Per-file diagnostics from Pass 1 for files that profiled
   /// successfully. Populated only by dry runs (a full render reports
   /// them through Pass 2).
   pub pass1_diagnostics: Vec<FileDiagnostics>,
   ```
   `FileDiagnostics { input, diagnostics, source_context }` has the
   same shape as `FileFailure` without `error`. Extend
   `diagnostic_counts()` and `promote_warnings_to_errors()` to cover
   it. Those are the two documented single seams, so `--strict` and
   the counts work with no other changes. Add a `dry_run: bool`, or
   an enum describing how far the run went, so the CLI can choose the
   summary wording.

The orchestrator changes are platform-agnostic, so the hub-client
could later use the same mode, for example as a "check project"
action. That is out of scope here.

### CLI (`crates/quarto`)

- `main.rs`: add the `--dry-run` arg (with `conflicts_with =
  "clean_cache"`) and thread it into `RenderArgs`.
- `commands/render.rs`:
  - `render_project`: skip `run_clean_cache` and `pre-render` scripts,
    and skip `post-render` scripts.
  - Both `render_single_doc` and `render_project`: call
    `.with_dry_run(args.dry_run)`.
  - `print_render_diagnostics` (text) and
    `print_render_diagnostics_json`: print `pass1_diagnostics` with the
    same coalescing and source-context handling as per-output
    diagnostics.
  - `present_report`: the "Checked N files" summary (text, on stderr)
    or the stdout JSON summary object (under `--json-errors`).
  - The would-render list: text under `-v`, or the JSON `files` key.
- Single-file renders (no `_quarto.yml`) currently print nothing as a
  summary. A dry run should always print one line, so that a clean
  result reads as clean and not as silence.

## Decisions (2026-11-13)

All of the original recommendations were accepted:

1. **Name: `--dry-run`.** This matches `q2 add --dry-run`. `--check` is
   not used because it collides with Q1 `quarto check`.
2. **Write nothing.** A dry run neither reads nor saves the Pass-1
   profile cache, so no `.quarto/` writes happen. It is safe on
   read-only checkouts. `--clean-cache` conflicts with `--dry-run`.
3. **`pre-render` and `post-render` scripts never run.** If a script
   generates inputs on every run, a faithful dry run is impossible
   anyway. The docs say this.
4. **`pre_render` hook errors** (book chapter list: Q-5-34/35/36) are
   reported as diagnostics next to the Pass-1 results and do not
   abort the run.
5. **The would-render list appears only under `-v`.** Dry runs are
   expected on large projects, where the list would be long.
6. **v1 stops at the Pass-1 / `pre_render` boundary.** Cheap
   engine-free Pass-2 stages are a possible follow-up.
7. **Machine-readable output must work well.** See the next section.

## Machine-readable output (`--dry-run --json-errors`)

### What `--json-errors` does today

- Stderr carries only NDJSON. Each line is one of the two wire shapes
  from the external `quarto-error-reporting` crate (0.4.1, schemas
  versioned under `https://quarto.org/schemas/v1/`):
  - `JsonDiagnostic` (`kind`, `title`, `code`, `problem`, `hints`,
    `start_line` / `start_column` / `end_line` / `end_column`,
    `source_file`, …): one per diagnostic;
  - `JsonPass1Failure` (`source_file`, `error`, `diagnostics[]`): one
    per file that failed Pass 1.
- `--json-errors` implies `--quiet`, so no status lines are printed.
  `project_json_errors_stderr_is_pure_ndjson` and
  `single_doc_json_errors_stderr_is_pure_ndjson` in
  `crates/quarto/tests/integration/json_errors.rs` enforce this.
- **No record marks the end of the run.** A consumer only has the exit
  code. It cannot tell "checked 500 files, found nothing" apart from
  "checked 0 files", and it cannot learn how many files were checked or
  whether `--fail-fast` cut the run short. For a real render the output
  files fill this gap. A dry run produces no files, so the gap is the
  main obstacle for automation.

### Design

1. **Diagnostics keep the real-render wire shapes.** A file that fails
   Pass 1 produces the same `JsonPass1Failure` record a real render
   would. Pass-1 warnings on files that parse successfully produce
   `JsonDiagnostic` lines attributed with `source_file`, just like the
   per-page diagnostics of a real render. `pre_render` and project
   diagnostics go through the existing project-diagnostics loop. A
   consumer that already parses `--json-errors` needs no changes to
   read dry-run diagnostics, and the dry run adds no new diagnostic
   records.
2. **One summary object on stdout at the end.** A dry run writes no
   render output, so stdout is free. Keeping the summary off stderr
   leaves the "stderr is pure diagnostics NDJSON" contract and both
   existing schemas unchanged. Draft shape:
   ```json
   {
     "dry_run": true,
     "project_dir": "/abs/path",
     "format": "html",
     "files_checked": 42,
     "files_failed": 2,
     "errors": 2,
     "warnings": 3,
     "strict": false,
     "stopped_early": false,
     "files": [
       { "source_file": "/abs/path/index.qmd", "format": "html", "status": "ok" },
       { "source_file": "/abs/path/broken.qmd", "format": "html", "status": "failed" }
     ]
   }
   ```
   - `errors` and `warnings` come from `diagnostic_counts()` after
     `--strict` promotion, so they agree with the stderr records and
     the exit code.
   - Paths use `wire_path`: absolute and `/`-normalized, like
     `source_file` in the diagnostics (`cross-platform.md`, wire
     format).
   - `files` (the would-render list) is present only under `-v`,
     matching the text mode. Without `-v` the key is omitted, not
     set to `[]`, so "not requested" cannot be confused with "nothing
     would render".
   - Pretty-printed or a single line? Use one line, so `jq` and
     line-oriented consumers both work.
3. **Text mode does not touch stdout.** The "Checked N files" line and
   the `-v` list go to stderr like the other status output. Stdout is
   used only for the JSON summary.
4. **Schema home.** The summary describes `q2 render` and is not a
   generic diagnostic, so it should live in q2 rather than in
   `quarto-error-reporting`. Define a `serde` + `schemars` struct in
   `crates/quarto` with a `$schema` URL in the same versioned style
   (for example `…/v1/render-dry-run-summary.json`), and check in the
   generated JSON Schema so consumers can rely on the contract. The
   exact location (a q2 `schemas/` directory, or next to the docs) is
   decided when we build it.

### Known caveat: `-v` and `--json-errors` together

`-v` raises the `tracing` filter, and tracing logs to stderr, so
`--json-errors -v` already breaks the pure-NDJSON guarantee on today's
render path. For dry runs, the would-render list goes in the stdout
JSON summary and never through `tracing::info!`, so the dry run itself
adds no non-JSON stderr. Two possible fixes for the general problem,
to choose while implementing:
(a) under `--json-errors`, keep the tracing filter at its default even
with `-v`, and use `-v` only as the "include `files`" switch; or
(b) document that `-v` logs can mix in, and that consumers should
filter for lines that start with `{`.
Recommendation: (a), but only on the dry-run path so that real-render
behavior does not change in this strand.

## Work items

1. Orchestrator: `with_dry_run`, live-parse cache bypass, Pass-1
   diagnostics plumbing, `FileDiagnostics` and
   `ProjectRenderSummary::pass1_diagnostics`, `run_dry`, and the
   counts and promotion extension. Unit tests in `orchestrator.rs` for
   counting and promotion over the new field.
2. CLI: the flag, script and cache skips, text and JSON printing, the
   summary line, and help text.
3. JSON summary: the `serde` + `schemars` struct in `crates/quarto`,
   printed to stdout under `--json-errors`, the checked-in JSON Schema,
   and the `-v` handling under `--json-errors` (caveat fix (a)).
4. Integration tests in
   `crates/quarto/tests/integration/render_dry_run_cli.rs`, registered
   in `tests/integration/main.rs`:
   - project with one broken file: exit 1, the diagnostic is reported,
     no output dir is created, `.quarto/` is untouched;
   - a warning-only file: exit 0 and the warning is printed; with
     `--strict`, exit 1;
   - a document with a `{python}` cell and no Python available: clean
     dry run (proves no execution);
   - a warm profile cache still reports the warnings (proves the
     bypass);
   - a book with a missing chapter file: Q-5-35 is reported;
   - `--json-errors` (put these in `json_errors.rs`, which already has
     the NDJSON helpers, or in a sibling file that reuses them):
     - stderr is pure NDJSON (`parse_ndjson_strict`), both for a
       project and for a single document;
     - **parity:** for the same broken project, the `JsonPass1Failure`
       records from `--dry-run` equal those from a real render, apart
       from Pass-2-only records;
     - Pass-1 warnings on a successful file appear as `JsonDiagnostic`
       lines with `source_file`;
     - stdout holds exactly one JSON object; `errors` and `warnings`
       equal the counts of `kind` on stderr; `files_checked` is right;
       `files` is absent without `-v` and present with `-v`;
     - `--strict` changes both the stderr `kind`s and the summary
       counts;
     - `--fail-fast` sets `"stopped_early": true`;
     - `-v --json-errors` still gives pure-NDJSON stderr on the dry-run
       path (caveat fix (a));
   - `--fail-fast`: `stopped_early`;
   - a single-file render: the summary line is printed.
5. Docs: a short section in the render CLI docs under `docs/`
   (location to be confirmed) that states clearly what a dry run does
   not catch.
6. `cargo xtask verify`, plus the Windows cross-check clippy for
   `quarto` and `quarto-core` per `.claude/rules/cross-platform.md`.
