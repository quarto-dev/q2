# QMD writer: Figure node emits empty div + duplicate caption

**Beads:** bd-f5qd
**Source:** [issue #150](https://github.com/quarto-dev/q2/issues/150), item 2

## Bug

The qmd writer wraps every `Figure` in a `::: {}` div, then emits the
caption text as a separate paragraph after the image. The result re-parses
to a `Div` containing a `Figure` and a `Para`, not a `Figure`.

Reporter's repro:

```
$ printf '![Webpage](image.png){.lightbox}\n' | cargo run --bin pampa -- -t qmd
::: {}

![Webpage](image.png){.lightbox}

Webpage

:::
```

`qmd → ast → qmd → ast` is non-idempotent.

## Round-trip trace (the user's example)

**Step 1 — original qmd → AST.** The reader has an "implicit figure" rule
in `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:884-941`:
when a paragraph contains exactly one Image with non-empty alt text, the
paragraph is desugared into a `Figure`:

```
[ Figure ( "" , [] , [] )                                 -- empty attr
         (Caption Nothing [ Plain [Str "Webpage"] ])      -- caption = alt
         [Plain [Image ( "" , ["lightbox"] , [] )         -- content = image
                       [Str "Webpage"]                    -- image alt
                       ("image.png" , "")]] ]
```

Specifically the rule:
- splits the original image's attr: any `id` goes onto the `Figure`,
  `classes` + `kvs` stay on the `Image`,
- copies the image's alt-text inlines into the caption as
  `Caption { short: None, long: Some([Plain[alt]]) }`,
- wraps the (re-attributed) image in a `Plain` and makes that the
  `Figure`'s sole content block.

So in the *normal* qmd-authored case, every `Figure` has a very specific
shape — the "implicit figure" shape — and the caption text and image alt
text are necessarily identical.

**Step 2 — AST → qmd via `write_figure`** at
`crates/pampa/src/writers/qmd.rs:721-761`:

```rust
fn write_figure(figure, buf, ctx) {
    write!(buf, "::: ")?;
    write_attr(&figure.attr, buf, ctx)?;   // empty attr → "{}"
    writeln!(buf)?;
    for block in &figure.content {          // emits Plain[Image(...)]
        writeln!(buf)?;
        write_block(block, buf, ctx)?;
    }
    if let Some(ref long_caption) = figure.caption.long {
        // emits the caption as a plain paragraph after the image
        for (i, block) in long_caption.iter().enumerate() { ... }
    }
    writeln!(buf, "\n:::")?;
}
```

Two problems with this output:
1. `::: {}` is a bare fenced div with no figure-specific marker. Even if
   the parser had a "div with caption → Figure" rule, it would have no
   way to recognize this div as a figure.
2. The caption is emitted as a sibling block of the image. The reader's
   div handler sees it as just another block in the div's body — it has
   no notion of "this paragraph is the caption of the preceding image."

**Step 3 — qmd → AST again.** The reader has no rule for converting a
fenced div into a `Figure`. (The only Figure-producing rule in the entire
reader pipeline is the implicit-figure one in step 1.) So:

```
[ Div ( "" , [] , [] ) [
    Figure ( "" , [] , [] )
           (Caption Nothing [ Plain [Str "Webpage"] ])
           [Plain [Image ( "" , ["lightbox"] , [] )
                         [Str "Webpage"]
                         ("image.png" , "")]],
    Para [Str "Webpage"]                  -- the caption, now a sibling
] ]
```

The `Figure` *inside* the div is recreated by the implicit-figure rule
firing on the image-only paragraph. The caption text becomes a free-
standing `Para` that's now structurally unrelated to the figure. The
outer `::: {}` becomes a `Div` wrapper that wasn't in the original.

## Why this is broken

**The reader and writer don't share a protocol for explicit figures.**
The reader only knows how to make a `Figure` from a single-image
paragraph (the implicit form). The writer always produces an explicit
form (a div wrapper). There is no syntax that lets a `Figure` make a
round trip if it isn't already in implicit form.

Two consequences:

1. *Even for the implicit-figure shape*, the writer produces a wrapper
   that needlessly bloats the output and then fails to round-trip. The
   user's example demonstrates exactly this: a Figure that *was*
   produced by the implicit rule, rewritten as a div, no longer
   re-parses as a Figure.

2. For Figure shapes that don't match the implicit form (id only on
   figure, caption ≠ alt, multiple content blocks, `caption.short`
   set, etc.), there is currently no qmd syntax at all that round-trips
   to a `Figure`. JSON input describing such a Figure is unreachable
   from the writer.

