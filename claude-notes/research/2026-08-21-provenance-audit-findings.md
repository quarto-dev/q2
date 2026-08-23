# Provenance audit — findings and measurements

**Epic:** `bd-mxa44voa`. **Companion to:**
`claude-notes/plans/2026-08-20-provenance-3-audit-and-fix.md` (Plan 3), which
carries the remaining *work*. This document carries the *findings* — the audit
that plan opened as a survey, now closed.

Measured over 2026-08-20/21 against q2 at `816f4ed47`, `quarto-source-map`
0.1.1 (byte-identical to the pinned 0.1.0), `quarto-yaml` 0.1.2,
`quarto-error-reporting` 0.2.1 on `fix/diagnostic-span-char-boundary`, and
comrak 0.52.0.

**Do not re-derive these.** They cost several sessions. If you disagree with
one, check it against its citation and say so.

---

## 1. The bug class, and the rule that falls out of it

A decoder returns a **decoded** string (quotes stripped, escapes resolved,
block-scalar indentation removed) paired with a `SourceInfo` describing the
**raw** source text. Callers then compute source positions as
`base + content_offset`, which is valid only when decoded content is a
byte-identical, prefix-aligned slice of the raw span.

### Accessor discipline

A nested re-parse whose content provenance is anything other than a single
verbatim run produces `Substring { parent: Concat }` on every node beneath it.
`ProvenanceBuilder::finish()` collapses to a contiguous `SourceInfo` only when
there is **exactly one piece and it is verbatim** — so a plain unescaped scalar
is unaffected, but anything with a fold or an escape stays a `Concat`.

> Do not restate that rule as "collapses when lengths match". Plan 1 tried
> that, and it would have collapsed the fold shape to a single `Original`,
> licensing precisely the byte-copy this epic exists to prevent — undetectably,
> because the length invariant passes.

| accessor | on `Concat` / `Substring{parent: Concat}` | verdict |
|---|---|---|
| `map_offset(k, ctx)` | locates the piece, recurses | **correct** |
| `root_file_id()` | `find_map` over pieces | **correct** |
| `preimage_in(fid)` | hull if source-contiguous, else `None` | **offsets only — not byte identity** |
| `resolve_byte_range()` | `None` unconditionally | honest failure |
| `start_offset()` | `0` | **silently wrong** |
| `end_offset()` | content length | **silently wrong** |

**So: file id → `root_file_id()`; positions → `map_offset`; a hull → the
`map_offset(0)` / `map_offset(length())` pair; never `preimage_in` for a hull,
and never `start_offset`/`end_offset`/`resolve_byte_range` on a
possibly-`Concat` span.**

### The rule, stated flatly

> A range is only composable over a parent that is **byte-identical** to its
> content. No accessor on `SourceInfo` can tell you whether it is. If you are
> deriving a source range from a parent's range plus an offset and the parent
> might be a `Concat`, the answer is `None`.

## 2. Why this keeps being got wrong

**Offsets *do* compose affinely over a piecewise parent, so every arithmetic
check passes.** What does not compose is byte-identity, and no length or
contiguity test can see the difference. That is why two of the instances below
survived review by people holding the counterexample at the time.

### The ancestor — a different mechanism, with the opposite fix

The epic's founding bug — `SourceInfo::substring(parent, a, b)` where `a`/`b`
index a *decoded* string while `parent` describes *raw* text — is **not** a
composition error. `substring` records two offsets and composes them
faithfully: measured on the `'it''s'` fixture (§ 7), `C.map_offset(0)` → source
1 and `C.map_offset(4)` → source 6, **both correct**, on the same value where
`C.preimage_in` returns the wrong `1..5`.

It is a **parent-selection** error, fixed by *supplying the right parent*
(`content_source_info` — this whole epic), **not** by refusing to answer. Keep
the distinction: a reader who internalises "the fix is to return `None`" and
then meets the founding bug will make every config diagnostic span-less.

### The family it spawned

Four instances, reached independently rather than by code reuse, each
*deriving a source range from a parent's resolved range plus a content offset*.
Two shipped; the type system caught none; review caught two.

| # | instance | status | fix |
|---|---|---|---|
| 1 | `preimage_in`'s `Substring` arm (Rust) | shipped | Plan 1, 0.1.2 — **refuse** (`None`) |
| 2 | `resolveChain`'s `Substring` arm (`annotated-qmd`, `source-map.ts:301-315`) | shipped | Plan 2 Phase 4 |
| 3 | a length-preserving predicate proposed as the `preimage_in` fix | caught in review | withdrawn |
| 4 | `ProvenanceBuilder::finish()`'s length-matching collapse rule | caught in review | now "exactly one piece and it is verbatim" |

