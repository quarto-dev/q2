# Adjacent footnote definitions merge — the second note is silently lost (bd-adjacent-footnote-definitions-miif1k1z)

**Date:** 2026-08-12
**Braid:** bd-adjacent-footnote-definitions-miif1k1z (bug, p2, label `parser`)
**Worktree:** `.worktrees/bd-adjacent-footnote-definitions-miif1k1z-adjacent-footnote-definitions-merge`
(branch `braid/bd-adjacent-footnote-definitions-miif1k1z-adjacent-footnote-definitions-merge`, based on `main` @ `7bcddf61`)
**Pre-flight:** `cargo xtask verify --skip-hub-build` **green** before any change
(11728 tests run, 11728 passed, 197 skipped; all 14 steps passed). Run in the
main checkout at `c28cfd81`; `main` has since been reset to `7bcddf61`, which
differs only by two unrelated Q-2-10 *plan* commits (docs).
**Status:** Design settled 2026-08-12 — **implementation approved and in
progress.** See "Design decisions (settled)" below.

> **Note on where this work lives.** The investigation was originally done in
> the main checkout. Partway through implementation another session reset
> `main` and switched branches *in that same working tree*, discarding the
> uncommitted scanner work. Nothing was lost permanently — the investigation
> commit was recovered from the reflog and cherry-picked here — but the fix is
> now developed in a dedicated worktree so a concurrent session cannot disturb
> it. If you are picking this up: work here, not in the main checkout.

## Triage verdict

**Ready to design**, but the fix as filed rests on one false premise and one
unexamined behavior choice, both of which the user has to settle first (Q1 and
Q2 below). The root cause is confirmed exactly as described; the mechanical fix
is a ~30-line mirror of 92737cdd. What is *not* settled is whether a `[^id]:`
line should interrupt an ordinary paragraph — Pandoc says no, and q2's own
model says yes.

## Issue context

Filed 2026-08-12 by "Claude (q2-connect-docs)", one day after the sibling
strand it references. Two footnote definitions on consecutive lines parse as
one: the second is absorbed as lazy paragraph continuation into the first
note's body, and its reference renders as an empty `<span>` — no marker, no
diagnostic. Origin strand in the connect-docs skein: `br-pj9i6ejq`. Real-world
hits: `user/manifest/index.qmd` and `admin/integrations/tableau`.

The description is accurate and well-researched. Everything below either
confirms it or adds a finding it did not have.

## Dependency graph

**The graph is empty.** `braid dep tree` and `braid dep list` both return only
the strand itself — no `discovered-from`, no `blocks`, no `related`. There is
no incoming pressure and no parent epic. The context that would normally come
from a `discovered-from` edge is instead carried in the description's prose
reference to **bd-digit-line-splits-paragraph-w6tod0gh**.

That sibling is the important context, and it is worth reading in full
(`braid show bd-digit-line-splits-paragraph-w6tod0gh` — both comments):

- Same subsystem, same two SOFT_LINE_ENDING gates, same "leader list is a
  blanket character class instead of a shape peek" defect.
- **Inverse symptom.** There, a line that was *not* a block opener wrongly
  ended the paragraph. Here, a line that *is* a block opener wrongly fails to.
- Fixed by commit **92737cdd** (`peek_ordered_marker` /
  `peek_dash_plus_opens_block`), which is **on `main`**. That commit is the
  template this fix should follow.
- 92737cdd then shipped a P0 regression in v0.18.0 (bd-j7be7kuc) because every
  new corpus case sat at indent 0. That is why
  `claude-notes/instructions/scanner-indentation-contexts.md` exists and why
  the strand asks for the indent × context sweep. **That sweep is done — see
  below.**

Two open neighbours in the same code, both listed here so the user can decide
whether to batch (see Q4):

- **bd-mt1ksg9b** — the gate-1/gate-2 `*` peeks still have no indentation
  guard; `hello\n     * item` is a hard parse error. Explicitly deferred from
  bd-j7be7kuc to keep that fix scoped.
- **bd-z69hr4o0** — indented backtick/star continuation lines emit a spurious
  `Space` after `SoftBreak`.

## What the code looks like today

Every file and line the description cites still exists and still has the
described shape (line numbers have drifted since filing):

