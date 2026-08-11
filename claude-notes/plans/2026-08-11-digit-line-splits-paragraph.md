# Continuation line starting with a digit terminates the paragraph (bd-digit-line-splits-paragraph-w6tod0gh)

**Date:** 2026-08-11
**Braid:** `bd-digit-line-splits-paragraph-w6tod0gh` (bug, P1, labels `bug` / `parity`)
**Branch:** investigated in place on `main` @ `fc2895b2` — no worktree created (see "Where this should land")
**Status:** Implemented on `braid/bd-digit-line-splits-paragraph-w6tod0gh-continuation-line-starting-digit`. Design questions answered by the user 2026-08-11; see "Answers" and "Implementation record" below.

## Triage verdict

**Ready to design.** The root cause is confirmed at the exact source lines the
strand names, both symptom variants reproduce at HEAD, the conformance target
is unambiguous (CommonMark — which q2 already implements for every neighbouring
case), and the repo already contains a *hand-escaped workaround in the spec
corpus* that doubles as the acceptance criterion. The one item the strand
flagged as "worth settling in the same pass" turns out to be already settled by
existing behavior. Remaining questions are narrow and listed below.

**Pre-flight:** `cargo xtask verify --skip-hub-build` passed clean at
`fc2895b2` (all 14 steps) before any investigation — the failures below are
pre-existing behavior, not local breakage.

**Discovered work:** `bd-cxiopjw7` — a `:`-leading continuation line *silently
deletes the colon*. Distinct bug, filed separately (see Finding 4).

## Issue context

A soft-wrapped paragraph line whose first character is a digit terminates the
paragraph, producing two paragraphs where the author wrote one and splitting a
sentence mid-clause. It fires on *any* digit-leading continuation line
regardless of what follows — verified against `30 minutes`, `1000,`, `1 apple`,
`9`, `3.14`, `0`, and a 14-digit run longer than any legal list marker.

**The fatal variant:** when the wrap falls inside link text, the block ends
before the closing `]` and the render fails with `[Q-2-1] Unclosed Span`. This
is what elevates the bug from cosmetic to build-breaking — and the error
message gives no hint that the line break is the cause.

**Real-world blast radius:** 8+ pages of the Posit Connect docs port, wherever
a version number, port, timeout or RFC number happened to land at the start of
a wrapped line. Authors cannot reasonably be expected to avoid wrapping before
a number.

Filed 2026-08-11 by Carlos Scheidegger; observed on 0.16.0, re-verified on
0.17.0 / `origin/main` @ `001cb6a5`.

## Dependency graph

**Empty.** `braid dep tree` shows the strand alone; `braid dep list` returns no
edges. There is no `discovered-from` parent in this skein — the originating
strand `br-mtg8ts05` lives in a *different* skein (the connect-docs porting
project), so the usual "why was this filed" context arrives via the handoff
prose rather than the graph.

No incoming `blocks` edges means no other strand is pinned on this. The urgency
is entirely external: it is a P1 because it breaks real documents, not because
it gates other work.

## What the code looks like today

The strand's root-cause analysis is **accurate and current** — verified line by
line against `fc2895b2`.

`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` has two
`SOFT_LINE_ENDING` gates, and both end with the same blanket character-class
exclusion:

- First gate, line **2750**: `first_lookahead > ' ' && !(first_lookahead >= '0' && first_lookahead <= '9')`
- Second gate, lines **2889–2890**: same test on `second_lookahead`

A digit-leading continuation line fails both gates, falls through to the
`LINE_ENDING` branch at line 2915, and `LINE_ENDING` terminates the paragraph.

**The asymmetry with `*` in the very same condition is the tell.** For `*` the
scanner peeks first — `first_starts_with_star_block` (2702–2718) /
`second_starts_with_star_block` (2857–2873) measure the delimiter run and
require trailing whitespace or EOL, so the soft break is blocked only when a
list marker or thematic break *really* forms. Backticks get the same treatment
(`first_starts_with_fence`, 3+ only). The digit branch has no equivalent peek.

`parse_ordered_list_marker` (line 1195) is **already correct** — it requires
digits followed by `.` or `)`, caps at 9 digits, and sets `dont_interrupt` for
anything but a bare leading `1`. It simply never gets consulted, because the
paragraph has already been closed by the time it would matter.

### Both variants reproduce at HEAD

```
$ cargo run -q --bin pampa -- digits.qmd -t html
<p>To make license leases last</p>
<p>30 minutes you would use this.</p>
...
<p>To make license leases last
thirty minutes you would use this.</p>     <- control: non-digit stays put

$ cargo run -q --bin pampa -- linkwrap.qmd -t html
Error: [Q-2-1] Unclosed Span
  I reached the end of the block before finding a closing ']' for the span or link.
```

