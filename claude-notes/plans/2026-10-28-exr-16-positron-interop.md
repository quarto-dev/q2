---
title: 'EXR-16: Positron interop for engine execution records'
date: 2026-10-28
description: 'Specifies what q2 does alone for Positron compatibility (schema subset, display-only import, conformance fixtures, language gate) and a series of spikes in a Positron checkout that test whether the shared fixtures, reading q2 records, and execution of inline expressions are viable, ending in a demo if they are.'
status: draft  # not started; the Positron spikes need no one else's agreement and send nothing to anyone
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Depends on:** [EXR-4](2026-10-28-exr-04-record-format.md) (schema and hashes; Phases 0 to 5 for the import; the spikes need nothing),
[EXR-2](2026-10-28-exr-02-unit-scanner.md) (conformance fixtures). Soft:
[EXR-3](2026-10-28-exr-03-jupyter-iee.md) (the IEE value rule that spike S4 compares).
**Blocks:** nothing in the epic; it is the compatibility check on EXR-4.
**Lane:** C (`workspace-6`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 2 on the q2 side (import: 1; fixtures: 1). The spikes are rough
estimates, 6 to 9 sessions in all (S0 0.5 to 1, S1 0.5, S2 1, S3 2 to 3, S4 0.5, S5 1 to 2
as a stretch, S6 1 to 2); the dev-build time in S0 is the largest unknown.

> **For implementers:** the q2-only work is TDD like every plan. The spikes are
> experiments, not deliverables: their code need not be production quality, and nothing
> here may block q2 work. No spike sends, proposes or opens anything.

## Goal

Keep the record compatible with Positron\'s inline-output cache without sharing a file, and
learn, by building, whether Positron could gain the same behaviors:

- q2 emits the subset of the schema Positron already understands, with the content hash
  computed exactly as Positron computes it and ids in Positron\'s format (the label segment
  and the index can differ; matching uses the hash);
- q2 can read Positron\'s cache as a display-only source, normalized at read and gated by
  language ownership so outputs never land on a unit another engine owns;
- both tools run the same conformance fixtures, so unit identity cannot silently drift;
- a series of spikes in a Positron checkout tests whether Positron reading q2 records and
  Positron executing inline expressions are viable, and, if all goes well, ends in a demo.
  The aim is experience for a future contribution or collaboration, not a patch to ship.

## Background (verified in the Positron and Ark sources on 2026-10-28 and 2026-10-30)

Sources: `external-sources/positron` and `external-sources/ark`.

**The Positron checkout (verified 2026-10-30).** `external-sources/positron` links to
`/Users/gordon/src/positron`. Gordon created the fork `gordonwoodhull/positron` on
2026-10-30. The remotes are `origin` (the fork) and `upstream` (`posit-dev/positron`);
local `main` tracks `upstream/main`. Nothing has been pushed and no spike branch exists.
`node_modules` is empty, so nothing is installed or built. Positron\'s own `CLAUDE.md`
sets the working rules: never run `tsc` directly on the main project; the background build
daemons are `npm run build-start`, `npm run build-check` and `npm run build-ps`; fast unit
tests use vitest (`npm run test:positron`, and `npm run test:positron:check-ts` for type
checks of vitest files) and need no daemons; extension-host tests are heavier; prefer new
files for Positron code and wrap unavoidable edits to upstream files in
`// --- Start Positron ---` and `// --- End Positron ---` markers.

**The feature code.** `src/vs/workbench/contrib/positronQuarto/` (README in the same
directory): `common/quartoParser.ts` and `common/quartoExecutionOptions.ts`;
`browser/quartoDocumentModel.ts`, `quartoExecutionManager.ts`, `quartoOutputManager.ts`
and `quartoOutputCacheService.ts`; vitest tests in `test/browser/*.vitest.ts` (for
example `quartoDocumentModel.vitest.ts`). The README says the feature activates when a
`.qmd` or `.rmd` file is open and the Quarto extension is present (whether a dev build
has that extension by default is **not verified**).

**The cache.** `browser/quartoOutputCacheService.ts`:

- Directory `quarto-inline-outputs` (`:32`), one `<sha1(documentUri)[0:16]>.ipynb` per
  document (`_getCachePath`, `_hashUri`, `:196` to `:211`). The path hash is over the VS
  Code URI string, which q2 cannot reliably reproduce, so the file cannot be shared.
