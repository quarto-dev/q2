# Plan: Q-2-36 — Clean parse error for old-style knitr chunk options

- **GH issue:** [#152](https://github.com/quarto-dev/q2/issues/152) (chunk-options half; the table-captions half closed via #154)
- **Triage:** `claude-notes/issue-reports/152/q236-triage.md`
- **Fixtures:** `claude-notes/issue-reports/152/q236-repro.qmd`, `q236-repro-variants.qmd`
- **Beads:** bd-j4fe
- **Branch:** `issue-152` (based on `bugfix/issue-184` @ `e2d224f6`; will rebase onto `main` once #184 lands)
- **Approach:** upgrade the existing Q-2-8 warning site to a Q-2-36 *error*; Merr-map the parse-error forms that already error today. **No `scanner.c` change, no `grammar.js` change.** See triage `Approach` section for why scanner-emit is the wrong shape here.

## Overview

Old-style knitr chunk headers — `{r echo=FALSE}`, `{r test}`, `{r, label="foo"}`, any engine — must fire a clean Q-2-36 parse error pointing users at the `#| key: value` body syntax. The **Pandoc class form** `{.r echo=FALSE}` (leading dot on the language) stays valid; it is the supported Quarto 2 spelling.

Two structurally distinct paths produce knitr-style errors today, and both need to land on Q-2-36:

- **Path (A) — space-separated kv pairs** (`{r echo=FALSE}`, `{python label="foo"}`, etc.). Grammar accepts them; a Q-2-8 *warning* is emitted in `crates/pampa/src/pandoc/treesitter.rs:1121-1144`. Fix = upgrade that warning to a Q-2-36 *error*, clip the highlight to the header line.
- **Path (B) — bare label and comma-form** (`{r test}`, `{r, label="foo", echo=FALSE}`). Grammar rejects them; tree-sitter raises a parse error, but the `(state, sym)` pair is unmapped in the Merr table so the user sees the generic fallback. Fix = add `Q-2-36.json` corpus entries; the build script captures the parse states; `widen_diagnostic_to_line` spreads the highlight across the full header line.

The negative control — Pandoc class form `{.r echo=FALSE}` — is already correctly accepted, because the Q-2-8 gate keys on the literal `{...}` braces, not the engine name.

## Test plan (TDD — write first, watch fail, then fix)

The error-corpus snapshot tests in `crates/pampa/tests/test_error_corpus.rs` are the failing-test artifact for path (B): once `Q-2-36.json` lands and the table regenerates, the new `Q-2-36-*.qmd` case files will be globbed in automatically. Snapshots are produced under `crates/pampa/snapshots/error-corpus/`.

For path (A) the failing-test artifact is the existing `test_code_block_with_header_options_produces_warning` in `crates/pampa/tests/test_warnings.rs:498-534`. We flip it from "expect Q-2-8 warning" to "expect Q-2-36 error" — that's the failing test that drives the upgrade.

- [x] **Phase 0a — Failing tests for path (A)** (before any code change)
  - [x] Rewrite `test_code_block_with_header_options_produces_warning` in `crates/pampa/tests/test_warnings.rs` to assert a `Q-2-36` *error* (kind `DiagnosticKind::Error`) is produced, not a Q-2-8 warning. Renamed to `test_code_block_with_header_options_produces_q236_error`. **Failing at HEAD as expected** — output: `Expected Q-2-36 error, but parse succeeded with warnings: [Some("Q-2-8")]`.
  - [x] Flip `test_code_block_with_id_and_options_produces_warning` to expect a Q-2-36 error too. `{python #fig-test key=value}` will become a parse error after Phase 1 (an id alone doesn't make a header Pandoc-shaped — only a class does), so the test must be rewritten or it would regress. Renamed to `test_code_block_with_id_and_options_produces_q236_error`. **Failing at HEAD as expected.**
  - [x] Retarget the three negative-control tests so they filter for `Q-2-36` (and still `Q-2-8`, for the transition window) instead of just `Q-2-8`. After the Phase 1 upgrade, `Q-2-8` will not fire anymore, so a filter on `Q-2-8` alone trivially passes and stops protecting against regressions. The triple-filter `matches!(code, Some("Q-2-36") | Some("Q-2-8"))` keeps the negative-controls meaningful in both states. Renamed: `…_no_q236`. **Passing at HEAD as expected.**
  - [x] Run `cargo nextest run -p pampa --test test_warnings 'test_code_block_with'` + `'simple_code_block_no_q236'`. Confirmed: 2 fail (the two positive cases), 3 pass (negative controls).

- [x] **Phase 0b — Merr corpus for path (B)** (before any code change to the parse path)
  - [x] Added `crates/pampa/resources/error-corpus/Q-2-36.json` with three cases: `bare-label`, `comma-args`, `comma-and-kv`.
  - [x] Ran `./crates/pampa/scripts/build_error_table.ts`. Regenerated `case-files/Q-2-36-*.qmd` and `_autogen-table.json` (Q-2-36 now has 6 `(state, sym)` mappings — multiple states per case is normal). No "duplicate (lr_state, sym)" warnings for Q-2-36 (the only duplicates flagged are pre-existing in Q-2-7).
  - [x] Ran `cargo nextest run -p pampa --test test_error_corpus`. All 4 corpus tests pass. The corpus snapshot tests glob `error-corpus/*.qmd` (top-level, currently empty) rather than `case-files/`, so they don't produce new snapshots for the case files. The `ariadne_output` and `json_locations` tests do iterate `case-files/` and assert that each file produces errors — those tests confirm the new case files all emit diagnostics. (Plan revision from earlier draft: the "snapshots missing" failure mode I anticipated didn't happen because of the test layout; the tests effectively go green the moment the corpus and table are in place.)
  - [x] Direct `cargo run --bin pampa -- <case-file>` on all three case files confirms each emits `[Q-2-36] Old-style knitr chunk options are not supported` with the planned message body. Highlights are still narrow (just the offending token) — Phase 2's `widen_diagnostic_to_line` will spread them.
  - [x] Ran full `cargo nextest run -p pampa --no-fail-fast`. **3685 pass / 2 fail / 2 skipped.** The 2 failures are exactly the Phase 0a TDD scaffolds; no other test was displaced by the new Q-2-36 `(state, sym)` entries.

### Error-corpus cases for `Q-2-36.json`

Each case is a small `.qmd` that exercises one parse-error shape:

| Case `name`     | Trigger                         | Why it matters                                                                         |
| ---             | ---                             | ---                                                                                    |
| `bare-label`    | `` ```{r test} `` + body        | The reporter's exact case. Bare identifier after the language token.                   |
| `comma-args`    | `` ```{r, echo=FALSE} `` + body | Canonical knitr "comma after language" form.                                           |
| `comma-and-kv`  | `` ```{r, label="foo", echo=FALSE} `` + body | Two-argument variant. Confirms the mapping is on the *first* error state, not later. |

Use `crates/pampa/resources/error-corpus/Q-2-35.json` as the structural template (no `notes`, `captures: []` since `widen_diagnostic_to_line` will spread the highlight). The Q-2-36 metadata block is:

```json
{
  "code": "Q-2-36",
  "title": "Old-style knitr chunk options are not supported",
  "message": "Quarto Markdown does not support knitr-style chunk options in the header. Move options into the body using `#| key: value` syntax, or use the Pandoc class form `{.r ...}` instead of `{r ...}`.",
  "notes": [],
  "cases": [ ... ]
}
```

The **space-kv form** (`{r echo=FALSE}`) is deliberately *not* in the corpus. It is handled by the upgraded `treesitter.rs` site (Path A), and adding it to the corpus would either parse cleanly (because the grammar accepts it) and crash the corpus harness, or — after the Phase 1 upgrade — fire two diagnostics for the same input.

## Work items

### Phase 1: upgrade Q-2-8 site to Q-2-36 error

- [x] In `crates/pampa/src/pandoc/treesitter.rs:1121-1144`, replaced `DiagnosticMessageBuilder::warning(...)` with `error(...)`. Updated code → `"Q-2-36"`, title → `"Old-style knitr chunk options are not supported"`, problem/info/hint text per plan.
- [x] Clipped the diagnostic location to the **header line** of the code block inline at the diagnostic site (walks `cb.source_info`, finds the first `\n`/`\r` in `input_bytes`, builds a clipped `SourceInfo::Original`). Sibling `widen_diagnostic_to_line` in `qmd_error_messages.rs` would have widened correctly too, but inlining keeps path A self-contained: Phase 2's widen gate now only has to handle path B's narrow-token highlights.
- [x] Two new TDD scaffolds in `test_warnings.rs` now PASS: `test_code_block_with_header_options_produces_q236_error`, `test_code_block_with_id_and_options_produces_q236_error`. Three negative controls (`{.python .marimo}`, `{r .myclass eval=FALSE}`, `{python}`) continue to pass.
- [x] Surfaced 3 sibling tests in `crates/pampa/tests/test_code_block_attributes.rs` that asserted the *legal* shape of the now-rejected `{python key=value}` form. **Disposition:** deleted `test_language_with_key_value` (redundant with `test_language_with_all_attributes`); retargeted `test_language_with_multiple_key_values` and `test_quoted_attribute_value` to add a Pandoc class (`.myclass`) so they bypass Q-2-36 while preserving their unique coverage (multi-kv extraction and quoted-value-with-spaces). Added a top-of-file comment explaining the constraint.
- [x] End-to-end smoke through `cargo run --bin pampa --`:
  - Reporter's `q236-repro.qmd` (`{r test}`) → clean `[Q-2-36]` error, highlight on the offending token. (Path B; widen pending Phase 2.)
  - Stdin `{r echo=FALSE}` → clean `[Q-2-36]` error, **highlight now spans only the header line** (`1 │ ` ` ```{r echo=FALSE}` ` with single-line underline). (Path A, clipped inline.)
  - Stdin `{.r echo=FALSE}` (negative control) → parses cleanly, no diagnostic.
- [x] Full pampa suite: **3686/3686 pass, 2 skipped, 0 failures.**

### Phase 2: widen path-B highlights to the full header line

After Phase 1 + Phase 0b, path A is already clipped to the header line (inline at the treesitter.rs site) and path B emits Q-2-36 with a narrow highlight on the offending token (`test` in `{r test}`, `r` in `{r, echo=FALSE}`, etc.). Phase 2 only widens path B.

- [x] Extended the location-widening gate in `crates/pampa/src/readers/qmd_error_messages.rs:40` so `Q-2-36` joins `Q-2-35`:

  ```rust
  if matches!(diag.code.as_deref(), Some("Q-2-35") | Some("Q-2-36")) {
      widen_diagnostic_to_line(diag, input_bytes);
  }
  ```

  Updated the doc-comment on `widen_diagnostic_to_line` with separate bullets explaining the Q-2-35 and Q-2-36 motivations (whitespace consumption vs. narrow-token report) so future readers can decide whether to enroll a third code without re-deriving the rationale.
- [x] **Confirmed path A and path B route through separate code paths.** Path A diagnostics are constructed in `treesitter.rs::process_fenced_code_block` and added directly to `error_collector`; they do not flow through `qmd_error_messages::produce_diagnostic_messages`, so the widen gate cannot double-apply or interfere with the inline-clipped path-A location. Verified by re-running path A inputs after Phase 2: highlights remain single-line, identical to the Phase 1 verification output.
- [x] **E2E path B verification** — all three case files now show full-header-line highlights:
  - `Q-2-36-bare-label.qmd` (`` ```{r test} ``): underline spans `` ```{r test} ``
  - `Q-2-36-comma-args.qmd` (`` ```{r, echo=FALSE} ``): underline spans `` ```{r, echo=FALSE} ``
  - `Q-2-36-comma-and-kv.qmd` (`` ```{r, label="foo", echo=FALSE} ``): underline spans the whole 31-character header.