Fixtures and the full evidence table are committed under
`claude-notes/plans/digit-line-splits-paragraph-investigation/`.

### Finding 1 — the corpus already carries a hand-escaped workaround

This is the most useful thing the investigation turned up.
`test/corpus/new-spec.txt:2039` reads:

```
Example 284 - https://github.github.com/gfm/#example-284 (qmd: start your soft breaks with \1, i'm sorry.)
```

The CommonMark spec input was rewritten from `14.` to `\14.` so the test could
pass, with an apology in the test title. A previous session hit this exact bug,
could not fix it, and encoded the workaround into the conformance suite.

Notably, the connect-docs porting session independently reached for the *same*
escape hatch (`\3986`). Two unrelated sessions invented the same workaround —
good evidence the bug is a recurring tax rather than a one-off.

Fed the real spec input, q2 does not merely split the paragraph — it **invents
a list**:

```html
<p>The number of windows in my house is</p>
<ol start="14" type="1">
<li>The number of doors is 6.</li>
</ol>
```

Pandoc's commonmark reader emits one paragraph. **Restoring Example 284 to the
unescaped spec input and deleting the apology is the natural acceptance
criterion for this fix.**

### Finding 2 — the conformance target is already settled

The strand flags section D (a genuine `1. apples` interrupting a paragraph) as
"worth settling in the same pass," since Pandoc's markdown reader forbids list
interruption entirely while CommonMark permits it for `1.`. **Investigation
suggests this is already decided.** The interruption matrix
(`interrupt.qmd`, full table in the investigation README):

| continuation line | q2 @ HEAD | pandoc commonmark | pandoc markdown (Q1) |
| --- | --- | --- | --- |
| `- apples` | interrupts | interrupts | no |
| `1. apples` | interrupts | interrupts | no |
| `2. apples` | **interrupts** ✗ | **no** | no |
| `1) apples` | interrupts | interrupts | no |
| `1.5 dollars` | **splits** ✗ | one `<p>` | one `<p>` |
| `3986 for details` | **splits** ✗ | one `<p>` | one `<p>` |

q2 already matches CommonMark and diverges from pandoc-markdown on `-`, `1.`
and `1)`. Adopting Q1's never-interrupt rule would mean changing the `-`
behavior too — a much larger, separate decision. The proposed fix lands q2 on
CommonMark for all six rows.

### Finding 3 — the fix also repairs the `2.` case for free

Because the blanket exclusion closes the paragraph *before*
`parse_ordered_list_marker` runs, that function's correct `dont_interrupt`
logic is dead code on this path — which is why `2. apples` currently opens a
spurious `<ol start="2">`, wrong under **both** specs. Gating on a real peek
restores the marker path and fixes this row without extra work. Worth stating
explicitly in the eventual commit message, since it is a behavior change beyond
the reported symptom.

### Finding 4 — the sibling exclusions are not harmless

`-`, `+` and `:` are blanket-excluded from the same gates without a peek. The
strand judges this harmless for `-` because a `-`-leading line genuinely can
open a list. Checking it directly shows otherwise:

| input | q2 @ HEAD | pandoc commonmark |
| --- | --- | --- |
| `Temperature dropped to` / `-5 degrees overnight.` | two `<p>` | one `<p>` |
| `Gain was` / `+5 percent.` | two `<p>` | one `<p>` |
| `Defined at` / `:host scope.` | one `<p>`, **colon deleted** | one `<p>`, colon intact |

`-5` and `+5` split exactly like digits — the same bug in a different character
class, and the same fix shape applies (measure the run, require trailing
whitespace).

**The `:` case is worse and is a different bug.** q2 keeps one paragraph but
silently drops the colon — confirmed at AST level as
`Str "at", SoftBreak, Str "host"`. A character disappears from the document with
no diagnostic. Because a single paragraph survives, the colon is being consumed
somewhere beyond the gate (most likely the fenced-div `:::` or definition-list
marker path), so the gate fix alone will not necessarily resolve it. **Filed as
`bd-cxiopjw7`** (P1 bug, `discovered-from` this strand) rather than folded in
here — it needs its own root-cause hunt.

### Blast radius on existing tests is small

Scanning all 50 corpus files for a digit-leading line preceded by a text line
yields exactly two hits:

- `new-spec.txt:2083` — Example 285 (`1.` interrupting). Correct today, must
  **stay** correct; `dont_interrupt` is false for a bare `1`.
- `issues.txt:144` — `$$ / x / 1. y / $$`, display math expected to stay one
  paragraph. Exercises the code-span/math bypass, not the digit class.