### The watch-item: it is not the arithmetic

What separates safe from wrong is **whether the parent's `Concat` arm refuses or
offers.** Verified side by side in `source_info.rs`:

| | `Substring` arm | `Concat` arm | outcome |
|---|---|---|---|
| `resolve_byte_range` | `parent_start + start_offset` (`:400-401`) | **`None`** (`:403`) | refuses — safe |
| `preimage_in` | `parent_range.start + start_offset` (`:452-453`) | `Some(hull)` | **lies** |

The arithmetic is *identical*. `resolve_byte_range` is safe **by accident, not
by design** — nothing about it is more careful; its parent simply declines to
supply a range to add to. TS's `resolveChain` behaves like `preimage_in`.

**So a fifth instance appears the moment some accessor's `Concat` arm starts
returning `Some` where it used to return `None`** — a change someone could
plausibly make believing they were improving it, and which would silently arm
every same-shaped `Substring` arm downstream. In particular this is an argument
against ever "improving" `preimage_in` to return a hull for the affine case.

### How to look: read the consumer, not the producer

Three findings in this epic were decided this way, and none of them was visible
at the site that looked defective:

| the site that looked wrong | where the answer actually was |
|---|---|
| `preimage_in`'s `Substring` arm | one level *down*: the arithmetic is byte-identical to `resolve_byte_range`'s; the difference is what the **parent's `Concat` arm** hands back |
| `incremental.rs:171`'s `preimage_in` call | one level *up*: whether the **baseline capture** can ever contain a fold-bearing `Concat` (`pipeline.rs:1013`) — it cannot, so the site is latent |
| the `shortcode_string` closure's range arithmetic | one call *up*: `process_shortcode_string` destructures the range away (`shortcode.rs:36`), so the arithmetic is dead and the site cannot drift by construction |

The shape that misleads is always the same — *a decoded string paired with a raw
range* — and it is visible by grep. Whether it is a bug is not. **Reading the
producer tells you the shape; only reading the consumer tells you whether the
value is used, and how.** Two of the three above would have been mis-scoped from
the producer alone: one as live when it is latent, one as a correctness fix when
it is dead code.

Practical rule for the Phase 1 classification pass: for each call site, do not
stop at "does this compute a range from a parent plus an offset". Ask **who
receives the result, and do they slice bytes with it, compare it, or throw it
away.**

## 3. `preimage_in` consumers

**What it guarantees:** not byte identity. The **bare `Concat` arm** has the gap
today, with no `Substring` involved:

```
source:  aaa\nbbb        (root plain scalar, 7 bytes)
value:   aaa bbb         (folded, 7 bytes)
pieces:  verbatim 0..3 | replacement 3..4 ("\n" -> " ") | verbatim 4..7
preimage_in(fid)  ->  Some(0..7)      <- licenses copying the wrong bytes
```

`SourceInfo` carries no verbatim tag — it lives in the builder, choosing which
method to call, and does not survive into the emitted value. So `preimage_in`
cannot distinguish a fold from a verbatim run and **cannot be fixed inside the
function**, which has no text to compare.

**Surface: 26 production calls in two files.**

| file | production calls | excluded |
|---|---|---|
| `pampa/src/writers/incremental.rs` | **20** — `:171`, `:421`, `:424`, `:669`, `:672`, `:675`, `:746`, `:798`, `:821`, `:826`, `:1116`, `:1253`, `:1264`, `:1299`, `:1306`, `:1365`, `:1372`, `:1564`, `:1599`, `:1668` | `:1935`, `:1968` past `#[cfg(test)]` (`:1863`); 8 further mentions are comments |
| `pampa/src/pandoc/treesitter_utils/postprocess.rs` | **6** — `:314`, `:315`, `:660`, `:1817`, `:1823`, `:1828` | `:1900` past `#[cfg(test)]` (`:1842`); 6 further mentions are comments |

`crates/wasm-quarto-hub-client/src/lib.rs` mentions it in a comment only.

**One confirmed *copy* site: `incremental.rs:171`**, the
`BlockAlignment::KeepBefore` arm:

```rust
match original_ast.blocks[*orig_idx].source_info().preimage_in(target_file_id) {
    Some(span) if original_qmd.get(span.clone()).is_some() => {
        CoarsenedEntry::Verbatim { byte_range: span, orig_idx: *orig_idx }
    }
    _ => CoarsenedEntry::Rewrite { new_idx: result_idx },
}
```