- [x] `cargo nextest run -p pampa --no-fail-fast`: **3686/3686 pass, 2 skipped.** No collisions, no regressions.

### Phase 3: end-to-end verification

- [x] `cargo run --bin pampa -- claude-notes/issue-reports/152/q236-repro.qmd` — clean `[Q-2-36]` error on the header line (line 5). Output captured in "Verification output" below.
- [x] Variants fixture — per-case isolation: each of cases (1)–(7) fires `[Q-2-36]` when run as a single-block file (verified via `/tmp/q236-cases/case{1..8}.qmd`). Case (8) (`{.r echo=FALSE}`) parses cleanly to `[ CodeBlock ( "" , ["r"] , [("echo", "FALSE")] ) "1+1" ]`.
- [x] **Discovered limitation, filed as `bd-jvxg`:** when the variants fixture is run as a single file (rather than one case per file), only the two **Path B** parse errors (cases 1 and 4) appear. Cases 2, 3, 5, 6, 7 are silently dropped because `crates/pampa/src/readers/qmd.rs:144` returns `Err(parse-error-diagnostics)` before `treesitter_to_pandoc` runs — and `treesitter_to_pandoc` is where the Path A diagnostic site lives. This is **pre-existing reader-architecture behavior** (would have happened at HEAD with the old Q-2-8 warnings too, just less noticeably), not a Phase 1 regression. Filed `bd-jvxg` as `discovered-from:bd-j4fe` so the architectural fix is tracked without blocking this ship.
- [x] `cargo nextest run --workspace --no-fail-fast`: **8858 pass / 0 fail / 195 skipped.** No downstream regressions in any crate (`qmd-syntax-helper`, `quarto-core`, `wasm-quarto-hub-client`, etc.).
- [x] `cargo xtask verify` (full, with hub-build leg): **All 9 verification steps passed.** This covered `cargo build --workspace`, `cargo nextest run --workspace`, `cargo xtask lint`, the hub-client TypeScript + Vite + WASM build, hub-client vitest, and trace-viewer vitest. Required `npm install` at worktree root first since this is a fresh worktree.
- [x] Snapshot impact: zero. No `.snap` files changed; the only modifications to `snapshots/` would have appeared if path-A or path-B touched a snapshotted output path, and they didn't.

