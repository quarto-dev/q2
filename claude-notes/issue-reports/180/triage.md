# Issue #180 — qmd writer drops trailing newline after implicit-figure shape, collapsing the next block

- **GitHub**: https://github.com/quarto-dev/q2/issues/180
- **Reporter**: @rundel (Colin Rundel), 2026-05-11
- **Triage date**: 2026-05-12
- **Worktree**: `.worktrees/issue-180` (branch `issue-180`, based on `main` @ `c5770004`)
- **Beads issue**: bd-cpzp
- **Scope**: Covers both reports in the issue — the original body ("Figure + Para collapse") and the comment ("layout/subfigure div children collapse"). They are the same root cause; this triage treats them as one bug.

## Summary

Both reports reproduce exactly as filed at `main` @ c5770004. They share a single root cause: in `write_figure`, the implicit-figure branch delegates to `write_image` and returns directly, skipping the trailing newline that every block writer is expected to emit. When such a Figure is followed by any other block (top-level *or* as a child of a `Div`), only one `\n` ends up between the two — not a blank line — and the re-parser glues the two blocks into one `Para`. Fix is one line in `write_figure`; the existing roundtrip corpus does not cover "implicit figure followed by another block," which is why this slipped through.

## Reproduction

All commands run from the worktree root, `main` @ c5770004.

### Bug A — top-level Figure + Para collapses (issue body)

Fixture: `claude-notes/issue-reports/180/repro-figure-para.qmd`

```
![cap](img.png){#fig-x}

Follow up text.
```

```
$ cargo run --quiet --bin pampa -- < repro-figure-para.qmd
[ Figure ( "fig-x" , ... ) ..., Para [Str "Follow", Space, Str "up", Space, Str "text."] ]

$ cargo run --quiet --bin pampa -- -t qmd < repro-figure-para.qmd
![cap](img.png){#fig-x}
Follow up text.

$ cargo run --quiet --bin pampa -- -t qmd < repro-figure-para.qmd \
    | cargo run --quiet --bin pampa --
[ Para [Image ( "fig-x" , ... ) [Str "cap"] ("img.png" , ""), SoftBreak, Str "Follow", Space, Str "up", Space, Str "text."] ]
```

Observed: writer emits one `\n` between the image and the paragraph, no blank line. Round-trip collapses two blocks into one `Para`.

Expected: a blank line between the two, so re-parsing yields the original `[Figure, Para]` pair.

### Bug B — layout/subfigure div children collapse (comment)

Fixture: `claude-notes/issue-reports/180/repro-layout-div.qmd`

```
::: {#fig-x layout-ncol=2}

![A](a.png){#fig-a}

![B](b.png){#fig-b}

Caption text
:::
```

```
$ cargo run --quiet --bin pampa -- -t qmd < repro-layout-div.qmd
::: {#fig-x layout-ncol="2"}

![A](a.png){#fig-a}
![B](b.png){#fig-b}
Caption text

:::
```

Observed: the three child blocks (`Figure`, `Figure`, `Para`) come out on consecutive lines with only single `\n`s between them. Re-parsing flattens the Div's content into one `Para`.

Expected: each child separated by a blank line; round-trip preserves the three child blocks.

### Counter-example — Para followed by Figure round-trips fine

Fixture: `claude-notes/issue-reports/180/repro-para-figure-OK.qmd`

```
Lead-in text.

![cap](img.png){#fig-x}
```

```
$ cargo run --quiet --bin pampa -- -t qmd < repro-para-figure-OK.qmd
Lead-in text.

![cap](img.png){#fig-x}
$ # (and round-trip back to native shows [Para, Figure] unchanged)
```

This confirms the bug is *asymmetric*: the missing newline only matters when the implicit-figure shape is **not** the last block in its container. When it is the last block, the loop never emits another separator, so the deficit is invisible.

### Extra coverage — Figure followed by Figure

Fixture: `claude-notes/issue-reports/180/repro-figure-figure.qmd`

```
![A](a.png){#fig-a}

![B](b.png){#fig-b}
```

Writer output collapses to two lines with one `\n` between them; re-parser yields a single `Para` with both images and a `SoftBreak`. Same root cause.

## Localization

`crates/pampa/src/writers/qmd.rs:759`

```rust
fn write_figure(
    figure: &Figure,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    if let Some(image) = match_implicit_figure_shape(figure) {
        let mut merged = image.clone();
        merged.attr.0 = figure.attr.0.clone();
        return write_image(&merged, buf, ctx);   // <-- bug: returns with no trailing '\n'
    }
    ...
}
```

