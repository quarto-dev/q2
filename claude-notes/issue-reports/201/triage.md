# Issue #201 — Writer drops `\'` escape on apostrophes that the reader would re-reject as quote opens

- **GitHub**: https://github.com/quarto-dev/q2/issues/201
- **Reporter**: @rundel (Colin Rundel), 2026-05-15
- **Triage date**: 2026-05-15
- **Worktree**: `.worktrees/issue-201` (branch `issue-201`, based on `main` @ `26b8943c`)
- **Beads issue**: bd-8lcm (filed during triage)
- **Scope**: The reader/writer asymmetry around the ASCII apostrophe `'` in the qmd writer (`crates/pampa/src/writers/qmd.rs`). No reader changes are in scope — the reader's behavior is the spec the writer must satisfy.

## Summary

Real bug, reproduces at HEAD. The qmd writer emits a bare `'` for an apostrophe whose source form required a `\'` escape, so `qmd → AST → qmd` is not a fixed point: the second round-trip through the reader fails with `Q-2-10` (Closed Quote Without Matching Open Quote). Fix is small in scope (one writer function plus context-aware lookahead across inline boundaries) but not literally one-line — see § Localization.

## Reproduction

Fixture: `claude-notes/issue-reports/201/repro.qmd`

```
reveal.js\' jump-to-slide.
```

Three commands, exactly as reported in the issue:

```bash
# 1. Reader accepts \' and produces an apostrophe in the AST.
$ printf -- "reveal.js\\\\' jump-to-slide.\n" | cargo run --bin pampa --
[ Para [Str "reveal.js’", Space, Str "jump-to-slide."] ]

# 2. Writer round-trips the AST as qmd, but DROPS the escape:
$ printf -- "reveal.js\\\\' jump-to-slide.\n" | cargo run --bin pampa -- -t qmd
reveal.js' jump-to-slide.

# 3. Feeding that output back into the reader fails with Q-2-10:
$ printf -- "reveal.js\\\\' jump-to-slide.\n" \
    | cargo run --bin pampa -- -t qmd 2>/dev/null \
    | cargo run --bin pampa --
Error: [Q-2-10] Closed Quote Without Matching Open Quote
   ╭─[ <stdin>:1:11 ]
   │
 1 │ reveal.js' jump-to-slide.
   │          ┬┬
   │          ╰─── This is the opening quote. ...
   │           │
   │           ╰── A space is causing a quote mark to be interpreted as a quotation close.
───╯
```

Inspected output: confirmed by hand; the bare `'` between `js` and the trailing space is exactly the position Q-2-10 fires on.

### Trigger boundary (probed during triage)

The Q-2-10 trigger is precisely **letter-on-left AND whitespace+content-on-right**, matching the issue body's description and the `Q-2-10.json` corpus case (`a' b.`). Two adjacent probes:

```bash
# Apostrophe at end of paragraph (no whitespace after): reader is happy.
$ printf "reveal.js\\\\'\n" | cargo run --bin pampa --
[ Para [Str "reveal.js’"] ]

# Apostrophe followed by " end." — Q-2-10 fires.
$ printf "reveal.js' end.\n" | cargo run --bin pampa --
Error: [Q-2-10] ...
```

So the writer must consider both sides of the apostrophe, including across inline boundaries (the `Space` after `Str "reveal.js'"` is a separate inline).

### Real-world incidence

The reporter cites two quarto-web sources that contain this pattern (`reveal.js' jump-to-slide`, and another in `tables.qmd`). This means the writer regression is observable on real documents the project already publishes — not a synthetic edge case.

## Localization

- **Writer escape table**: `crates/pampa/src/writers/qmd.rs:1379` (`fn escape_markdown`). Currently has no `'` arm — the comment at line 1405 explicitly notes "characters that don't need escaping in most contexts: . , - + ! ? = : ; / ( ) % & ' \""`. That comment is wrong about `'` in the letter-then-whitespace context.
- **Str writer entry point**: `crates/pampa/src/writers/qmd.rs:1414` (`fn write_str`). Currently has signature `(s: &Str, buf, ctx)` and calls `escape_markdown(reverse_smart_quotes(&s.text))`. A correct fix needs at least lookahead to the next inline (Space vs other) and lookbehind to the previous inline's trailing character. The `ctx: &mut QmdWriterContext` is the natural place to thread that state — or `write_inline` (line 2159) could pre-compute boundary flags and pass them down.
- **Reverse smart-quote helper**: `crates/pampa/src/writers/qmd.rs:1371` (`fn reverse_smart_quotes`). Already turns U+2019 (`’`) into ASCII `'`. The fix interacts with this: post-conversion, any ASCII `'` produced by this helper is a candidate for escaping based on context.
- **Reader-side trigger (reference only, not modified)**: `Q-2-10` error corpus at `crates/pampa/resources/error-corpus/Q-2-10.json` defines the canonical trigger `a' b.`. Existing roundtrip test `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/smart_quotes_apostrophes.qmd` only exercises the *safe* contexts (`project's`, `can't`, `it's`, `We're`) and so doesn't catch this regression. A new roundtrip fixture in the same directory (e.g. `apostrophe_before_space.qmd` containing `reveal.js\' jump-to-slide.`) is the obvious TDD entry point.

