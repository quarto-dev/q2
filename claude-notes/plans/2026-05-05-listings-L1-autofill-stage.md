# L1 — `ListingItemInfoStage` (auto-fill, pre-checkpoint)

**Date:** 2026-05-05
**Beads:** `bd-izqh`. Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessor:** L0 (`bd-n8a4`, closed) — adds the
`DocumentProfile.listing_item: ListingItemInfo` field and the
`profile.categories_raw: Option<ConfigValue>` field. L1 *populates*
that field at the metadata layer; L0 *reads* it at extraction time.
**Status:** In progress (worktree `.worktrees/bd-izqh-listing-item-info-stage/`).

**Implementation-session decisions (2026-05-06).** The user reviewed
this plan ahead of implementation and made or confirmed five calls.
Each is recorded inline in §"Decisions log" (D11–D14) and reflected
in the relevant sections below. Summary:

- **D11 — no truncation in L1.** Store the *full* first paragraph in
  `meta.listing-item.description`. L3 truncates at render time when
  the listing's `max-description-length` is known. Removes
  `MAX_DESCRIPTION_LEN` from L1's surface entirely.
- **D12 — datetime crate is `time` (not `chrono`).** `time = "0.3"`
  becomes a `quarto-core` dep. Reused for L9's RFC 822 RSS dates.
- **D13 — shortcode-bearing image `src` is out of scope.** L1 runs
  before pre-engine sugaring, so an `Image` whose `src` was
  originally `{{< meta thumbnail >}}.png` still carries the literal
  shortcode text in `target.0`. Filed `bd-8h9o` as a discovered-from
  follow-up to study the problem in isolation; L1 does not filter
  these today.
- **D14 — `?Send` async traits.** `#[async_trait(?Send)]` is the
  project-wide convention; native + WASM compile from one trait
  definition. Codified in `.claude/rules/wasm.md` for colleagues.
- **Audit checkpoints cleared.** `ConfigValue::insert_path` /
  `contains_path` already exist; `StageContext.runtime` is already
  `pub`; `SystemRuntime::path_metadata` is already implemented on
  both backends; `metadata_normalize::inlines_to_plain_text` only
  needs a `pub(crate)` lift. None of the "stop and ask the user"
  branches in the preparation section apply.

## Goal of this phase

Add a new pipeline stage between `IncludeExpansionStage` and
`DocumentProfileStage` that **enriches `meta.listing-item`** with
auto-derived values when the author hasn't supplied them. The stage:

1. **Reads** `ast.meta.listing-item.*` (current author-supplied
   values), `ast.blocks` (post-include AST for word-count /
   first-paragraph / first-image), and the source-file path
   (filesystem mtime for `date-modified`).