| Description says | Actually at HEAD |
| --- | --- |
| `inline_ref_def: seq(ref_id_specifier, _whitespace, pandoc_paragraph)` @ grammar.js:283 | grammar.js:283 — **exact match** |
| gate 1 leader list @ scanner.c ~2939-2945 | scanner.c:2940-2947 |
| gate 2 leader list @ scanner.c ~3120-3123 | scanner.c:3118-3125 |
| `parse_ref_id_specifier` @ scanner.c:1795 | scanner.c:1795 — **exact match** |
| `peek_ordered_marker` / `peek_dash_plus_opens_block` | scanner.c:1359 / 1400 |

Confirmed: `'['` is absent from both leader lists. A `[`-leading continuation
line falls through to `first_lookahead > ' '` and becomes a soft break.

### Reproduced at HEAD — three levels

Fixtures committed under
`claude-notes/plans/adjacent-footnote-definitions-investigation/`.

**1. CST** (`tree-sitter parse repro.qmd`) — the definition swallows both lines:

```
(inline_ref_def [4, 0] - [6, 0]          <-- spans BOTH definition lines
  (ref_id_specifier [4, 0] - [4, 8])
  (pandoc_paragraph [4, 9] - [6, 0]
    ...
    (inline_note_reference [5, 0] - [5, 7])   <-- '[^asgi]' became a *reference*
```

The control (section B, blank-line-separated) yields two sibling
`inline_ref_def` nodes as expected.

**2. AST** — `cargo run --bin pampa -- -t native repro.qmd` errors with
`[Q-3-10] Inline note definitions not supported` (the native writer does not
support them at all), but the diagnostic's own span is the proof: it underlines
**lines 5–6 together** as a single node.

**3. End-to-end HTML** — `cargo run --bin q2 -- render repro.qmd --to html`,
output file inspected (`repro.html`, committed):

```html
<!-- line 22: the second reference is an INVISIBLE empty span -->
<p>App modes: <code>python-api</code> (flask<span id="fnref1">…1…</span>)
and <code>python-fastapi</code><span class="quarto-note-reference"
data-reference-id="asgi"></span>.</p>

<!-- lines 33-34: note 1's body has the second definition glued on -->
<p>Other WSGI-compliant application frameworks may be served via this app mode.
<span class="quarto-note-reference" data-reference-id="asgi"></span>: Other
ASGI-compliant application frameworks may be served via this app mode.<a …>↩︎</a></p>
```

Matches the filed symptom byte for byte. Section B renders correctly (two
numbered notes).

### Finding 1 — the bug is broader than "adjacent definitions"

The title describes the case the Connect docs happened to hit. The actual rule
is more general: **no `[^id]:` line ever opens a definition when it follows a
non-blank line.** A plain paragraph is enough:

```
hello there.
[^b]: two.
```

→ one paragraph; no `inline_ref_def` at all. Definition-after-definition is
just the instance that shows up in real documents, because a note body is
itself a paragraph.

### Finding 2 — but Pandoc *agrees with q2* on that broader case

This is the finding that complicates the fix. Sweep run against
`pandoc 3.9.0.2` (`pandoc-sweep.txt`, committed):

| # | Input (2 lines) | Pandoc | q2 today |
| --- | --- | --- | --- |
| A0 | `[^a]: one.` / `[^b]: two.` | **two defs** | one def (BUG) |
| A1 | `[^a]: one.` / `␣[^b]: two.` | **two defs** | one def (BUG) |
| A3 | `[^a]: one.` / `␣␣␣[^b]: two.` | **two defs** | one def (BUG) |
| A4 | `[^a]: one.` / `␣␣␣␣[^b]: two.` | one def (absorbed) | one def ✓ |
| B | `hello there.` / `[^b]: two.` | **one paragraph, literal `[^b]:` text** | one paragraph ✓ |
| D | `hello there.` / `[^b] is a ref.` | one paragraph | one paragraph ✓ |
| E | `> [^a]: one.` / `> [^b]: two.` | two defs in quote | one def (BUG) |
| F | `- item text` / `␣␣[^b]: two.` | one paragraph | one paragraph ✓ |