## Open questions — resolved during triage

**Q1. Is the trigger letter-on-left-only, or letter-on-left AND whitespace-on-right?**
Experiment: ran `printf "reveal.js\\'\n"` (no trailing whitespace) and confirmed the reader accepts it.
Conclusion: the trigger requires *both* sides. A writer fix that escaped every letter-then-apostrophe occurrence would over-escape `project's`, `can't`, etc. The fix must look at the next inline (or end-of-block) too.

**Q2. Does the Str body itself ever contain the boundary?**
Experiment: inspected the AST from the failing input. The boundary lives across inlines: `[Str "reveal.js'", Space, Str "jump-to-slide."]`. The `'` is at the end of one Str; the Space is the next inline.
Conclusion: a fix that only looks within a single Str body is insufficient. The writer must know what inline follows the current Str (Space, SoftBreak, end-of-block, etc.).

**Q3. Is the writer's current "don't escape `'`" comment defensible at all?**
Experiment: read the comment at `qmd.rs:1405–1407`.
Conclusion: the comment is correct that escaping `'` everywhere would be verbose (every contraction). It is wrong that "very specific contexts" can be ignored — the letter+whitespace case is exactly such a context and it is what this bug is about. The right fix is context-aware escaping, not blanket escaping.

## Outcome / recommended next step

Filed beads issue with the fix scope below. No GH comment needed yet — issue is already specific and well-reproduced; the bd-XXXX will be referenced when the fix PR lands.

**Fix scope (for the beads issue):**

1. **Test first (TDD per `crates/pampa/CLAUDE.md`)**:
   - Add `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/apostrophe_before_space.qmd` containing `reveal.js\' jump-to-slide.` (or similar). Confirm the round-trip test fails at HEAD.
2. **Implement**: Extend `write_str` (or the surrounding `write_inline` driver) to detect a trailing `letter + '` at the end of a `Str` whose **next** inline is `Space`/`SoftBreak`/etc., and emit `\'` instead of `'`. The minimal correct rule is: escape ASCII `'` in a `Str` body iff the char immediately to its left is a Unicode letter AND the next byte the writer would emit is ASCII whitespace.
3. **Verify**: Round-trip test passes; `cargo nextest run --workspace` clean; the two quarto-web fixtures from the issue round-trip cleanly through `pampa -t qmd | pampa`.

Out of scope: any reader changes; over-escaping `'` in the safe contexts (`project's`, `can't`).

## Verification commands used

```bash
# Pre-flight (Rust-only; hub-client & trace-viewer skipped due to missing node_modules — unrelated bootstrap)
cargo xtask verify --skip-hub-build --skip-hub-tests

# Reproduction
printf -- "reveal.js\\\\' jump-to-slide.\n" | cargo run --bin pampa --
printf -- "reveal.js\\\\' jump-to-slide.\n" | cargo run --bin pampa -- -t qmd
printf -- "reveal.js\\\\' jump-to-slide.\n" | cargo run --bin pampa -- -t qmd 2>/dev/null | cargo run --bin pampa --

# Boundary probes
printf "reveal.js\\\\'\n"     | cargo run --bin pampa --
printf "reveal.js' end.\n"  | cargo run --bin pampa --

# Issue intake
gh issue view 201 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
```

## Cross-references

- Reader-side spec: `crates/pampa/resources/error-corpus/Q-2-10.json`
- Existing apostrophe roundtrip coverage (does NOT cover this case): `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/smart_quotes_apostrophes.qmd`
- Real-world incidence (quarto-web):
  - https://github.com/quarto-dev/quarto-web/blob/41a5a98d20449d970821b59be1711a0710a9cee3/docs/presentations/revealjs/presenting.qmd#L85
  - https://github.com/quarto-dev/quarto-web/blob/41a5a98d20449d970821b59be1711a0710a9cee3/docs/authoring/tables.qmd#L772
- Writer file: `crates/pampa/src/writers/qmd.rs` (escape table + `write_str` + `write_inline` driver)