2. **Computes** any missing curated fields:
   - `description` — first plain-text paragraph from `ast.blocks`
     (full text, **not truncated**; per D11 the listing host's
     `max-description-length` is L3's concern).
   - `image` — first `Inline::Image` `target.0` from `ast.blocks`
     (document order). Shortcode-bearing `src` (e.g. `{{< meta
     thumb >}}.png`) is **not** filtered here; tracked separately
     under `bd-8h9o` (see D13).
   - `word_count` — tokenized scan of `ast.blocks` plain text.
   - `reading_time_minutes` — `word_count / 200` (200 wpm
     constant, ceiling rather than floor for sub-minute texts so
     a one-word post still reports "1 min").
   - `date_modified` — filesystem mtime of `doc.path`, formatted
     as ISO-8601 date string (`YYYY-MM-DD`). Skipped when mtime
     is unavailable.
3. **Writes** the computed values back into
   `ast.meta.listing-item.<field>` as `ConfigValue` entries, only
   for fields the author left unset. Author-supplied values are
   never overwritten — the stage strictly *fills holes*.
4. The downstream `DocumentProfileStage` (already present from
   L0) calls `extract_listing_item(meta)` and produces a
   `ListingItemInfo` whose curated fields reflect both author
   values and L1 fill-ins.

**Critical architectural property** (per L0 handoff item 4): L1 is
the **only** place a profile field gets *populated outside the
extractor*. The mechanism is **pre-checkpoint metadata enrichment,
not post-checkpoint mutation.** L1 mutates `ast.meta` before the
profile is built; the profile itself remains read-only after
`DocumentProfileStage` runs. The contract doc's "Mutability"
section is preserved.

**No user-visible behavior change yet.** The auto-filled values
sit in profiles that no consumer reads until L3 lands. L1's
verification is structural: profiles built from fixtures *with*
the stage carry expected `listing_item` fields; profiles built
*without* it (or with the author already setting those fields)
remain unchanged.

## Reference material

Read before writing code:

- Parent epic plan §"L1":
  `claude-notes/plans/2026-05-05-listings-epic.md`.
- L0 sub-plan and handoff notes:
  `claude-notes/plans/2026-05-05-listings-L0-profile-extension.md`
  (especially §"Decisions log" D1–D7 and the post-impl handoff
  items 1–10 the orchestrator received).
- L0 implementation (already merged):
  - `crates/quarto-core/src/document_profile.rs` —
    `ListingItemInfo` struct (line 159), `is_empty` (236),
    `extract_listing_item` (578), `extract_u32_field` (603),
    `extract_listing_item_extra` (618). L1 reuses these
    helpers; specifically, `extract_listing_item` already
    knows how to read every kebab-case key L1 will write.
  - `crates/quarto-core/tests/document_profile_pipeline.rs` —
    the byte-identical clone+resume integration test L1 must
    not regress.
- The pipeline-stage shape:
  - `crates/quarto-core/src/stage/stages/include_expansion.rs`
    (the predecessor stage; see how `recorded_includes` is
    populated as a side-channel on `DocumentAst`).
  - `crates/quarto-core/src/stage/stages/document_profile.rs`
    (the successor; line 90's `profile.includes =
    std::mem::take(&mut doc.recorded_includes);` is the
    nearest precedent for "stage drains state into the
    profile" — though L1 takes a different approach, see
    §"Why metadata enrichment, not a side-channel" below).
  - `crates/quarto-core/src/stage/data.rs` — `DocumentAst`
    shape (line 314); the `ast.meta` is mutable through
    `&mut DocumentAst`, which is what L1 needs.
- Plain-text extraction: **reuse**
  `crates/quarto-core/src/transforms/metadata_normalize.rs`
  line 110 (`inlines_to_plain_text`). The other five
  call sites in tree do **not** share semantics
  (see §"Plain-text helper choice" below for the
  comparison); deliberate consolidation is tracked
  separately as `bd-zzke` (chore, P3) and is **not**
  L1's job.
- Pipeline assembly:
  `crates/quarto-core/src/pipeline.rs` — both
  `build_html_pipeline_stages_with_apply_config` (line ~217)
  and `build_wasm_html_pipeline` insert
  `IncludeExpansionStage` immediately before
  `DocumentProfileStage`. L1 inserts between them, in both
  builders.

## Why metadata enrichment, not a side-channel

The include-expansion precedent populates a side-channel
(`DocumentAst::recorded_includes`) which `DocumentProfileStage`
drains into `profile.includes`. L1 takes a different approach:
write into `ast.meta` (the same `ConfigValue` map that
`extract_listing_item` already reads at L0).

Reasons:

1. **Single extraction path.** With L1 writing to `meta`, all of
   `listing_item`'s population — author-supplied *and* auto-
   filled — flows through `extract_listing_item` at the
   checkpoint. There is exactly one site that decides "what does
   `listing_item` look like." A side-channel would split that
   into "author keys go through `extract_listing_item`,
   auto-fills go through a separate side-channel drainer in
   `DocumentProfileStage`," doubling the surface.
2. **Author-vs-auto check is local.** "Did the author set this?"
   is "is `meta.get("listing-item").get("title")` populated?"
   right inside the L1 stage. With a side-channel, L1 would have
   to carry the author-check separately or look up
   `ast.meta.listing-item.*` anyway.
3. **No new `DocumentAst` field.** `recorded_includes` justifies
   its existence by carrying a content hash (private to includes)
   and by being plumbing for cache invalidation. L1's auto-fills
   are plain `ConfigValue` strings/integers; they belong in the
   metadata graph that already carries everything else of that
   kind.
4. **Cache-key correctness for free.** `DocumentProfile` already
   serializes `listing_item` (L0). Phase-8's cache key is
   computed from the serialized profile. Author-set or L1-set,
   the value lands in the same struct, so cache invalidation
   logic doesn't need to know L1 exists.

The cost: `ast.meta` is mutated mid-pipeline. `MetadataMergeStage`
already mutates `ast.meta` (it builds the merged value); L1 is
mutating it slightly later. The mutation window is bounded:
between `IncludeExpansionStage` (finished writing) and
`DocumentProfileStage` (starts reading), no other stage looks at
`ast.meta`. The contract doc's "Mutability" rule is about
*profiles* being read-only, not `ast.meta`. No contract change
needed.

## Stage design

### Module location

`crates/quarto-core/src/stage/stages/listing_item_info.rs` —
matches sibling stage modules per epic decision 6.

### Type sketch

```rust
//! Pre-checkpoint stage that enriches `meta.listing-item` with
//! values derived from the post-include AST when the author has
//! not supplied them. Runs between `IncludeExpansionStage` and
//! `DocumentProfileStage`; the latter then reads the enriched
//! map via `extract_listing_item` (L0).

use crate::stage::data::{DocumentAst, PipelineData, PipelineDataKind};
use crate::stage::{PipelineError, PipelineStage, StageContext};

#[derive(Debug, Default)]
pub struct ListingItemInfoStage;

impl ListingItemInfoStage {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl PipelineStage for ListingItemInfoStage {
    fn name(&self) -> &'static str { "listing-item-info" }
    fn input_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentAst }
    fn output_kind(&self) -> PipelineDataKind { PipelineDataKind::DocumentAst }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        autofill_listing_item(&mut doc, ctx);
        Ok(PipelineData::DocumentAst(doc))
    }
}
```

### Auto-fill function

```rust
fn autofill_listing_item(doc: &mut DocumentAst, ctx: &StageContext) {
    // Compute candidate fill-ins from the AST (no I/O for these).
    let blocks = &doc.ast.blocks;
    let cand_description = compute_description(blocks);  // full paragraph, no truncation (D11)
    let cand_image       = first_image_src(blocks);      // shortcode filtering deferred to bd-8h9o (D13)
    let cand_word_count  = word_count(blocks);
    let cand_reading     = cand_word_count.map(|w| div_ceil(w, WORDS_PER_MINUTE));

    // mtime via the runtime trait; native reads filesystem, WASM
    // returns None today (see bd-a3we). Either way, L1 is the same
    // code; the backend semantics live in quarto-system-runtime.
    let cand_date_modified = mtime_iso(ctx.runtime.as_ref(), &doc.path);

    // Find or create `meta.listing-item` as a ConfigValue map.
    let li = doc.ast.meta.ensure_map_entry("listing-item");

    // Each fill is "set the key only if not already present."
    li.fill_if_absent_string("description",       cand_description);
    li.fill_if_absent_string("image",             cand_image);
    li.fill_if_absent_u32   ("word-count",        cand_word_count);
    li.fill_if_absent_u32   ("reading-time-minutes", cand_reading);
    li.fill_if_absent_string("date-modified",     cand_date_modified);
}
```

(Exact `ctx.runtime` accessor name is TBD — verify against
`StageContext`'s actual fields in the worktree.)

The shapes `ensure_map_entry`, `fill_if_absent_string`, etc. are
**not** implied to exist — they're the API L1 wants. The
implementation work is figuring out the right idiom against
`ConfigValue` (see §"`ConfigValue` mutation idioms" below).

### Constants

```rust
const WORDS_PER_MINUTE: u32 = 200;
```

Per D11 (2026-05-06), L1 stores the *full* first-paragraph text.
Truncation to `max-description-length` is L3's responsibility at
render time, where the listing host's per-listing config is known.
Removing `MAX_DESCRIPTION_LEN` from L1 also removes the L1/L3 seam
where a listing wanting a description longer than 175 chars would
have been silently capped upstream.

### `ConfigValue` mutation idioms

`ConfigValue` is the merged-metadata blob throughout Q2. L1 needs
to:

- Look up nested keys (`listing-item.description`) — already
  supported via `meta.get("listing-item").and_then(|li|
  li.get("description"))`.
- *Mutate* nested keys: ensure the `listing-item` map entry
  exists, then insert children when absent.

Survey the existing `ConfigValue` API in
`crates/quarto-pandoc-types/src/config_value.rs`. Look for:
- An existing `as_map_entries_mut` or equivalent. If present, L1
  uses it directly.
- An existing builder for a `ConfigValue::Mapping` from
  `(String, ConfigValue)` pairs.
- An existing `set` / `insert` method on the mapping flavor.

If the API supports mutation cleanly, L1's stage code is small
(maybe 50 lines). If the API is read-only, **stop and discuss
with the user** before adding a mutable accessor. Mutating a
shared serializable type is a contract-doc-level decision; L1's
sub-plan should not carry the precedent without sign-off.

A minimum-impact alternative if the type is hard to mutate in
place: rebuild the `listing-item` sub-map by reading the existing
entries, computing the fill-ins, and replacing the whole sub-map
on the parent. The parent map is already mutable in
`MetadataMergeStage`, so the same idiom works here.

### Plain-text helper choice

There are six `inlines_to_(plain_)text`-flavored functions in
tree, audited 2026-05-06. They are not duplicates — they are
six different functions that happen to share names. Coverage
divergences:

| Site                                        | `Code` | `Math` | `Quoted`            | `Note` (footnote) | `RawInline` | `Custom` slots | `Image` alt | `LineBreak` |
|---------------------------------------------|--------|--------|---------------------|-------------------|-------------|----------------|-------------|-------------|
| `quarto-pandoc-types/src/config_value.rs`   | text   | text   | flatten             | skip              | skip        | skip           | recurse alt | space       |
| `quarto-core/src/transforms/title_block.rs` | text   | —      | —                   | —                 | —           | —              | —           | newline     |
| `quarto-core/src/transforms/metadata_normalize.rs` | text   | text   | wrap with `'`/`"` (per QuoteType) | recurse blocks    | text        | recurse slots  | recurse alt | newline     |
| `quarto-core/src/template.rs`               | text   | text   | wrap with `"` only  | recurse blocks    | —           | —              | recurse alt | newline     |
| `quarto-config/src/format.rs`               | skip   | skip   | skip                | skip              | skip        | skip           | skip        | skip        |
| `quarto-lsp-core/src/analysis.rs`           | (separate fn) | (separate fn) | recurse | recurse | …           | …              | …           | drop        |

A "consolidate to one shared helper" pass requires either
choosing one shape (and silently changing the others' output —
a snapshot-churn risk on five render paths) or building an
options-driven helper with five booleans plus a per-site
audit. That is a separate hygiene project, deliberately
deferred — see `bd-zzke`.

**For L1, reuse `metadata_normalize::inlines_to_plain_text`.**
Reasons:

- It lives in the same crate as L1's stage code, so no new
  cross-crate visibility surface.
- It is the most complete: covers `Custom` slots, every
  formatting variant, math, code, raw-inline, footnote
  recursion. L1 won't need to walk anything itself for
  description-text rendering.
- Its only divergence from L1's ideal is that it recurses into
  `Note` content (footnote text). For *description preview*
  this is fine — the description is the first paragraph's
  rendered text, footnotes-and-all is what authors see. For
  **word count**, L1's own block-level walk handles the
  exclusion (see below) — we don't need to fix the helper.

**`metadata_normalize::inlines_to_plain_text` is currently `fn`
(private to the module).** L1 needs it `pub(crate)` or
exported through `crate::transforms`. Implementation note:
add a one-line doc comment noting "Re-used by
`stages::listing_item_info` (`bd-izqh`); if a third consumer
arrives, file `bd-zzke` to consolidate." This documents the
deferral inline with the helper.

For **block-level walking** (used by `compute_description`,
`first_image_src`, `word_count`), L1 writes its own walker.
It needs to:

- Walk only block containers L1 cares about (description-
  candidate paragraphs, image-bearing blocks, word-count
  blocks).
- Skip footnote blocks for word-count (Q1 parity — footnote
  text doesn't count toward reading time).

The block walker is small (~50 lines) and L1-specific. Don't
try to reuse `metadata_normalize::blocks_to_plain_text`; its
needs are different (full block-text rendering for metadata
keys, with footnote inclusion).

### Helper specifications

#### `compute_description(blocks: &[Block]) -> Option<String>`

- Walk `blocks` in document order; find the first
  `Block::Para(p)` (or `Block::Plain(p)`) whose
  `metadata_normalize::inlines_to_plain_text(&p.content)`
  produces a non-empty string after `trim()`.
- Return that text **untruncated** (per D11). Truncation to
  `max-description-length` is L3's job at render time.
- If no paragraph is found, return `None`. Empty documents,
  documents starting with a heading and no paragraphs, etc.
  produce `None` and the field stays unset.

#### `first_image_src(blocks: &[Block]) -> Option<String>`

- Walk `blocks` and inline content in document order.
- Return the `target.0` (URL) of the first `Inline::Image`
  encountered.
- Recurse into containers: `Inline::Emph`, `Strong`,
  `Underline`, `Strikeout`, `Superscript`, `Subscript`,
  `SmallCaps`, `Quoted`, `Link`, `Span`, and through
  `Block::Para`, `Block::Plain`, `Block::Header`, `Block::Div`,
  `Block::BlockQuote`, `Block::BulletList`, `Block::OrderedList`,
  `Block::DefinitionList`, `Block::Table` (including caption /
  cell content), `Block::Figure`, `Block::LineBlock`. (Cover the
  obvious containers; if a less-common one is missed, fix in a
  follow-up. Q1's `findPreviewImgEl` walks the rendered DOM,
  so its block coverage is implicit.)
- A stylized image *inside* a `Link` is still found (Q1
  matches first preview image regardless of link wrap).
- Skip images whose `target.0` is empty.

#### `word_count(blocks: &[Block]) -> Option<u32>`

- L1's own block walker. Visit `Block::Para`, `Plain`,
  `Header`, `BlockQuote` (recurse), `Div` (recurse),
  `BulletList`/`OrderedList`/`DefinitionList` (recurse into
  items), `LineBlock`, `Figure`, table caption / cells. Use
  `metadata_normalize::inlines_to_plain_text` on each Inline
  vector visited.
- **Skip footnote text.** `Inline::Note` content is not
  counted toward word-count (Q1 parity — footnote prose
  doesn't affect reading time). Since
  `metadata_normalize::inlines_to_plain_text` *does* recurse
  into notes, L1's block walker must either (a) strip
  `Inline::Note` from each inline list before passing to
  the helper, or (b) implement its own inline walk that
  excludes notes. (a) is simpler; choose at implementation
  time based on which is more idiomatic against the inline
  type.
- Tokenize on whitespace runs (`split_whitespace().count()`).
- Return `None` if the count is `0`; `Some(n)` for `n >= 1`.

#### `div_ceil(numerator: u32, denominator: u32) -> u32`

`(numerator + denominator - 1) / denominator`. Use
`u32::div_ceil` if the MSRV permits. Ceiling so a 1-word post
reports "1 min" not "0 min."

#### `mtime_iso(runtime: &dyn SystemRuntime, path: &Path) -> Option<String>`

```rust
fn mtime_iso(runtime: &dyn SystemRuntime, path: &Path) -> Option<String> {
    let metadata = runtime.path_metadata(path).ok()?;
    let modified = metadata.modified?;
    let dt = time::OffsetDateTime::from(modified);  // SystemTime → OffsetDateTime
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    dt.format(&fmt).ok()
}
```

**Use `SystemRuntime::path_metadata`, not `std::fs::metadata`
directly.** The runtime trait already abstracts native vs.
WASM: `crates/quarto-system-runtime/src/native.rs` reads from
the filesystem; `wasm.rs` reads from the Automerge-backed VFS.
On native, `metadata.modified` is `Some(SystemTime)`; on WASM,
it's currently `None` (the VFS doesn't yet track modification
times — see `bd-a3we`). Either way, L1's code is the same.

This is the right shape architecturally: the difference between
"filesystem mtime" and "Automerge change-history time" is a
backend property, not a consumer concern. L1 asks "when was
this last modified?" and the runtime answers (or returns
`None` if it doesn't know yet).

**Datetime crate (per D12): `time = "0.3"`.** Pre-decision draft
referenced `chrono`; that was inaccurate (chrono is not a
workspace dep). The `time` crate is already in use by
`quarto-hub`; adding it as a `quarto-core` dep consolidates
onto one datetime crate and unblocks L9's RFC 822 RSS dates
without re-deciding. `time::OffsetDateTime::from(SystemTime)` is
infallible; the format-description macro is checked at compile
time. UTC is implicit because `SystemTime` carries no zone — for
mtime on a build machine that's the conventional choice and
matches Q1's behavior on shared CI.

**`StageContext` access to the runtime:** the L1 stage runs
inside the pipeline, so `ctx.runtime` (or whatever the field
is called on `StageContext`) provides the trait object.
Confirm the access pattern by reading
`crates/quarto-core/src/stage/mod.rs` during the worktree
audit. If the runtime is not currently threaded through
`StageContext`, this becomes a small additional plumbing
task — flag in the orchestrator handoff if it requires
non-trivial wiring.

**Hub-client behavior today:** `bd-a3we` is open as a P2
follow-up to teach the WASM VFS to surface a meaningful
`modified` from Automerge change-op timestamps. Until that
lands, `listing_item.date_modified` stays `None` for
hub-client renders and listings simply omit the
"last modified" column for those documents (or fall back to
`listing_item.date`). When `bd-a3we` lands, L1 needs **no
code change** — the field will start populating
automatically.

## Stage idempotence

The L1 stage **must** be idempotent. Re-running it on a document
whose `meta.listing-item.description` is already set leaves the
field unchanged. This matters for:

- The byte-identical clone-and-resume integration test
  (`pipeline_at_profile_to_end_produces_expected_html`) which
  clones the AST at the profile checkpoint and re-runs the tail
  pipeline. L1 runs *before* the checkpoint, so this test isn't
  directly affected, but having idempotence as a property keeps
  later resume-from-disk caches safe.
- Phase-8 cache rebuilds: a cached profile with auto-fill
  values, when "rebuilt" from a re-run pipeline, must produce
  the same values. As long as L1 is a pure function of `ast`
  (and mtime, which is a known invalidation key), this holds.

Idempotence is testable: run the stage twice in a row, assert
the second run is a no-op.

## Pipeline integration

Insert `ListingItemInfoStage` between `IncludeExpansionStage`
and `DocumentProfileStage` in **both** pipeline builders:

```rust
// crates/quarto-core/src/pipeline.rs
// in build_html_pipeline_stages_with_apply_config:
Box::new(IncludeExpansionStage::new()),
Box::new(ListingItemInfoStage::new()),  // NEW
Box::new(DocumentProfileStage::new()),
Box::new(UnwrapProfileStage::new()),
// …

// in build_wasm_html_pipeline:
// same insertion at the analogous position.
```

Update the doc comments at the top of those builders that list
the stage order (e.g. "3. `IncludeExpansionStage` ... 4.
`DocumentProfileStage` ..." becomes "3. `IncludeExpansionStage`
... 4. `ListingItemInfoStage` ... 5. `DocumentProfileStage` ...").

If the analysis pipeline (`build_analysis_pipeline_stages` if
it exists) is *not* updated, document why: analysis doesn't
need `listing_item` data and adding the stage there is dead
weight. Same precedent as `DocumentProfileStage` itself in L0.

## Tests

TDD: write tests first, watch fail, implement, watch pass.

### Unit tests in `listing_item_info.rs`

1. **`autofill_no_op_when_meta_listing_item_complete`** — set
   every curated field on `ast.meta.listing-item` to an author
   value; run the stage; assert no field changes.
2. **`autofill_populates_description_when_unset`** — fixture
   with three paragraphs; first non-empty paragraph text becomes
   the description, **untruncated** (D11). Use a long paragraph
   (e.g. 300 chars) and assert the stored value is the full
   text, byte-for-byte.
3. **`autofill_skips_description_when_no_paragraph`** —
   document with only a heading; `description` stays `None`.
4. **`autofill_description_skips_empty_paragraphs`** —
   document where the first paragraph is whitespace-only after
   plain-text extraction (e.g. an `<aside>` style markup that
   normalizes to empty); the next non-empty paragraph wins.
5. **`autofill_populates_image_from_first_inline_image`** —
   paragraph with an `Inline::Image` whose target is
   `"figs/cover.png"`; after the stage,
   `meta.listing-item.image` is `"figs/cover.png"`.
6. **`autofill_image_walks_into_link`** — image inside a link;
   image src still surfaces.
7. **`autofill_image_skips_empty_targets`** — first image has
   empty target, second has `"plot.png"`; field is
   `"plot.png"`.
8. **`autofill_no_image_leaves_field_unset`** — image-free
   document; field stays `None`.
9. **`autofill_word_count_matches_simple_doc`** —
   a document with exactly 47 words → `word_count == 47`.
10. **`autofill_reading_time_ceiling`** — 1-word doc →
    `reading_time_minutes == 1`. 200-word → 1. 201-word → 2.
    Verifies the ceiling div.
11. **`autofill_word_count_zero_returns_none`** — empty
    document → both `word_count` and `reading_time_minutes`
    stay `None`.
12. **`autofill_date_modified_via_runtime`** — using a
    test `SystemRuntime` impl that returns a known
    `SystemTime` from `path_metadata`, run the stage; assert
    `date_modified` matches `YYYY-MM-DD` of that time.
    Backend-agnostic — same test runs on both native and
    WASM. (Native CI also exercises a real filesystem path
    indirectly via the integration tests below.)
13. **`autofill_date_modified_skipped_when_runtime_returns_none`** —
    using a test runtime that returns `modified: None` (the
    current WASM behavior), `date_modified` stays `None`.
    No panic, no error. This is the contract `bd-a3we` will
    eventually flip from `None` to `Some(...)` in the WASM
    runtime impl.
14. **`autofill_idempotent`** — run the stage; capture
    `meta.listing-item`; run it again; assert no change.
15. **`autofill_preserves_author_extra`** — author set
    `listing-item.extra.status = "draft"`; the stage must not
    touch `extra` at all. The stage only writes the curated
    keys.
16. **`autofill_creates_listing_item_when_absent`** —
    document with **no** `listing-item:` key in frontmatter;
    after the stage, `listing-item` exists with auto-fills.
    L0's `extract_listing_item` then produces a populated
    `ListingItemInfo`.

### Stage tests (PipelineStage trait)

17. **`stage_advances_documentast_to_documentast`** —
    standard input/output kind check, mirroring
    `document_profile.rs` test 7.
18. **`stage_rejects_non_documentast_input`** — passing
    e.g. a `LoadedSource` returns
    `PipelineError::UnexpectedInput`.

### Pipeline integration tests in
`crates/quarto-core/tests/document_profile_pipeline.rs`

19. **`pipeline_listing_item_autofill_end_to_end`** — fixture
    with a paragraph, an image, and no `listing-item:` key;
    run the full pipeline; after the checkpoint, the
    extracted `profile.listing_item` carries `description`,
    `image`, `word_count`, `reading_time_minutes`,
    `date_modified` (when run natively).
20. **`pipeline_listing_item_author_overrides_winner`** —
    fixture sets `listing-item.description: "Author"` and
    has paragraph text; profile reflects "Author".
21. **`pipeline_clone_and_resume_unchanged_by_l1`** — the
    existing
    `pipeline_at_profile_to_end_produces_expected_html` test
    must continue to produce byte-identical HTML. L1 runs
    pre-checkpoint, so this test exercises the resume tail
    of the pipeline; the auto-filled `meta.listing-item`
    keys are present in both runs and don't affect render
    output (no consumer yet). Add a *new* assertion to that
    test if needed: assert the at-profile profile's
    `listing_item.description` is non-`None` for the test
    fixture.

### Snapshot tests

None for L1. No render output changes. **Any snapshot diff
during workspace nextest is a red flag** — investigate per
CLAUDE.md.

### End-to-end CLI verification

Per CLAUDE.md §"End-to-end verification before declaring
success":

- Run `cargo run --bin q2 -- render <fixture>.qmd` on three
  existing fixtures from `crates/quarto-core/tests/fixtures/`.
  Output **must** be byte-identical before and after L1.
  Auto-filled `listing_item` data has no consumer yet.
- Render one new fixture under
  `crates/quarto-core/tests/fixtures/listings-l1/` containing
  a paragraph + image + no `listing-item:`; capture the
  rendered output's MD5 (should equal the same fixture
  without L1, because the listings consumer doesn't exist).
- Inspect the *profile* indirectly by running the
  `pipeline_listing_item_autofill_end_to_end` integration
  test and recording the assertion outcomes in the L1
  completion note. Profiles aren't user-visible yet; this
  is the closest equivalent of "did the feature work."

## Implementation steps

### Preparation

- [ ] Re-read
      `claude-notes/instructions/testing.md` and
      `coding.md`.
- [ ] Create a worktree under `.worktrees/listings-l1/` per
      `.claude/rules/worktrees.md` (branch
      `beads/bd-izqh-listing-item-info-stage`).
- [ ] `npm install` from worktree root.
- [ ] `cargo xtask verify --skip-hub-build` baseline.
- [ ] **Confirm `metadata_normalize::inlines_to_plain_text`
      visibility.** Make it `pub(crate)` (or export via
      `crate::transforms`); add the one-line doc-comment
      noting L1 is the second consumer and pointing at
      `bd-zzke` for any future third consumer. **Do not**
      audit or consolidate the other five sites — that is
      `bd-zzke`'s job, deliberately deferred.
- [x] **`ConfigValue` mutation idioms — cleared 2026-05-06.**
      `ConfigValue::insert_path` (auto-creates intermediate
      maps), `contains_path`, `get_path`, `get_path_mut` are
      already public. No new public API on `ConfigValue` is
      required; `fill_if_absent_*` is a thin local helper
      around `contains_path` + `insert_path`.
- [x] **`StageContext` runtime access — cleared 2026-05-06.**
      `pub runtime: Arc<dyn SystemRuntime>` is already a public
      field on `StageContext`
      (`crates/quarto-core/src/stage/context.rs:55`). No
      plumbing task; the stage reads `ctx.runtime` directly.

### TDD phase — tests first, observe failures

- [ ] Add `ListingItemInfoStage` skeleton with a no-op
      `autofill_listing_item` so tests compile.
- [ ] Write unit tests 1–16 in
      `crates/quarto-core/src/stage/stages/listing_item_info.rs`'s
      test module. Run; observe expected failures.
- [ ] Write stage trait tests 17–18.
- [ ] Write integration tests 19–21 in
      `crates/quarto-core/tests/document_profile_pipeline.rs`.
      Test 21 extends the existing
      `pipeline_at_profile_to_end_produces_expected_html`
      assertion list.

### Implementation

- [ ] Implement `compute_description`, `first_image_src`,
      `word_count`, `div_ceil`, `mtime_iso` (using
      `SystemRuntime::path_metadata`; no `#[cfg]` gate).
- [ ] Implement `autofill_listing_item` with the
      "fill-if-absent" mutation pattern decided during the
      audit.
- [ ] Wire the stage into both
      `build_html_pipeline_stages_with_apply_config` and
      `build_wasm_html_pipeline`.
- [ ] Update the pipeline-builder doc comments to list the
      new stage.
- [ ] Run unit + stage + integration tests; all 21 must
      pass.

### Documentation

- [ ] Update `claude-notes/designs/document-profile-contract.md`:
      add a paragraph note under the `listing_item` row
      indicating "auto-filled by `ListingItemInfoStage` for
      `description`, `image`, `word_count`,
      `reading_time_minutes`, `date_modified` when the
      author leaves them unset" with a cross-link to this
      sub-plan. **No version bump** — the field shape
      doesn't change; only its production path does.
- [ ] Add a doc comment on `ListingItemInfoStage` pointing
      at this sub-plan.
- [ ] If a consolidation follow-up was filed during the
      audit, link it from this sub-plan and from the
      epic's "Resolved decisions" log.

### Verification and close-out

- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace`. Any snapshot diff is
      a red flag — investigate.
- [ ] `cargo xtask lint` passes.
- [ ] `cargo xtask verify` (full, including hub-client)
      passes. Critical: hub-client's WASM build picks up
      the new stage; the WASM mtime gate must work.
- [ ] End-to-end CLI MD5 hashes recorded for three
      existing fixtures + listings-l1 fixture.
- [ ] Stop and request user permission before any push.
- [ ] `br update bd-izqh --status closed` after approval.
- [ ] `br sync --flush-only && git add .beads/ && git commit`
      from the **main repo**.

## Risks and mitigations

- **Risk: `ConfigValue` doesn't expose mutation cleanly.**
  *Mitigation:* the audit step is a hard checkpoint. If the
  type lacks an `as_map_entries_mut`-style API, **stop and
  ask the user** before adding one. Adding mutation to a
  shared type is a contract decision.
- **Risk: WASM build breaks because `time` or `std::fs` is
  unavailable.** *Mitigation:* `std::fs` is no longer used —
  L1 routes mtime through `SystemRuntime::path_metadata`,
  which already has a working WASM impl. `time = "0.3"` (per
  D12) is `no_std`-friendly and already used by `quarto-hub`;
  no known WASM blockers. The full `cargo xtask verify`
  (which includes the WASM hub-client build) catches anything
  else.
- **Risk: existing snapshots move because of a subtle ordering
  effect of the new stage.** *Mitigation:* L1 doesn't mutate
  AST blocks, only metadata not consumed by render today. If
  snapshots move, investigate per CLAUDE.md before proceeding.
- **Risk: `metadata_normalize::inlines_to_plain_text`'s
  decision to recurse into footnotes and to wrap
  `Inline::Quoted` in quote characters surprises listings
  consumers later.** *Mitigation:* L1's word-count walker
  strips notes before passing inlines to the helper (so
  reading time excludes footnote prose, matching Q1).
  Quote-character wrapping appears in description-preview
  output but matches what authors see in their rendered
  document; if L3 finds it wrong for listings specifically,
  L3's sub-plan re-opens the choice. The other five
  variants stay untouched per `bd-zzke`.
- **Risk: idempotence broken by a "create-if-absent" idiom
  that subtly recreates the value on repeat.** *Mitigation:*
  test 14 enforces idempotence; if it fails, the fix is in
  the stage, not the test.
- **Risk: byte-identical regression test
  (`pipeline_at_profile_to_end_produces_expected_html`)
  diverges.** *Mitigation:* L1 runs *before* the
  checkpoint. The post-checkpoint AST blocks are unchanged
  (we only touched `meta.listing-item`). The HTML render
  doesn't read `meta.listing-item`. If this test
  diverges, the most likely cause is a metadata-read at
  render time we didn't expect; investigate before
  proceeding.
- **Risk: handoff item 8 — `b"…\n…"` continuation eats
  whitespace in test fixtures.** *Mitigation:* this
  sub-plan acknowledges; tests 1–18 use plain newlines in
  YAML fixtures (no `\` line continuations). Reviewers
  watch for it.

## Explicit non-goals for this phase

- **No L7 placeholder substitution.** L1 produces the
  *fallback* values that L7 may later upgrade with engine-
  rendered content.
- **No `listing:` schema or transform.** L2 / L3.
- **No reading from `profile.listing_item` outside the
  listings code path.** Reviewer discipline per L0's
  §"Scoped feature surfaces."
- **No author-preview-image discovery from rendered
  content.** Static-AST first-image is L1's scope; L7 may
  upgrade.
- **No filtering of shortcode-bearing image `src` (D13).**
  Tracked separately as `bd-8h9o`; L1 stores whatever
  `target.0` carries.
- **No date parsing / re-formatting.** `date_modified` is
  raw mtime as ISO `YYYY-MM-DD`. Render-time formatting
  is the listing template's job.
- **No `categories` auto-fill in L1.** D7 from L0 stores
  the tagged form; L1 does not duplicate
  `profile.categories` into `listing_item.categories`.
  The merge happens at the listings consumer (L3+), per
  L0 handoff item 2.

## Decisions log

- **D1 (mechanism):** pre-checkpoint metadata enrichment via
  `meta.listing-item` mutation, **not** a side-channel on
  `DocumentAst`. Rationale: §"Why metadata enrichment, not a
  side-channel."
- **D2 (location):**
  `crates/quarto-core/src/stage/stages/listing_item_info.rs`,
  per epic decision 6.
- **D3 (constants):** `WORDS_PER_MINUTE = 200` (matches Q1).
  `MAX_DESCRIPTION_LEN` was previously listed here at 175;
  superseded by D11 — L1 stores the full first paragraph and
  L3 owns truncation.
- **D4 (reading time):** ceiling division. 1-word post →
  "1 min" not "0 min."
- **D5 (image walk):** static AST only, document order, no
  scoring. First image with non-empty target wins. Engine-
  generated images are L7's job.
- **D6 (word-count zero):** `Some(0)` → `None`. Empty doc
  reports unset, not zero. Avoids "0-minute read" listings.
- **D7 (date-modified format):** `YYYY-MM-DD` ISO date,
  not full datetime. Listings display dates, not times.
  Render-time formatting is the template's job.
- **D7b (mtime backend abstraction):** L1 reads via
  `SystemRuntime::path_metadata().modified`. Native impl
  returns filesystem mtime; WASM impl returns `None` until
  `bd-a3we` lands. The "different backend, different
  semantics" question (filesystem mtime vs. Automerge
  change-history time) is encapsulated *inside the runtime
  trait*, not in L1's stage. No `#[cfg]` gate on L1; no
  L1 code change required when `bd-a3we` resolves.
- **D8 (no `categories` auto-fill):** L1 does not write
  `categories` into `listing-item`. The L0 D7 decision
  preserves `categories_raw` on both `profile` and
  `profile.listing_item`; merge is the listings
  consumer's job.
- **D9 (idempotence):** required, tested at 14.
- **D10 (helper consolidation):** out of scope for L1.
  Tracked as `bd-zzke` (chore, P3). L1 reuses
  `metadata_normalize::inlines_to_plain_text` directly;
  the other five sites are not L1's concern. Reasoning:
  the six in-tree variants don't share semantics, so a
  consolidating refactor needs an options-driven helper +
  per-site audit + snapshot-diff investigation, which is
  a separate hygiene project.
- **D11 (no truncation in L1; 2026-05-06):** L1 stores the
  *full* first-paragraph text in
  `meta.listing-item.description`. L3 truncates at render
  time using the listing host's `max-description-length`.
  Rationale: L1 doesn't know the listing host's per-listing
  config, and pre-truncating to 175 chars would silently
  cap a listing that requested a longer preview. Q1
  truncates at `completeListingItems` time too; this aligns
  with that ordering.
- **D12 (datetime crate is `time`, not `chrono`; 2026-05-06):**
  Add `time = "0.3"` to `quarto-core`. `time` is already in
  use by `quarto-hub`; adopting it in `quarto-core`
  consolidates rather than introducing a new crate choice.
  L9 (RSS feeds) needs RFC 822 dates and reuses the same
  crate. Earlier-draft references to `chrono` were factually
  wrong — `chrono` is not a workspace dependency.
- **D13 (shortcode-bearing image src out of scope; 2026-05-06):**
  L1 does not filter images whose `src` carries an
  unresolved shortcode (e.g. `{{< meta thumb >}}.png`). L1
  runs after `IncludeExpansionStage` but before
  `PreEngineSugaringStage`, so such images are present in
  `target.0` as literal text. `bd-8h9o` (discovered-from
  `bd-izqh`) tracks the dedicated investigation. Until
  resolved, listings consuming `meta.listing-item.image`
  (L3+) may surface unresolved shortcode markup; the
  user-flagged correctness of this is acceptable for v1.
- **D14 (`?Send` async traits; 2026-05-06):** All async
  traits in `quarto-core` (and downstream) use
  `#[async_trait(?Send)]`. The same trait must compile for
  native and WASM (single-threaded), and several captured
  types in WASM aren't `Send`. Codified in
  `.claude/rules/wasm.md` so future work doesn't drift.

## Open sub-questions (defer; do not block L1)

- ~~**Plain-text helper consolidation.**~~ Resolved:
  `bd-zzke` filed as a P3 chore; L1 reuses
  `metadata_normalize::inlines_to_plain_text` without
  consolidating. See §"Plain-text helper choice".
- ~~**`ConfigValue` mutation API.**~~ Resolved 2026-05-06.
  `insert_path` and `contains_path` already exist; no public
  API extension needed. L1's `fill_if_absent_*` is local.
- ~~**Listings-aware default for `MAX_DESCRIPTION_LEN`.**~~
  Resolved 2026-05-06 (D11). L1 stores the full paragraph;
  L3 owns truncation.
- **`reading_time_minutes` for very long docs.** No upper
  cap. A 100k-word document reports `500`. Fine.

## Filing reminder

This sub-plan corresponds to `bd-izqh`. After implementation:

1. Update the issue with status `closed` plus a one-line
   reason linking back to this plan.
2. `br sync --flush-only && git add .beads/ && git commit`
   from the main repo.
3. Add bd ids and resolved follow-ups to the bottom of
   this sub-plan in a "Follow-ups filed" section before
   closing.
4. Send a handoff message to the orchestrator covering:
   - Whether the `metadata_normalize::inlines_to_plain_text`
     visibility change introduced any cross-crate ripple
     (it shouldn't — same crate as L1's stage).
   - Any contract-doc edits beyond the planned `listing_item`
     row note.
   - Whether `StageContext` already exposed the
     `SystemRuntime` cleanly or required plumbing. If
     plumbing was needed, name the change so L2/L3
     plans can rely on it.
   - Reminder: the `bd-a3we` follow-up (Automerge VFS
     mtime) stays open. L1 changes nothing about the
     hub-client experience for `date_modified` — it
     still reads `None` until `bd-a3we` lands.
   - Profile-cache invalidation observation: did
     `cargo nextest` re-derive any cached profiles? If
     so, that's the expected v4-cache-rebuild pattern.
   - Confirmation that no work was done on `bd-zzke`
     (consolidation chore) — it stays open for a future
     dedicated session.