The span slices `original_qmd` and those bytes are emitted. Its comment at
`:162-169` asserts the retracted claim outright — *"A kept block is
Verbatim-copied out of `original_qmd`, so it must have a byte preimage in the
target file"* — and the `.get()` guard checks **bounds, not identity**. This arm
already exists to fix a related bug (bd-f6h40a9r, foreign provenance sliced at
another file's offsets), so the failure mode has precedent at this exact line.

**One confirmed *locate* site:** `postprocess.rs:660` computes a min/max span
and never slices text.

> **Classified 2026-08-23 (Plan 3 Phase 1).** All 26 are now classified; the
> table is in Plan 3 § Evidence → Phase 1. The result corrects the count above:
> there are **three** copy sites, not one. `assemble_inline_content`'s
> `InlineAlignment::KeepBefore` arm and `assemble_recursed_container`'s
> verbatim early return also emit a hull's bytes as a node's text. Both are
> latent for exactly the reason `:171` is — verified at the consumer, since
> `incremental_write`'s only two production callers both supply an
> untransformed, parent-less baseline (one of which is test-pinned, the other
> only argued — see the note under § Reachability below). Every remaining site is *locate*,
> including seven that slice `original_qmd` at **complement** ranges (gap,
> prefix/suffix, delimiters), where only the offset claim load-bears. Line
> numbers in the tables above are as of `816f4ed47` and have since shifted; the
> Plan 3 table carries the rebased numbers.

### Reachability: LATENT, not live

Three findings, each closing one producer:

1. **`combine()` cannot introduce a fold piece.** It pairs each piece with
   `piece.length()` — the piece's own source extent (`source_info.rs:322-330`)
   — and each piece is a whole `Original`/`Substring` span, so a
   `combine`-produced `Concat` is length-matched *and* byte-identical by
   construction. That eliminates the postprocess-coalescing family, the main
   producer of Block/Inline-level `Concat`s. (It introduces none; it does not
   sanitize one it is *given*, because `Concat::length()` is content length.)
2. **So a fold piece exists only in content provenance** — which *does* reach
   body nodes by design: `parse_yaml_string_as_markdown_to_config`
   (`pampa/src/pandoc/meta.rs`, arms at ~`:303` and ~`:316`) yields
   `PandocInlines` **and** `PandocBlocks`, and every node beneath them carries
   `Substring { parent: content_source_info }` because the nested reader
   threads the parent through `node_source_info_with_options`
   (`pampa/src/pandoc/location.rs:214-217`).
3. **But they cannot reach the copy site.** `incremental_write`'s baseline is
   `capture_untransformed_ast_json` (`quarto-core/src/pipeline.rs:1006-1022`,
   called at `:920`), which (a) **re-parses the raw bytes** through
   `pampa::wasm_entry_points::qmd_to_pandoc(content)` (`:1007`) with a fresh,
   parent-less reader context, so no content-provenance `Substring` can exist
   in the baseline (*correction 2026-08-23:* an earlier revision credited the
   `parent_source_info: None` at `:1013` — that `ASTContext` is built after the
   parse and is read only by the JSON writer; `parent_source_info` is consumed
   at parse time, `pampa/src/pandoc/location.rs:214`, so `:1013` is inert); and (b) runs
   *before any pipeline stage*, so no config-derived node — spliced into
   navbar, footer or title by transforms — can be present.

**The safety is incidental, not structural.** Nothing about `preimage_in`
protects `incremental.rs:171`; the shape of the preview capture does. That is
what Plan 3's guard exists to preserve. The guard's reach is narrower than the
class, though, and Plan 3 § Evidence Phase 1 states the split: it pins the
`capture_untransformed_ast_json` artifact (`apply_node_edit` inherits it);
`incremental_write_qmd`'s own raw-byte re-parse
(`wasm-quarto-hub-client/src/lib.rs:2952`) is latent by an **analogous
invariant that no test exercises**.

> **Measured 2026-08-23 (Plan 3 Phase 1).** The invariant is narrower than
> "the baseline pool is all `Original`": the captured pool also carries
> front-matter **metadata** provenance, which is legitimately `Substring` over
> the front-matter `Original`. What load-bears — and what
> `preview_untransformed_baseline_body_pool_is_all_original_own_file`
> (`quarto-core/src/pipeline.rs`) pins — is that every `SourceInfo` the
> **document body** reaches is an `Original` in the document's own file. The
> writer copies bytes from body spans only.

## 4. The `SourceInfo::original(` surface

145 hits across 69 files; **17 production across 10 files.** 128 are test code
(inside a `#[cfg(test)]` module, under `tests/`, or in `*_tests.rs`). The
original draft's "~132 untriaged across ~53 files" was a `grep -c` line count.

| verdict | sites |
|---|---|
| **the comrak defect** (five construction sites, one bug) | `comrak-to-pandoc/src/text.rs:108`, `:120`, `:140`, `:146`, `:151` |
| **safe — raw source coordinates** | `comrak-to-pandoc/src/source_location.rs:51`; `pampa/src/pandoc/location.rs:295`; `pampa/src/pandoc/treesitter.rs:1375`; `pampa/src/readers/qmd_error_messages.rs:90`; `quarto-lsp-core/src/document.rs:102` (`0..content.len()`) |
| **safe — sentinel / test helper in a non-test module** | `comrak-to-pandoc/src/lib.rs:31`; `quarto-ast-reconcile/src/generators.rs:359`, `:365` |
| **safe by shape, but a drift amplifier** | `postprocess.rs:317`, `:669`, `:1833` |
| **latent, not live** | `quarto-xml/src/parser.rs:628` — § 5 |

**Drift amplifiers** collapse `preimage_in()` results into a flat
`SourceInfo::original`, freezing any upstream drift and discarding the
provenance chain. An **ordering constraint, not a bug**: fix producers before
these consumers or the fix silently does not reach the output. `:1833`
additionally hardcodes `attr_end + 1` on the assumption the closing `}` is the
next raw byte.

## 5. `quarto-xml`, `quarto-csl`, `quarto-citeproc` — latent, and dead

**`quarto_xml::parse_with_parent` is dead code.** Zero callers anywhere,
including tests. The only references are its definition (`parser.rs:55`), its
re-export (`lib.rs:86`) and a doc mention (`lib.rs:74`). (`pampa`'s
`table_caption_provenance.rs` defines a local helper of the same name —
unrelated.) `quarto-xml` is workspace-internal, not one of the externalized
published crates, so there are no outside consumers either. `XmlParser::parent`
and the `Substring` branch of `make_source_info` (`parser.rs:625-629`) are dead
paths with it — so in production `parent` is always `None` and the `Original`
branch always runs.

**`quarto-csl` and `quarto-citeproc` do no offset arithmetic.** Exhaustive grep
for `SourceInfo::substring|original|concat`, `map_offset`, `start_offset()`,
`end_offset()`, `preimage_in`, `resolve_byte_range` across both crates' `src/`:
**no matches.** They only `.clone()` whole `SourceInfo`s —
`attr.value_source.clone()` at ~15 sites in `quarto-csl/src/parser.rs`.
`quarto-citeproc/src/locale_parser.rs` has no `SourceInfo` mention at all.

**The mismatch is nonetheless real, if anyone revives the dead path:**
`parser.rs:469` calls `attr.unescape_value()`, so attribute values **are**
entity-decoded, while `value_source` spans raw text **including the quotes**
(`parser.rs:548-558`) — the identical shape to the YAML instance.

## 6. Other closed questions

### The `quarto.config.md` Lua path — inert

`crates/pampa/src/lua/config_value.rs:601-631` has the same call shape as the
YAML instance, its base being `filter_source_info(lua)`
(`pampa/src/lua/types.rs:2291`) = `Generated { by: By::filter(…), from: [] }`.
Inert on three independent grounds, any one sufficient:

1. `map_offset`'s `Generated` arm returns `None` **unconditionally**
   (`mapping.rs:73-77`) — not conditionally on an empty anchor list. Adding an
   anchor would not make this live.
2. **Zero production `append_anchor` call sites.** All 7
   (`quarto-ast-reconcile/src/hash.rs:2352`, `:2376`, `:2382`, `:2430`;
   `pampa/src/writers/incremental.rs:1902`, `:1904`;
   `pampa/src/lua/diagnostics.rs:857`) are past their file's `#[cfg(test)]`
   (`:1043`, `:1863`, `:399`).
3. **No `Arc::make_mut`/`Arc::get_mut` anywhere in `crates/`**, so the
   `Arc<SourceInfo>` parent cannot be mutated after construction.

> **Corrected 2026-08-23 (Plan 3 Phase 4).** "Any one sufficient" is false for
> `resolve_byte_range`, and ground 1 as written overstates its reach.
> **Measured**, by applying the T8 revert hunk — attaching an
> `AnchorRole::Invocation` anchor in `filter_source_info`
> (`pampa/src/lua/types.rs:2291`) in place of `from: SmallVec::new()`:
> `resolve_byte_range()` on a `quarto.config.md('x')` node went from `None` to
> `Some((0, 0, 1))`. So ground 1 — `map_offset`'s `Generated` arm returning
> `None` unconditionally — is sufficient for **`map_offset` only**, and its
> closing sentence ("Adding an anchor would not make this live") is true of
> that accessor and false of `resolve_byte_range`. `resolve_byte_range` rests
> on grounds 2 and 3 alone. Ground 1's own claim about `map_offset` was also
> measured, and holds: with the same hunk applied and only the
> `resolve_byte_range` assertion neutralized, `map_offset(0, ctx)` was still
> `None`.
>
> The node's measured shape is
> `Substring { parent: Generated { by: By::filter(..), from: [] }, 0..1 }` —
> the `Substring` inherits the parent's answer through
> `parent.resolve_byte_range()?`, which is why the hunk reaches it.
>
> Grounds 2 and 3 were re-verified against the current tree and both hold, with
> one count correction: `append_anchor` now has **8** call sites, not 7 —
> `quarto-config/src/span_assert.rs:577` postdates this document. All 8 still
> sit inside their file's sole `#[cfg(test)]` module (`hash.rs:1050`,
> `span_assert.rs:434`, `incremental.rs:1947`, `diagnostics.rs:399`), which in
> each case opens the file's last top-level item. Ground 3 is unchanged: still
> zero matches in `crates/`.
>
> **Read ground 2 as a claim about *in-place mutation only*.** Now that it and
> ground 3 are the sole support for `resolve_byte_range`, the scope matters.
> `append_anchor` is not the only way production code attaches an anchor —
> three production sites do it by **constructing a new `Generated`**:
> `quarto-core/src/transforms/shortcode_resolve.rs:1177` (unconditionally an
> `Invocation`), `pampa/src/readers/json.rs:502` and
> `pampa/src/lua/diagnostics.rs:195` (both take the role from the data they
> decode; the second is in the Lua subsystem itself). The conclusion survives —
> none of the three replaces the value `filter_source_info` mints, which passes
> straight into `qmd::read` with nothing in between — but an auditor who
> re-derives ground 2 by grepping `append_anchor` alone will conclude
> "production never attaches anchors", which is false about this repo. Ground 2
> establishes that this *particular* `SourceInfo` is never mutated after it is
> built; it does not establish that anchors are unused.
>
> Line numbers: the `append_anchor` sites listed above have shifted (e.g.
> `hash.rs:2352` → `:2359`), and so has this subsection's own opening citation
> — `config_value.rs:601-631` was staled by the commit that added this
> correction. The `quarto.config.md` constructor is now `:686-713`, preceded by
> the provenance comment at `:613-685`.
>
> Note the version gap: this document was measured against `quarto-source-map`
> **0.1.1**, the correction above against the currently-locked **0.1.3**. Both
> accessor behaviours were re-read there — `map_offset`'s `Generated` arm at
> `mapping.rs:75-79` (cited above as `:73-77`), `resolve_byte_range`'s at
> `source_info.rs:404-406`.
>
> **This is now guarded.** `quarto_config_md_yields_no_byte_range` (T8, in
> `pampa/src/lua/config_value.rs`'s `mod tests`) asserts the
> `resolve_byte_range() == None` half and goes red under exactly the hunk
> above; the constructor carries a comment stating the two accessors
> separately. Nothing else in this subsection is amended. In particular, this
> correction says nothing about *which consumers call which accessor* on these
> nodes — that was not exercised, so it is not claimed either way.

**`ProvenanceBuilder` would not fix it if it were live.** The builder maps
content offsets onto source ranges within a parent that has a byte extent;
`Generated { by: By::filter }` is a line-granular attribution to a `.lua` file
and has none. The failure mode would be garbage offsets into a Lua file, and
the fix would be an ephemeral `SourceFile`.

### The engine `map_offset` pair — two sites, and not this bug class

- **Two production sites, not three.** `quarto-core/src/engine/ts_engine.rs:683`
  (`build_source_map`) and `engine/jupyter/text_execute.rs:494`
  (`describe_location`). `stage/stages/engine_execution.rs:2293` is inside
  `#[cfg(test)]` (module opens `:777`).
- **A test exists and is vacuous.**
  `ts_engine.rs:2977 test_build_source_map_maps_lines_to_file_provenance` builds
  `input = &file_content[7..]` with a matching `Original` span, and even asserts
  `assert_eq!(&file_content[7..], input)` — byte-identical by construction.
- **Not decode-vs-raw-span.** In production `ctx.source_info` for the serialized
  path comes from `pampa::writers::qmd::write_with_source_info`
  (`engine_execution.rs:732`), whose `Concat` pairs each block's `source_info()`
  with **bytes written** — writer provenance. `ProvenanceBuilder` does not fix
  it.

### The workaround census — seven sites, one deletion

"The workarounds collapse" is a claim about *capability*, not deletions. Most
stay, having become unnecessary rather than impossible.

| site | owner | disposition |
|---|---|---|
| `callout.rs:431-447` | Plan 2 Phase 4 | **deleted** (the match block; the enclosing function ends at `:448` and keeps its bd-3aolj duplicate-key guard) |
| `use_cmd/config.rs:229` | Plan 2 Phase 4 | **kept** — simplification achievable via the `map_offset` hull, "still optional; the function is correct today, merely limited" |
| `cell_options/mod.rs:196-228` | nobody | **untouched, deliberately** — the *exemplary* case, not a workaround |
| `theorem.rs:344-360`, `proof.rs:181-197` | Plan 2 Phase 4 | output changes as a side effect (wrong-span, not drifting) |
| `codeblock_shorthand.rs:486` | Plan 3 Phase 6 (renumbered 2026-08-23) | in neither sibling plan; byte-searches decoded text inside the raw span (`block_text.find(&cb.text)`) |
| `project/website_post_render.rs:213-222` (`copy_footer_images`) | Plan 2, final fix wave (FIX-2) | **fixed** — appended 2026-08-23, see the note below |

Four independent authors hit this bug class and routed around it.

> **Appended 2026-08-23 (provenance Plan 3, Phase 8).** Two amendments,
> both to the table above; nothing else in this subsection is touched.
>
> **(1) A seventh site.** Plan 2's final whole-branch review found and fixed a
> decoded/raw pairing this table predates:
> `crates/quarto-core/src/project/website_post_render.rs`'s `copy_footer_images`
> re-parsed `cv.as_plain_text()` against `&cv.source_info` — the *raw* span —
> under a comment claiming to parse "the same way" as `ConfigMarkdownTransform`,
> which stopped being true once that transform moved to content provenance. It
> now reads `content_source_info.as_ref().unwrap_or(&cv.source_info)` (`:222`),
> the same expression as `transforms/config_markdown.rs:326`, with the comment
> at `:208-217` naming that line. Verified 2026-08-23: both sites read as
> described.
>
> **The recount, and what each number ranges over.** The heading's count is now
> **seven sites**, counting each row's *sites* (the `theorem.rs` / `proof.rs`
> row is two), and it ranges only over rows of this table. The **"one deletion"
> is unchanged** — `callout.rs` is still the only row whose code was deleted;
> the new row was *fixed in place*. The following subsection's heading, "A
> seventh site: the shortcode-string closure", is **left as written**: it counts
> from the six-row census as it stood, and its subject is deliberately not a
> census row. Counting both enumerations, the closure is the eighth site this
> document names. **"Four independent authors"** likewise ranges over the
> original six rows only — authorship of the new row was not re-derived.
>
> **(2) One row's disposition is now stale.** `codeblock_shorthand.rs:486` was
> **fixed** by Plan 3 Phase 6a (bounded between-fences search, replacing
> `block_text.find(&cb.text)`), guarded by
> `body_source_for_locates_the_body_not_the_info_string`. Its row still
> describes the pre-fix state. The other five original rows were cross-checked
> against the tree on 2026-08-23 and each still reads true: `callout.rs`'s match
> block is gone (the function now ends at `:418`, `#[cfg(test)]` at `:420`, and
> its bd-3aolj guard survives at `:400-412`); `use_cmd/config.rs:229`
> `scalar_value_span` is still present and still returns `None` on mismatch, and
> its hull simplification is now **declined permanently** rather than left
> optional (see the comment at that site); `cell_options/mod.rs` is untouched
> and its constraint is now recorded in that file's header comment;
> `theorem.rs:344-360` and `proof.rs:181-197` are unedited and their output
> tightened exactly as Plan 2 Phase 4 predicted.