- `CACHE_VERSION = 1` (`:35`). Notebook metadata: `quarto_source`, `quarto_cache_version`,
  `last_updated` (a number) (`:83` to `:85`); `nbformat: 4`, `nbformat_minor: 5`
  (`:789`, `:790`).
- The loader (`:218` to `:290`) validates only a numeric `nbformat`, a `cells` array, an
  object `metadata` and a numeric `metadata.quarto_cache_version`. It **deletes the file**
  on invalid structure (`_validateCacheStructure`, `:223-227`; delete at `:255-266`), on a
  version other than 1 (`:262`), or when anything throws (`:292-298`, for example a
  cell without an `outputs` array), with no migration. It skips non-code cells and reads
  only `quarto_cell_id`, `quarto_content_hash`, `quarto_label`, `source` and the outputs.
  `last_updated` is read with a default (`:851`), so omitting it is safe.
- `findCacheByContentHash` (`:611` to `:690`) scans every `.ipynb` in its cache directory
  when a document has no entry of its own (the moved-file feature). On **one** matching
  cell hash (`matchCount > 0`, `:658`) it loads that whole file into the target document,
  marks it dirty, and **deletes the file** (`_deleteCacheFile`, `:686`); Positron then
  rewrites it from its own fixed fields, so q2 metadata is lost. A file placed there under
  any name is consumed, and it can seed an unrelated document that shares a single cell
  hash.
- Cell metadata: `quarto_cell_id`, `quarto_content_hash` (`:44`, `:45`, written at
  `:777`, `:778`). Rich outputs are written as `execute_result` with all mimes in `data`
  plus `metadata.quarto_webview_metadata` (`:921` to `:926`).
- Output representation (verified 2026-10-29, reading `:880-990`; not run against a real
  Positron): every rich output, `display_data` included, is collapsed to
  `execute_result`; every payload is held as a string, so a JSON mime (`application/json`,
  anything `+json`) is written as a string although nbformat stores JSON mimes as objects,
  and the reader `JSON.stringify`s any non-string value. Non-conformant but not lossy: a
  valid JSON string parses back to the same value.
- The cell metadata carries no language and no kernel identity, so an importer cannot tell
  from the file which language a cached cell ran as.

**Identity (verified 2026-10-31).** `quartoDocumentModel.ts:22`: `computeContentHash` is
SHA-1 over UTF-8 of the content, first 16 hex characters; the content is the lines between
the fences joined with `\n` (`common/quartoParser.ts:173`). `StringSHA1` turns a lone
surrogate into U+FFFD (`external-sources/positron/src/vs/base/common/hash.ts:181-205`).
The four vectors in S1 were recomputed and match. Cell ids are
`{index}-{hash8}-{label|unlabeled}` (`generateCellId`, `:32` to `:40`). Restore is
content-hash first with the index as a tiebreak (`findCellByContentHash`, `:185`). The
language is in neither the hash nor the id.

**Where q2 and Positron diverge on ids (verified 2026-10-31).** Same id format and same
content hash, but not necessarily the same id:

- *Label segment.* Positron\'s label is the first option **inside the fence braces**
  (`quartoParser.ts:37`, `options.split(',')[0]`), so a `#| label: fig-x` line is invisible
  to it: the cell is `fig-x` in q2 and `unlabeled` in Positron. `{python.marimo}` and
  `{python .marimo}` both give the label `.marimo` in Positron.
- *Nesting and index.* Positron\'s parser matches only column-0, exactly-three-backtick
  fences with a `\w+` language and has no nesting rule (`quartoParser.ts:21-30`), so a
  `{python}` line inside an unbraced display fence is a phantom cell there (q2\'s mask
  removes it), and indices differ for documents with blockquote, list or four-backtick
  cells.
- *CRLF.* Positron splits lines on `\r?\n` (`quartoParser.ts:113`) and rejoins with `\n`, so
  CRLF **is** normalized on Positron\'s side. q2 does the same when it hashes (EXR-4), so a
  CRLF cell and its LF twin hash equal on both sides.

Hence matching uses the hash and never the id (EXR-14); the ids are a shared format, not a
shared key.

