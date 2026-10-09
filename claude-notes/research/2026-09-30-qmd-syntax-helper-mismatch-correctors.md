# Research: `qmd-syntax-helper`\'s unclosed-delimiter autofix rules

**Date:** 2026-09-30
**Status:** Research only — no decision made, no strand filed for the new
findings below. Written to lay out the shape of the problem so Gordon can
decide what (if anything) to do.
**Trigger:** while checking braid coverage for the 2026-09-29 Typst
smoke-all follow-up plan, found `bd-vz340zg6` (open, P1) describing a
corruption bug in the `Q-2-23` (dollar/inline-math) autofix rule. Gordon
asked whether caret (`^`) was "the third character" with the same problem.
It is, and so — with less certainty — is tilde (`~`) and possibly double
quote (`"`).

## The pattern

`qmd-syntax-helper` (`crates/qmd-syntax-helper/`) has two families of
conversion rules:

1. **AST-based rules** (`reference-links`, `literal-brackets`,
   `grid-tables`, `definition-lists`, ...) — walk a successfully-parsed
   document and rewrite specific patterns. These require the file to parse
   first.
2. **Diagnostic-driven rules** (`q_2_*.rs`, `apostrophe_quotes.rs`) — read
   *parse failures* as input. Each one matches a specific `Q-2-N` error
   code from a failed parse, then edits the source to make that error go
   away, and (per the crate's iterative-fixing design) re-parses to see if
   another error surfaces.

This doc is about family 2, specifically the ones for **unclosed inline
delimiters** — emphasis, quotes, math, sub/superscript, strikeout, code
spans, editorial marks, footnotes, images. There are 18 such rules.

## Full inventory

| rule | delimiter | fix strategy | `opt_in_only`? |
|---|---|---|---|
| Q-2-5 | `_..._` (emphasis) | **append** closing `_` at end-of-block | no |
| Q-2-7 | `'` (apostrophe as quote-open) | **escape**: insert `\` before the mark | no |
| Q-2-11 | `"..."` (quote) | **append** closing `"` | no |
| Q-2-12 | `*...*` (emphasis) | **append** closing `*` | no |
| Q-2-13 | `**...**` (strong) | **append** closing `**` | no |
| Q-2-15 | `__..__` (strong) | **append** closing `__` | no |
| Q-2-16 | `^...^` (superscript) | **append** closing `^` | no |
| Q-2-17 | `~...~` (subscript) | **append** closing `~` | no |
| Q-2-18 | `~~...~~` (strikeout) | **append** closing `~~` | no |
| Q-2-19 | editorial insert | append `++]` | no |
| Q-2-20 | editorial delete | append `--]` | no |
| Q-2-21 | editorial comment | append `]` | no |
| Q-2-22 | editorial highlight | append `]` | no |
| Q-2-23 | `$...$` (inline math) | **append** closing `$` | no |
| Q-2-24 | `` `...` `` (code span) | append `` ` `` | no |
| Q-2-25 | image `![...](url)` | append `](url)` | no |
| Q-2-26 | inline footnote | append `]` | no |
| apostrophe-quotes | `'` (possessive, Q-2-10) | **escape**: insert `\` before the mark | no |

Only two of eighteen rules **escape** rather than **append**: Q-2-7 and
`apostrophe-quotes`, both handling `'`. Every other rule blindly inserts a
closer at the parser's error-recovery offset (end of block), with zero
awareness of what the "unclosed" content actually is. `literal-brackets`
(a different, AST-based rule) is the *only* rule in the crate marked
`opt_in_only()` — confirmed by grepping the trait: `rule.rs` defines the
default (`false`), and only `literal_brackets.rs` overrides it. So all 18
of the rules above run by default under `convert --rule all`.

## The confirmed-bad case: dollar (`bd-vz340zg6`)

`$` is structurally ambiguous between a math delimiter and ordinary prose
(currency: `50$`). The parser fails both in the identical LR state — there
is no AST to reason about, because the parse failed. `Q223Converter`
"fixes" this by appending a `$`, turning `The upgrade costs 50$` into
`The upgrade costs 50$$` — which still doesn't parse (`$$` opens display
math), so a fixpoint `convert --rule all` pass stalls on broken output,
having silently rewritten the author's prose.