### A seventh site: the shortcode-string closure — wrong-span, and its range is dead

Filed by Plan 2 (2026-08-21) as out of its Phase 4 scope, with the scoping
question left open. **Answered here: it is a tightening, not a correctness bug.**

`treesitter.rs:989` defines a **local closure** also named
`extract_quoted_text` — distinct from the shared `text_helpers.rs:28` function —
which open-codes the same strip-and-unescape for `shortcode_string`, then builds
`IntermediateBaseText(text, range)` at `:1006`. Plan 2 was right that Phase 4's
plumbing change does not reach it: it has its own implementation, so changing
the shared function's return type does not force it.

Two findings settle the scope:

1. **The closure's computed range is discarded.** `process_shortcode_string`
   (`treesitter_utils/shortcode.rs:31-45`) destructures it away —
   `let PandocNativeIntermediate::IntermediateBaseText(id, _) = …` at `:36` —
   and then recomputes the range itself from
   `node_source_info_with_context(node, context)`, the **whole node**. So the
   closure's range arithmetic is dead code; only the decoded string survives.
2. **No consumer does sub-offset arithmetic on the surviving range.** Every
   `ShortcodeArg::String` consumer takes the *string*
   (`shortcode_resolve.rs:135`, `:171`, `:837`, `:848`, `:2232`, `:2265`); the
   range travels separately in `IntermediateShortcodeArg` and is never added to
   a content offset.