### Phase 4: documentation + commit

- [x] `docs/syntax-notes.md` (or nearest user-facing doc): **skipped.** No existing page under `docs/` discusses fenced-language chunk-header syntax (`grep -l '\`\`\`{' docs/**/*.qmd` returns empty). Per plan, "do not invent a new page." The Q-2-36 diagnostic message itself carries the redirection to `#| key: value` / `{.r ...}`, which is the user's actual touchpoint.
- [x] Three commits on `issue-152` branch:
  - `9bbb1de1` Q-2-36 Phase 0: failing tests + Merr corpus
  - `bd93ffa2` Q-2-36 Phase 1: upgrade Q-2-8 warning site to Q-2-36 error
  - `e848e47e` Q-2-36 Phase 2: widen path-B highlights to the full header line
- [ ] Phase 3 verification commit (this plan update + Phase 3 outcome) — pending.
- [ ] From the **main** repo (not this worktree), run `br sync --flush-only && git add .beads && git commit -m "sync beads: bd-j4fe + bd-jvxg"`.
- [ ] Wait for explicit user approval before pushing.

## Out of scope (do not creep)

- Scanner or grammar changes. The triage doc spells out why these are wrong-shape for Q-2-36 — see *Approach* section there.
- Generalising the warning-to-error infrastructure. Q-2-8 is the only warning we're touching; no need for a code-mapping framework.
- Treating `{r ...}` as a *valid* dialect under any flag (`--loose`, `--knitr`, etc.). Project decision per the GH issue: clean error, no compatibility shim.
- Auto-fix (rewriting `{r echo=FALSE}` → `{r}\n#| echo: false`). Tracked separately if anyone wants it; out of scope for this plan.
- Cleaning up the Q-2-8 code path globally. The site is the only emitter of Q-2-8 today; once it becomes Q-2-36 the Q-2-8 code is dead. **Decision for after Phase 1:** verify with `grep -rn "Q-2-8" crates/` that nothing else references it (tests, snapshot fixtures, docs). If clean, retire `Q-2-8` from the error registry; if not, leave the code intact and let a follow-up beads handle the cleanup.

