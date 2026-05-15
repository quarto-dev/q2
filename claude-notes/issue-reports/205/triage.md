# Issue #205 — Writer drops `\'` escape when `'` starts a Str whose preceding inline ends in non-alnum

- **GitHub**: https://github.com/quarto-dev/q2/issues/205
- **Reporter**: @rundel (Colin Rundel), 2026-05-15
- **Triage date**: 2026-05-15
- **Worktree**: `.worktrees/issue-205` (branch `issue-205`, based on `main` @ `09b2de7e`)
- **Beads issue**: bd-nsb9 (filed; see Outcome)
- **Scope**: covers the writer-only escape gap described in the issue
  body. The reader's smart-quote classification is **not** in scope —
  its stricter behavior (compared to what the issue's first command
  shows) is intentional and is what motivated the writer fix in #201.

## Summary

Follow-up to #201 / bd-8lcm. The writer's `escape_markdown` decides
whether to emit `\'` from purely *intra-`Str`* context (`prev_char` and
the peeked `next_char` inside the current `Str` body). When `'` sits at
**index 0** of a `Str`, `prev_char` is `None`, so the local rule treats
it as "not after alphanumeric" and emits a bare `'`. The reader's
smart-quote-apostrophe classifier, however, is keyed off the
*surrounding byte stream* — not Str boundaries — so it sees the closing
backtick of a preceding `Code` (or `*` of an `Emph`, `)` of an
`Image`, etc.) on the left and `s` on the right, classifies the `'`
as an unclosed opening single quote, and emits **Q-2-7**. Round-trip
fails. Confirmed locally with a JSON-AST repro and with a parseable
qmd repro (`` `x`\'s end ``) that exercises the same path through the
existing `qmd-json-qmd` roundtrip harness.

## Reproduction

Two equivalent repros are committed alongside this doc:

- `repro.qmd` — a parseable qmd whose AST contains the offending
  inline shape (`Code` followed by `Str "'s"`). Suitable for the
  existing `tests/roundtrip_tests/qmd-json-qmd/` harness.
- `repro.json` — the AST directly, for bypassing the parser when
  demonstrating the writer in isolation.

### Parser bypass — writer in isolation

```
$ cat claude-notes/issue-reports/205/repro.json | ./target/debug/pampa -f json -t qmd
`x`'s end
$ cat claude-notes/issue-reports/205/repro.json | ./target/debug/pampa -f json -t qmd | ./target/debug/pampa
Error: [Q-2-7] Unclosed Single Quote
   ╭─[ <stdin>:1:10 ]
   │
 1 │ `x`'s end
   │    ┬     ┬
   │    ╰────── This is the opening quote. If you need an apostrophe, escape it with a backslash.
   │          │
   │          ╰── I reached the end of the block before finding a closing "'" for the quote.
───╯
```

### Through the parser — `qmd-json-qmd` round-trip

```
$ cat claude-notes/issue-reports/205/repro.qmd
`x`\'s end
$ ./target/debug/pampa claude-notes/issue-reports/205/repro.qmd
[ Para [Code ( "" , [] , [] ) "x", Str "’s", Space, Str "end"] ]
$ ./target/debug/pampa claude-notes/issue-reports/205/repro.qmd -t qmd
`x`'s end                                  # ← the escape is dropped
$ ./target/debug/pampa claude-notes/issue-reports/205/repro.qmd -t qmd | ./target/debug/pampa
Error: [Q-2-7] Unclosed Single Quote …     # ← re-parse fails
```

The issue's original first-command output `[Para [Code "x", Str "'s",
…]]` from raw `` `x`'s end `` no longer reproduces — the reader now
rejects that input at parse time (today, `pampa <input>` emits Q-2-7
directly). That stricter reader behavior is **the reason the writer
needs to escape** in this position; it does not change the bug.

### Generalization — not Code-specific

The same defect fires whenever a `Str "'..."` follows *any* inline
whose last emitted byte is non-alphanumeric, and also when the `Str`
is the first inline of a block:

```
=== Str starting at block start ===
$ echo '{…blocks:[{Para:[{Str:"'\''sup"},{Space},{Str:"end"}]}]…}' | pampa -f json -t qmd
'sup end                                   # unescaped — would Q-2-7

=== After Emph ===
$ echo '{…[{Emph:[{Str:"hi"}]},{Str:"'\''s"}]…}' | pampa -f json -t qmd
*hi*'s                                     # unescaped — would Q-2-7

=== After Image ===
$ echo '{…[{Image:[…,"url",…]},{Str:"'\''s"}]…}' | pampa -f json -t qmd
![alt](url)'s                              # unescaped — would Q-2-7
```

In every case the bytes immediately surrounding the `'` are
`<non-alnum> ' <alnum>` — exactly what the reader's classifier
rejects.

## Localization