That strand's investigation (full notes on branch
`braid/bd-dollar-not-math-fatal-fpnqrpil-prose-that-not-valid`) concluded
this **cannot be fixed by making the rule smarter** — nothing distinguishes
`$20` from `$x + y` from `$abc` structurally — and laid out three options:

- **(a) `opt_in_only()`** — stops default bulk corruption; cheap; but an
  explicit `-r q-2-23` run still corrupts.
- **(b) flip to escape** — insert `\` before the *opening* `$` instead of
  appending a closing one. The diagnostic already carries the opening
  mark's location (the `math-start` capture, surfaced as the blue "This is
  the opening '\$' mark." note) — `q_2_23.rs` just doesn't use it. This
  matches how Q-2-7 and `apostrophe-quotes` already behave, and is what
  `docs/errors/markdown/Q-2-23.qmd` already tells users to do by hand.
  Trade-off: a genuinely-unclosed math span becomes visibly-wrong literal
  text (`$x + y`) instead of silently-wrong prose-turned-broken-math —
  worse looking, but honest, and always parseable.
- **(c) delete the rule.**

That strand explicitly flags two follow-ups it did not do: check whether
Q-2-11 (double quote, also an append) has the same exposure, and it holds
up `apostrophe-quotes` as the *correct* reference implementation, not
something that also needs fixing.

## Do the same capture hooks exist for the other codes?

Checked the error-corpus JSON for each code (`crates/pampa/resources/error-corpus/Q-2-*.json`).
**Yes** — every append-rule's diagnostic already carries an "opening mark"
detail capture, in the same shape Q-2-23's `math-start` does:

| code | opening-mark capture label |
|---|---|
| Q-2-11 | `quote-start` |
| Q-2-16 | `super-start` |
| Q-2-17 | `sub-start` |
| Q-2-18 | `strike-start` |
| Q-2-23 | `math-start` |

So option (b) (flip append → escape) is not a one-off hack for dollar —
the plumbing already exists uniformly, and would very likely generalize
the same way to Q-2-11/16/17/18 with the same shape of one-line change
per rule (`q_2_N.rs` currently uses `violation.offset` — the append-site —
instead of the capture's start offset). This is worth confirming rule by
rule, not assumed.

## The untracked cases

### Caret (`^`, Q-2-16) — Gordon's "third character"

`^` is ambiguous the same way `$` is: exponents (`2^10`, `x^2`), XOR,
control-character notation (`^C`), git's parent-commit suffix (`HEAD^`).
No braid strand mentions this (searched `caret`, `superscript`, `q-2-16`,
`exponent` — nothing on point). Structurally, this looks like the same bug
as dollar: the autofix appends `^` at end-of-block regardless of what
"unclosed" content actually is.

**One caveat that may narrow the exposure vs. dollar.** The Q-2-16 and
Q-2-17 error-corpus case files both carry this note on their `simple`
case:

> "Opener whose closer is swallowed by a code span. Since
> `bd-star-as-str-qigl02pz` a sub/superscript only opens when its closer
> appears before the next whitespace, so a plain unclosed opener is
> literal text, not this error."

If accurate and currently in effect, a bare `2^10` with no matching second
`^` before whitespace wouldn't trigger Q-2-16 at all — it'd already parse
as literal text, same as Q-1's tolerant behavior. That would mean Q-2-16
only fires in narrower cases (e.g. a closer accidentally swallowed by an
adjacent code span, as in the case file's `` `^a`b^` `` example), which is a
much smaller and rarer surface than "any trailing `$`." **This needs
empirical confirmation** — `bd-star-as-str-qigl02pz` is still
`in_progress` in braid, not closed, so it's unclear whether the guard
described in that comment is fully live today or partially landed /
aspirational. Worth a five-minute check (`qmd-syntax-helper check -r
q-2-16` against a `2^10 exponent` fixture) before deciding this needs the
same urgency as dollar.

### Tilde (`~`, Q-2-17 subscript / `~~`, Q-2-18 strikeout)

Same ambiguity shape: approximation (`~5`, `~10`), home-directory paths
(`~/foo`), chemistry subscript notation where `H~2~O` is the *intended*
use. The parser-level version of this ambiguity is already independently
tracked — **but as a grammar bug, not an autofix-corruption bug**:

- `bd-5px05kui` (closed) — `~37MB` in a changelog parsed as unclosed
  subscript, broke a WASM test.
- `bd-dew2vn28` (open) — same shape, TS test suite.
- `bd-whitespace-flanked-delimiters-0ncy8bgq` (open) — the general case:
  whitespace-flanked `*`/`^`/`~` silently open emphasis/sup/sub.

None of these three describe what happens *after* the parse fails and
`qmd-syntax-helper`\'s Q-2-17/Q-2-18 autofix runs on the resulting
diagnostic — which, per the inventory above, would append a bogus closing
`~`/`~~` the same way Q-2-23 appends a bogus closing `$`. The same
`bd-star-as-str-qigl02pz` caveat noted for caret applies here too (same
comment appears in Q-2-17's case file) — strikeout (`~~`) is likely lower
risk than subscript (`~`) since a double-tilde is a much stronger, less
ambiguous signal than a single one.

### Double quote (`"`, Q-2-11)

Not a new finding — `bd-vz340zg6` itself asks "worth checking whether
q-2-11 ... has the same exposure" and never spun that into its own
strand. Ambiguous uses: inch/feet marks (`5"`), typo'd nested quotes.
Same append-only fix strategy as dollar/caret/tilde.

### Already correct: apostrophe (`Q-2-7`, `apostrophe-quotes`/`Q-2-10`)

Both apostrophe rules already escape rather than append, and are the
crate's *reference* implementation for this problem — not something that
needs fixing. Worth noting only because it's the proof that the escape
strategy is viable and already-shipped, not a hypothetical.

## Where this leaves things

Two-for-two so far on "does this delimiter have the same failure mode as
dollar": caret, plausibly yes (pending the empirical check above); tilde,
plausibly yes for subscript, less clear for strikeout; double-quote,
already flagged as an open question by the dollar strand itself. That's
suggestive of a **crate-wide pattern**, not four isolated one-off bugs —
16 of 18 delimiter rules append instead of escape, and the only two that
escape happen to be the only two ever audited for this problem.

## Open questions (not decisions)

- **Scope of fix**: handle each delimiter as its own strand (mirroring how
  `bd-vz340zg6` scoped dollar alone), or treat this as one crate-wide
  policy question — e.g. "audit all 16 append-rules against the
  escape-strategy playbook Q-2-7/apostrophe-quotes already established,
  case by case" — with one strand tracking the audit and possibly several
  landing separately?
- **Verify before prioritizing**: does Q-2-16/17 actually still fire on a
  bare unclosed `^`/`~` in ordinary prose today, or did
  `bd-star-as-str-qigl02pz`\'s whitespace-flanking rule already close that
  door for these two specifically (unlike dollar, which has no such
  guard — `bd-dollar-math-flanking-wzjx4hn8` is still open)? This changes
  how urgent caret/tilde are relative to dollar.
- **Is "escape" always the right default**, or only "always safer than
  silent corruption"? Escaping a genuinely-intended-but-typo'd delimiter
  still changes the author's file without asking, just in a way that
  fails visibly (`\$x + y`) instead of silently. Is "visibly wrong" the
  right default for a bulk, unattended `convert --rule all` pass, or
  should more of these move to `opt_in_only()` (option (a)) instead,
  leaving `check` to report but nothing to auto-edit until a human looks?
- **Q-2-11** never got its own strand despite `bd-vz340zg6` asking the
  question five weeks ago (created 2026-08-25) — is that an oversight, or
  a deliberate "wait until we see the pattern repeat" call that this
  research doc now resolves?
- Is there a **general lint** worth adding to the crate itself — e.g. a
  test asserting every diagnostic-driven rule either overrides
  `opt_in_only()` or has been explicitly reviewed for this failure mode —
  so a *new* Q-2-N rule added later doesn't silently reintroduce the
  pattern?

## Non-decision

This doc does not recommend a specific action. No braid strand has been
filed for caret, tilde, or double-quote — only `bd-vz340zg6` (dollar)
exists today. Whether this becomes one strand, several, or a crate-wide
policy strand is Gordon's call once the empirical caret/tilde question
above is answered.