## Verification output

### Reporter's exact case (`q236-repro.qmd`)

Invocation:

```
$ cargo run --bin pampa -- claude-notes/issue-reports/152/q236-repro.qmd
```

Output (ANSI escapes stripped, hyperlink data stripped, layout preserved):

```
Error: [Q-2-36] Old-style knitr chunk options are not supported
   ╭─[ claude-notes/issue-reports/152/q236-repro.qmd:5:1 ]
   │
 5 │ ```{r test}
   │ ─────┬─────
   │      ╰────── Quarto Markdown does not support knitr-style chunk options in
   │              the header. Move options into the body using `#| key: value`
   │              syntax, or use the Pandoc class form `{.r ...}` instead of
   │              `{r ...}`.
───╯
```

The diagnostic spans the full header line. Output was inspected directly. (This goes through path B — the bare-label form is a tree-sitter parse error mapped through `Q-2-36.json` in the corpus, then widened to the line via `widen_diagnostic_to_line`.)

### Representative path-A case (`{r echo=FALSE}`)

Path A — the upgraded `treesitter.rs` site — is exercised by inputs the grammar accepts structurally:

```
$ printf '%s\n' '```{r echo=FALSE}' '1+1' '```' | cargo run --bin pampa --

Error: [Q-2-36] Old-style knitr chunk options are not supported
   ╭─[ <stdin>:1:1 ]
   │
 1 │ ```{r echo=FALSE}
   │ ─────────┬───────
   │          ╰─────── This code block uses knitr-style options in the header
───╯
ℹ Quarto Markdown reads chunk options from the body, not the header
ℹ Move options into the body using `#| key: value`, or — if you only want a Pandoc class — write `{.r ...}` instead of `{r ...}`
```

Path A uses the rich `problem/info/hint` structure (single inline ariadne span on the header, two info bullets below); path B uses a single inline message paragraph (the corpus JSON's `message` field). The two presentations differ in style but converge on the same `[Q-2-36]` code and message content, so a user encountering either form learns the same thing.

### Pandoc class form (negative control) — must stay valid

```
$ printf '%s\n' '```{.r echo=FALSE}' '1+1' '```' | cargo run --bin pampa --
[ CodeBlock ( "" , ["r"] , [("echo", "FALSE")] ) "1+1" ]
```

No diagnostic. The Pandoc-class spelling (`{.r ...}`) is parsed cleanly with `echo=FALSE` as an attribute. This is the alternative spelling the Q-2-36 diagnostic recommends.

### Test totals

- `cargo nextest run -p pampa --no-fail-fast`: **3686/3686 pass, 2 skipped.**
- `cargo nextest run --workspace --no-fail-fast`: **8858/8858 pass, 195 skipped.**
- `cargo xtask verify` (full, including hub-client TypeScript + Vite + WASM build, hub-client vitest, and trace-viewer vitest): **All 9 verification steps passed.**
- `cargo xtask lint`: not separately run — `cargo xtask verify` runs lint as one of its steps.

### Snapshot-test impact

Zero modified or new `.snap` files. Confirmed via `git status` showing no changes under any `snapshots/` directory at any point during Phases 1–3.
