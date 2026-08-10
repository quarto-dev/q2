# qmd-syntax-helper rules: migrate reference-style links and escape literal brackets (bd-reference-links-unsupported-ddc4skac)

**Date:** 2026-08-10
**Braid:** `bd-reference-links-unsupported-ddc4skac` (feature, p1, labels: `diagnostics`, `parity`)
**Branch:** `braid/bd-reference-links-unsupported-ddc4skac`, rebased onto `main` @ `d05e021e`
**PR:** [#497](https://github.com/quarto-dev/q2/pull/497)
**Status:** All phases complete; full `cargo xtask verify` (including the WASM leg) green.

## Scope, restated

This is **not** a request to make the qmd parser accept reference-style
links. `[...]` is reserved for span syntax and that is not being given back
(decided 2026-08-10, Carlos). The deliverable is source migration in
`crates/qmd-syntax-helper/`, plus a diagnostic so the breakage becomes
self-reporting.

## Decisions (2026-08-10)

| Question | Decision |
| --- | --- |
| Image arm (`![alt][ref]`, `![solo]`) | **Fold in.** Same detection pass, same rewrite targets. |
| Gating the destructive arm | **Two separately-named rules.** Fine-grained rule boundaries are wanted. |
| Diagnostic | **This strand**, not a sibling. |
| Rule names | `reference-links` (safe arm) and `literal-brackets` (destructive arm). |
| `[a][b][c]` | **Decline to rewrite runs of ≥3 and report**, given the guard is a length check on data already in hand. Zero occurrences in the motivating corpus. |

## Triage verdict

**Ready to implement.** The behavior reproduces verbatim at HEAD, the
routing decision holds, the crate already has the machinery, and the five
open questions are settled. `cargo xtask verify --skip-hub-build` was green
at `05c2454e` before any of this work.

## Issue context

Filed 2026-08-10 by Carlos. The strand is unusually complete: it front-loads
the scope decision, names the crate and trait, sketches the rewrites, and
flags the escaping arm as the destructive-if-wrong one.

Real-world impact is ~7 Posit Connect doc pages, in two failure classes.
Broken links plus a leaked definition paragraph
(`admin/process-management`, `admin/integrations/package-manager`); and
silently deleted brackets, of which three **change documented meaning**
rather than formatting: `admin/security` (the `[1]`/`[2]` markers keyed to a
numbered diagram), `admin/appendix/branding` and `admin/email` (the default
mail subject prefix is literally `[Posit Connect]`, so the docs now state
the wrong value), and `admin/opentelemetry/signal-reference-guide`
(histogram bucket lists).

## Dependency graph

**Empty.** `braid dep tree` and `braid dep list` both return the strand
alone; a skein-wide search turned up no neighbours. Two consequences worth
stating:

- **No incoming pressure.** Nothing in q2 is blocked on this; urgency is
  external, from the Connect docs port.
- **The `discovered-from` context is outside this skein** — origin strand
  `br-raalju6n` in the connect-docs porting skein, repro in a local-only
  repo. That is why the strand description carries so much inline. The repro
  is now copied into this repo so the record survives independently.

## What the code looks like today

Full detail with byte ranges and probe outputs:
`claude-notes/plans/reference-links-migration-investigation/findings.md`
(seven probe files alongside it).

**The bug reproduces verbatim at HEAD.** `pampa -t html` on the strand's
repro yields `<span>the RedHat documentation</span><span>gcc-toolset</span>`,
`<span>Version TBD</span>`, and the trailing definition paragraph — no
diagnostics, exit 0.

**Detection is AST-based, not regex-based.** This is the finding that most
shapes the work. `pampa` gives every `Span` an empty-or-not `Attr` *and* a
`SourceInfo` byte range **including the brackets**; likewise every `Image`
carries its url and range. So:

| shape | predicate |
| --- | --- |
| brackets will be eaten | `Inline::Span` with empty `attr` |
| image will break | `Inline::Image` with empty url |
| `[label][ref]`, `![alt][ref]`, `[label][]` | two of the above whose ranges **touch exactly** |
| `[x]{.cls}`, `[link](u)`, `![real](u.png)`, brackets in code | *not matched* — excluded structurally |

The strand's §3 asks the escaping pass to skip `]` followed by `{`, `(`, or
`[`. **The AST does that work**, and with it goes the class of bug that
lookahead was guarding against.

**Escaping is idempotent and cross-engine safe**, verified in both
directions for both arms: `\[…\]` produces no `Span` and `!\[…\]` produces
no `Image` in q2, and both q2 @ `05c2454e` and `quarto pandoc` render them as
literal brackets. This matters because `convert` defaults to `-r all` and
iterates up to `--max-iterations` (default 10).

**Framework fit is good.** `check -r <rule>` already emits one `CheckResult`
*with a `SourceLocation`* per violation — that **is** the strand's requested
"`--check` mode enumerating every bracket it would escape", already built.
(`convert --check` reports only a count; `check` is the stronger of the two
and the one to point people at.) `apostrophe_quotes.rs` is the model for
applying edits: collect offsets, apply in **reverse offset order**, write
back. No existing rule walks the pampa AST, so this is the first — a small
new pattern for the crate, but no new dependency.

## The two rules

The split is by **risk**, not by syntax: every shape below is detected in
one shared pass, then routed to whichever rule owns it.

### `reference-links` — mechanical, safe

A use with a **matching definition**. Both the span and image forms:

| before | after |
| --- | --- |
| `[label][ref]` | `[label](url)` |
| `[label][]`, `[ref]` (shortcut) | `[label](url)` |
| `![alt][ref]`, `![alt][]`, `![alt]` | `![alt](url)` |
| definition `[ref]: url "Title"` | folded into the use as `(url "Title")` |

The `[ref]: url` definition line is dropped once its last use is gone.

### `literal-brackets` — destructive, opt-in

A bracketed run with **no matching definition**, escaped so the brackets
survive:

| before | after |
| --- | --- |
| `[Version TBD]`, `[1]`, `[Posit Connect]` | `\[Version TBD\]` etc. |
| `![solo]` (no definition) | `!\[solo\]` |

This rule is the one that writes an edit indistinguishable from author
intent afterwards. It is separately named precisely so `convert -r all`
never fires it unasked.

## Phases

- [x] **Phase 0 — Test plan (TDD, failing first).** Fixtures under
      `crates/qmd-syntax-helper/tests/fixtures/` covering: full / collapsed /
      shortcut references; definitions with and without titles; unmatched
      brackets; genuine `[x]{.cls}`; inline link; code span; multi-line
      span; all four image shapes; and a `[a][b][c]` chain. Tests live in
      `tests/integration/reference_links_test.rs` and
      `tests/integration/literal_brackets_test.rs`, registered in
      `tests/integration/main.rs` — **no new top-level test binaries**
      (`.claude/rules/integration-tests.md`). Include tests that assert the
      *detector* sees each shape, not only that the rewrite is correct (see
      Risks).
- [x] **Phase 1 — Shared detection.** AST walk collecting bare spans and
      empty-url images with byte ranges; adjacency runs; definition-line
      recognition; use↔definition matching with case-insensitive,
      whitespace-normalized labels. Runs ≥3 are flagged `Ambiguous` here.
- [x] **Phase 2 — `reference-links` rewrite.** Uses with definitions →
      inline form; drop each definition at last use. Span and image forms
      together.
- [x] **Phase 3 — `literal-brackets` rewrite.** Escaping arm, span and image
      forms. Reverse-offset edits.
- [x] **Phase 4 — Registration + CLI.** Both rules into `RuleRegistry`;
      README's *Future Converters* entry moves to shipped and gains the
      two-rule explanation plus the `check`-before-`convert` guidance.
- [x] **Phase 5 — Diagnostic.** See below.
- [x] **Phase 6 — End-to-end verification** per CLAUDE.md: run the real
      binary against the Connect docs, inspect output, record the exact
      invocation and a snippet here.

## Phase 5 in detail — the diagnostic, and what it does *not* cover

Two high-precision triggers, both keying off shapes that are never
intentional qmd. The codes are **Q-2-45** (reference-style use) and
**Q-2-46** (definition line). They were originally written as 42/43; `main`
claimed 42, 43 and 44 for conditional content and callout titles while this
branch was in flight, so they were renumbered during the rebase.

1. **A block-level line matching the link-reference-definition shape** —
   the strand's own proposal. Catches the leaked-definition-paragraph
   symptom.
2. **Two adjacent bare spans / an empty-url image followed by a bare span**
   — the `[label][ref]` and `![alt][ref]` shapes. Unambiguously reference
   syntax; a genuine span never abuts another bare span this way.

**The limitation to be explicit about:** neither trigger fires on a *lone*
bare `[Version TBD]`. Diagnosing every bare span would be noisy and
sometimes wrong, since `[text]` → `<span>text</span>` can be deliberate. So
the diagnostic covers the `reference-links` shapes well and the
`literal-brackets` shapes **not at all** — which is exactly why
`literal-brackets` stays a run-`check`-first, opt-in rule rather than
something that can ride on a diagnostic. The three meaning-changing Connect
pages (`admin/security`, `branding`, `email`) are all lone-bracket cases and
would **not** be caught by this diagnostic.

**Implemented as a render-pipeline transform, not a parse error.** The
error-corpus / `build_error_table.ts` machinery in `crates/pampa/CLAUDE.md`
maps *parse states* to messages — but these documents parse fine, so there is
no error state to key off. The in-tree precedent for a shape-detected,
non-parse-error code is **Q-2-30**, which has a catalog entry and no
error-corpus file.

Q-2-30 is detected only in `qmd-syntax-helper`, which would leave `q2 render`
silent — the exact gap the strand complains about. So detection instead lives
in `crates/quarto-core/src/transforms/reference_link_diagnostics.rs`, a
read-only `Normalization`-phase transform that pushes
`DiagnosticMessage::warning(...).with_code(...)` onto `ctx.diagnostics` —
the same channel `crossref_resolve` uses for unresolved `@ref`s. It runs
first in the phase so it sees the document as the author wrote it.

Detection there is **AST-only** and needs no source text: `[a][b]` is simply
two consecutive bare-`Span` siblings in one `Inlines` list (a space between
them would insert a `Space` node), and a definition line is a line-initial
bare `Span` followed by a `Str` starting with `:`. That meant no new pampa
API and no shared-crate surgery — the only thing borrowed from pampa is the
one-line `is_empty_attr` predicate, which is where drift would actually
hurt.

## Phase 6 — end-to-end verification (done 2026-08-10)

Run against a **copy of the real Posit Connect docs** (`docs-quarto-2`, 383
files), not a fixture. Note the corpus is mostly `.md`, not `.qmd` — a first
pass globbing only `**/*.qmd` matched 206 files and found 1 issue, which
looked like a clean bill of health and was not.

```bash
qmd-syntax-helper check   -r reference-links -r literal-brackets "**/*.md" "**/*.qmd"
qmd-syntax-helper convert -r reference-links -r literal-brackets --in-place "**/*.md" "**/*.qmd"
```

`check` found **12 issues across exactly the 7 pages the strand named** —
and three instances the strand had not catalogued: `[3]` and `[4]` in
`admin/security`, a second `[version TBD]` in `user/quarto`, and
`[PositConnect]` in `admin/appendix/branding`.

`convert` changed those 7 files and **nothing else** (`diff -ru` against the
original tree). Re-running `check` on the converted tree reports **0 issues
in 383 files**, confirming convergence at corpus scale.

Rendered output inspected with `pampa -t html`, before vs. after:

| | before | after |
| --- | --- | --- |
| `process-management` link | `the RedHat documentation</span><span>gcc-toolset</span>` | `<a href="https://docs.redhat.com/…">the RedHat documentation</a>` |
| `process-management` leaked definitions | 2 occurrences of `gcc-toolset</span>` | 0 |
| `admin/security` CSRF markers | `upon an initial user session <span>1</span>,` | `upon an initial user session [1],` |
| `admin/email` documented value | `with “<span>Posit Connect</span>.”` | `with “[Posit Connect].”` |

The `admin/email` row is the one worth dwelling on: before the fix the page
stated the default subject prefix was `Posit Connect`, when the real default
is `[Posit Connect]`. The docs were *wrong*, not merely ugly, and the render
now matches the product.

## Risks / tradeoffs

- **`literal-brackets` writes an edit indistinguishable from author intent.**
  The strand's own caveat and the real risk. Mitigated by the separate rule
  name (never in `-r all` by accident) plus `check`'s per-violation
  locations.
- **AST coupling.** If `[...]`-with-no-attrs ever stops producing a bare
  `Span` — e.g. a future diagnostic changes the parse — the rules go *quiet*
  rather than failing loudly. Hence the Phase 0 requirement for
  detector-level tests, so that regression trips a test instead of silently
  migrating nothing.
- **First AST-walking rule in the crate.** Small new-pattern cost; contained.
- **Not a parser fix, by decision.** Sources migrated by these rules stop
  being reference-link documents. Intended, but one-way for the corpus it is
  run on.
- **The repro's home repo is local-only.** Mitigated by the in-repo copy.

## Where this landed

Investigated on `main` @ `05c2454e` in the primary checkout, per the skill's
"work in the checkout you were invoked in". This checkout's
`CLAUDE.local.md` still carries stale worktree context for an unrelated
strand (`bd-09aja9gl`); it was not touched. **No worktree was created** — if
implementation wants isolation, create one deliberately.