So this is the **`theorem.rs`/`proof.rs` category** — a decoded value paired
with a whole raw quote-inclusive span, wrong-span but not drifting. Fixing it
tightens a span; it does not correct a wrong byte position. Scope it
accordingly, and note that the dead range computation at `:1000-1005` can simply
be deleted rather than corrected.

**`cell_options`' constraint, named:** a language's option-line syntax may only
*elide* spans, never *transform* them, because every byte of the reassembled
YAML must be a real source byte. Plan 1's reversal to **store** zero-content
pieces makes a deletion expressible, so `replacement(src_range, 0)` would lift
it — but no language needs it.

## 7. comrak `NodeValue::Text`

**The bug, verified through the real binary.**
`crates/comrak-to-pandoc/src/inline.rs:49-52` passes comrak's raw
`ast.sourcepos` as `base_offset` into `tokenize_text_with_source`
(`src/text.rs:90-140`), which computes `base_offset + byte_idx` over the
**decoded** `NodeValue::Text`. On `aa\*bb cc &amp; dd ee`:

| token | reported | true | drift |
|---|---|---|---|
| `aa*bb` | 0..5 | 0..6 | end short |
| Space | 5..6 | 6..7 | −1 (points at `b`) |
| `cc` | 6..8 | 7..9 | −1 |
| `&` | 9..10 | 10..15 | −1, len 1 vs 5 |
| `dd` | 11..13 | 16..18 | **−5** |
| `ee` | 14..16 | 19..21 | **−5** |

