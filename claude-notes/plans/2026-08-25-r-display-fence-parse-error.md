# R display fences (``` r) are a fatal parse error (bd-knitr-inline-r-eats-fence-2ofk91x1)

**Date:** 2026-08-25
**Braid:** bd-knitr-inline-r-eats-fence-2ofk91x1 (P0, bug, labels `engine` `parity`)
**Worktree:** `.worktrees/workspace-1` (branch `braid/bd-knitr-inline-r-eats-fence-2ofk91x1-r-display-fence-parse-error`, based on `main` @ `d05e96ee8`)
**Status:** **Implemented** on branch `braid/bd-knitr-inline-r-eats-fence-2ofk91x1-r-display-fence-parse-error` (`7e6e479ba`). All phases done; not yet pushed.

## Triage verdict

**Was: ready to design. Now: done.** The defect was one regex in one function
with no callers outside its own module. The fix direction came from upstream
ground truth (knitr's own pattern, captured below), and one thing the
investigation did *not* predict — that the guard must also exclude a
backslash, because the YAML-syntax-error fallback separates a fence's
backticks with escapes. See *Phase 2 discovery*.

All six of the strand's fixtures render; workspace 13385 passed / 199 skipped
against a 13368 / 199 baseline, the +17 fully accounted for by this branch's
new tests.

## Issue context

Filed today (2026-08-25) by Gordon at P0. An R **display** fence — a fence
carrying a language but no braces — is a fatal parse error in any document the
knitr engine runs, and the whole page is lost. Q1 renders all spellings. Two
stages compound:

- **Stage 1** — `crates/pampa/src/writers/qmd.rs:783-786` (`write_codeblock`)
  writes a single-class code block's language as a bare word, so `` ``` r ``,
  `` ```{.r} `` and `` ```r `` all re-serialize to `` ```r ``.
- **Stage 2** — `crates/quarto-core/src/engine/knitr/preprocess.rs:43`
  runs `` `r\s+([^`]+)` `` over the whole serialized document. Against
  `` ```r `` it anchors on the fence's *third* backtick, `\s+` consumes the
  newline, and `[^`]+` swallows the block body.

Real-world cost is in the strand: the Positron docs port loses `download.qmd`
entirely, which breaks 85 site-wide references because every page's navbar
carries one.

The strand is unusually complete — it already contains the two-stage analysis,
the history (`748856f50`, byte-identical since), the Q1 reference, the
`` ```R `` workaround and its cost, and a six-fixture repro at
`/Users/gordon/src/q2-positron-docs/llms-info/repros/knitr-inline-r-eats-fence/`.
This investigation adds the upstream-knitr ground truth, a measured comparison
of candidate patterns, and a recommendation *against* the AST-based direction
the strand floats.

## Dependency graph

**Empty.** `braid dep tree` and `braid dep list` return the strand alone — no
`discovered-from` parent in this skein, no blockers, nothing blocked. The
origin strand lives in a different skein (the Positron-docs porting project,
`br-knitr-inline-r-eats-fence-oqr1n5im`) and is not reachable from here.

Practical consequence: no incoming pressure to date the urgency, and no sibling
context to inherit. The P0 rests entirely on the strand's own argument —
whole-page loss, unavoidable by the author, universal idiom — which the
investigation confirms.

## What the code looks like today

Every path in the strand is current at `d05e96ee8`; nothing has been refactored
out from under it.

- `preprocess.rs:43` — `Regex::new(r"`r\s+([^`]+)`")`, byte-identical to the
  strand's quote.
- The only callers are `knitr/mod.rs:158-159` (`has_inline_r_expressions` as a
  fast path, then `resolve_inline_r_expressions`). Nothing else in the
  workspace references either function or the pattern. **Blast radius is one
  module.**
- No sibling engine has an equivalent pass — the jupyter and ts engines do no
  inline-expression preprocessing at all. (Q1 *does* support `` `{python} x` ``
  via `execute-inline.ts`; that q2 doesn't is a separate parity gap, not this
  bug.)
- Stage 1 is likewise unchanged: `qmd.rs:783` still takes the bare-word branch
  under `classes.len() == 1 && id.is_empty() && keyvals.is_empty()`.

### Reproduced at HEAD, at the pattern level

`2026-08-25-r-display-fence-parse-error-investigation/regex-candidates.py` runs three patterns over all six of
the strand's fixtures, with stage 1's collapse simulated. Full output in
`regex-candidates.out`. Summary:

| fixture | q2 today | knitr upstream | proposed `(^\|[^`])` port |
|---|---|---|---|
| `repro/` (``` r) | **matches → fatal** | no | no |
| `nospace/` (```r) | **matches → fatal** | no | no |
| `attr-fence/` (```{.r}) | **matches → fatal** | no | no |
| `yaml-title/` (```r in a YAML scalar) | **matches → fatal** | **matches** | no |
| `control/` (```bash) | no | no | no |
| `workaround/` (```R) | no | no | no |

Every inline form the existing 15 unit tests pin still matches under the
proposed pattern.

### Reproduced at HEAD, end-to-end through the binary

Minimal fixture at `2026-08-25-r-display-fence-parse-error-investigation/fixture/`
— one executable `` ```{r} `` cell and one `` ``` r `` display fence, 15 lines:

```
$ cd claude-notes/plans/2026-08-25-r-display-fence-parse-error-investigation/fixture
$ q2 render
```

Observed (full capture in `../repro-at-head.txt`):

```
Error: Parse error
     ╭─[ .../fixture/index.knitr.rmarkdown:151:1 ]
 151 │ ```r .QuartoInlineRender(pak::pak(c("usethis", "cli")))```
     │ ╰── unexpected character or token here

Rendered 0 of 1 files ... — 1 error
```

That one line is the whole bug made visible: the opening fence's third
backtick, the language `r`, the newline eaten by `\s+`, and the entire block
body pulled inside `.QuartoInlineRender(...)`.

Two of the strand's secondary claims are confirmed in passing. The cited
`index.knitr.rmarkdown` **is not on disk** after the failure (the fixture
directory holds only `_quarto.yml` and `index.qmd`), and the cited line 151
is meaningless against a 15-line source — the author can neither open the
file the error names nor map its line back.

### Upstream ground truth: knitr's own pattern

Captured from knitr 1.50 on this machine (details and reasoning in
`2026-08-25-r-display-fence-parse-error-investigation/knitr-upstream-pattern.md`):

```
(?<!(^``))(?<!(\n``))`r[ #]([^`]+)\s*`
```

This is the single most useful thing the investigation turned up, and it is
**better evidence than the Q1 source the strand cites**. Q1's
`execute-inline.ts` handles the *braced* `` `{r} expr` `` form used by the
jupyter/julia engines; it is a cousin, not the thing that actually consumes
this text. knitr's pattern *is* the contract — after q2 rewrites, knitr
re-scans the same string with the pattern above and evaluates what it finds.

knitr carries three defenses q2 has none of: two negative lookbehinds against a
fence's third backtick, and `[ #]` — a **single** space or hash, so a newline
can never open an expression. That third defense is what stops a 4+-backtick
fence, which neither lookbehind catches.

### Why not port knitr's pattern literally

Rust's `regex` crate has no lookbehind, so the guard must be re-expressed
regardless. The natural re-expression is Q1's idiom — capture the preceding
character and re-emit it:

```
(^|[^`])`r[ \t]([^`]+)`
```

That form is *strictly stronger* than knitr's lookbehinds: it rejects a
backtick prefix anywhere, not only at line start. The `yaml-title/` row above
is the measured consequence — a `` ```r `` inside a front-matter scalar is
mid-line, so knitr's lookbehinds miss it and **knitr upstream would eat it
too**. The proposed guard does not. We would be fixing a case knitr still gets
wrong.

Relative to today's pattern the change is a **pure narrowing**: it removes
newline-opened and backtick-prefixed matches and adds nothing. No existing test
covers either removed case.

### Recommendation: reject the AST-based direction

The strand suggests "better still, do the substitution over parsed block
structure rather than raw text." I recommend explicitly **not** doing that:

1. **It would silently drop a real feature.** Inline R in front-matter scalars
   (`` title: "Report for `r params$name`" ``) works in Q1 because knitr scans
   the whole file including the YAML. Excluding metadata to fix `yaml-title/`
   would trade a crash for a regression. The text-level guard fixes the bad
   case *and* keeps the good one — verified in the table above.
2. **The contract is textual, because knitr is textual.** q2's pass exists only
   to hand knitr a string knitr will then re-scan with its own regex. The
   correctness condition is "q2's pattern agrees with knitr's", which is a
   statement about text. An AST pass could wrap something knitr's regex won't
   match (or miss something it will) and reintroduce divergence in a new place.
3. **It costs a trait change.** `ExecutionEngine::execute` takes `&str`
   (`engine/traits.rs:61`). AST-level preprocessing needs either a new trait
   method or knitr-specific logic in `EngineExecutionStage` — a real
   architectural surface, for a defect a five-character guard closes.

One genuine point in the AST direction's favour, recorded so it isn't lost:
because the regex runs *after* `serialize_ast_to_qmd`, the `SourceInfo` handed
to `ExecutionContext` (`engine_execution.rs:432,467`) describes the
**pre**-substitution string while knitr executes the post-substitution one, so
every offset after a wrapped expression is short by 21 bytes. This is
**currently harmless** — `ctx.source_info` is consumed only by the jupyter/ts
engines, never by knitr — but it is a live trap for anyone who later tries to
give knitr errors real source locations, which is exactly what the strand's
"undiagnosable" complaint asks for. Note it in the code; don't fix it here.

## Phases

Implementation began 2026-08-25 after Gordon's go-ahead. Baseline before any
change: 13368 passed / 199 skipped at `d05e96ee8`.

- [x] **Phase 0 — Test plan (TDD, tests written and watched failing first).**
  - 11 unit tests added to `preprocess.rs`: each fence spelling, a 4-backtick
    fence, a fence spelling inside a YAML scalar, a backtick-prefixed
    `` ``r x` ``, a newline after `r`, the escaped-backtick form (below), plus
    three regression guards (inline R beside a fence, a tab separator, a body
    spanning lines). **8 failed against the old pattern**; the three guards
    passed before and after, which is what makes them guards.
  - `crates/quarto-core/tests/integration/knitr_display_fence.rs` — 6 tests
    through `render_document_to_file`, the entry `q2 render` itself uses,
    gated on `knitr_available()`. **Verified red by stashing the fix**: all
    six failed, five with `Error: Parse error` and the YAML one with
    `Execution failed in knitr: R process failed` — the two shapes the strand
    describes. Registered in `tests/integration/main.rs` per
    `.claude/rules/integration-tests.md`.
- [x] **Phase 1 — Replace `INLINE_R_PATTERN`** with
  `` (^|[^`\\])`r[ \t]([^`]+)` ``, re-emitting the captured prefix. The doc
  comment, which claimed a guard the code never had, is replaced by one that
  explains each guard and cites its provenance.
- ~~**Phase 2 — Stage-1 writer parity.**~~ **Out of scope** (Q2, decided
  2026-08-25). The qmd writer keeps emitting `` ```r `` for a bare display
  language. Recorded for whoever revisits it: Pandoc's own markdown writer
  emits `` ``` r `` (verified), and exactly one `.snap` in the repo contains
  any bare-word fence, so the churn would have been near-zero — but the fix
  is not needed once stage 2 is guarded.
- [x] **Phase 3 — Verification against the strand's fixtures.** See below.
- [x] **Phase 4 — Docs: none required.** The docs site documents usage, and
  nothing an author writes changes: `` ``` r `` was always the correct
  spelling and simply works now. No error page is implicated either — the
  failure was uncoded, and adding a `Q-` code is finding (b), deliberately
  not filed.

### Mid-implementation discovery: the guard also has to exclude a backslash

`fence_spelling_in_yaml_scalar_renders` stayed red after Phase 1, and the
reason was **not** the one the strand describes. When a YAML scalar fails to
parse as markdown, q2 warns `Q-1-20` and the `.yaml-markdown-syntax-error`
fallback re-serializes the scalar with every backtick **backslash-escaped**.
Confirmed with `pampa -t qmd`:

```
title: "[Backtick r in the title: \\`\\`\\`r blocks]{.yaml-markdown-syntax-error}"
```

The three backticks are no longer adjacent — each is preceded by a backslash —
so the third is preceded by `\`, a non-backtick, and a guard that excludes
only backticks lets it anchor. **Neither Quarto 1 nor knitr excludes the
backslash, and neither survives this input.**

The fix extends the class to `` [^`\\] ``, on the general ground that an
escaped backtick cannot open a code span and therefore cannot open an inline R
expression. Still a pure narrowing. Known cost, recorded in the doc comment:
`` \\`r x` `` — an escaped *backslash* followed by a genuine expression — is
declined. Telling that from `` \`r x` `` needs a count of preceding
backslashes, which this shape of regex cannot express; declining a vanishingly
rare expression is much the cheaper failure than losing the page.

### Phase 3 — all six of the strand's fixtures, through the binary

Copied to a scratch dir (a Q1 run mutates the originals) and rendered with
`q2 render` from the branch build:

| fixture | before | after | last `<pre class=…>` |
|---|---|---|---|
| `repro/` (``` r) | fatal | **renders** | `sourceCode r` |
| `nospace/` (```r) | fatal | **renders** | `sourceCode r` |
| `attr-fence/` (```{.r}) | fatal | **renders** | `sourceCode r` |
| `yaml-title/` | fatal | **renders** (1 pre-existing `Q-1-20` warning) | — |
| `control/` (```bash) | renders | renders | `sourceCode bash` |
| `workaround/` (```R) | renders unhighlighted | unchanged | `R code-with-copy` |

`grep -c QuartoInlineRender` is **0** in all six outputs. The `workaround/` row
is unchanged on purpose — that is finding (a), deliberately not filed, and the
fix must not disturb it.

Output inspected, not inferred. The display block in `repro/index.html`:

```html
<pre class="sourceCode r"><code class="sourceCode r"><span class="hl-comment">
# if you're a pak person (we are!)</span>
<span class="hl-namespace">pak</span><span class="hl-operator">::</span>…
```

Q1 renders the same block as `<pre class="sourceCode r code-with-copy">` with
skylighting's `co`/`fu` span classes. The span vocabulary differs between the
two highlighters — not part of this bug; what matters is that the block is a
highlighted, non-executed R block in both.

### Code review (2026-08-25)

Reviewed over `c9f4023f0..32c1f0407`. No Critical issues; the reviewer could
construct no input that still eats a fence and no reachable input where a
legitimate expression is lost, and confirmed via a guard-mutation matrix that
every guard is independently pinned by a test.

Two Important findings, both fixed:

1. **The doc comment inverted which guard defends against a fence.** It said
   `[ \t]` was "the one that stops a four-backtick fence, which the prefix
   guard alone does not". That is false, and it is false in the dangerous
   direction — a maintainer could have deleted the load-bearing guard on its
   authority. Verified independently before fixing:

   | input | old | prefix guard only | class only | shipped |
   |---|---|---|---|---|
   | 3-backtick fence | eats | ok | ok | ok |
   | 4-backtick fence | eats | ok | ok | ok |
   | fence in YAML scalar | eats | ok | **eats** | ok |
   | escaped-backtick fence | eats | ok | **eats** | ok |

   The prefix guard covers **every** fence shape, because the backtick that
   would anchor a match is always itself preceded by a backtick; `[ \t]` is
   knitr parity plus defense-in-depth, and its own regression case is a
   mid-prose `` `r\nx` ``. The claim came from the knitr analysis — where
   `[ #]` genuinely *is* what stops a fourth backtick, since knitr's
   lookbehinds are line-anchored — and was mis-transposed onto q2's guard.
   Corrected in all three places it appeared, now naming the test that
   reddens when each guard is removed.

2. **Two integration assertions were satisfiable by an empty block.** The
   four-backtick and YAML-scalar tests asserted only "renders" and "no
   wrapper", neither of which fails if the content were swallowed. This is
   not hypothetical: knitr re-scans the string we hand it, and its
   lookbehinds check for *adjacent* backticks, so it would not reject the
   escaped form on its own. Both now pin the surviving text.

Minor findings also applied: the `` ```R `` note pointed at bd-ps046hi3 (which
tracks finding (c)) rather than §(a) here, and this section's heading said
"Phase 2", which is the struck-out writer phase. Declined: aligning the skip
message wording with `engine_visibility.rs` — the mechanism already matches
and naming the test is more useful in a six-test file.

The reviewer also asked for the `SourceInfo` byte-offset skew to be recorded
in `preprocess.rs` rather than only here, since that file is where someone
giving knitr real source locations would be standing. Added to its module doc.

## Open design questions for the user

1. **Which character class after `` `r ``?** — **DECIDED 2026-08-25: `[ \t]`.**
   - `[ \t]` — Q1's idiom; a pure narrowing of today's `\s+`; keeps the tab
     form working. **Chosen.**
   - `[ ]` — space only; makes q2's matches a strict subset of knitr's.
   - `[ #]` — knitr-exact, but it *widens* q2: `` `r#x` `` doesn't match today
     and would start being wrapped as `.QuartoInlineRender(#x)`, which is
     broken R (the comment eats the closing paren). I'd avoid this.

2. **Is stage 1 (the qmd writer) in scope?** — **DECIDED 2026-08-25: out of scope.** Fixing stage 2 alone closes the
   bug. But `write_codeblock` writing `` ```r `` where Pandoc writes
   `` ``` r `` is a round-trip parity divergence in its own right, and fixing
   it is independent defense-in-depth. Measured cost looks near-zero — no
   `.snap` file in the workspace contains a bare-word `r` fence. The care
   needed is that the same branch also emits `` ```{r} `` for executable
   cells, which must not gain a space. In scope as Phase 2, or a separate
   strand?

3. **Which of these become follow-up strands rather than part of this fix?**
   — **DECIDED 2026-08-25: file (c) only; (a) and (b) deliberately not filed.**
   All three were verified during this investigation and are genuinely separate
   defects. Only the third became a strand — **bd-ps046hi3**
   (`discovered-from` this one). The other two are recorded here so the
   evidence isn't lost, but are not tracked:
   - **(a) NOT FILED — `` ```R `` renders unhighlighted.** Verified: q2 emits
     `<pre class="R code-with-copy">` for `` ```R `` but
     `<pre class="sourceCode bash">` for a `` ```bash `` control, so the
     degradation is specific to the capital, not to display fences. Cause is
     mechanical: `quarto-highlight/src/registry.rs:87` resolves a language by
     exact `HashMap` hit, and `:148` registers `("r", &[])` — no aliases — so
     `"R"` misses, no highlight spans are produced, and
     `pampa/src/writers/html.rs:539` only prepends `sourceCode` when spans
     exist. Q1 emits `sourceCode r`, i.e. it normalizes case. **Its urgency
     drops once this strand lands** — `` ```R `` stops being the only working
     spelling. Keep the strand's warning visible: do not make
     `INLINE_R_PATTERN` case-insensitive, which would make `` ```R `` fatal
     too; fixing the *highlighter* has the opposite sign and is safe in either
     order.
   - **(b) NOT FILED — uncoded `Parse error` citing a generated intermediate.**
     Two problems in one message: no `Q-` code (so no docs page, nothing to
     search for), and a file/line the author cannot act on. The source-mapping
     half is the expensive one and must start from the `SourceInfo` skew noted
     above; adding a code is independently cheap. See bd-ps046hi3, which
     records the skew warning for whoever picks this up.
   - **(c) FILED as bd-ps046hi3** — `--debug` does not retain the intermediate.
     Verified: after a `--debug` run the project holds only
     `.quarto/render-manifest.json` and a cache profile, with nothing matching
     `*knitr*`. This is what makes (b) unsurvivable rather than merely
     annoying, and it is the cheapest of the three.

## Risks / tradeoffs (draft)

- **Low blast radius.** One regex, one module, two callers, 15 existing tests
  that all keep passing under the proposed pattern.
- **The `` ```R `` trap.** The strand flags it and it is worth repeating: do
  not make the pattern case-insensitive as part of "tidying". `` ```R `` is
  currently the only way authors can ship a working document, and the
  highlighter is case-sensitive too, so a case-insensitive regex would turn the
  workaround fatal.
- **Testing needs R.** The end-to-end leg only runs where `Rscript` and the
  knitr package are installed. The unit tests carry the real regression
  coverage; the e2e test is the CLAUDE.md-mandated proof, gated on the standard
  skip.
- **Pre-flight note (environment, not code).** The first
  `cargo xtask verify --skip-hub-build --skip-hub-tests` in this worktree
  failed with 10 tree-sitter corpus failures, several named after open strands.
  That was a **stale gitignored `markdown.dylib`** left from the worktree's
  previous branch, not a red main: after `tree-sitter generate && tree-sitter
  build` the corpus is 609/609. Worth remembering whenever a worktree is
  repurposed onto a new branch. The re-run was green — **13368 passed, 199
  skipped** at `d05e96ee8`; that is the live baseline to compare any
  phase-boundary workspace run against.
