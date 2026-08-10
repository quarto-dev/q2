# qmd-syntax-helper rule: migrate reference-style links and escape literal brackets (bd-reference-links-unsupported-ddc4skac)

**Date:** 2026-08-10
**Braid:** `bd-reference-links-unsupported-ddc4skac` (feature, p1, labels: `diagnostics`, `parity`)
**Branch:** `main` @ `05c2454e` — investigated in place, no worktree created (see *Where this landed*)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design**, with one scope decision that has to be made first: the
investigation turned up a **fourth damage shape the strand does not cover**
(`![alt][ref]` renders as `<img src="">`, a broken image rather than lost
text), and the strand's own open question about a diagnostic now has a
concrete answer available. Everything else about the strand holds up
exactly as written — the behavior reproduces at HEAD, the routing decision
is sound, and the crate already has the machinery.

## Issue context

Filed 2026-08-10 by Carlos, still `open`. The strand is unusually complete:
it front-loads the scope decision (**this is not a request to make the
parser accept reference links** — `[...]` is reserved for span syntax), names
the crate and the trait, sketches the three rewrites, and flags that the
escaping arm is the destructive-if-wrong one.

Real-world impact is ~7 Posit Connect doc pages. Two failure classes:
broken links plus a leaked definition paragraph (`admin/process-management`,
`admin/integrations/package-manager`), and silently deleted brackets — of
which three **change documented meaning**, not just formatting:
`admin/security` (the `[1]`/`[2]` markers keyed to a numbered diagram),
`admin/appendix/branding` and `admin/email` (the default mail subject prefix
is literally `[Posit Connect]`, so the docs now state the wrong value), and
`admin/opentelemetry/signal-reference-guide` (histogram bucket lists).

## Dependency graph

**Empty.** `braid dep tree` and `braid dep list` both return the strand
alone; no `blocks`, no `parent-child`, no `discovered-from` inside this
skein. A skein-wide search for neighbours turned up nothing related either.

That changes the calculus in two directions worth stating plainly:

- **No incoming pressure.** Nothing in q2 is blocked on this. The urgency is
  entirely external — the Connect docs port.
- **The context that would normally live in a `discovered-from` edge is
  external to this skein**: the origin strand is `br-raalju6n` in the
  connect-docs porting skein, and the repro lives at an absolute path
  (`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/reference-links-unsupported/`)
  in a local-only repo. The strand description carries that context inline,
  which is why it reads so long. **I copied the repro into this repo** (see
  below) so the record survives independently of that checkout.

## What the code looks like today

Everything the strand points at still exists and still behaves as described.
`cargo xtask verify --skip-hub-build` passes at `05c2454e` before any
changes. Full detail, with byte ranges and probe outputs, is in
`claude-notes/plans/reference-links-migration-investigation/findings.md`;
the four `.qmd`/`.md` probes are alongside it. Summary:

**The bug reproduces verbatim at HEAD.** `pampa -t html` on the strand's
repro gives `<span>the RedHat documentation</span><span>gcc-toolset</span>`,
`<span>Version TBD</span>`, and the trailing definition paragraph — no
diagnostics, exit 0.

**Detection should be AST-based, not regex-based.** This is the finding that
most changes the shape of the work. `pampa` already gives every `Span` an
empty-or-not `Attr` *and* a `SourceInfo` byte range **that includes the
brackets**. So:

- "these brackets will be eaten" ⟺ `Inline::Span` with empty `attr`;
- `[label][ref]` ⟺ two bare spans whose ranges touch exactly;
- `[x]{.cls}`, `[link](u)`, and brackets inside `` `code` `` are simply
  *not* bare spans, so they are excluded structurally.

The strand's §3 asks the escaping pass to skip `]` followed by `{`, `(`, or
`[`. The AST does that work for us — that lookahead becomes unnecessary,
and with it the class of bug it was guarding against.

**Escaping is idempotent and safe in both engines**, verified in both
directions: `\[…\]` produces *no* `Span` in q2 (so `convert`'s default
iteration to `--max-iterations 10` cannot re-escape its own output), and
both q2 @ `05c2454e` and `quarto pandoc` render it as literal brackets.

**Four shapes exist, not three.** `![alt][ref]` — and bare `![alt]` — do not
produce spans; they produce `<img src="" alt="alt" />`, an image with an
empty `src`. That is a worse outcome than the `[…]` cases (broken element,
not lost text) and it needs its own arm keyed off `Inline::Image`. Also
newly pinned down: `[label][]` yields a bare span plus an *empty* span;
spans cross soft line breaks, so edits must be offset-based; and `[a][b][c]`
is genuinely ambiguous between `[a][b]`+`[c]` and `[a]`+`[b][c]`.

**Framework fit is good.** `check -r <rule>` already emits one `CheckResult`
*with a `SourceLocation`* per violation — that *is* the strand's requested
"`--check` mode enumerating every bracket it would escape", already built.
`apostrophe_quotes.rs` is the model for applying edits (reverse offset
order, then write back). No existing rule walks the pampa AST, so this would
be the first — a small new pattern for the crate, but no new dependency.