Pandoc does **not** treat a footnote definition as a paragraph-interrupting
block opener. Its note body is a raw-line collector that stops at a blank line
or at the next `[^id]:` — it interrupts a *definition body*, never an ordinary
paragraph. So a naive "add `[` to both gates" fix repairs A0/A1/A3/E and
simultaneously changes B *away* from Pandoc.

### Finding 3 — q2's note body is already an ordinary paragraph, and already diverges

The reason Finding 2 is not fatal. Pandoc's note body absorbs *everything*:

```
[^a]: one.        Pandoc: Note [Para [Str "one.", SoftBreak, Str "#", Space, Str "Heading"]]
# Heading         q2:     inline_ref_def ... THEN a real (section (atx_heading))

[^a]: one.        Pandoc: Note [Para [Str "one.", SoftBreak, Str "-", Space, Str "item"]]
- item            q2:     inline_ref_def ... THEN a real (pandoc_list (list_item))
```

q2 already treats the note body as an ordinary paragraph that any block opener
interrupts. Under that model, `[^id]:` interrupting a paragraph is the
*consistent* answer, and case B's divergence is the same divergence q2 already
accepts for `#` and `-`. Matching Pandoc on B instead would require the scanner
to know "am I inside an `inline_ref_def` body or a plain paragraph" — state it
does not have (`inline_ref_def` is a grammar rule, not a scanner open-block).

Related deliberate narrowing already in the tree: **Q-2-30** ("Multi-Paragraph
Footnote Indentation Not Supported") rejects Pandoc's indented multi-paragraph
note form outright and points at `::: ^ref … :::`. q2's note-def body is
already a deliberately smaller construct than Pandoc's. Verified firing at
HEAD.

### Finding 4 — the indent rule comes out right for free

92737cdd's guards (`s->indentation <= claimable_list_indentation(s) + 3` at
gate 1, residual `<= 3` at gate 2) reproduce Pandoc's A0–A4 column exactly:
≤3 columns opens a new definition, ≥4 is body content. A4 already behaves
correctly today by accident, and the guard preserves that. No new indent policy
has to be invented.

### Finding 5 — the "link reference definition" premise is false

The description suggests designing the peek "to answer for both bracket forms
rather than footnotes alone", citing `[ref]: url` as a sibling issue. **There is
no such construct in qmd and no such strand.**

- `CLAUDE.md`: "the qmd format only supports the inline syntax for a link
  `[link](./target.html)`, and not the reference-style syntax `[link][1]`."
- `grammar.js` has no `link_reference_definition` rule (grep: no hits).
- Observed: `hello there.` / `[ref]: http://example.com` parses to
  `pandoc_span` + literal text — a bracket span, not a definition.

Generalizing the peek would build a mechanism with no consumer. Recommend
scoping it to `[^id]:` only (Q3).

### Finding 6 — a second, independent defect: unresolved references vanish silently

`crates/quarto-core/src/transforms/footnotes.rs:445-450`:

```rust
if let Some(number) = collector.resolve_reference(&ref_id, source_info.clone()) {
    *inline = create_footnote_ref(number, &source_info, collector.is_margin);
}
// If not resolved, leave as-is (broken reference).
```

The span left behind is **empty** (pampa lowers `[^id]` to a contentless
`Span` carrying only `class="quarto-note-reference"` + `data-reference-id`), so
a broken reference renders as an invisible `<span></span>` — no marker, no
text, no diagnostic. Pandoc keeps the literal `[^id]` text *and* warns.

This is why the strand's title says "**silently** lost". It is a separate
defect from the scanner gate: even with the parser fixed, any typo'd note id
still disappears without a trace. Should be its own strand (Q5).

### Finding 7 — missing `Footnotes` appendix heading (from the repro README)

Confirmed at HEAD. The rendered footnotes section is `<section id="footnotes">`
→ `<hr />` → `<ol>`, with no `<h2 class="anchored quarto-appendix-heading">
Footnotes</h2>`. Q1 emits it. `grep -rn '"Footnotes"' crates/ --include='*.rs'`
returns **zero hits** — nothing in the tree ever emits it. Cosmetic, but it is
why several Connect pages show up in a text diff against Q1. Separate strand
(Q5).

### Finding 8 — blast radius is essentially nil

- **tree-sitter corpus**: scanning all 50 `test/corpus/*.txt` for a
  `[...]:`-leading line preceded by a non-blank line yields **0 hits**.