**Language tokens.** Positron\'s fence regex is
``^```\{(?<language>\w+)(?<options>[^}]*)\}\s*$`` (`common/quartoParser.ts:21`), so it
reads `{python.marimo}` as language `python` with options `.marimo`, and `{python .marimo}`
the same. q2 reads `python.marimo` (no space) as one language token owned by marimo, and
`{python .marimo}` as `python` with first class `marimo` (verified 2026-10-29 by rendering
one-case files with `trace: true`; EXR-2 states the rule). The hashes agree in both cases;
the ids differ in the label segment (Positron\'s label is `.marimo` for both spellings, q2\'s
is the `#| label:` value or `unlabeled`); ownership differs too.

**Execution (verified 2026-10-30).**

- *In core.* Positron runs Quarto cells through `session.execute(...,
  RuntimeCodeExecutionMode.Interactive, ...)` with an observer that collects every
  message type (`browser/quartoExecutionManager.ts:835` to `:854`), one cell at a time per
  document. A spike inside core is therefore not limited by the two extension-API gaps
  below. Cells run as `Interactive`; the cache service stores no execution count, order
  or kernel identity, so its records can only be trusted for display.
- *Through the extension API.* `src/positron-dts/positron.d.ts` defines
  `RuntimeCodeExecutionMode` including `Transient` (`:178`: displayed to the user, not
  combined with pending code, not stored in history) and `Silent` (`:186`: not displayed),
  and `positron.runtime.executeCode` (`:3676` to `:3687`) and `evaluateCode` (`:3709`).
  `executeCode` resolves only with the data of the **first `Result` message**, and with
  `{}` when none arrives; `Output` messages (what Jupyter calls `display_data`) go only to
  the observer (`src/vs/workbench/api/common/positron/extHostLanguageRuntime.ts`,
  `handleObserverMessage`: `Result` calls `onCompleted`, `Output` calls `onOutputMessage`).
  And `Transient` runs are displayed, so each span\'s code and value would echo in the
  console. These two are the gaps spike S3 confirms by test.

**Ark.** `external-sources/ark/crates/ark/src/console/console_repl.rs:2215` answers
`user_expressions` with an empty object (verified; the handoff cited an older path).
The handoff also reports that Ark has no knitr or Quarto emulation beyond the
`positron` execute-request fields; not re-verified here.

**Positron has no execution of inline expressions** (handoff; the cache service and
parser contain no span handling).

## Design decisions (from the epic and the 2026-10-30 review; do not re-litigate)

1. Two files. q2\'s record is its own committable file; Positron\'s cache is never the
   source of truth for q2, and q2 never writes its file into Positron\'s cache directory.
   (Spike S2b places one there by hand, as an experiment, to learn what Positron does.)
2. Matching metadata is Positron\'s in format: `quarto_content_hash` (computed exactly as
   Positron does), `quarto_cell_id` (same format, label segment and index may differ,
   decision 39), `quarto_label`; the notebook keys `quarto_source` and
   `quarto_cache_version: 1`. q2 adds its own keys beside them (`quarto_record_version`,
   `quarto_engine`, `quarto_exec_hash` and the document-level execution hashes).
   `quarto_cache_version: 1` has no consumer in q2\'s own flow, because q2 never writes
   into Positron\'s cache directory; it is defensive only, so a record that is dropped in
   by hand passes Positron\'s validator (and is then consumed, see the Background). Keeping
   it ties q2 to Positron\'s validation rules.
3. A Positron-written record is display-only: never feeds reuse or freeze (no provenance,
   possibly out of order, stale kernel state).
4. knitr asymmetry is stated, not solved here: a q2 knitr record holds rendered
   `text/x-quarto-markdown`, which Positron cannot present; executor independence for R
   holds only when `{r}` runs through a Jupyter kernel (the JOL plans allow it). The
   follow-on knitr epic (epic decision 6) aims to close it.
5. The reasons the record is not Positron\'s file are in EXR-4 (Design decisions) and are
   not repeated here; the schema differences are tabulated there.
