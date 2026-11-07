---
title: 'OPR-6: q2 render reads execute: record-sources'
date: 2026-11-08
description: 'Makes q2 render and q2 preview --static read the execute: record-sources key (OPR-5), per document, with no new option: read only, default engine, a stale or format-mismatched record counts as not supplied, a document no source supplies warns and stays inert, a skip guard keeps the engine from starting after a record is spliced. The preview render routes use the host ceiling under --share.'
status: ready
depends-on: [opr-5, opr-1b]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/6-render-execution-records` · **Lane C** (`workspace-5`) · **Blocks:** OPR-7a.
**Needs:** R 4.3 with knitr and Julia 1.12 for end-to-end runs (they skip when absent); `hub-client` tooling for the WASM gate.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c opr/6-render-execution-records feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/6-render-execution-records
```

## Rules (decision D6, Gordon, 2026-11-08; do not reopen)

- **Read only.** The key `execute: record-sources` (OPR-5; `-M 'execute:{record-sources: ...}'` sets it for one run)
  names where a document\'s record is read from, or says to run the engine. Writing stays with `execute: record: true`;
  the two combine. **No new options:** `render` gets no flag for it (decision R5, Gordon, 2026-11-09).
- **Default `engine`** (today\'s behavior; opt-in). Grammar and handling per OPR-5: `cache` is a valid entry that render
  cannot use (no preview cache directory), so it is ignored with one info line. The list is resolved **per document**
  by OPR-5\'s early reader (this supersedes D6\'s one list), so documents of one project render may differ.
- **No source supplies a record and `engine` is not listed:** warn and leave the cells inert, as preview and
  `--no-execute` do. `--strict` makes it an error (exit 1, through `should_exit_nonzero`, `commands/render.rs:~1423`).
  A prose-only document stays silent. The warning names the reason (no source had a record; a record was stale; a
  record was for another format).
- **Stale counts as not supplied.** Use the record-level check preview uses (`source_staleness` and `record_staleness`
  in `crates/quarto-preview/src/capture_driver.rs:~309-350`, over `record_is_stale` and
  `imported_record_is_stale`); **move that wrapper into `quarto-core`** so preview and render share it. Add a **format
  check** that `record_is_stale` deliberately lacks (it takes no target format; its test says "format alone is not an
  edit", right for preview): a record whose recorded format differs from the render\'s target counts as stale on
  render, and the warning says so. Preview keeps showing a stale record; the docs (OPR-8) say render differs.
- **`want_record` is not changed.** A supplied record is spliced and presented by the record path
  (`CaptureSpliceStage`, `stage/stages/capture_splice.rs`, which calls `record_splice::splice_record`) whatever
  `want_record` is.
- **`--no-execute` vetoes `engine`** (the entry is ignored; a fresh record is still read; the warning above reports any
  document left inert). **`--replay` wins over a key** with an info line and is not an error; `--replay` is
  `Option<String>` with a `QUARTO_REPLAY` environment fallback read in `commands/render.rs` (not clap `env`), so this is
  a **runtime check** that also covers the environment variable. The parsed-but-dropped flags (`--cache`, `--use-freezer`, the `--execute*`
  family) are left alone.
- `preview.engine` is a preview-only YAML setting; render does not read it. Say so in the notes for OPR-8.
- **The preview\'s render routes under `--share` use the host ceiling.** `render_blocking` in `quarto-preview`
  (`POST /api/preview/render` and the downloads) takes a guest-supplied `content` and `metadata` (the runtime layer, the
  highest precedence) and calls `render_to_file`, which after this plan reads the list. When `--share` is on, resolve the
  list there with OPR-5\'s `Ceiling` mode: the allowed set is the host\'s `-M` value or the preview default, a
  `content` front-matter key may reorder or narrow it, and `metadata.execute.record-sources` in the request is ignored
  (the host list is passed apart from the request). Without `--share` the request metadata is an ordinary override.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests through the binary first** (model: `crates/quarto/tests/integration/no_execute_cli.rs`, a fixture whose
      cells fail if executed, so a passing run proves no engine ran). Each rule above: a fresh `q2` record is used
      and no engine runs; stale record with `engine` listed runs; stale without `engine` warns and is inert; format
      mismatch; no source; `--strict`; `--no-execute` plus `engine`; `--replay` and `QUARTO_REPLAY` win over a key with
      an info line; `cache` ignored with an info line; prose-only silence; a project render with several documents; a
      document-level key that differs from the project-level one in the same render; a `-M` value that overrides a
      document key. Set the list both ways (the key in front matter, and `-M 'execute:{record-sources: ...}'`). Register new files in
      `crates/quarto/tests/integration/main.rs`.
- [ ] Call OPR-5\'s early reader (`resolve_record_sources`) with the render declaration (default `engine`, `cache`
      ignored) per document, from `render_to_file`, and from the `preview --static` path. Add **no option**.
- [ ] **The per-document seam.** (read) `render_to_file.rs` builds `HtmlRenderConfig::with_resolver(..)` and copies only
      `engine_registry` and `execution_policy` (`:~495-524`); `options.captures` (`:~199`) goes only to the pandoc leg
      (`:~539-544`). `HtmlRenderConfig` already has `record`, `captures` and `with_record`/`with_payload`
      (`pipeline.rs:~143-215`), and `render_qmd_to_html` honors them (`:~2013-2019`). Add a field to
      `RenderToFileOptions` for the command declaration and the trust mode (the list itself is read per document by the
      early reader); inside `render_to_file` (per document, in the rayon worker of
      `project/pass2_renderer.rs:~330, ~472, ~596`) compute `compute_source_inputs_with`, build a render
      `RecordLookup` (move the `Q2` arm of `PreviewLookup`, `quarto-preview/src/record_lookup.rs:~37-69`, into
      `quarto-core`; `Engine` yields no record; `positron` and directory arms share the preview code), call
      `resolve_records`, and on a chosen run set `config.with_record(..)` **and** pass a record payload to the
      pandoc leg. (relied) One shared options value is cloned only for book `chapter_seeds`: that clone is the
      precedent for per-document values.
- [ ] **Skip guard.** (read) After the splice replaces a document\'s cells, `EngineExecutionStage` still resolves the
      engine from metadata and can run it (`capture_splice.rs` header: "could in principle re-execute";
      `ts_engine.rs:~1058` calls `ensure_launched`; knitr\'s `execute` has no zero-chunk early return). When a record
      was supplied for the document, set `ctx.execution_policy` to none for it (the existing `--no-execute` stop,
      `engine_execution.rs:~418-433`). Prove with the failing-cell fixture.
- [ ] **Preview render routes under `--share`.** Tests first (through the handler, with `share` set and a request body
      whose `metadata` is `{execute: {record-sources: positron}}`, and another whose `content` front matter names an entry
      outside the allowed set): both are ignored, the page carries the warning diagnostic, and nothing is read from a
      Positron cache. Then wire the `Ceiling` mode into `render_blocking` as the rule above says.
- [ ] The merged single-file book path builds its own pipeline (`single_file_render.rs`) and would ignore the seam:
      read it and, unless carrying the record there is trivial, warn clearly that `record-sources` is not honored there; record
      which in the notes.
- [ ] The new warning: wording that names the reason; if you add a `Q-` code, add `docs/errors/<subsystem>/<code>.qmd`
      and the `docs/_quarto.yml` sidebar entry in the same commit (the error-docs lint rules).
- [ ] **End-to-end** through `target/debug/q2 render` on a Jupyter-record document, a knitr document and a Julia
      document, each with a record from a prior `execute: {record: true}` render; read the HTML; record the
      invocations and what you saw. Say which engines were absent.
- [ ] Gates: `cargo clippy -p quarto -p quarto-core -p quarto-preview --all-targets -- -D warnings`;
      `cargo nextest run` for the same crates; the Windows cross-clippy for each; `cd hub-client && npm run build:wasm`
      (the staleness wrapper moved into `quarto-core`\'s `record` module); `cargo xtask lint`.
- [ ] **Do not edit `docs/`** (except an error page if you added a code); OPR-8 owns the docs.

## Exit criteria

- Every rule above has a passing test through the binary; a record supplied to render means no engine started
  (shown by the failing-cell fixture) and none written.
- Preview behavior is unchanged except for using the moved staleness wrapper (preview tests pass unchanged).
- End-to-end runs recorded; the book path decision recorded.

## Notes from the run

(Fill in at landing.)