- **pandoc-match corpus** (`crates/pampa/tests/pandoc-match-corpus/`): **no
  footnote fixtures at all** — so `unit_test_corpus_matches_pandoc_markdown`
  cannot be perturbed by case B's divergence.
- **repo-wide `.qmd`/`.md`**: one hit, `docs/errors/markdown/Q-2-30.qmd:24`,
  and it is *inside a fenced ` ```markdown ` block* — illustrative, not parsed.

Snapshot churn is the residual risk (this changes block structure), same as
92737cdd — which in the end changed zero snapshots.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD, failing first).** tree-sitter corpus cases for
  the full indent × context sweep per
  `claude-notes/instructions/scanner-indentation-contexts.md`: indent 0/1/3/4+
  × {top level, inside list item at and past the content column, inside block
  quote}, plus the negative cells that must *not* trip the peek (`[not a
  footnote] in prose`, `[^b] is a ref.`, `[ref]: url`). Plus a pampa
  end-to-end HTML test on the `[^wsgi]`/`[^asgi]` pair.
- **Phase 1 — `peek_ref_id_specifier`.** Shape-only helper next to
  `peek_ordered_marker` / `peek_dash_plus_opens_block`; no `mark_end`, no
  `EMIT_TOKEN`; mirrors `parse_ref_id_specifier`'s id-character rule
  (scanner.c:1808-1815) and requires the closing `]` *and* the `:`.
- **Phase 2 — wire into both gates.** `'['` branch in each, guarded by the
  92737cdd indentation rule; add `first_peeked` / `second_peeked` bookkeeping.
  Re-read the "treacherous" list in `scanner-indentation-contexts.md` against
  the change.
- **Phase 3 — sweep + full verify.** `tree-sitter test`, then full
  `cargo xtask verify` (**not** `--skip-hub-build` — the grammar feeds the WASM
  parser), then end-to-end `q2 render` on the committed repro, HTML inspected.
- **Phase 4 — divergence note + docs.** Record the case-B decision (Q2) wherever
  it lands: a code comment in the gates at minimum, and — if the answer is
  "diverge deliberately" — a line in the qmd-vs-Pandoc docs.
- **Phase 5 — file the spin-off strands** (Q5).

## Design decisions (settled)

User answers, 2026-08-12. These close Q1–Q5 below; the questions are kept for
the reasoning that produced them.

1. **`[^id]:` DOES interrupt an ordinary paragraph.** `hello there.` /
   `[^b]: two.` becomes paragraph + definition. This is a deliberate divergence
   from Pandoc, in the same direction q2 already diverges for `#` and `-`
   (Finding 3).
2. **A code comment is enough** for that divergence — no user-facing docs pass
   in this change. Phase 4 shrinks to a comment in the gates.
3. **Peek is scoped to `[^id]:` only.** No general bracket form; the
   link-reference-definition motivation does not apply (Finding 5).
4. **bd-mt1ksg9b stays separate.** This strand goes first.
5. **Both spin-offs filed** (Finding 6 / Finding 7):
   - **bd-jttkymsw** (p2) — unresolved reference renders as an invisible empty
     span with no diagnostic. Survives this fix.
   - **bd-v9zs83zj** (p3) — footnotes appendix omits the `Footnotes` heading.
     Dispatched separately by the user.

## Work items

- [x] Phase 0 — indent × context corpus cases, confirmed failing first
- [x] Phase 1 — `peek_ref_id_specifier`
- [x] Phase 2 — wire into both gates
- [x] Phase 3 — `tree-sitter test` + full `cargo xtask verify` + end-to-end render
- [x] Phase 4 — divergence code comment

## Implementation record

**Files touched: `src/scanner.c` and `test/corpus/inline_ref_def.txt` only.**
No grammar change, so `parser.c` is untouched and there are no generated
diffs — same shape as 92737cdd.

### Phase 0 — TDD

11 corpus cases appended to `test/corpus/inline_ref_def.txt`, covering the
indent × context sweep required by
`claude-notes/instructions/scanner-indentation-contexts.md`. Confirmed failing
first, and failing in exactly the right places: **6 failed / 5 passed**, where
the 5 passers are the cells that must *not* change (indent 4, over-indented in
a list, and the three negatives — bracket span in prose, bare `[^b]` reference,
`[ref]: url`). After the fix: **11/11**.