Plus `new-spec.txt:2039` — Example 284, the escaped workaround, which we intend
to change.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD; failing tests written and confirmed failing first).**
  - Restore Example 284 in `new-spec.txt` to unescaped spec input, drop the apology.
  - New `paragraph.txt` cases following the established `bd-af1e` / `bd-1xph`
    naming convention: digit-leading continuation (prose), digit + punctuation,
    non-digit control, multi-digit run > 9, `2.` non-interrupting, `1.`
    interrupting, digit-leading inside link text, digit-leading in a list-item
    continuation, digit-leading after a block-quote prefix (second gate).
  - Confirm each fails at HEAD before touching `scanner.c`.
- **Phase 1 — Scanner change.** Add `first_starts_with_ordered_marker` /
  `second_starts_with_ordered_marker` peeks mirroring the star peek; gate the
  digit exclusion on the flag instead of the raw character class.
- **Phase 2 — Regenerate + corpus green.** `tree-sitter generate; tree-sitter
  build; tree-sitter test` in `crates/tree-sitter-qmd/tree-sitter-markdown`.
- **Phase 3 — Workspace regression sweep.** `cargo nextest run --workspace`;
  review any snapshot churn per the CLAUDE.md snapshot-reporting rules (expect
  some — this changes block structure).
- **Phase 4 — End-to-end verification.** Render the investigation fixtures
  through the real binary and inspect output; confirm `linkwrap.qmd` renders
  instead of erroring.
- **Phase 5 — `cargo xtask verify`** (full, not `--skip-hub-build` — the
  grammar feeds the WASM parser).

## Open design questions for the user

1. **Confirm the CommonMark target.** Finding 2 argues q2 has already chosen
   CommonMark interruption semantics (it matches on `-`, `1.`, `1)`), so the
   fix should land all six matrix rows on CommonMark and section D of the repro
   is *not* a divergence to fix. Do you agree, or do you want Q1/pandoc-markdown
   parity (never interrupt) — which would be a larger change touching `-` too,
   and probably its own strand?

2. **Is the `2. apples` change in scope for this strand?** It is a genuine
   behavior change beyond the reported symptom (today: spurious `<ol start="2">`;
   after: stays in the paragraph). It falls out of the fix for free and moves
   toward the spec, but it is not what the strand reports. Ship it here and note
   it in the commit, or split it out?

3. **The peek's bail-out bound.** `parse_ordered_list_marker` caps at 9 digits.
   Should the peek bail after 10 digits (cheapest, matches the marker parser),
   or scan the full run? Only matters for pathological input; I'd default to
   bailing at 10 unless you prefer otherwise.

4. **The `-` / `+` exclusions (Finding 4) — same strand or a follow-up?** They
   reproduce the identical split and the fix is the same shape as the digit
   peek (measure the run, require trailing whitespace). Doing all three
   character classes in one pass is arguably cheaper than three separate
   grammar rebuilds and three rounds of snapshot review. But it widens a P1
   bug fix. My inclination is to fold `-`/`+` in here and leave `:` to
   bd-cxiopjw7, but it is your call.

## Answers (user, 2026-08-11)

1. **CommonMark target** — confirmed. Get as close to CommonMark as possible;
   flag it if existing tree-sitter spec tests break.
2. **`2. apples`** — in scope. Behavioral changes are better batched so people
   relearn habits once.
3. **Digit bail-out** — bail past 9 digits, the conservative check.
4. **`-` / `+`** — folded into this strand; `:` left to bd-cxiopjw7.

## Implementation record

### The change

`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` only — no grammar
change, so `parser.c` is untouched and there are no generated diffs.

Two peek helpers, mirroring the existing `*` peek:

- `peek_dash_plus_opens_block` — a single `-`/`+` followed by whitespace/EOL
  (list marker), or 3+ `-` followed by whitespace/EOL (thematic break). `--`
  and `-5` are prose.
- `peek_ordered_marker` — returns an `OrderedMarkerPeek { well_formed,
  may_interrupt }`, bailing past 9 digits.

Both SOFT_LINE_ENDING gates now consult those flags instead of excluding the
raw character classes.

### The one non-obvious part

The two gates need **different answers from the same peek**, which the first
implementation attempt got wrong and six existing tests caught:

- **Gate 1 runs before `match_line`**, so it cannot yet distinguish a sibling
  list item from a paragraph continuation, and it has no `all_will_be_matched`
  guard. It must ask only `well_formed`. Asking the stricter question here
  swallowed `2.` in `1. a` / `2. b` into item 1's paragraph — breaking
  `issues.txt` #72, `list.txt` 4/15/16, and CommonMark Examples 261/282.
- **Gate 2 runs after `match_line`**, where `all_will_be_matched` proves the
  line belongs to the open blocks. Only there is the CommonMark interruption
  rule (`may_interrupt`) the right question.