The block-writer contract in this file: each top-level block writer ends its output with exactly one `\n`. See `write_paragraph` (`qmd.rs:2197`), `write_plain` (`qmd.rs:2209`), `write_figure`'s own fallback path that closes with `writeln!(buf, "\n:::")?` (`qmd.rs:805`), etc. The top-level driver `write_impl` (`qmd.rs:2331`) and `write_div` (`qmd.rs:442`) both rely on this: they emit *one* additional `\n` between blocks, which only becomes a blank line if the previous block already ended in `\n`.

`write_image` (`qmd.rs:1490`) is an inline writer and correctly does *not* emit a trailing newline. The bug is the early-return in `write_figure`: it bypasses the block-level wrap-up and reuses the inline writer's output verbatim as a block.

## Fix scope

One-line fix in the implicit-figure branch of `write_figure` — replace the early-return with a call that delegates and then appends `writeln!(buf)?`. Conceptually:

```rust
if let Some(image) = match_implicit_figure_shape(figure) {
    let mut merged = image.clone();
    merged.attr.0 = figure.attr.0.clone();
    write_image(&merged, buf, ctx)?;
    writeln!(buf)?;
    return Ok(());
}
```

Test coverage to add (TDD-first per `crates/pampa/CLAUDE.md`):

1. `tests/roundtrip_tests/qmd-json-qmd/figure_implicit_then_para.qmd` — bug A.
2. `tests/roundtrip_tests/qmd-json-qmd/layout_div_subfigures.qmd` — bug B.
3. Optionally: `figure_implicit_then_figure.qmd` to lock in the second extra case.

The existing roundtrip corpus has only `figure_implicit_with_id.qmd` and `figure_implicit_id_and_classes.qmd`, both single-block documents that never exercise the inter-block separator.

## Open questions — resolved during triage

- **Are both reports the same bug?** Yes. Both reduce to "implicit-figure write path violates the block-trailing-newline contract." Verified by reproducing each separately and observing that the byte-level output matches the prediction in both cases.
- **Is this related to bd-emr4** (existing Figure round-trip beads issue)? No. bd-emr4 is about *non-implicit* Figure shapes (caption ≠ alt, multi-block content, etc.) round-tripping through a fallback fenced div that the reader can't turn back into a Figure. This bug is in the *implicit* path, which today does round-trip *its own block* correctly — it just corrupts the *next* block. Different code paths, different fixes.
- **Does the fallback (non-implicit) path have the same trailing-newline problem?** No. The fallback at `qmd.rs:805` ends with `writeln!(buf, "\n:::")?`, which writes `\n:::\n`. Block contract satisfied.
- **Why does Para → Figure round-trip cleanly when Figure → Para does not?** Because the trailing-newline deficit only matters when there's a *next* block to separate from. When the implicit Figure is the last block in its container, the missing `\n` is harmless.

## Outcome / recommended next step

Real bug, single root cause, small fix. **Filing as a beads issue** with the fix scope above, after the triage commit lands.

## Verification commands used

```bash
gh issue view 180 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build --skip-hub-tests   # pre-flight green
cargo run --quiet --bin pampa -- < repro-figure-para.qmd
cargo run --quiet --bin pampa -- -t qmd < repro-figure-para.qmd
cargo run --quiet --bin pampa -- -t qmd < repro-figure-para.qmd | cargo run --quiet --bin pampa --
# (same three commands against repro-layout-div.qmd, repro-para-figure-OK.qmd, repro-figure-figure.qmd)
br search "Figure"                          # ruled out duplicate
br show bd-emr4 --json                      # confirmed scope differs
```

## Cross-references

- Related (different code path, not a duplicate): bd-emr4 — qmd writer/reader: explicit Figure shapes don't round-trip.
- Writer contract: `crates/pampa/src/writers/qmd.rs` — `write_paragraph` (`:2197`), `write_plain` (`:2209`), `write_div` (`:442`), `write_impl` (`:2325`).
- TDD rule for roundtrip fixes: `crates/pampa/CLAUDE.md` — "When fixing roundtripping bugs: FIRST add the failing test to `tests/roundtrip_tests/qmd-json-qmd`."

## Pre-flight note

`cargo xtask verify --skip-hub-build` fails at the hub-client test step on `main` @ c5770004 with `Cannot find package 'compression'` (vitest config can't resolve a transitive dep). This is unrelated to the qmd writer and appears to be an `npm install` state issue. The Rust portion (`cargo build --workspace`, `cargo nextest run --workspace`) plus trace-viewer build + tests pass cleanly under `cargo xtask verify --skip-hub-build --skip-hub-tests`. Mentioning here so a follow-up agent doesn't get derailed by it.
