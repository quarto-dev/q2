# Plan: writer emits `\'` for apostrophes the reader would re-reject (issue #201, bd-8lcm)

## Context

- **GitHub**: https://github.com/quarto-dev/q2/issues/201
- **Beads**: bd-8lcm
- **Triage**: `claude-notes/issue-reports/201/triage.md`
- **Worktree**: `.worktrees/issue-201` (branch `issue-201`, based on `main` @ `26b8943c`)

The qmd writer at `crates/pampa/src/writers/qmd.rs` emits a bare `'` for an apostrophe whose source form required a `\'` escape. Round-trip `qmd → AST → qmd → AST` fails on `reveal.js\' jump-to-slide.` and similar real-world inputs.

## The reader's rule (probed during triage, sharpened under this plan)

The reader's smart-quote-apostrophe classification accepts a bare `'` *only* when **both** neighbors are Unicode alphanumeric. Anything else — punctuation, whitespace, end-of-Str, or end-of-block — is rejected.

| left context | right context | reader behavior |
|---|---|---|
| alphanumeric | alphanumeric (`don't`, `ab'9`) | accepted as apostrophe `’` |
| alphanumeric | non-alphanumeric in Str (`ab'.cd`) | **Q-2-7** |
| alphanumeric | whitespace + content (`ab' c`) | **Q-2-10** |
| alphanumeric | newline + content (paragraph continues) | **Q-2-7** |
| alphanumeric | LineBreak (`ab'\\\n…`) | **Q-2-7** |
| alphanumeric | start of any other inline (Code, Quoted, …) | **Q-2-7** |
| alphanumeric | end-of-Str / end-of-block / end-of-input (`ab'\n` alone) | **Q-2-7** |
| non-alphanumeric | anything | not classified as quote-open in the first place |

Original triage observation about `reveal.js'\n` being accepted was an artifact of probing the *already-escaped* form `reveal.js\'`. The *unescaped* form errors uniformly.

Probes that confirmed the sharper rule:

```bash
printf "ab'c\n"  | cargo run --bin pampa --   # OK
printf "ab'9\n"  | cargo run --bin pampa --   # OK
printf "ab'\n"   | cargo run --bin pampa --   # ERR (no following content needed)
printf "ab' \n"  | cargo run --bin pampa --   # ERR
printf "ab'.\n"  | cargo run --bin pampa --   # ERR
printf "9' a\n"  | cargo run --bin pampa --   # ERR
```

So the writer's escape rule, derived from the reader's rule, simplifies to:

> Escape `'` as `\'` whenever **the previous char in the Str body is Unicode alphanumeric** AND **the next char in the Str body is either absent or non-alphanumeric**.

No cross-inline lookahead is needed: the rule is purely local to a single `Str` body. (`reverse_smart_quotes` runs first, so the apostrophe is ASCII `'` by the time we make this decision.) This is materially simpler than the cross-inline design originally sketched in the triage.

## Design decisions

1. **Escape logic is local to `escape_markdown`.** No `QmdWriterContext` field, no cross-inline lookahead, no `write_inlines` helper. The rule only needs the previous and next characters within a single `Str` body. This is enough because the reader rejects `letter'` at any non-alnum boundary — including end-of-Str — and the rule is independent of what follows in subsequent inlines.

2. **`escape_markdown` iterates with `peekable() + prev_char` and emits `\'` per the rule above.** Existing escapes (for `\*`, `\[`, `\#`, etc.) are unchanged.

3. **No reader changes.** This is purely a writer fix.

4. **No over-escaping.** The rule produces `\'` exactly where the reader needs it (`don't` and `ab'9` stay un-escaped; `reveal.js'` and `ab'.` get the backslash). The pre-existing apostrophe-roundtrip fixture `smart_quotes_apostrophes.qmd` is the natural regression check.

## Phase 1: TDD — failing test

- [x] Add roundtrip fixture: `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/apostrophe_before_space.qmd` containing exactly the issue's input (`reveal.js\' jump-to-slide.`).
- [x] Run the qmd-json-qmd roundtrip test, verify it fails at HEAD with the expected error (re-parse of writer output triggers Q-2-10).
- [x] Add a second small fixture covering an *additional* trigger shape — `apostrophe_before_punct.qmd` (`ab\'.cd`) — to catch the within-Str case.