## Proposed fix — discussion needed

There are several plausible directions. I want to align on which before
implementing.

### Option A — bare image syntax for implicit-shape Figures, fenced div fallback (with parser support added)

For a Figure matching the "implicit" shape — single `Plain[Image]`
content, caption equal to image alt text, attribute layout consistent
with the reader's split — emit the bare image syntax:
```
![Webpage](image.png){.lightbox}
```
Parser already round-trips this through the implicit rule.

For everything else, emit a fenced div with a recognizable marker
(e.g. a `figure` class or `fig-` id) and add a reader rule that
converts such a div back into a `Figure`. We'd also need a way to
distinguish the caption block within the div — Quarto's TS implementation
uses a leading or trailing paragraph, but we'd need to commit to a
convention and document it.

Pros: round-trips everything.
Cons: requires reader changes too — biggest scope.

### Option B — bare image syntax always, with a warning when info is dropped

Always emit `![alt](src){...}` for any Figure containing a single Image.
Drop attributes that don't fit and warn. For Figures that *don't* contain
a single Image (rare; usually only via JSON input), fall back to current
broken div syntax with a warning, OR refuse to write and emit an error.

Pros: small change; round-trips the common case perfectly.
Cons: some JSON-input Figures lose information; we silently can't
represent "fancy" figures.

### Option C — bare image syntax for implicit shape, status quo fallback

Detect the implicit-figure shape and emit bare image syntax for it. For
non-implicit Figures, keep the current `::: {}` + sibling-caption
output (still broken on round-trip) but add a `# TODO` and beads issue
for the explicit-figure protocol design.

Pros: smallest change; fixes the user's reported bug; defers the harder
question.
Cons: explicit Figure round-trip remains broken.

### My recommendation

**Option C as the immediate fix** — the user's bug is squarely the
implicit-shape case, and that's the only case the existing reader can
round-trip anyway. The "what should explicit figures look like in qmd?"
question is an architectural design that deserves its own beads issue
and a separate plan, with consideration of crossref/figure-numbering
semantics. Treating that as a follow-up keeps this PR scoped.

### Decisions (user, 2026-04-30)

1. **Option C** (implicit-shape detection + status-quo fallback).
2. **Strict detection** — all four conditions must hold.
3. Keep the non-implicit fallback as-is (current broken `::: {}`
   output). After the implicit fix lands, construct a minimal example
   of an explicit-shape Figure that fails round-trip and document the
   limitation in the follow-up beads issue.

## Implementation sketch (assuming Option C, strict detection)

In `crates/pampa/src/writers/qmd.rs:721 write_figure`:

```rust
fn write_figure(figure, buf, ctx) -> std::io::Result<()> {
    if let Some(image) = match_implicit_figure_shape(figure) {
        return write_image(image, buf, ctx);
    }
    // ... existing div-wrapping code (with caveat that round-trip is
    // currently broken for this branch — tracked in bd-XXXX)
}

fn match_implicit_figure_shape(figure: &Figure) -> Option<&Image> {
    // 1. content is exactly [Plain[Image]]
    let [Block::Plain(plain)] = figure.content.as_slice() else { return None; };
    let [Inline::Image(image)] = plain.content.as_slice() else { return None; };
    // 2. caption.short is None
    if figure.caption.short.is_some() { return None; }
    // 3. caption.long is Some([Plain[alt-inlines]]) where alt-inlines == image.content
    let Some(long) = &figure.caption.long else { return None; };
    let [Block::Plain(caption_plain)] = long.as_slice() else { return None; };
    if caption_plain.content != image.content { return None; }
    // 4. attr split: figure.attr has id-only or empty, image.attr has classes+kvs but no id
    let (fig_id, fig_classes, fig_kvs) = &figure.attr;
    let (img_id, _, _) = &image.attr;
    if !fig_classes.is_empty() || !fig_kvs.is_empty() { return None; }
    if !img_id.is_empty() { return None; }
    Some(image)  // need to merge fig_id into image's attr for the write
}
```

Then write the merged image — emit `figure.attr.0` as the id in the
image's attribute block, image's classes and kvs unchanged.

## Test plan (TDD — write tests first, watch them fail, then fix)

Add round-trip fixtures under
`crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`:

- [ ] `figure_implicit_simple.qmd` — reporter's exact case:
      `![Webpage](image.png){.lightbox}`
- [ ] `figure_implicit_with_id.qmd` — id on the figure:
      `![Caption](src.png){#fig-id}`
- [ ] `figure_implicit_id_and_classes.qmd` —
      `![Caption](src.png){#fig-id .lightbox .extra-class}`
- [ ] `figure_implicit_with_title.qmd` — image with title:
      `![Caption](src.png "Title text"){.lightbox}`

Run `cargo nextest run -p pampa test_qmd_roundtrip_consistency` and
confirm every fixture fails before any code change. Then implement and
re-run.

## Non-goals (this plan)

- Explicit-figure protocol (caption ≠ alt, multiple content blocks,
  caption.short set). Tracked separately.
- Cross-reference handling (`fig-id` collation with `@fig-id`).
- Subfigures.

## Implementation steps (post-decision)

- [x] Confirm Option choice with user — Option C, strict detection,
      keep status-quo fallback.
- [x] Add failing test fixtures (4: simple, with id, id+classes, with
      title).
- [x] Run roundtrip suite, confirm fixtures fail.
- [x] Implement `match_implicit_figure_shape` + early return in
      `write_figure`.
- [x] Re-run; fixtures pass.
- [x] Run `cargo nextest run --workspace` — 7610 passed; no snapshot
      drift.
- [ ] Run `cargo xtask verify --skip-hub-build`.
- [x] End-to-end: reporter's case writes
      `![Webpage](image.png){.lightbox}` and re-parses to identical AST.
      Two-cycle round-trip (qmd→qmd→ast) also matches.
- [x] Open follow-up beads issue **bd-emr4** with concrete
      caption-≠-alt repro for non-implicit Figure shapes; reference it
      from the writer's fallback comment.
- [ ] Close bd-f5qd; sync beads; commit.

## Explicit-shape failure case (documented in bd-emr4)

Trigger: any Figure where caption text ≠ image alt text (also: multiple
content blocks, caption.short set, figure-level classes/kvs). Minimal
repro using JSON input with `caption=A different caption.` and
`image alt=Image alt`:

```
$ cat /tmp/explicit_fig.json | cargo run --bin pampa -- -f json -t qmd
::: {}

![Image alt](image.png){.lightbox}

A different caption.

:::

$ ... | cargo run --bin pampa -- -t native
[ Div ( "" , [] , [] ) [
    Figure ( "" , [] , [] ) (Caption Nothing [ Plain [Str "Image",Space,Str "alt"] ]) [...],
    Para [Str "A",Space,Str "different",Space,Str "caption."]
] ]
```

The original caption is destroyed (the inner implicit-figure rule fires
on the image-only paragraph and replaces it with the alt text); the
original caption text becomes a free-standing sibling `Para`; the
outer `::: {}` becomes a `Div`. Tracked in bd-emr4.