- `crates/pampa/src/writers/qmd.rs:1388` — `escape_markdown` (the
  function added in #201). Its `prev_char` is the previous char *of
  the current `Str` body*; it has no view of inter-inline state.
- `crates/pampa/src/writers/qmd.rs:1436` — `write_str` calls
  `escape_markdown(&text)` and writes the result. This is the only
  call site, so the fix can either:
    1. accept an additional `prev_byte: Option<u8>` argument here
       and thread it through every inline-list iteration site
       (≈20 call sites of `write_inline` — see grep below), or
    2. track "last emitted byte in the current inline stream" on
       `QmdWriterContext` (single field, written by every leaf
       emitter, reset at block boundaries). Less invasive at call
       sites but requires discipline at every place that writes
       bytes.
- Iteration sites that would need to participate (per
  `grep -n 'write_inline(' crates/pampa/src/writers/qmd.rs`): lines
  268, 579, 688, 726, 835, 861, 1173, 1179, 1336, 1479, 1502, 1579,
  1600, 1621, 1633, and within `write_inline` itself at 2205.

## Open questions — resolved during triage

**Q1.** Does the issue's first-command output (`pampa <input>`
producing `[Code "x", Str "'s", …]` from raw `` `x`'s end ``)
reproduce today?
*Experiment.* Ran exactly that command on `main` @ `09b2de7e`.
*Result.* No — the reader now rejects with Q-2-7 at parse time. The
reader's classifier has tightened (or always was strict and the issue
was written from an older snapshot). This **does not** invalidate the
bug, because:
- the writer's job is round-trip fidelity for any AST it is given,
  including ASTs that originate from other tools (TS Quarto, pandoc,
  filters);
- the wild quarto-web links in the issue body contain the same
  `Code'…` shape, and any AST produced from those that goes through
  this writer round-trips lossily.

**Q2.** Is the bug Code-specific?
*Experiment.* Fed ASTs with `Emph`, `Image`, and "Str at block start"
as the preceding context (see Reproduction § Generalization).
*Result.* All produce unescaped `'`. The bug is "previous emitted byte
is non-alphanumeric", not "previous inline is `Code`".

**Q3.** Is there a parseable qmd that exercises the bug through the
existing `tests/roundtrip_tests/qmd-json-qmd/` harness?
*Experiment.* `` `x`\'s end `` parses to `[Code "x", Str "'s", Space,
Str "end"]` (the `\'` is an explicit escape that becomes a literal
apostrophe in the `Str` body). Confirmed end-to-end:
- parse: produces the offending AST shape;
- qmd writer: emits `` `x`'s end `` (drops the escape);
- re-parse: Q-2-7.
*Result.* Yes — a single fixture `qmd-json-qmd/apostrophe_after_code_inline.qmd`
(or similar) will lock in the fix using the existing harness, parallel
to the two fixtures `apostrophe_before_space.qmd` /
`apostrophe_before_punct.qmd` that #201 added.

**Q4.** Does the same defect fire on the symmetric *trailing* case
(`Str "abc'"` followed by `Code "x"`)?
*Experiment.* Reading `escape_markdown` again: at end of a `Str`, the
in-`Str` peek of `next_char` is `None`, so the rule sees
`prev_is_alnum=true, next_is_alnum=false` → emits `\'`. So a trailing
`'` is already escaped today.
*Result.* The trailing case is already handled by the existing logic.
Only the *leading* case is broken. The fix can be asymmetric or
symmetric (the latter is cleaner and matches the issue body's
suggestion). Recommend symmetric — easier to reason about.

## Outcome / recommended next step

**Filed bd-nsb9 with the fix scope below.** Concrete writer work; not
duplicate, not docs, not WAI.

### Fix scope (for the beads issue)

- **Approach.** Option 2 (track last emitted byte on
  `QmdWriterContext`) is preferred. Rationale: it keeps the call-site
  signatures untouched and naturally generalizes if other escape
  rules later need inter-inline context.
- **Reset points.** Reset the tracked last-byte to `None` at the
  start of each block, and at every newline emission inside a block
  (line breaks reset the reader's surrounding-byte context too).
- **Escape rule (symmetric).** Inside `escape_markdown`, the
  `prev_char` source becomes "the in-`Str` previous char if any, else
  the context's last-emitted-byte (treated as a `char` for the
  alphanumeric check)". The rest of the logic is unchanged.
- **TDD.** Add `tests/roundtrip_tests/qmd-json-qmd/apostrophe_after_code_inline.qmd`
  (content: `` `x`\'s end ``) **first**, watch
  `test_qmd_roundtrip_consistency` fail, then implement.
- **Coverage to add alongside.** Two more fixtures to cover the
  generalization: one with `*hi*\'s end` (preceding Emph) and one
  with `\'sup end` (Str at block start, no preceding inline). Both
  parse today and both fail the round-trip with the current writer.

## Verification commands used

```bash
gh issue view 205 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments

cargo xtask verify --skip-hub-build --skip-hub-tests
cargo build --bin pampa

# parser-bypass repro
./target/debug/pampa -f json -t qmd < claude-notes/issue-reports/205/repro.json
./target/debug/pampa -f json -t qmd < claude-notes/issue-reports/205/repro.json | ./target/debug/pampa

# parseable repro through the qmd-json-qmd harness shape
./target/debug/pampa claude-notes/issue-reports/205/repro.qmd
./target/debug/pampa claude-notes/issue-reports/205/repro.qmd -t qmd
./target/debug/pampa claude-notes/issue-reports/205/repro.qmd -t qmd | ./target/debug/pampa

# generalization probes — see Reproduction § Generalization
```

## Cross-references

- #201 / bd-8lcm — original apostrophe-escape fix; introduced
  `escape_markdown`'s intra-`Str` `prev_char/next_char` logic at
  `crates/pampa/src/writers/qmd.rs:1388`.
- `claude-notes/issue-reports/201/triage.md` — sibling triage.
- `claude-notes/plans/2026-05-15-issue-201-apostrophe-escape.md` —
  sibling plan; this issue's fix should follow the same TDD shape
  (fixture first, then writer change).
- `crates/pampa/CLAUDE.md` — the mandatory test-first checklist that
  governs the fix.
