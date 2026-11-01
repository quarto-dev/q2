---
title: 'EXR-3: execution of inline expressions for the Jupyter engine'
date: 2026-10-28
description: 'Executes brace-spelling inline expressions through the Jupyter kernel as one-line units in document order, presents the value with Q1 semantics, deletes the dead user_expressions stub and closes bd-u996g8g2.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Strand:** bd-u996g8g2 (open, P3; this plan closes it).
**Depends on:** EXR-2 (scanner, unit loop and the `kernel_for(unit)` seam), JOL-1 (the
resolver's owned-language set on the context, `ctx.owned_languages`, which Jupyter's span
ownership reads), JOL-2a (the engine runs for a document whose only executable content is
spans, and picks its kernel). Soft: EXR-1
(`render_inline` goes into the extracted presenter module). It does not depend on
JOL-5: this plan builds the base fake kernel (Phase 1) and JOL-5 extends it.
**Blocks:** EXR-5 (records span units and reuses the fake kernel). Per the dependency table in the epic, it also blocks EXR-18, JOL-5; soft prerequisite of EXR-16, JOL-3.
**Lane:** A (`workspace-4`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 2-3.

> **For implementers:** TDD. Every divergence from Q1 below is decided (Gordon,
> 2026-10-29, 2026-10-30 and 2026-10-31). Build the diagnostics for a span behind one construction
> function so the wording lives in one place; the answer to strand bd-30k2j00f (render
> nothing for an empty string) would then be a one-line change.

## Goal

A document with a Jupyter-owned language can use the brace spelling in prose or
attributes, for example the Python brace form followed by an expression, and the
computed value replaces the span in the rendered output, matching Q1 where Q1
has defined behavior. The docs already promise this
(`docs/guides/authoring/computations.qmd`, section "Inline expressions": the brace
spelling "works across engines"); today only knitr does it.

## Background (verified 2026-10-28, workspace-1 at `2ee0bd966`)

**q2 today.** Jupyter has no execution of inline expressions. `KernelSession::evaluate_expressions`
(`engine/jupyter/execute.rs:202-241`) is a dead stub: no caller anywhere in
`crates` (grep), `silent = true` (which would suppress the `execute_result` we
need), a `TODO: extract user_expressions`, and it returns `vec![None; n]`. The only
earlier implementation was a prototype in the retired AST-path
`JupyterTransform` (`engine/jupyter/transform.rs`, `extract_inline_expressions` and
`parse_inline_expression`; recoverable from git history, per the strand text).
knitr has it (`engine/knitr/preprocess.rs`, `.QuartoInlineRender` in
`engine/knitr/resources/rmd/execute.R:181`; tests in
`tests/integration/knitr_inline_expressions.rs`).

**Execution plumbing.** `execute_blocks_inner` (`text_execute.rs:262`) runs each
cell with `daemon.execute_in_session(&key, &cell.code)` (`daemon.rs:285`), which
calls `session.execute(code)`. There is no per-call `store_history` or `silent`
control; only the stub sets those fields (`execute.rs:219-220`). Cell errors abort
unless `error: true` (`text_execute.rs:339-349`), with a message that includes the
source location via `describe_location` (`:494`).

**Masking.** Display-block examples are masked before serialization and restored
after (`engine_execution.rs:515-537,623`), so a span shown inside a display fence
never reaches the scanner.

**Q1 behavior (the port target)** (all under `external-sources/quarto-cli/src/`):

- Python side `resources/jupyter/notebook.py:636-690` (`cell_execute_inline`):
  per markdown cell, spans are found with ``(?:^|[^`])`{language}[ \t]([^`]+)` ``
  where `language` is the notebook language only; one `execute("", user_expressions=...)`
  carries all of a cell's expressions. The kernel `eval`s them, so a statement is a
  `SyntaxError`, and a Python expression whose value is `None` yields the text `None`.
- TypeScript side `core/jupyter/jupyter-inline.ts` (`resolveUserExpressions`):
  picks the mime type with `displayDataMimeType` (`core/jupyter/display-data.ts`),
  which **depends on the output format**: for HTML targets widget state, widget
  view, javascript and `text/html` come first, then `text/markdown`, svg, png,
  jpeg, then `text/plain`. Then: html gives `` `data`{=html} ``, latex gives
  `{=tex}`, markdown is inserted as is, and plain text goes through
  `asEscapedMarkdown`.
- **Correction to the handoff:** `asEscapedMarkdown` transforms only a value
  that is quoted (starts and ends with a single or double quote): it strips the
  quotes, doubles backslashes and escapes the characters
  ``\ ` * _ { } [ ] ( ) > # + - . !`` (note `+-.` is a character range in the
  regex, which also covers the comma). An unquoted value such as `2` or `3.14` is
  inserted unchanged.
- With `execute.cache` true Q1 throws "Inline expressions cannot be used with
  Jupyter Cache". q2 has no jupyter-cache, so there is no equivalent.

**knitr and Julia, for comparison.**

- knitr `.QuartoInlineRender` (`execute.R:181-195`): `NULL` becomes the text
  `NULL`; an `AsIs` value is inserted as is; any character value is escaped with
  the class ``[`*_{}()>#+-.!]`` plus the square brackets (no quote stripping, no
  backslash doubling); other values go through knitr's default inline hook.
- Julia (`external-sources/QuartoNotebookRunner.jl/src/cell_processing.jl:105-119`,
  `evaluation.jl:476-540`): statements allowed, only `text/markdown` (as is) and
  `text/plain` (escaped) are used, errors are recorded with the span text, and the
  result is substituted into the markdown cell source.

So Q1's own Jupyter and knitr paths already differ on escaping. This plan ports
the **Q1 Jupyter** behavior and documents the knitr difference.

**Positron.** It writes every non-stream, non-error output as `execute_result`,
never `display_data` (`external-sources/positron/src/vs/workbench/contrib/positronQuarto/browser/quartoOutputCacheService.ts`,
rich-output branch around `:900-945`; re-read 2026-10-29). A span's value must
therefore not depend on `output_type`.

**Routing of span-only documents (verified 2026-10-29 and 2026-10-31).** The resolver
walks code blocks only (`resolution.rs:128`), and Q1 detects languages from fences only
(`external-sources/quarto-cli/src/core/pandoc/pandoc-partition.ts:144-166`). A document
with spans and no executable cell therefore resolves to the markdown engine in both,
and the spans render as literal code. This plan keeps that. The workaround is an
engine-selecting front matter key (`jupyter: python3`, `engine: jupyter`, or a `jupyter:`
map with a `kernelspec`); Q1 evaluates the spans with any of them (verified by run,
2026-10-31, four variants) and starts a kernel even when there are no spans. Q1 reads the
engine only from the document's own front matter (`markdownExecutionEngine`,
`external-sources/quarto-cli/src/execute/engine.ts:155-171`). q2 does not do that today:
the resolver returns an empty engine list as soon as it finds no code
cell, before it reads the declared engine (`resolution.rs:489-503`), so the stage's
nothing-to-run path (`engine_execution.rs`, empty `to_run`) skips every engine. JOL-2a
fixes that (epic decision 42), for a declaration in the document's own front matter. Whether to route span-only documents automatically,
without a key, is strand bd-my5fqvok, outside this epic.

**Not verified:** whether `user_expressions` would have been usable on the kernels q2
targets (Positron's supervisor hard-codes `{}` and Ark does not implement it, per the
handoff; the Ark stub is at `external-sources/ark/crates/ark/src/console/console_repl.rs:2215`).

## Design decisions (from the epic and handoff section 4; do not re-litigate)

1. **Cells, not `user_expressions`.** Each span runs through the ordinary execute
   path as a one-line unit, in document order, with `store_history = false` and
   **`silent = false`**. Uniform capture and error policy, source locations,
   and it works on kernels without `user_expressions`.
2. **Value.** The last output carrying a text mime type (`execute_result` or
   `display_data`; streams and errors excluded). Streams and other displays from
   the span are dropped with a debug log, as Q1 drops everything but the value.
3. **Mime choice** by a port of Q1 `displayDataMimeType` restricted to the mime
   types q2 handles, parameterized by the output format. It is a new function in
   the presenter module and does not change the cell path's precedence
   (`format_outputs`, a separate and older rule).
4. **Escaping** follows Q1 `asEscapedMarkdown` (quoted values only).
   `IPython.display.Markdown` plays the role of knitr's `I()` because `text/markdown`
   is inserted as is. Raw html and latex use `ticks_for_code` so a value containing
   backticks cannot break the span (an improvement on Q1's fixed single backtick).
5. **Units come from the stage, not from a scan of this engine\'s own** (epic decision 38).
   The engine stage scans units once, on the text entering the stage before any engine runs,
   fills each unit\'s id with EXR-4\'s id function (span: `{n}-{hash8}-inline`), and passes the
   one list to every engine through `ExecutionContext.units`. This plan reads its spans from
   that list. The span counter `n` is document-wide over all spans, owned or not, like the
   cell index, so a span\'s id does not depend on which engine runs it. Each engine finds a
   unit\'s `line` in the text it receives and validates counts for its owned units only.
   Every span in the gaps between braced blocks, including fences whose info string names a language, is in that list
   (EXR-2), whatever its language. Jupyter's view owns a span when its language is one
   Jupyter owns (the resolver's set, `ctx.owned_languages` from JOL-1, built from the
   cells; JOL-2b) **or** is the language of the kernel that `select_kernel` returns
   (JOL-2a: the `jupyter:` key, or the engine's `kernel` config). The second rule is what
   makes a span-only document work, since such a document has no cell for the resolver to
   see. If `select_kernel` returns a kernel whose language cannot be read, the span is not
   owned and the error in divergence 6 applies. Spans of any other language are left as is
   (decision 7).
6. **Errors.** An inline error is fatal and names the span and its location
   (`inline expression at file.qmd:12:34 raised NameError: ...`). `error: true`
   does not apply to spans (verified in Q1, see divergence 5).
7. **A span of an unowned language is left untouched** (never run in the first
   cell's kernel).

### Divergences from Q1 and how each is decided

1. **Statements: decided, fatal.** A statement such as `val = 1+1` is an error in Q1
   (`notebook.py:676`, `raise_error`; verified 2026-10-29). Cell mode produces no value,
   so q2 reports a fatal "inline expression produced no value", with the span\'s
   location. (Julia allows statements.)
2. **Expressions whose value is `None`: decided, mimic Q1 (Gordon, 2026-10-29).** Q1
   evaluates spans through `user_expressions`, whose `eval` returns the text `None` as
   `text/plain` (`external-sources/quarto-cli/src/resources/jupyter/notebook.py:650-682`,
   `external-sources/quarto-cli/src/core/jupyter/jupyter-inline.ts`), and prints it. In
   Python `None` is a value, so a span whose value is `None` renders the text `None`.
   Interaction (resolved in Phase 1, see the Phase 1 notes: a Python wrapper): in cell mode a kernel returns no
   `execute_result` for `None` (IPython suppresses it), exactly as it returns none for a
   statement (divergence 1, which stays fatal because Q1 raises). Telling the two apart
   needs either `user_expressions` on the execute request (Ark\'s is a stub,
   `external-sources/ark/crates/ark/src/console/console_repl.rs:2215`) or a
   language-specific evaluation wrapper; the choice and its per-kernel fallback are open.
3. **Multi-line bodies and `a; b`: decided, accepted for non-Python kernels** (a superset of Q1); not policed. **On a Python kernel `a; b` is a `SyntaxError`, as in Q1**, because the divergence 2 wrapper `eval`s the span (see the Phase 1 notes); the divergence is therefore only for other kernels.
4. **Round trips: decided, one request per span** instead of Q1\'s one per markdown
   cell. Fine at normal sizes; measure on a document with many spans before optimizing.
5. **`error: true`: decided, ignored for spans** (an inline error is fatal), as above, and
   parity with Q1. Q1 raises on any failing expression inside `cell_execute_inline`
   (`notebook.py:640-641,676-677`, verified 2026-10-29), and a run of Q1 with
   `execute: error: true` and a span that raises `NameError` still failed the render
   (verified by run, 2026-10-31).
6. **A document that selects Jupyter but has only spans: decided (epic decision 42).** It
   selects Jupyter by `engine: jupyter` or a `jupyter:` key, as in Q1 (verified by run,
   2026-10-31), since a span-only document otherwise never reaches this engine. JOL-2a makes
   the resolver keep the declared engine when the document has no code cells, so the
   engine runs; the span is then owned by the kernel-language rule in design decision 5.
   If `select_kernel` (JOL-2a) returns a kernel, start it. The error ("inline expressions
   need an executable cell or a `jupyter:` kernel") fires when the engine is declared,
   there are zero cells, `select_kernel` returns `None`, and the document has at least one
   span. A declared engine with no cells and no spans is not an error (nothing to
   evaluate). The declaration counts only when the document's own front matter makes it
   (JOL-2a); a project-level declaration on a span-only page leaves the spans literal.
7. **Empty values and missing results: decided (Gordon, 2026-10-29 and 2026-10-30).**
   - **An empty-string value keeps the span source in place, as in Q1** (parity). The
     Python value `''` arrives as `text/plain` `''`; `asEscapedMarkdown` strips the quotes
     and the empty result is falsy, so `resolveUserExpressions` leaves the match
     (`execute-inline.ts:19-24`; verified 2026-10-29). This looks accidental in Q1 (the
     truthiness check conflates empty with missing) and the ideal for authors is to render
     nothing, which the QNR path already does (EXR-8). Strand bd-30k2j00f asks whether to
     change it later; until then `{python} ""` shows its source.
   - **A span with no usable result** (the kernel returned nothing, or no text mime type)
     **leaves the span source in place and emits a warning diagnostic with the file and
     line.** It is never a failure and never silent. Q1 is silent here, so this is an
     intentional divergence (the render still succeeds and the output matches Q1\'s).
   - Statements stay fatal (divergence 1) and `None` renders as text (divergence 2), so
     the warning covers only what remains: a span the kernel accepted whose output has
     no usable text mime. Which of the three outcomes (statement, `None`, no result) a
     silent execution is depends on the open mechanism in divergence 2.
8. **A span inside a braced fence: q2 divergence, not run (decided, Gordon, 2026-10-31).**
   Q1 evaluated `` `{python} x+10` `` written inside a `{r}` fence in a Jupyter document
   (it rendered `rspan 13`, verified by run, 2026-10-31). The cause is an accident of
   Q1's notebook conversion: only fences of the kernel language become code cells, so the
   whole `{r}` fence is part of one markdown cell, and `cell_execute_inline`
   (`external-sources/quarto-cli/src/resources/jupyter/notebook.py:636-690`) scans that
   cell's text with a regex that knows nothing about fences. In q2 the scanner claims
   spans only in the gaps between braced fences (EXR-2 design decision 2), and the mask
   leaves a braced fence alone (`nested_cell_mask.rs:253-263`), so such a span is not a
   unit: it stays in the fence as literal text, and the engine that runs the fence sees
   it as code. The rule is "spans live in prose". Evaluating it would also put one
   engine's values into another engine's code before that engine runs. Considered and
   declined: evaluating spans in every braced fence (Q1 behavior), and evaluating only
   in fences no engine runs.
9. **Spans ignore `eval`: parity.** Q1 passes no `eval` to the span path
   (`notebook.py:319-320`: only the code-cell branch receives it), and a span still
   evaluated under `execute: eval: false` (verified by run, 2026-10-31). q2 does the same:
   `eval: false` at document scope or on a cell does not stop a span.
10. **Front matter and HTML comments (decided, Gordon, 2026-10-31).**
    - **Front matter spans are evaluated, as in Q1, when they come from the document's own
      front matter (Gordon, 2026-10-31).** Q1 evaluates them: with
      `title: "T `{python} 1+1` end"` it renders `title: T 2 end` (verified by run,
      2026-10-31; the notebook's first markdown cell holds the front matter and is
      scanned like any other). q2 gets the same result by construction: the qmd writer
      emits the front matter into the text the engine sees (`pampa/src/writers/qmd.rs:446-455`,
      values converted by `config_value_to_yaml`, `:346`)
      and the reconcile step takes the executed document's metadata wholesale
      (`quarto-ast-reconcile/src/apply.rs:32-33`: "executed metadata wins entirely"), so
      later stages read the evaluated values. The scanner does not skip the front matter
      block, and a front matter span is a unit like any other, **except** that the metadata
      the stage serializes is the merged map (project, `_metadata.yml`, extension and format
      defaults included). q1 evaluates only the document's own front matter, so a span from
      any other layer stays literal, takes no span counter number and passes through the
      engine unchanged. EXR-2 decision 11 defines the provenance rule (the value's
      `SourceInfo` root file id is the document's own file; synthesized values are not the
      document's) and its mechanism; this plan adds the test
      `quarto_yml_front_matter_span_stays_literal`.
    - **YAML escaping (verified with `pampa -t qmd`, 2026-10-31).** The qmd writer always
      re-emits a front matter string double-quoted: `title: 'x `{python} f("a")` y'` comes
      out as `title: "x `{python} f(\"a\")` y"`. The scanner would therefore extract the
      escaped text `f(\"a\")` and the kernel would raise a `SyntaxError`. So a front matter
      unit unescapes the YAML double-quote sequences (`\\`, `\"`, `\n`) before execution, and
      re-escapes (`\`, `"`, newline) when the value is written back into the scalar. A value
      escaped for markdown (`a\*b`) would be an invalid escape inside a double-quoted
      scalar, which breaks the whole front matter parse after execution; the re-escape step
      prevents that, and a value that still cannot be written is a fatal error that names
      the span and its location. Q1 has the same hazard and no such rule. A newline in an evaluated
      value would fold to a space inside a double-quoted scalar, so it is a fatal error that
      names the key and its location, never a fold (settled arbitrarily, Gordon 2026-10-31: revisit if wrong).
    - **Error location.** `write_with_source_info` tracks the whole front matter as one
      piece (`qmd.rs:3558-3564`), so whether `SourceInfo::substring` resolves a span inside it
      to the right line and column is UNVERIFIED. Phase 2 adds a location test (a failing
      `title:` span names `file.qmd:2:...`); if that cannot work, the error names the key
      instead (`inline expression in front matter key `title` raised ...`).
    - **HTML comment spans are not evaluated: q2 divergence.** Q1 evaluates them (verified
      by run: `<!-- comment span `{python} x+30` -->` rendered `33`, a multi-line comment
      too, and a failing span inside a comment failed the whole render with a
      `NameError`). Pampa represents a comment as a raw HTML node (`RawBlock` or
      `RawInline`, not a span node), so an AST walker never sees a span inside one, while
      a text scanner does; evaluating them would make the scanner and the preview splice
      (EXR-14, which keys spans by language, code and occurrence) disagree, which is epic
      Risk 1. Authors also comment text out to disable it. So the scanner recognizes
      comment ranges outside fences and treats them as not-a-span: a span inside one is
      not claimed, not counted by the span counter and not run. Considered and declined:
      evaluating them as Q1 does, and evaluating them with errors downgraded to warnings.
11. **Capitalized language tokens: `{Python}` is evaluated in q2, not in q1.** The scanner
    compares tokens case-insensitively (EXR-2 decision 5), so `{Python} 1+1` is a span for a
    Python kernel. Q1 builds its span regex from the notebook's kernel language, which is
    lowercase (`notebook.py:651-653`, ``rf"(?:^|[^`])`{{{language}}}[ \t]([^`]+)`"``), so it
    leaves `{Python} 1+1` literal. Same family as strand bd-7gvl455a (capitalized cell
    fences); record both in the divergence list.

## Out of scope

- knitr and Julia spans (EXR-12, EXR-8); `text/latex` and widget handling in the
  cell path; making the resolver count spans (decided: no change from Q1, strand
  bd-my5fqvok); batching spans into one request.

## Phased checklist

### Phase 0: presentation (pure, no kernel)

- [x] `display_data_mime_type(bundle, format)`: tests for each target class
  (html, latex, markdown, other) mirroring Q1's priority lists, including the
  widget-before-markdown html case and the html-table addition.
- [x] A shared JSON fixture of precedence cases consumed by this test and by a
  new TS test against `ts-packages/quarto-api/src/jupyter/display-data.ts`
  (`displayDataMimeType`, `:80`), so the two ports cannot drift silently.
- [x] `render_inline(bundle, format)`: tests for markdown as is, html raw span,
  latex raw span, quoted `'hello_world'` stripped and escaped, `'a\\b'`
  doubled, unquoted `2` and `3.14` unchanged, text containing backticks, an empty
  bundle (no value, so `None`), and a quoted empty string `''` (an empty result, which
  the caller treats like Q1 does: leave the span source in place).
- [x] Escape-set test table written from running Q1's regex on each input,
  checked in with the expected values.

Phase 0 notes (2026-10-31): the code is `presenter/inline.rs` (`InlineTarget`,
`display_data_mime_type`, `render_inline`). `render_inline` keeps only the four text mime
types before choosing, because Q1's plain-text branch would paste raw base64 or widget JSON
for an image-only value; such a bundle gives `None` (the no-usable-result warning). The
shared fixtures are in `ts-packages/quarto-api/src/jupyter/__fixtures__/`
(`display-data-precedence.json`, `inline-escape.json`, the latter generated by running
Q1's `asEscapedMarkdown` in node); the TS side is `display-data-precedence.test.ts`.
Raw spans pad with a space when the value starts or ends with a backtick.

### Phase 1: execution seam

- [x] Add `execute_in_session_with(key, code, ExecuteOpts { store_history, silent })`
  and thread it to `KernelSession`; test with a fake session that the request
  carries the flags.
- [x] One diagnostics function for a span (fatal statement, fatal error, the
  no-usable-result warning with file and line), with tests that pin each wording and
  severity. The warning must not fail the render. Register it as a new engine-subsystem
  code (epic decision 29; landed as `Q-18-10`, see the Phase 1 notes): an entry in
  `crates/quarto-error-catalog/error_catalog.json` after `Q-18-3`, a docs page
  `docs/errors/engine/Q-18-4.qmd`, and the corpus and tests that the existing codes have,
  following `Q-18-3` end to end. Confirm that `Q-18-4` is still the next free number
  when this lands.
- [x] Decide how a span whose value is `None` is told apart from a statement when the
  kernel returns no `execute_result` for either (see divergence 2), and pin the choice
  with tests for both outcomes.
- [x] Read `runtimelib` 2.0 to decide the fake-kernel design (server-side helpers present
  or not; a binary target or a self-spawning test executable) and record the answer here.
  JOL-5 Phase 1 used to carry this spike.
- [x] Build the base fake kernel for kernel-free tests of the unit loop: it answers
  `kernel_info_request`, heartbeat and `execute_request`; for `echo <text>` it emits
  `status busy`, one `stream` stdout message, an `execute_result` with `text/plain`,
  `status idle` and a matching `execute_reply`; it can emit an `error` message and an
  execution that produces no `execute_result` (the `None` and statement cases). Add a
  self-test that drives it with the real `get_or_start_session`
  (`test_fake_kernel_echo_round_trip`). JOL-5 extends it (a second name, file writes, a
  kernelspec-directory helper); EXR-5 reuses it.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

Phase 1 notes (2026-10-31):

- **Seam.** `ExecuteOpts { store_history, silent }` (`ExecuteOpts::INLINE` is
  `store_history = false, silent = false`), `KernelSession::execute_with`,
  `JupyterDaemon::execute_in_session_with`, and a pure `build_execute_request`. The flags
  are tested on the request builder: a real session cannot expose what it sent, and the
  fake kernel (`TestKernel`) does not report request flags back.
- **Error code is `Q-18-10`, not `Q-18-4`.** JOL-2a took `Q-18-4` ("Jupyter Kernel Not
  Named"). `Q-18-5` is claimed by pandoc-hybrid P7-foundation, `Q-18-6` and `Q-18-7` by
  P5, `Q-18-8` and `Q-18-9` by the engine-result diagnostics plan; none has landed, so I
  skipped to `Q-18-10` rather than race them. One code covers all three outcomes (raised,
  no value, no usable result); the warning is the only one that is not an error. Wording
  lives in `engine/diagnostics.rs` (`inline_span_diagnostic`). Catalog entry, docs page
  `docs/errors/engine/Q-18-10.qmd` and the `docs/_quarto.yml` listing are in.
  Epic decision 29 names `Q-18-4`; read it as `Q-18-10`.
- **`None` versus a statement (divergence 2): decided, a Python wrapper.** For a kernel
  whose kernelspec language is `python`, the span is sent as
  `(lambda v: 'None' if v is None else v)(eval(compile(<src>, ..., 'eval')))`
  (`jupyter/inline.rs`, `wrap_inline_code`). That is Q1's semantics exactly: a statement
  raises `SyntaxError` (fatal), `None` becomes the text `None` (the quoted-value rule strips
  the quotes), and any other value is displayed as usual. Consequence: `a; b` is a
  `SyntaxError` as in Q1, so divergence 3's "superset" does not hold for Python; for the
  other kernels it still does. For every other kernel the span is sent as written, and a
  silent execution (`InlineOutcome::Silent`) is the fatal "produced no value" of divergence
  1; there a value that prints nothing cannot be told from a statement. `user_expressions`
  is not used (Ark's is a stub, Positron's supervisor hard-codes `{}`). The warning
  (no usable result) is `InlineOutcome::NoTextValue`: outputs arrived but none had a text
  mime. Phase 2 must look up the kernel's language (`KernelSession::language()`) to choose.
- **`runtimelib` 2.0 and the fake kernel.** `runtimelib` has the server half behind the
  `test-kernel` feature: `TestKernel::start_from_file` serves a connection file, answers
  `kernel_info_request`, heartbeat and `execute_request`, and sends canned outputs for exact
  code matches (an empty list is a silent execution), else echoes the code as stdout.
  It is a library, so the process the session spawns is the test executable itself: the
  kernelspec argv re-runs the lib test binary on one entry test
  (`fake_kernel_process_entry`), which becomes the kernel when `QUARTO_FAKE_KERNEL_SCRIPT`
  is set and passes at once otherwise. No binary target. `FakeKernelHome` writes
  kernelspecs under a temp dir named by `JUPYTER_PATH` (process-global, safe under
  nextest). Deviation from the sketch above: behavior is a per-test JSON script (code to
  outputs: `Stdout`, `Result`, `Display`, `Error`, plus `TestKernel`\'s echo fallback) rather
  than an `echo <text>` rule, because `TestKernel` matches code exactly. Self-test:
  `test_fake_kernel_echo_round_trip` (through the real `get_or_start_session`). JOL-5 adds a
  second name and file writes on top (`FakeKernelHome::install`).
- Gate run at the end of Phase 1: clippy clean; `nextest -p quarto-core -p
  quarto-error-catalog` 6697/6697 (one slow Julia test).

### Phase 2: the unit loop

- [x] In the EXR-2 unit loop, handle `Unit::Inline` taken from `ExecutionContext.units`
  (decision 5; the engine does no scan of its own): run, take the value,
  `render_inline`, and splice by byte range in the same pass that copies gaps. Test that
  the loop uses the supplied units unchanged (a stub list whose ids differ from what a
  fresh scan would give is honored).
- [x] Locations via `SourceInfo::substring` over the span's expression. Include the
  front matter location test of divergence 10 (a failing `title:` span names `file.qmd:2:`),
  or switch the front matter error to name the key if it cannot work.
- [x] Front matter, tests first (divergence 10): `title: 'x `{python} f("a")` y'` evaluates
  `f("a")` (the unescape step); an evaluated value containing `\`, `"` and `*` leaves the
  front matter valid YAML after the re-escape; `quarto_yml_front_matter_span_stays_literal`
  (a span in a `_quarto.yml` value is not evaluated and the page renders it as written).
  Then implement the unescape, re-escape and the provenance-based claim (EXR-2 decision 11).
- [x] Jupyter's span ownership, tests first: a span is owned when its language is in the
  engine's owned set or equals the language of the kernel selected by `jupyter:`; a
  span-only document with `jupyter: python3` owns its `{python}` spans and not a `{r}` span;
  a document with a Python cell and no key owns `{python}` spans through the owned set.
  The error test: `jupyter_engine_declared_spans_only_without_kernel_errors` (engine
  declared in the document's front matter, zero cells, `select_kernel` returns `None`, at
  least one span: the error of divergence 6; with zero spans, no error). First, check how to read a kernelspec's `language` for a kernel name
  (`kernelspec::find_kernelspec`, `daemon.rs:84-133`); not verified.
- [x] Delete `evaluate_expressions` and its now-unused imports (confirm no
  callers again at implementation time).
- [x] Spans of an unowned language and spans in ceded or display blocks are
  untouched (unit tests on the pure splice), including a `{python}` span inside an
  executed `{r}` fence (divergence 8) and a span in an HTML comment (divergence 10).
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

Phase 2 notes (2026-10-31):

- **Loop.** `execute_qmd` builds the run list from the executable cells and the owned
  spans, sorted by start, and `plan_units` routes them. `execute_blocks_inner` takes
  `&[Unit]`; a span goes through `evaluate_span` (wrap, `ExecuteOpts::INLINE`,
  `classify_inline_result`, `render_inline` with `InlineTarget::from_format`). Cells and spans
  are spliced in one pass by `router::splice_mixed`; only `SpliceKind::Block` gets the blank-line
  padding (`splice_units` is now a wrapper), so an inline value is inserted exactly.
- **Ownership.** `span_is_owned(lang, ctx.owned_languages, kernel_language)`. The kernel
  language is `kernelspec::find_kernelspec(name).language`, read before the plan (only when the
  document has spans). A kernel that cannot be found fails there with the same error the start
  gives, which refines the line in design decision 5 ("language cannot be read: not owned").
  With no cells, no `jupyter:` key and an owned span, `execute_qmd` fails with "inline
  expressions need an executable cell or a `jupyter:` kernel" (divergence 6).
- **Front matter.** A new fatal problem, `InlineSpanProblem::FrontMatterValue { key, reason }`
  (same code `Q-18-10`), covers a newline in the value; the key comes from the span\'s line
  (`front_matter_key_at`). Unescape and re-escape are `unescape_yaml_double` and
  `escape_yaml_double`. Provenance masking was already done by EXR-2 (`mask_document`); the
  jupyter-level test `quarto_yml_front_matter_span_stays_literal` checks that the engine leaves
  a masked span alone.
- **Locations.** Verified for a source map that holds the document as one file (`doc.qmd:2:`).
  UNVERIFIED through the real pipeline, where `write_with_source_info` tracks the whole front
  matter as one piece (`qmd.rs:3558-3564`): Phase 3\'s front matter integration tests decide
  whether `SourceInfo::substring` is enough or the error should name the key only (it
  already names the key for the newline case).
- **HTML comments** were already excluded by the scanner (EXR-2); `spans_in_fences_and_comments_are_left_alone`
  pins it at the engine level. A span inside an executed `{r}` fence is not claimed by the
  scanner (pinned in EXR-2), so the loop never sees it; the Phase 3 test covers it end to end.
- **"Supplied units honored"** is not observable in output (`locate_units` re-adds an unmatched
  scanned unit), so `supplied_units_with_ids_are_used_as_they_are` only shows that ids on
  the supplied list do not disturb the loop.
- `evaluate_expressions` had no callers and is deleted.

### Phase 3: integration tests (kernel-gated like `jupyter_integration.rs`)

Modeled on `knitr_inline_expressions.rs`. Names: `jupyter_inline_in_prose`,
`jupyter_inline_in_attribute_value`, `jupyter_inline_markdown_value`,
`jupyter_inline_html_value`, `jupyter_inline_string_is_escaped`,
`jupyter_inline_statement_is_an_error`, `jupyter_inline_none_renders_as_text`,
`jupyter_inline_empty_string_leaves_source` (parity with Q1; strand bd-30k2j00f),
`jupyter_inline_no_usable_result_warns_and_leaves_source` (a warning with file and
line, the render succeeds),
`jupyter_inline_sees_state_from_earlier_cell`,
`jupyter_inline_in_front_matter_with_newline_is_fatal_error` (the error names the key and location),
`jupyter_inline_in_any_fence_stays_literal` (a span in any non-braced fence, whether class-less, `markdown`, or labelled with a language such as `r` or `text`, is masked and never scanned: decision 2 of EXR-2, revised by Gordon, 2026-10-31),
`jupyter_inline_in_ceded_block_is_not_run` (a `{python}` span inside an executed `{r}`
fence stays literal: the divergence from Q1 in divergence 8),
`jupyter_inline_in_display_fence_is_not_run`, `jupyter_inline_error_true_is_ignored`,
`jupyter_inline_ignores_eval_false` (document `execute: eval: false`; the span still
runs), `jupyter_inline_in_front_matter_is_evaluated` (a `title:` span reaches the
rendered title; a value that cannot be written into the YAML string is a fatal error),
`jupyter_inline_in_html_comment_is_not_run` (block, inline and multi-line comments; a
failing span inside a comment does not fail the render),
`jupyter_inline_unowned_language_untouched`,
`jupyter_span_only_document_without_engine_key_stays_literal`,
`jupyter_span_only_document_with_jupyter_key_is_evaluated`,
`jupyter_engine_declared_spans_only_without_kernel_errors`,
`jupyter_inline_in_front_matter_with_quotes_and_backslashes_stays_valid_yaml`,
`jupyter_inline_in_project_metadata_is_not_evaluated`,
`jupyter_inline_capitalized_language_is_evaluated` (divergence 11).

- [x] Add an inline case to `engine_output_parity.rs` so knitr and Jupyter span
  shapes stay congruent (requested by bd-u996g8g2).
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core` (kernel-gated tests skip without ipykernel).


Phase 3 notes (2026-10-31):

- The tests are in `tests/integration/jupyter_inline_expressions.rs`; each skips with a message
  unless the jupyter engine is available (`jupyter` on `PATH`) and a Python kernelspec exists,
  as in the knitr suite. Locally: put a venv with ipykernel on `PATH` and its `share/jupyter`
  in `JUPYTER_PATH` (for example the Positron venv). The parity case is `parity_inline_span`
  in `engine_output_parity.rs`.
- **Front matter locations are verified through the real pipeline:** a newline error in a
  `title:` span names `...qmd:2:` (the whole-front-matter piece in `write_with_source_info`
  does not break `SourceInfo::substring`), so the Phase 2 fallback (name the key only) is not
  needed. The error names the key as well.
- **`jupyter_inline_no_usable_result_warns_and_leaves_source` is not an integration test.** IPython
  always adds `text/plain` to a bundle (checked with `display_formatter.format` on an
  image-only `_repr_mimebundle_`), so a real ipykernel never gives an image-only value;
  `no_usable_result_warns_and_leaves_the_span_source` (fake kernel, `text_execute.rs`) covers it.
- **A Python `str` carrying a newline cannot reach the page as a newline:** the quoted-value rule
  renders the repr, so `a\nb` (a Python `str`) shows as the two characters `\n` (Q1 does the same; the
  `inline-escape.json` fixture pins the doubling). The front matter newline test therefore
  uses a `Markdown(...)` value, built with `__import__` because a front matter span runs
  before any cell.
- **Found and fixed by these tests:** `engine: jupyter` on a page with spans and no cells and
  no `jupyter:` key rendered the spans literally, because with an empty owned set the span was
  "unowned" and the loop passed through. Divergence 6 now fires on "no cell at all, no
  kernel, at least one span" (a span of any language), as the plan says.
- `jupyter_inline_error_true_is_ignored` and `jupyter_inline_ignores_eval_false` pass as written.
- The parity case needs a visible setup cell: knitr emits an empty `.cell` div for a hidden
  cell with no output and jupyter emits nothing, a difference that predates spans.
\n

### Phase 4: Q1 equivalence fixtures

- [x] Render a small set of documents (a `NULL`/`None` case, markdown, html, a
  statement, an empty string, a span with no usable result) with Q1 locally (needs
  `QUARTO_PYTHON` pointing at a venv with jupyter; a span inside a `{r}` fence is a
  fixture where q2 deliberately differs from Q1, and a span under `eval: false` one where
  it matches) and check the resulting html fragments into `crates/quarto-core/tests/fixtures/jupyter-inline/`. CI compares q2
  only. The empty-string fixture shows the source in both; the no-usable-result fixture
  matches Q1\'s output but q2 also warns.
- [x] Document each intentional difference next to its fixture (`fixtures/jupyter-inline/README.md`).

Phase 4 notes (2026-10-31):

- Fixtures are `<case>.qmd` plus `<case>.q1.html` (the span-bearing lines of Q1\'s output) and, where q2
  differs, `<case>.q2.html`. Whole-page diffs were noise (title block, whitespace), so the test
  compares only those lines. Tests: `q1_equivalence_fixtures`, `q1_equivalence_statement_fails_in_both`.
- Q1 ran with `QUARTO_PYTHON` set to the Positron venv; the `quarto` on PATH (99.9.9) worked, no scratch worktree needed.
- Identical to Q1: `None`, markdown, html, plain value, string, `eval: false`. Differs: empty string
  (q2 escapes quotes in `<code>` as `&quot;`, same page), `{r}` fence (q2 leaves the span literal, intended).
- A statement (`y = 5`) is fatal in both, so it is an error fixture, not an html fragment.
- The no-usable-result case has no Q1 fixture (a real ipykernel cannot produce it); the fake-kernel test covers it.
- Gate: clippy clean; `nextest -p quarto-core` 6861 passed, 32 skipped, with ipykernel available.

### Phase 5: docs and strand

- [x] Supply the content for the "Inline expressions" section of `docs/guides/authoring/computations.qmd` to EXR-18, which owns edits to that page (EXR-18 Phase B); this plan does not edit the page. The content:
  Jupyter supported, the statement, `None`, that `a; b` is a `SyntaxError` on Python as in Q1, empty-string and no-result rules, that a span
  inside a braced fence or an HTML comment is not evaluated (unlike Q1), that a span in the
  front matter is (and only the document's own, not `_quarto.yml`), the escaping difference from knitr, that a capitalized `{Python}` span is evaluated in q2 (not in Q1), and that a span-only document needs a `jupyter:` key in its own front matter.
- [x] Close bd-u996g8g2 on merge (closed after the fast-forward into feature/execution-records-epic, 2026-10-31).
- Exit: every divergence implemented as decided above;
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

## Risks and open questions

- **Open: how to tell `None` from a statement** when the kernel returns no
  `execute_result` for either (divergence 2); `user_expressions` or a per-language
  wrapper, and the per-kernel fallback.
- **Strand bd-30k2j00f** asks whether an empty string should render nothing. Until it is
  answered, Jupyter shows the source while Julia (QNR, EXR-8) renders nothing, so the
  two engines differ on an empty value.
- **Strand bd-my5fqvok** asks whether span-only documents should route automatically.
- **Format dependence of the mime choice** means a record of a span value
  (EXR-5) must store the raw bundle, not the chosen text, or format changes
  would need a re-run.
- **Front matter after execution (UNVERIFIED consequences, from the metadata round
  trip).** (1) The `DocumentProfile` is snapshotted before the engine runs, so consumers
  that read it (sidebar, listing, search) may see the unevaluated `{python}` text of a
  front matter value; which consumers are affected is not checked. (2) The reconciled
  metadata is the reparsed executed text, so its `source_info` points into the engine's
  serialized text, not back to the document or `_quarto.yml`; whether anything later
  depends on per-key provenance is not checked. Neither affects the claim filter, which runs
  before the engine.
- Kernels with unusual `execute_result` shapes (SoS subkernels; the JOL epic's
  caveat that a span runs in whichever subkernel the previous cell left active).

## References

- `engine/jupyter/execute.rs:202`, `text_execute.rs:262,339,494`, `daemon.rs:285`
- `external-sources/quarto-cli/src/core/jupyter/{jupyter-inline,display-data}.ts`,
  `external-sources/quarto-cli/src/resources/jupyter/notebook.py`
- `engine/knitr/resources/rmd/execute.R:181`; bd-u996g8g2