6. **The importer normalizes at read**, so records
   only ever hold standard nbformat. It accepts `execute_result` where q2 would have
   `display_data`, parses string-valued JSON mimes into objects (left as the original string
   with a logged notice if the string is not valid JSON), and ignores
   `quarto_webview_metadata`. Tolerance lives in the importer because a presenter that
   embedded the string would double-encode it.
7. **A language gate.** Positron records no language
   per cell, so the gate checks the q2 side: import outputs only into units whose owner is
   the Jupyter engine and whose language token is a plain kernel language. Never attach
   Positron outputs to a unit another engine owns (for example `python.marimo`, owned by
   marimo, which Positron reads as `python`). The gate cannot catch a Positron cell and a
   q2 cell of a different language with identical text (the hash has no language); the
   occurrence rule and the owner check are the only protection, and a test documents it.
8. `quarto_lang` in a record holds the full token (`python.marimo`), and the conformance
   fixtures include dotted and class-attribute forms: equal hashes, different languages and
   different label segments, documented (Phase A).
9. **The Positron items are spikes, not proposals** (decided with Gordon, 2026-10-30). They
   are built and run in a checkout of Gordon\'s fork of Positron to test viability and, if
   all goes well, to show a demo. Spike code need not be production quality. Nothing is
   sent to anyone: no pull request, no issue, no message. What the spikes teach may later
   become a contribution or a collaboration, which is a separate decision.

## Out of scope

- Contributing, proposing or sending anything to the Positron or Ark projects. Spike
  branches stay in Gordon\'s fork; any pull request or discussion is a separate decision
  made after the findings.
- Production-quality Positron code, and support for q2 records in a shipped Positron.
- Freeze, reuse, or any trust in imported records.

## What q2 does alone

### Phase A: tests first

- [ ] Hash conformance: a fixture table of cell texts (with `#|` lines, CRLF (equal to its
  LF twin, as Positron splits on `\r?\n`), trailing
  newline, Unicode, empty cell) and the expected 16-character hashes. The Positron-side
  expected values come from running Positron\'s `computeContentHash` in spike S1 and are
  committed as data; until S1 has run, the table holds the node `crypto` vectors listed
  in S1 and is marked provisional. q2\'s hash function is tested against the table.
  (Cross-site tests of this kind are the main protection against risk 1 in the epic.)