## Proposed phases (draft)

Skeleton only; contents wait on the design discussion below.

- **Phase 0 — Test plan (TDD, failing first).** Fixtures under
  `crates/qmd-syntax-helper/tests/fixtures/` covering each shape:
  full/collapsed/shortcut reference, definition with and without title,
  unmatched brackets, genuine `[x]{.cls}`, inline link, code span,
  multi-line span, image reference, and the `[a][b][c]` ambiguity. Tests go
  in `tests/integration/reference_links_test.rs`, registered in
  `tests/integration/main.rs` (per `.claude/rules/integration-tests.md` —
  no new top-level test binaries).
- **Phase 1 — Detection.** AST walk collecting bare spans + byte ranges;
  definition-line recognition; matching uses to definitions
  (case-insensitive, whitespace-normalized labels).
- **Phase 2 — Rewrite: references with definitions** → inline
  `[label](url "Title")`, dropping each definition once its last use is
  gone.
- **Phase 3 — Rewrite: escaping unmatched brackets.** The destructive arm;
  gated behind whatever the design discussion decides (see Q2).
- **Phase 4 — Image arm** (`![alt][ref]`, `![alt]`), if in scope (Q1).
- **Phase 5 — Registration + CLI**, README "Future Converters" entry moved
  to shipped.
- **Phase 6 — Optional diagnostic** (Q3), if it rides on this strand.
- **Phase 7 — End-to-end verification** on the real Connect docs, per
  CLAUDE.md's end-to-end rule: run the binary, inspect output, record the
  invocation and a snippet.

## Open design questions for the user

1. **Does the image arm belong in this strand?** `![alt][ref]` and bare
   `![alt]` emit `<img src="">` — a broken image, arguably worse than the
   text cases, and not mentioned in the strand. Fold it in as Phase 4, or
   split it to a sibling strand? (I lean fold-in: same detection pass, same
   rewrite target, and shipping the `[…]` rule alone would leave a
   *worse*-rendering shape unmigrated.)

2. **How is the escaping arm gated?** The strand calls it
   "destructive-if-wrong" and asks for a `--check` pass before any
   `--in-place` run. The framework already gives per-violation locations via
   `check`. Options, roughly increasing in caution: (a) one rule, both arms,
   rely on `check` discipline; (b) one rule, escaping behind an opt-in flag;
   (c) two separately-named rules, so `convert -r all` never escapes unless
   asked. This matters because `convert` defaults to `-r all` **and**
   iterates up to 10 times — an escaping rule in the default set will fire
   on every file in a bulk run. (I lean (c): the two arms have genuinely
   different risk profiles, and named rules are cheap.)

3. **Does the diagnostic ride on this strand or a sibling?** The strand
   leaves this undecided. Concretely it would be a Q-code for a block-level
   line matching the link-reference-definition shape — which would make the
   breakage self-reporting and give the rule a `q_2_NN.rs` key. Note the
   detection work is *not* shared: the rule can key off AST shape today
   without any diagnostic, so the diagnostic is additive rather than a
   prerequisite. Separate strand, or Phase 6 here?

4. **What is the rule's name?** `reference-links` covers arm (1);
   `literal-brackets` or `escape-brackets` covers arm (2). If Q2 lands on
   (c), we need both names. The README's *Future Converters* line says
   "Reference-style links → inline links", which only names arm (1).

5. **`[a][b][c]` resolution.** Follow CommonMark left-to-right greedy
   (`[a][b]` consumes, `[c]` is then literal and gets escaped)? Or refuse to
   touch chains of three-or-more adjacent bare spans and report them for
   human review? The Connect corpus may well contain none of these — worth
   a grep before deciding.

## Risks / tradeoffs (draft)

- **Arm (2) writes an edit that is indistinguishable from author intent
  afterwards.** This is the strand's own caveat and it is the real risk.
  Mitigation is Q2 plus the existing per-violation `check` output.
- **The AST-based approach couples the rule to pampa's span
  representation.** If `[...]`-with-no-attrs ever stops producing a bare
  `Span` (e.g. a future diagnostic changes the parse), the rule goes quiet
  rather than failing loudly. Worth a test that asserts the *detector* sees
  the shapes, not only that the rewrite is correct.
- **First AST-walking rule in the crate.** Slight new-pattern cost; contained.
- **The repro's home repo is local-only.** Mitigated by copying it into
  `claude-notes/plans/reference-links-migration-investigation/`.
- **Not a parser fix, by decision.** Sources migrated by this rule stop
  being reference-link documents. That is the intended direction (2026-08-10,
  Carlos) but it is one-way for the corpus it is run on.

## Where this landed

Investigated on `main` @ `05c2454e` in the primary checkout, per the
skill's "work in the checkout you were invoked in". Note this checkout's
`CLAUDE.local.md` still carries worktree context for an unrelated strand
(`bd-09aja9gl`); it was not touched. If implementation wants isolation, a
worktree should be created deliberately — I did not create one.