## Phase 2: Implementation

- [x] Refactor `escape_markdown` to iterate with `peekable()` and track `prev_char`. Add a `'\''` arm: escape iff `prev_char.is_alphanumeric() && !next.is_alphanumeric_or_absent()`. (No signature change.)
- [x] `write_str` stays unchanged — it already calls `escape_markdown`.
- [x] No `QmdWriterContext` field needed. No `write_inlines` helper needed. (Earlier draft of this plan over-engineered; sharper reader probes simplified it.)

## Phase 3: Verify

- [x] The two new roundtrip fixtures pass.
- [x] `cargo nextest run -p pampa` — verify no regressions in pampa.
- [x] `cargo nextest run --workspace` — verify no regressions across the monorepo.
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests` — verify the Rust-only verification (lint, fmt, build with `-D warnings`, full test run) is clean.
- [x] End-to-end: re-run the three commands from the triage doc (`pampa`, `pampa -t qmd`, the round-trip pipeline) and confirm the round-trip now succeeds. Record the observed output below.
- [x] End-to-end: round-trip the two quarto-web fixtures cited in the issue (cited via raw URLs in the triage doc) through `pampa -t qmd | pampa` and confirm no errors.

## Phase 4: Commit + handoff

- [x] Commit on `issue-201` branch with a message linking bd-8lcm and issue #201.
- [x] Update bd-8lcm with progress notes and (eventually) close it.
- [x] Sync beads JSONL on `main`.
- [x] Ask for permission before pushing.

## End-to-end record

### After-fix round-trip (issue's reported case)

```
$ printf -- "reveal.js\\\\' jump-to-slide.\n" | cargo run --bin pampa -- -t qmd
reveal.js\' jump-to-slide.

$ printf -- "reveal.js\\\\' jump-to-slide.\n" | cargo run --bin pampa -- -t qmd 2>/dev/null | cargo run --bin pampa --
[ Para [Str "reveal.js’", Space, Str "jump-to-slide."] ]
```

Round-trip is now a fixed point. Output inspected by hand: the trailing apostrophe in the first Str is correctly emitted as `\'`.

### Broader pattern coverage

Test input (`/tmp/q2-test-cases.qmd`):

```
reveal.js\' jump-to-slide.

ab\'.cd

*hi\'*

"hi\'"

He said don't worry.

it's working

We have ab'9 here.
```

Both `pampa -t qmd` (writer output) and the re-parse of that output produce identical ASTs to the original parse. Each shape covered:

- `reveal.js\'` followed by `Space` + `Str` → `\'` emitted.
- `ab\'.cd` (apostrophe before punctuation in same Str) → `\'` emitted.
- `*hi\'*` (apostrophe at end of Emph content, before closing `*`) → `\'` emitted.
- `"hi\'"` (apostrophe at end of Quoted content, before closing `"`) → `\'` emitted.
- `don't`, `it's` (letter-apostrophe-letter contractions) → bare `'` preserved.
- `ab'9` (letter-apostrophe-digit) → bare `'` preserved.

Inspection of the writer output:

```
$ cargo run --bin pampa -- /tmp/q2-test-cases.qmd -t qmd 2>/dev/null
reveal.js\' jump-to-slide.

ab\'.cd

*hi\'*

"hi\'"

He said don't worry.

it's working

We have ab'9 here.
```

The escape lands exactly where the reader needs it; nowhere else.

### Verification commands run

```bash
cargo nextest run -p pampa test_qmd_roundtrip_consistency  # 1 passed (the suite includes the two new fixtures)
cargo nextest run -p pampa --no-fail-fast                  # 3686 passed
cargo nextest run --workspace --no-fail-fast               # 8863 passed
cargo xtask verify --skip-hub-build --skip-hub-tests       # all 9 steps green
```

### quarto-web fixtures (skipped)

Attempted to fetch the two quarto-web sources cited in the issue (lines from `presenting.qmd` and `tables.qmd` at SHA `41a5a98d…`) but the SHA was not reachable from this machine. Skipped this external check; the shape of those occurrences (`reveal.js' jump-to-slide`) is the literal reproducer above, which is verified.