- [ ] Cell id conformance: `quarto_cell_id` strings for the same fixtures, including a
  duplicate pair and a document with an inline expression span before a cell (the cell\'s id
  is the same as without the span; span ids use their own counter, EXR-4). Divergences are
  asserted as divergences, with the expected Positron id stored beside q2\'s: a cell with
  `#| label: fig-x` (q2 `fig-x`, Positron `unlabeled`), a brace-label cell
  (`{python, fig-x}`), and cells in a blockquote, a list and a four-backtick fence (index
  differs; EXR-2\'s corpus carries the expected Positron index).
- [ ] Language-token fixtures (shared with Positron): `{python.marimo}`, `{python .marimo}`
  and `{Python}` for the same text. Expected: equal hashes on both sides; q2 tokens
  `python.marimo`, `python` plus class `marimo`, and `python` (case-insensitive); Positron
  reads `python` for all three, with label `.marimo` for the first two and `unlabeled` for
  the third. The differing languages and labels are asserted, not hidden.
- [ ] Import tests: given a Positron cache file (a fixture in its exact shape), q2 produces
  display-only unit outputs matched by content hash; an unmatched or stale cell is skipped
  silently; a file with a wrong `quarto_cache_version` is read but flagged, never deleted;
  an `execute_result` carrying all mimes in `data` is accepted; `quarto_webview_metadata` is
  preserved or ignored without error.
- [ ] Normalization tests: a Positron `execute_result` for what q2 would call
  `display_data` imports as the standard form; a string-valued `application/json` and a
  string-valued `+json` mime come out as objects; an invalid JSON string is kept as the
  string with a logged notice; the imported output is spec-conformant nbformat.
- [ ] Language-gate tests: a cell whose q2 token is `python.marimo` receives no Positron
  outputs; a plain `{python}` Jupyter-owned cell does; a cell owned by another engine
  receives none; a different-language cell with identical text is documented as not
  caught (Risks).
- [ ] Round-trip safety test: a q2 record read by a Positron-compatible validator (a
  fixture-level reimplementation of its structure checks) passes with
  `quarto_cache_version: 1`, and the test documents what happens if the key is set to
  any other value (Positron deletes the file). This guards the defensive key only; no q2
  flow depends on it. Spike S2a replaces the reimplementation
  with Positron\'s real loader once it has run.
- [ ] Trust test: imported units carry a display-only provenance marker and cannot be used
  where a reuse decision is made (the type system, or an explicit check with a test).

### Phase B: implementation

- [ ] Import function in the record library (EXR-4\'s crate): read a Positron cache file,
  normalize at read, apply the language gate, match by content hash, return display-only
  unit outputs. No CLI surface here; callers are EXR-14 (preview) later if wanted, and
  spike S6 uses it for the demo.
- [ ] A fixture generator in q2 that writes the conformance fixtures (qmd plus the units,
  ids and hashes q2 expects) into `tests/fixtures/positron-conformance/` of the crate that
  holds EXR-4\'s record library (the crate path is fixed by EXR-4 Phase 1; write it into
  this task then), with a README in that directory that states the format and the exact
  commands to regenerate them. Spike S1\'s vitest finds the directory through the
  environment variable `Q2_POSITRON_FIXTURES` (an absolute path; the vitest fails with a
  message naming the variable when it is unset), because Positron\'s checkout is outside
  the q2 worktree. Whether the fixtures are ever offered to Positron is decided after S1.
- [ ] Keep the two version keys documented in one place (EXR-4\'s schema doc), stating
  plainly that `quarto_cache_version` is defensive, and assert in a test that q2 never emits
  it with a value other than 1.

## Positron spikes

Each spike answers one question about whether an idea is viable and ends in a short
findings note with a go or no-go. All of them are **not started**.

### Ground rules

- **Where.** Gordon\'s fork `gordonwoodhull/positron`, which exists since 2026-10-30
  (`origin` in `/Users/gordon/src/positron`; `upstream` is `posit-dev/positron`). Spike
  branches are local or pushed to the fork only. Nothing goes to `upstream`.
- **Branches.** One branch per spike, named `spike/q2-<id>-<topic>` (for example
  `spike/q2-s1-conformance`; epic decision 30 fixes the naming). Each is cut from the
  then-current `upstream/main`; that cut-from rule is set here and is not part of
  decision 30.
- **Worktree.** Every spike works in a separate `git worktree` of the fork, never in
  `/Users/gordon/src/positron` itself: `external-sources/positron` is a symlink to that
  checkout, and other plans cite line numbers in it, so it stays on `main`.
- **Ark stays local.** `external-sources/ark` has only `origin`
  (`https://github.com/posit-dev/ark.git`) and no Gordon fork. The S5 branch is local; never
  push it to `origin` or anywhere else.
- **Positron\'s conventions.** New files over edits; unavoidable edits to upstream files wrapped
  in `// --- Start Positron ---` and `// --- End Positron ---`; no direct `tsc` on the
  main project; `npm run test:positron` and `npm run test:positron:check-ts` for vitest.
- **Findings.** Each spike writes a short markdown research note at
  `claude-notes/research/2026-MM-DD-exr-16-<spike id>-<topic>-findings.md` (the `research/`
  directory already holds dated notes; CI renders `claude-notes` with `--strict`, so use `.md`
  and render the note with `q2 render --strict` before pushing) with
  the question, the method, the result, a go or no-go per idea, and the branch name.
  Findings may later become a contribution or a proposal; nothing is sent.
- **Quality bar.** Experiments. Hard-code, skip edge cases and leave rough code, as long as the
  result answers the question and the note says what was not tested.

### S0: environment

- **Question.** How hard is it to get a working Positron dev build with a Python kernel and
  an R kernel (Ark) from the fork, and how long does it take?
- **Setup.** The fork already exists and the branch naming scheme is decided (see the
  conventions above). Create the separate worktree required by the ground rules. Run `npm install`, start the build daemons
  (`npm run build-start`, `npm run build-check`), and launch the dev build.
- **Method.** Record each step and its duration in the findings note. Confirm that a `.qmd`
  opens with inline outputs working (the README says the feature needs the Quarto extension
  present; check whether a dev build has it). Confirm a Python session and an R session
  (Ark) start. Run `npm run test:positron` once to confirm vitest works without the daemons.
- **Success and failure.** Success: a dev build that runs a cell in a `.qmd` and shows its
  output inline. Failure: no working build after at most 2 sessions; then S3 and S6 are
  blocked and the unit-level spikes (S1, S2a, S4) are all that remains.
- **Deliverable.** Findings note S0, with the commands and timings.
- **Effort.** 0.5 to 1 session; the build time is unknown.
- **Dependencies.** None. S1 and S2a need only vitest and can start before the dev build works.

### S1: conformance in Positron\'s parser

- **Question.** Where do Positron\'s parser, `computeContentHash` and cell ids disagree
  with q2\'s on the same documents?
- **Setup.** Branch `spike/q2-s1-conformance`. A new vitest file next to
  `quartoDocumentModel.vitest.ts` that reads q2\'s fixture directory through
  `Q2_POSITRON_FIXTURES` (qmd plus the units, ids and hashes q2 produced; the generator and
  directory are in Phase B above). No build daemons needed.
- **Method.** Run each fixture through Positron\'s parser and `computeContentHash` and
  compare with q2\'s expected values. Cover: the non-ASCII and lone-surrogate hash vectors,
  dotted and class-attribute fence forms, labeled cells (a `#| label:` line and a brace
  label), blockquote, list and four-backtick fences, a `{python}` line inside an unbraced
  display fence (a phantom cell in Positron), CRLF documents, and the exclusion of raw
  `{=format}` blocks. The vectors computed with node `crypto` (SHA-1 over
  UTF-8, first 16 hex characters) in the review, **not yet run against Positron\'s
  `StringSHA1`**:

  | Text | Expected hash |
  |---|---|
  | `x <- 1` | `4390f67858f3f1c7` |
  | `café` | `f424452a9673918c` |
  | `😀` | `9c533688a979a858` |
  | `#| label: fig-é` then a newline then `x <- 'é😀'` | `774a7e618b133ed6` |

  Add a lone-surrogate vector; Positron\'s `StringSHA1` turns a lone surrogate into U+FFFD
  where a plain UTF-8 encoder may not.
- **Success and failure.** Success: every hash agrees, and every id or index disagreement
  is tabulated with its cause. Each divergence found against Positron\'s parser (label
  segment, nesting and index, phantom cells, `.marimo` labels) is recorded as a finding, not
  fixed. Failure is not a spike failure; a disagreement is a finding.
- **Deliverable.** Findings note S1 with the table of agreements and disagreements. It
  closes the SHA-1 UTF-8 spike in EXR-4 and supplies the Positron-side expected values for
  Phase A.
- **Effort.** 0.5 session.
- **Dependencies.** The q2 fixture generator (Phase B). It can start with the provisional
  vectors above.

### S2: Positron reads a q2-shaped record

- **Question.** What does Positron do with a record that has extra keys, inline-expression
  units and rendered outputs, and is reading q2 records a viable way to seed its cache?
- **Setup.** Branch `spike/q2-s2-read-record`. Part (a) is a vitest on the cache service
  with an in-memory file service. UNVERIFIED that Positron\'s vitest harness can supply one:
  no `quartoOutputCacheService.vitest.ts` exists today (checked 2026-10-31 in
  `src/vs/workbench/contrib/positronQuarto/test/browser/`), and the four existing tests that
  mention the service (`quartoKernelManager`, `quartoOutputManagerSaveAs`,
  `quartoOutputManagerReopen`, `quartoOutputManagerService`) are the places to look for a
  mock file service first. If none works, part (a) becomes a dev-build test. Part (b) is
  manual in the dev build.
- **Method.**
  - (a) Load a record containing: extra notebook and cell keys, inline-expression cells,
    `display_data` and JSON-mime objects, a `text/x-quarto-markdown` output, a cell
    without an `outputs` array, and a wrong `quarto_cache_version`. Assert what loads, what
    is ignored and what is deleted, and what survives a Positron rewrite (unknown: whether
    unmatched inline cells and orphan cells are kept when Positron writes the file).
  - (b) Drop a q2 record into Positron\'s cache directory under an arbitrary name and open
    the matching document; this exercises the hash-scan seed path (`findCacheByContentHash`,
    `:611` to `:690`). Expected from reading the code, to confirm: one matching cell hash
    loads the whole file into the document, the file is **deleted** (`:686`), and Positron
    rewrites it from its fixed fields, dropping q2 metadata. Also try a record whose only
    match is one cell shared with an unrelated document, to confirm that it seeds that
    document. Observe what is shown for plain cells, for rendered units with an unknown
    mime, and for inline units.
- **Success and failure.** Success: matching cells restore their outputs and nothing is
  deleted that should not be (a consumed hand-placed record is expected, and is a finding
  about interop, not a spike failure). Failure: any structural check deletes a valid record, or an
  unknown mime breaks rendering; both are recorded as findings with the minimal fix.
- **Deliverable.** Findings note S2, with the vitest cases kept on the branch. Replaces
  the fixture-level validator in the q2 round-trip test (Phase A).
- **Effort.** 1 session.
- **Dependencies.** A sample q2 record (EXR-4\'s builder, or one written by hand). Part (b)
  needs S0.

### S3: execution of inline expressions in the editor

This is the main spike and the demo candidate.

- **Question.** Can Positron run inline expressions on a document\'s kernel and show the
  values inline, and which route should a future contribution take: inside core, or
  through the extension API with an upstream change?
- **Setup.** Branch `spike/q2-s3-inline-exec`, cut after S0. Work inside
  `quartoExecutionManager.ts` and its queue (new files where possible, marked edits
  otherwise), plus a rendering path in or beside `quartoOutputManager.ts`.
- **Method.**
  - Detect spans with a regex (a span is a code span whose text starts with a language in
    braces, then a space, then the expression), starting with the document\'s primary
    kernel language; later switch to the scanner S1 has checked.
  - Run each span as a one-line unit through the existing queue with `session.execute`,
    in document order with the cells, with the same observer that collects every message.
    Try `Transient`, `Silent` and `Interactive` modes.
  - Render the value inline in the editor (a decoration or injected text, not a view zone).
  - Measure: console echo, history entries, `display_data` and error values, ordering with
    cells, interrupt and restart behavior, and cost with many spans.
  - Also run the extension-API route, `positron.runtime.executeCode` with an observer, and
    confirm by test the two gaps: only the first `Result` message resolves, so a span whose
    value is `display_data` returns `{}`; and `Transient` echoes in the console.
- **Success and failure.** Success: a document with cells and spans shows span values inline
  after a run, in order, with a defensible answer on echo and history. Failure: no mode gives
  values without unacceptable console or history side effects; then the finding is what
  Positron would have to add.
- **Deliverable.** Findings note S3 with the measurements, a recommended route, and the
  working branch. A short screen capture is optional here; the S6 demo has one (decision 30).
- **Effort.** 2 to 3 sessions.
- **Dependencies.** S0. S4 runs alongside.

### S4: the span value rule

- **Question.** Does "the last output carrying a text mime type, never keyed on
  `output_type`" give the same values as q2\'s rule for execution of inline expressions
  (EXR-3)?
- **Setup.** Branch `spike/q2-s4-value-rule`, or a commit on the S3 branch. A vitest, with
  fixtures shared with EXR-3.
- **Method.** Implement the rule over recorded message sequences and compare with EXR-3\'s
  expected strings for: an integer, a string, markdown, HTML, `None`, a DataFrame repr, a
  statement (which yields no value), an error, and a value that arrives only as
  `display_data`.
- **Success and failure.** Success: identical strings on every case, or a short list of
  differences with a recommended rule change on one side.
- **Deliverable.** Findings note S4 and the shared fixture file.
- **Effort.** 0.5 session.
- **Dependencies.** EXR-3 (its expected-strings table; soft), S3 for live message sequences.

### S5 (stretch): Ark and `user_expressions`

- **Question.** Is the kernel-protocol route, `user_expressions` answered by Ark for R,
  viable, compared with running R spans as cells?
- **Setup.** A local branch in Gordon\'s checkout of Ark, `external-sources/ark` (not
  Positron; see the ground rule that Ark stays local), and an Ark build. Skip this spike if S3 shows that running spans as cells is enough.
- **Method.** Replace the stub at `ark/crates/ark/src/console/console_repl.rs:2215` with a
  minimal implementation that evaluates each expression in R and replies with
  `text/plain`. Test with a plain `jupyter_client` against the built kernel, without
  Positron.
- **Success and failure.** Success: correct values, errors reported per expression, and no
  disturbance of the console state. Failure: evaluation needs the console lock or changes
  user-visible state; then cells remain the route.
- **Deliverable.** Findings note S5 and the Ark branch.
- **Effort.** 1 to 2 sessions; needs a Rust build of Ark.
- **Dependencies.** S3 (to know whether it is needed).

### S6: interop demo

- **Question.** Can one document be shown end to end across both tools?
- **Setup.** One `.qmd` with Python cells and spans. Needs the q2 import function (Phase B)
  and the S2 and S3 branches merged into one demo branch of the fork.
- **Method.** Run the document in Positron so the cache is written and spans show inline;
  have q2 import Positron\'s cache and preview the document with those outputs and no
  kernel; seed Positron\'s cache from a q2 record and open the document (Positron consumes the file
  and rewrites it; keep a copy of the record). Write a short
  script of the steps and record the screen (a short capture; epic decision 30).
