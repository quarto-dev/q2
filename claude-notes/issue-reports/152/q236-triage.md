# Triage: issue #152 (chunk-options half) — old-style knitr chunk options

- **GitHub:** https://github.com/quarto-dev/q2/issues/152
- **Reporter:** @rundel (Colin Rundel), 2026-05-03 (first half closed via PR #154; this triage covers the second half flagged in the 2026-05-04 comment)
- **Triage date:** 2026-05-14
- **Worktree:** `.worktrees/issue-152` (branch `issue-152`, based on `bugfix/issue-184` @ `e2d224f6`)
- **Beads issue:** bd-XXXX (filed alongside this triage; see Outcome)
- **Scope:** the *chunk-options* half of issue #152. The earlier table-captions half lives in `triage.md` / `repro.qmd` / `exp-*.qmd` in this directory (closed via #154). All Q-2-36 fixtures and docs use a `q236-` prefix to keep the two record-sets visually distinct.

## Summary

Old-style knitr chunk headers (`{r echo=FALSE}`, `{r test}`, `{r, label="foo"}`, etc.) need to fire a clean Q-2-36 parse error directing users to the `#| key: value` body syntax. Crucially, the **Pandoc class form** `{.r echo=FALSE}` (leading `.` on the language) **stays valid** — it is the supported Quarto 2 spelling.

Reproduced all three behavioral classes at HEAD; the work crosses the **existing Q-2-8 warning site** in `crates/pampa/src/pandoc/treesitter.rs` and the **Merr error table** in `crates/pampa/resources/error-corpus/`. **No scanner change is required**, contrary to the Q-2-35 template originally suggested as the model. See *Approach* below.

## Reproduction at HEAD (`bugfix/issue-184` @ `e2d224f6`)

All inputs share the same body (`1+1`) so any visible error is header-shaped. Fixtures: `q236-repro.qmd` (the reporter's exact case) and `q236-repro-variants.qmd` (all seven knitr forms + one Pandoc-form negative control).

Three distinct behaviors observed today:

### (A) Forms that already parse, with a Q-2-8 *warning*

```
$ printf '%s\n' '```{r echo=FALSE}' '1+1' '```' | cargo run --bin pampa --
Warning: [Q-2-8] Code block options in header
   ╭─[ <stdin>:1:1 ]
 1 │ ╭─▶ ```{r echo=FALSE}
 ...
[ CodeBlock ( "" , ["{r}"] , [("echo", "FALSE")] ) "1+1" ]
```

Same with `{r label="foo"}`, `{python echo=FALSE}`, `{julia label="foo"}`. The grammar (`grammar.js:459-490`) **explicitly accepts** these via the `language_specifier → _language_specifier_token + _commonmark_specifier_start_with_kv` rule. The class becomes the literal `"{r}"` (braces kept) and the kv pairs land in the attribute list. The warning is emitted in `crates/pampa/src/pandoc/treesitter.rs:1121-1144`, gated on `classes[0].starts_with('{') && classes[0].ends_with('}')` and `!attrs.is_empty()`.

### (B) Forms that already produce a parse error (generic message)

```
$ printf '%s\n' '```{r test}' '1+1' '```' | cargo run --bin pampa --
Error: Parse error
 1 │ ```{r test}
   │      ──┬──
   │        ╰──── unexpected character or token here
```

Same with `{r, label="foo", echo=FALSE}` (comma form). The grammar has no rule for "language token followed by a bare identifier" or "language token followed by comma," so tree-sitter raises a parse error at the first offending token. The `(state, sym)` pair is in the Merr table but unmapped, so the user sees the generic fallback message.

### (C) Pandoc class form — **passes cleanly, must stay valid**

```
$ printf '%s\n' '```{.r echo=FALSE}' '1+1' '```' | cargo run --bin pampa --
[ CodeBlock ( "" , ["r"] , [("echo", "FALSE")] ) "1+1" ]
```

No warning, no error. Class is `"r"` (no braces) because it came from the CommonMark class form `.r`, not the language-token form `r`. This is the supported Pandoc spelling in Quarto 2; the existing Q-2-8 gate already excludes it because `classes[0]` doesn't start with `{`.

## Localization

| Site | File / line | What it does |
| --- | --- | --- |
| Existing Q-2-8 warning emission | `crates/pampa/src/pandoc/treesitter.rs:1121-1144` | Detects `{lang ...}` with kv attrs, emits warning. **The single most important site for this fix** — upgrade message + level. |
| Existing Q-2-8 test | `crates/pampa/tests/test_warnings.rs:498-563` | Asserts `{r eval=FALSE}` produces a `Q-2-8` warning. Will need to flip to expect Q-2-36 error. The companion `test_code_block_with_class_no_warning` (`{python .marimo}`) stays as-is — it's the negative control for the discrimination. |
| Grammar rules that accept knitr-style | `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:459-490` | `language_specifier` accepts `_language_specifier_token` + optional commonmark kv block. **Not changed** under Approach 1 — we keep the parse path so we have a structural location for the diagnostic. |
| Merr error table — existing entries | `crates/pampa/resources/error-corpus/Q-*.json` (esp. `Q-2-32.json`, `Q-2-35.json`) | Template for adding `Q-2-36.json` with the bare-label and comma-form cases. |
| Build script for the Merr table | `crates/pampa/scripts/build_error_table.ts` | Runs the parser against each case file, captures `(state, sym)`, writes `_autogen-table.json`. Must be re-run after editing `Q-2-36.json`. |
| Source-info helper for whole-line highlight | `crates/pampa/src/readers/qmd_error_messages.rs::widen_diagnostic_to_line` | For the Merr-mapped error forms (B), widens the highlight from the offending token to the whole header line, per the scope decision. The treesitter.rs warning currently spans the whole code block — for case (A) we may need to clip it to *just the header line* (line 1 of the code block) so the highlight matches the scope of the offence. |

## Approach (confirmed with user, 2026-05-14)

**Approach 1: upgrade Q-2-8 warning → Q-2-36 error, plus Merr-map the parse-error forms.** No scanner.c change, no grammar.js change. Q-2-36 is structurally unlike Q-2-35 (and the rejected Q-2-32 / TRIPLE_STAR template) because nothing is being *silently consumed* — half the bad forms already error out, the other half already produce a warning at a structural site.

### Plan-of-record (TDD shape, to be expanded in the plan doc)

1. **Phase 0 – test scaffolding**
   - Move the `test_code_block_with_header_options_produces_warning` assertion logic into a new error-expectation form (asserts Q-2-36 *error*, not Q-2-8 warning). Keep `test_code_block_with_class_no_warning` (the `{.python .marimo}` case) as the negative control.
   - Add `Q-2-36.json` to the error corpus with at least these cases:
     - `bare-label` — `{r test}`
     - `comma-args` — `{r, echo=FALSE}`
     - `comma-and-kv` — `{r, label="foo", echo=FALSE}`
     - (the space-kv form is the one upgraded inline, but we may add a Merr case anyway for documentation symmetry)
   - Run `crates/pampa/scripts/build_error_table.ts` to regenerate `_autogen-table.json`. Confirm tests fail with the expected "no entry" or wrong-code messages.

2. **Phase 1 – upgrade Q-2-8 site to Q-2-36 error**
   - In `treesitter.rs:1121-1144`, replace `DiagnosticMessageBuilder::warning(...)` with `error(...)`, code `"Q-2-36"`, message+hints pointing at `#| key: value`. Clip the highlight to just the header line.
   - Run the updated warning tests; they should now expect (and find) a Q-2-36 error.

3. **Phase 2 – wire up Merr for (B)**
   - With `Q-2-36.json` in place and the table regenerated, the bare-label and comma-form parse errors should pick up the Q-2-36 mapping automatically. If not, add the `(state, sym)` entries by hand following the Q-2-32 pattern.
   - Apply `widen_diagnostic_to_line` (or a sibling helper) so the highlight covers the full header line, per scope.

4. **Phase 3 – end-to-end verification**
   - Run `cargo run --bin pampa -- claude-notes/issue-reports/152/q236-repro.qmd` and confirm the reporter's case produces a clean Q-2-36 error.
   - Run the variants fixture; cases (1)–(7) error, case (8) parses cleanly.
   - `cargo nextest run --workspace`, `cargo xtask verify` (full, including hub-build, since pampa is a WASM-dep crate).

### Why **not** scanner emit (notes for plan-doc author)

Scanner-emit would either (a) be redundant with the existing grammar acceptance (we'd emit a token but the grammar would also produce a valid parse — confusing), or (b) require deleting `_commonmark_specifier_start_with_kv` from `language_specifier`, which changes the Merr `(state, sym)` table for *other* error codes and rebuilds the parser. The user explicitly approved "be willing to backtrack if grammar changes become unwieldy"; this is exactly the kind of unwieldy that earns us nothing.

## Scope decisions (confirmed with @cscheid)

1. **Discrimination:** `{r ...}` (no leading dot on the language) → Q-2-36 error. `{.r ...}` (Pandoc class form) → unchanged, valid. The existing Q-2-8 gate already implements this discrimination by checking for literal `{...}` braces in `classes[0]`.
2. **Forms flagged:** comprehensive — space+kv, comma+kv, bare label, any engine (R / Python / Julia / others). User's framing: *"be willing to backtrack if the grammar changes become unwieldy. This is, in the end, meant as a kindness to the users."* Approach 1 needs no grammar changes, so the comprehensive scope is cheap to implement.
3. **Engine scope:** any engine (R, Python, Julia, etc.). Approach 1 inherits this for free because the Q-2-8 gate already keys on the braces, not the engine name.
4. **Mixed-mode (`{r echo=FALSE}` with `#| label: ...` in body):** always error on the header; ignore the body. The Q-2-8 gate already fires regardless of body content, so this is automatic.
5. **Highlight span:** whole chunk header line. For the upgraded Q-2-8 site, clip `cb.source_info` to line 1 (currently it spans the entire code block). For the Merr-mapped forms, apply `widen_diagnostic_to_line`.

## Open questions resolved during triage

| Question | Resolution |
| --- | --- |
| Which forms should fire Q-2-36? | All knitr-style headers: comma+kv, space+kv, bare label, any engine. Pandoc class form (`.lang`) stays valid. |
| Engine scope? | Any engine. |
| Mixed-mode behavior? | Always error if header is knitr-style. |
| Highlight span? | Whole header line. |
| **Scanner-emit or no?** | **No scanner-emit.** Grammar already accepts the warning forms structurally; upgrading the existing Q-2-8 site is the surgical fix. Bare-label and comma-form parse errors get Merr mappings. (User confirmed Approach 1, 2026-05-14.) |

## Outcome / recommended next step

- File beads `bd-XXXX` ("Q-2-36: clean parse error for knitr-style chunk options (issue #152)") with the plan-of-record above and a pointer to this triage doc.
- Write the implementation plan at `claude-notes/plans/2026-05-14-q-2-36-knitr-style-chunk-options.md` with the four TDD phases expanded.
- Commit this triage doc + fixtures on branch `issue-152`.
- *Discovered incidental work* (filed separately): `cargo xtask create-worktree --issue 152 --base bugfix/issue-184` printed `Branch: issue-152` but actually checked out `bugfix/issue-184` directly in the worktree (no `issue-152` branch was created — `git reflog` confirms). Recovered in-place via `git checkout -b issue-152`. File a beads issue so the xtask is fixed before the next person hits the same surprise.

## Verification commands used

```bash
gh issue view 152 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build --skip-hub-tests          # green at e2d224f6
cargo run --bin pampa -- claude-notes/issue-reports/152/q236-repro.qmd
cargo run --bin pampa -- claude-notes/issue-reports/152/q236-repro-variants.qmd
# Spot checks of individual variants and the .r negative control
printf '%s\n' '```{.r echo=FALSE}' '1+1' '```' | cargo run --bin pampa --
printf '%s\n' '```{r echo=FALSE}' '1+1' '```' | cargo run --bin pampa --
printf '%s\n' '```{r test}' '1+1' '```' | cargo run --bin pampa --
printf '%s\n' '```{r, label="foo", echo=FALSE}' '1+1' '```' | cargo run --bin pampa --
cargo run --bin pampa -- -v /tmp/q236-bare.qmd 2>&1 | head -40
```

## Cross-references

- bd-7l1u — Q-2-35 (issue #184). Cited as template by the user; in practice Q-2-36 diverges (see *Approach*). Still the right reference for error-corpus mechanics, Merr `(state, sym)` mapping, and `widen_diagnostic_to_line` usage.
- bd-f3pl — Q-2-152-tables (closed via #154). The first half of issue #152.
- `crates/pampa/CLAUDE.md` — error-corpus authoring conventions.
- `crates/tree-sitter-qmd/tree-sitter-markdown/CONTRIBUTING.md` — Known Limitations entries (precedent: Q-2-32 / Q-2-35).