### The load-bearing constraint: the peek must never be looser than the parser

The non-obvious part of this fix. `inline_ref_def` is
`seq(ref_id_specifier, _whitespace, pandoc_paragraph)`, and *both* the
whitespace and a non-empty body are mandatory. Probed at block start:

| Input | Parses to |
| --- | --- |
| `[^x]: two.` | `inline_ref_def` |
| `[^x]:\ttwo.` | `inline_ref_def` |
| `[^]: two.` | `inline_ref_def` (empty id is legal) |
| `[^x]:two.` | **ERROR node** |
| `[^x]:` | **ERROR node** |
| `[^x]: ` (empty body) | **ERROR node** |

So `peek_ref_id_specifier` requires the colon, then whitespace, then a
non-whitespace character before EOL — stricter than `parse_ref_id_specifier`,
deliberately. Had the peek merely mirrored `parse_ref_id_specifier`, a line
like `[^x]:two.` would have had its soft break suppressed and then failed to
form any block: a **hard parse error, which drops the whole file from the
render**. That is the bd-j7be7kuc failure mode. The invariant is written into
the helper's comment: stricter is always safe (the line stays paragraph
continuation, today's behavior), looser turns benign prose into an
unrenderable file.

### The one place this fix departs from 92737cdd's shape

The dash/plus branch sets `first_peeked = true` *outside* its indentation
guard, with a comment explaining that the over-indent verdict must still take
the peeked emission path. **Copying that verbatim for `[` was wrong**, and the
over-indented-list-item corpus case caught it: it regressed from passing to
failing, gaining a `(block_continuation)` child inside its `pandoc_soft_break`.

The reason the two differ: on the over-indent path the `[` branch never
advances the lexer, so leaving `first_peeked` false reproduces byte for byte
what the line did before `[` had a branch at all. Both `first_peeked` and
`second_peeked` are therefore set *inside* the guard. This is noted in the code
so nobody "restores symmetry" later.

### Behavior change beyond the reported symptom

Batched deliberately, per the user's decision (Design decision 1):

- `hello there.` / `[^b]: two.` is now a paragraph **plus a definition**
  (previously one paragraph). Pandoc keeps one paragraph. Same for a
  definition at a list item's content column.

The full sweep after the fix (`sweep-after.txt`) matches the Pandoc column
(`pandoc-sweep.txt`) on **every cell except B and F**, which are exactly this
divergence.

### Results

- `tree-sitter test`: **593/593** (582 pre-existing + 11 new), zero regressions.
- Full `cargo xtask verify` (**not** `--skip-hub-build`): **all 14 steps
  passed**, exit 0. Rust tests **11728 passed**, 197 skipped. Step 4
  (tree-sitter grammars) and step 7 (hub-client incl. WASM) both ran — the
  WASM leg matters because the grammar feeds `wasm-qmd-parser`.
- **Zero snapshot files changed.** The churn flagged as the main risk did not
  materialize — same outcome as 92737cdd.

### End-to-end

`cargo run --bin q2 -- render repro.qmd --to html`, output file inspected.
Both references resolve to real superscript links, four correctly-numbered
notes, note 1's body no longer carries the second definition, and **no
`quarto-note-reference` span survives anywhere in the output**:

```html
<p>App modes: <code>python-api</code> (flask<span id="fnref1"><sup><a href="#fn1"
class="footnote-ref" role="doc-noteref">1</a></sup></span>) and
<code>python-fastapi</code><span id="fnref2"><sup><a href="#fn2"
class="footnote-ref" role="doc-noteref">2</a></sup></span>.</p>
...
<li><div id="fn1"><p>Other WSGI-compliant application frameworks…</p></div></li>
<li><div id="fn2"><p>Other ASGI-compliant application frameworks…</p></div></li>
```

Before the fix the same file produced
`<span class="quarto-note-reference" data-reference-id="asgi"></span>` — an
invisible empty span — with the second definition's text pasted into note 1.

### A trap for the next person: `sweep.sh` was path-absolute