**It accumulates** — −1 per backslash escape, −4 per `&amp;`.

**The mechanism is not parse-time decoding.** `handle_backslash`
(`comrak-0.52.0/src/parser/inlines.rs:454`) emits a *separate* `Escaped > Text`
node whose sourcepos correctly points at the escaped character; `handle_entity`
(`:493`) emits a separate `Text` node. Each is individually correct. The damage
is done in `Parser::postprocess_text_nodes` (`parser/mod.rs:2396`), which joins
adjacent `Text` siblings while extending `sourcepos.end`, and — with
`coalesce_escaped = true`, which it is, since `Options::default()` leaves both
`parse.escaped_char_spans` and `render.escaped_char_spans` false — splices
`Escaped` children into their neighbours, extending `sourcepos.end` again.

Two consequences: **`inline.rs:94`'s `NodeValue::Escaped` arm is dead code**
under `Options::default()` — do not build a fix on it. And **comrak already
solves this internally and discards it**:
`postprocess_text_node_with_context` builds
`spxv: VecDeque<(Sourcepos, usize)>` wrapped as `Spx`, a run table of (source
span, decoded byte count), so tasklist and autolink processing can translate
decoded offsets. It is not exposed on the AST.

**The other `NodeValue` arms are unaffected.** The only per-offset arithmetic in
the crate is `inline.rs:51-52`; every other arm goes through
`sourcepos_to_source_info`, a whole-node raw span. Two caveats worth a code
comment: `Code` pairs `code.literal` (backticks and one leading/trailing space
stripped) with the backtick-inclusive span, and `Link`/`Image` carry
entity-decoded URLs with `TargetSourceInfo::empty()`.