That split is what separates two cases identical at the character level:
`14.` after a paragraph must soft-break (CM 284), while `2.` after `1. a`
must open a new item (gate 2 never fires, because `match_line` fails to match
item 1's indent).

A second attempt consulted `valid_symbols[LIST_MARKER_*]` the way
`parse_ordered_list_marker` does. That fails: at line-ending scan time the
parser is asking for `LINE_ENDING`/`SOFT_LINE_ENDING`, so the `LIST_MARKER_*`
symbols are not in the valid set and read as false for every marker. The code
comment records this so the next person doesn't retry it.

Also extended: the `mark_end` guard after gate 2 was keyed on the *character*
(`!= '`' && != '*'`). With three more peeking branches that proxy no longer
holds, so it is now an explicit `second_peeked` boolean — otherwise a peeked
run would be swallowed into the SOFT_LINE_ENDING token's range.

### Tests

- 15 new `paragraph.txt` cases (15–29) covering digit prose, digit +
  punctuation, a run longer than any legal marker, `14.`/`2.` non-interrupting,
  `1.`/`1)` still interrupting, the fatal link-text variant, both second-gate
  paths (list-item and block-quote continuation), `-5`/`+5`, and guards for
  `- item` / `+ item` / `---`.
- **Three hand-escaped workarounds restored to their real inputs**, all three
  of which failed before the fix and pass after:
  - `new-spec.txt` Example 284: `\14.` → `14.`, apology dropped from the title.
  - `block_quote.txt` test 4: `\1` → `1`, "but don't start continuation line
    with 1" dropped from the title. (Lazy continuation into a block quote —
    found during implementation, a third instance of the same tax.)
  - Example 284's expected tree also needed updating, since the escaped input
    had produced an extra `pandoc_str` for the backslash.

TDD sequence: 12 tests confirmed failing before any scanner edit; corpus is
now **572/572**.

### Results

- `tree-sitter test`: 572/572 (was 560/572 with the new tests added).
- `cargo nextest run --workspace`: **11626 passed**, 197 skipped.
- **Zero snapshot files changed.** The predicted snapshot churn did not
  materialize — the AST only changes for inputs that were previously
  mis-blocked, and no existing snapshot covered one.

### End-to-end verification

`cargo run --bin q2 -- render doc.qmd`, output inspected in `doc.html`:

```html
<p>To make license leases last
30 minutes you would use the following syntax.</p>

<p>The set of characters that must be encoded can be found in
<a href="https://www.rfc-editor.org/rfc/rfc3986#section-3.2.1">Section 3.2.1 of RFC
3986</a>.</p>

<p>Buy these:</p>
<ol type="1">
<li>apples</li>
<li>oranges</li>
</ol>
```

The first paragraph is no longer split; the link that previously failed with
`[Q-2-1] Unclosed Span` renders intact; a real ordered list still works.

Against the reference, `pampa -t native` on the committed fixtures now matches
`pandoc -f commonmark -t native` **exactly** for `interrupt.qmd` (all six
matrix rows) and `dashplus.qmd` — including `Str "+5"`, confirming that the
two `pandoc_str` CST nodes for `+5` (pre-existing `+` tokenization, present
mid-line too) merge into a single `Str` at the AST level. The only remaining
divergence in `dashplus.qmd` is the `:host` colon, deliberately left to
bd-cxiopjw7.

## Risks / tradeoffs (draft)

- **Snapshot churn.** This changes block structure, so downstream snapshot tests
  across the workspace may move. Per CLAUDE.md, every `.snap` change needs to be
  counted, summarized, and any surprising one flagged. Budget review time for
  this — it is the most likely place a real regression hides.
- **The scanner's peek-without-`mark_end` idiom is subtle.** The existing
  comments (2658–2662, 2752–2769) are explicit that tree-sitter rewinds to the
  last `mark_end` between scan calls, and that the star/backtick peeks
  deliberately avoid marking. The new peek must follow the same discipline; the
  `first_peeked` flag also feeds the second gate's short-circuit at 2842, so the
  ordered-marker peek needs to participate in that handshake correctly rather
  than being bolted on beside it.
- **Two gates, one rule.** The fix must land in both gates; the second one runs
  after `match_line` (post block-quote prefix). A block-quote-prefixed test case
  is the cheapest way to prove the second gate is actually covered.
- **WASM leg.** The grammar feeds `wasm-qmd-parser`, so full `cargo xtask
  verify` is required before this can ship — not the `--skip-hub-build` variant.

## Where this should land

Investigated in place on `main` per the skill's contract (this skill does not
create branches or worktrees). Given the snapshot-churn risk and that the
grammar rebuild produces large generated diffs, **a dedicated branch or worktree
is warranted for the implementation** — recommend
`cargo xtask create-worktree bd-digit-line-splits-paragraph-w6tod0gh`, but
that is the user's call to make.