- **Success and failure.** Success: the three legs work in sequence on the same document.
  A leg that fails is reported with its cause; a partial demo still counts.
- **Deliverable.** The script, the demo branch, and findings note S6 (the capture, if any).
- **Effort.** 1 to 2 sessions.
- **Dependencies.** S2, S3 and the q2 import function.

### Ordering

S1 and S2a first (no build needed), with S0 in parallel; S3 after S0, with S4 alongside it;
S5 only if wanted; S6 last.

## Exit criteria

- All Phase A tests green; `cargo nextest run` on the record crate.
- The fixture generator and its README are committed, and S1 has run the fixtures through
  Positron\'s parser.
- Each spike that ran has a findings note with a go or no-go and its branch name. A spike
  that was skipped or blocked says so and why.
- If S0 through S4 are go, the S6 demo exists; otherwise the notes say what blocked it.
- Nothing has been sent to or proposed to anyone.

## Risks and open questions

- **Drift in Positron\'s hash or id format** invalidates matching silently on one side. The
  ids already differ in label and index today (Background), so only the hash is a shared key. The
  conformance fixtures are the mitigation; S1 is the first Positron-side run, and the
  generator makes a rerun cheap on each q2 release.
- **The dev build may be slow or fragile.** S0 may take much longer than estimated, and
  blocks S3 and S6. The vitest-only spikes (S1, S2a, S4) do not depend on it.