**Lockstep is the right fix and is well-posed.** Per Plan 1 § The shared
builder, comrak "hands us decoded text with raw sourcepos and no access to its
escape handling, so it is forced into the lockstep form" — which means **no
re-implementation of the HTML5 named-entity table**: segmentation for an entity
is "find the `;`", and the decoded string supplies the content lengths. Three
measured facts:

1. **A `Text` node's span is contiguous and single-line.** Drift resets at every
   `SoftBreak`, so block prefixes are never inside a `Text` node. Measured on
   `> aa\*bb cc`⏎`> dd &amp; ee`: `dd` reports 14..16, which is *correct* — the
   `> ` at 12..14 sits outside both nodes. **Lockstep needs no deletion rule.**
2. **Replacements are n→m, not n→1.** `&#x1F600;` is 9 source bytes → 4 content
   bytes. `replacement(src_range, out_len)` covers it; no API gap.
3. **Escape must be tried before verbatim.** `&amp;` begins with `&` and its
   decoded value *is* `&`, so a verbatim-first walker consumes it 1:1 and
   strands on `a`. Plan 1 measured that reordering desyncs 9 of 24 YAML shapes.

Worked tiling: `verbatim(0..2)=2 | replacement(2..4→1) | verbatim(4..10)=6 |
replacement(10..15→1) | verbatim(15..21)=6` → 16 content bytes =
`"aa*bb cc & dd ee".len()`.