The committed sweep script originally `cd`-ed to an absolute path in the main
checkout. Run from this worktree it silently swept the *main checkout's*
parser and reported pre-fix results. It now resolves the grammar directory
relative to `BASH_SOURCE`, so it always sweeps the checkout it lives in.
`sweep-after.txt` was regenerated after that fix.

### Still open, unchanged by this fix

- **bd-jttkymsw** — an unresolved reference still renders as an invisible empty
  span with no diagnostic. This fix removes one *cause* of unresolved
  references; it does not touch the resolution path.
- **bd-v9zs83zj** — the `Footnotes` appendix heading is still missing.
- **bd-mt1ksg9b** — the `*` gate peeks still lack their indentation guard.

## Open design questions for the user

1. **Does `[^id]:` interrupt an ordinary paragraph?** (The central question —
   Findings 2 + 3.) Pandoc says no; q2's existing model says yes, because q2
   already lets `#` and `-` interrupt a note body where Pandoc absorbs them.
   Concretely, after the fix, what should `hello there.` / `[^b]: two.`
   produce — one paragraph (Pandoc parity, needs scanner state that does not
   exist) or a paragraph + a definition (consistent with q2's own model, ~30
   lines)? **My recommendation: the latter**, documented as a deliberate
   divergence.

2. **Should the divergence in Q1 be documented user-facing, or is a code
   comment enough?** If a user writes a definition immediately after a
   paragraph, Q1's answer decides whether they get a note or literal text —
   and Pandoc gives the other one.

3. **Scope of the peek: `[^id]:` only, or bracket forms generally?** Finding 5
   says the link-reference-definition motivation in the description does not
   apply — qmd has no such construct. **My recommendation: `[^id]:` only.**
   (If you *want* link reference definitions in qmd, that is a much larger,
   separate feature and should not ride along here.)

4. **Batch bd-mt1ksg9b (the `*` peeks' missing indent guard) into the same
   pass, or keep one-bug-at-a-time?** Both touch the same two gates and would
   otherwise conflict. The project rule says one at a time, and bd-mt1ksg9b was
   explicitly deferred once already for that reason — but it also notes `*` has
   emphasis wrinkles that need their own sweep. **My recommendation: keep them
   separate**, and do this one first since it has no emphasis ambiguity.

5. **File Findings 6 and 7 as their own strands now?** (a) unresolved note
   reference renders as an invisible empty `<span>` with no diagnostic —
   arguably the more dangerous half of "silently lost", since it survives this
   fix; (b) missing `<h2>Footnotes</h2>` appendix heading. Both are independent
   of the scanner. I have not filed them yet.

## Risks / tradeoffs (draft)

- **The gates are the highest-risk lines in the parser.** 92737cdd passed
  572/572 corpus tests and still shipped a P0 that dropped whole files from
  renders, because every new case sat at indent 0. The indent × context sweep
  in Phase 0 is not optional. A hard parse error here drops the *file*, not the
  block.
- **Gate 2 reuses gate 1's verdict** (`first_peeked` short-circuits), so a
  gate-1 misjudgment propagates. `s->indentation` means different things at the
  two sites (raw vs post-prefix) — the guards must differ accordingly, exactly
  as 92737cdd's do.
- **`mark_end` placement.** Adding a fourth peeking branch means the
  `second_peeked` bookkeeping needs the `'['` case too, or the peeked run gets
  swallowed into the SOFT_LINE_ENDING token range.
- **Verify must include the WASM leg.** Grammar/scanner changes feed
  `wasm-qmd-parser`; `--skip-hub-build` is not sufficient for the final gate.
- **Case B is a behavior change beyond the reported symptom** (if Q1 goes the
  recommended way). Per the pattern set by 92737cdd's comment, batch and call
  it out explicitly in the commit message rather than letting users discover it.
- **Low risk of test churn**, per Finding 8 — but snapshots were also predicted
  to churn for 92737cdd and did not, so do not skip the check.

## Investigation artifacts

`claude-notes/plans/adjacent-footnote-definitions-investigation/`

- `repro.qmd` — minimal failing pair + blank-line-separated control
- `repro.html` — the rendered output inspected above (with `repro_files/`)
- `sweep.sh` / `sweep-before.txt` — q2 indent × context sweep at HEAD
- `pandoc-sweep.sh` / `pandoc-sweep.txt` — the same sweep against pandoc 3.9.0.2