- **Spike branches drift from `upstream/main`.** Rebase before each spike, and keep spike
  code in new files to limit conflicts.
- **The Quarto feature may not be active in a dev build.** Not verified; S0 checks it.
- **`quarto_cache_version` coupling.** The key has no consumer in q2\'s flow (q2 never writes
  into Positron\'s directory) and is defensive only; emitting `1` ties q2 to Positron\'s
  validation rules, and if Positron bumps its version, q2 must decide whether to follow or
  drop the key. Tracked as an EXR-4 schema note.
- **A q2 record in Positron\'s cache directory is consumed.** Positron deletes the file
  on a single matching cell hash and rewrites it without q2 metadata; it can also seed an
  unrelated document. q2 documents this and never does it itself.
- **Display-only import value** is unproven: how often users have both tools on one project
  is unknown. The import is cheap; do not extend it without evidence.
- **The language gate is partial.** Positron\'s cache has no language, so identical text in
  different languages still matches by hash; only the owner check and the occurrence rule
  guard it. Tests document the gap.
- **Normalization and the real Positron** are untested together until S2 runs: no real
  Positron round trip has been run, and the JSON-mime string behavior comes from reading its
  code.

## References

- `external-sources/positron/src/vs/workbench/contrib/positronQuarto/browser/quartoOutputCacheService.ts`,
  `.../quartoDocumentModel.ts`, `.../quartoExecutionManager.ts`,
  `external-sources/positron/src/positron-dts/positron.d.ts`
- `external-sources/positron/src/vs/workbench/api/common/positron/extHostLanguageRuntime.ts`
- `external-sources/ark/crates/ark/src/console/console_repl.rs`
- Handoff section 6, `~/docs/claude/ipynb-result-caching/HANDOFF.md`