**Downstream: one writer, and nothing reads it.** `preimage_in`'s two consumer
files are both off the commonmark path — `transform_divs` is called only from
the JSON-input arm (`main.rs:308`), and `incremental.rs` is not reachable from
any `--to` value (`json`, `raw-json`, `native`, `markdown`/`qmd`, `html`,
`plaintext`, `ansi`). So the only consumer of a comrak `Concat` is the JSON
writer's `r`/`p` output: `writers/json.rs:357-361` emits the `Substring`'s
content-relative pair, `:363-379` emits `(0, sum_of_piece_lengths)` for the
parent. **Do not infer from `r` alone that a reader breaks** — Plan 2 retracted
exactly that inference after finding `annotated-qmd`'s `resolveChain` treats
`info.r` as an error path and walks the pieces properly
(`source-map.ts:317-375`). The observable is the pool chain and snapshot churn.

Also worth a comment, not a fix: a sub-character offset inside an
entity-produced character maps to an arbitrary byte inside `&#x1F600;` —
harmless, since the source is ASCII and the whole entity is the honest
provenance.

## 8. Measurements

### `Substring{parent: Concat}` accessor behavior

The `Concat` fixtures were **hand-built**, not produced by a
`ProvenanceBuilder` that does not exist yet — they measure the crate's behavior
given that shape, faithful by inference to Plan 1's derivation but not to the
eventual pipeline.

Fixture: `A = concat([(Original{fid,1,3},2), (Original{fid,3,5},1),
(Original{fid,5,6},1)])` modelling `'it''s'` — content 4 bytes, source extent
1..6; `B` = the same with a source gap at 10..12; `C = substring(A, 0, 4)`.

```
A.resolve_byte_range() = None
C.resolve_byte_range() = None
C.root_file_id()       = Some(FileId(9415328668825900988))
A.preimage_in(fid)     = Some(1..6)      <- correct hull, gap-free
B.preimage_in(fid)     = None            <- gappy
C.preimage_in(fid)     = Some(1..5)      <- WRONG, truth is 1..6
C.map_offset(0)        = offset 1        <- correct
C.map_offset(4)        = offset 6        <- correct
C.start_offset() = 0   C.end_offset() = 4   (content coords, not file offsets)
bind_config_source(&C, [path])              = None
bind_config_source(&Original(1,6), [path])  = Some(path)   <- control, first try
```

The control passing on the first attempt is what makes the `None` attributable
to the `Concat` rather than to a broken fixture. This is the basis of Plan 2's
Phase 3 binding fix and Phase 4 TS remedy.

### comrak drift

The table in § 7, from
`cargo run --bin pampa -- --from commonmark --to json --json-source-location full`
on `.scratch/prov3/{cm,bq,ent}.md`. Output inspected directly.

### Site counts, and how they were derived

Both of the original draft's headline counts were `grep -c` line counts
mistaken for call counts. These are line-classified:

- `SourceInfo::original(` — 145 hits, **17 production**
- `SourceInfo::substring(` — 17 hits, 9 production
- `map_offset(` — 42 hits, 20 real production calls (4 of the 24 non-test hits
  are comment or doc-comment lines)
- `append_anchor` — 7 hits, **0 production**
- `preimage_in` — **26 production calls**: 20 in `incremental.rs` (30 lines
  mention it; 8 are comments, `:1935`/`:1968` past `#[cfg(test)]` at `:1863`)
  and 6 in `postprocess.rs` (13 lines mention it; 6 are comments, `:1900` past
  `#[cfg(test)]` at `:1842`)

## 9. Glossary

- **preimage** — the byte range in an original file that a `SourceInfo` covers.
- **hull** — the smallest single range containing all of a `Concat`'s pieces.
  Exists only when the pieces tile the source without gaps.
- **fold** (*fold piece*, *fold-shaped*) — a `Concat` piece whose source run and
  content run have **equal length but different bytes**. YAML's line folding
  produces them: source `\n` → content `" "`. They are why length and
  contiguity checks cannot establish byte identity.
- **lockstep** — deriving provenance by walking a decoded string against its raw
  source, taking *segmentation* from the grammar and *content lengths* from the
  decoded value, treated as the oracle. Full definition and its four rules:
  Plan 1 § How the pieces are derived.
- **synthesis** — a piece with content but no source bytes
  (`replacement(eof..eof, 1)`).
- **drift amplifier** — a site that collapses resolved ranges into a flat
  `Original`, freezing upstream drift and discarding the provenance chain.
