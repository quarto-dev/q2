# L1 — `ListingItemInfoStage` (auto-fill, pre-checkpoint)

**Date:** 2026-05-05
**Beads:** `bd-izqh`. Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessor:** L0 (`bd-n8a4`, closed) — adds the
`DocumentProfile.listing_item: ListingItemInfo` field and the
`profile.categories_raw: Option<ConfigValue>` field. L1 *populates*
that field at the metadata layer; L0 *reads* it at extraction time.
**Status:** Draft. Awaiting implementation.

## Goal of this phase

Add a new pipeline stage between `IncludeExpansionStage` and
`DocumentProfileStage` that **enriches `meta.listing-item`** with
auto-derived values when the author hasn't supplied them. The stage:

1. **Reads** `ast.meta.listing-item.*` (current author-supplied
   values), `ast.blocks` (post-include AST for word-count /
   first-paragraph / first-image), and the source-file path
   (filesystem mtime for `date-modified`).
2. **Computes** any missing curated fields:
   - `description` — first plain-text paragraph from `ast.blocks`,
     truncated.
   - `image` — first `Inline::Image` `target.0` from `ast.blocks`
     (document order).
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
- Plain-text extraction precedent:
  `crates/quarto-core/src/transforms/metadata_normalize.rs`
  line 110 (`inlines_to_plain_text`). Five other modules
  re-implement variants; before adding a sixth, evaluate
  whether to lift a single shared helper into
  `quarto-pandoc-types` or `quarto-util`. See §"Open
  sub-question on plain-text helper" below.
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
fn autofill_listing_item(doc: &mut DocumentAst, _ctx: &StageContext) {
    // Compute candidate fill-ins from the AST (no I/O for these).
    let blocks = &doc.ast.blocks;
    let cand_description = compute_description(blocks, MAX_DESCRIPTION_LEN);
    let cand_image       = first_image_src(blocks);
    let cand_word_count  = word_count(blocks);
    let cand_reading     = cand_word_count.map(|w| div_ceil(w, WORDS_PER_MINUTE));

    // mtime is the only I/O; gracefully degrade if it fails.
    let cand_date_modified = filesystem_mtime_iso(&doc.path);

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

The shapes `ensure_map_entry`, `fill_if_absent_string`, etc. are
**not** implied to exist — they're the API L1 wants. The
implementation work is figuring out the right idiom against
`ConfigValue` (see §"`ConfigValue` mutation idioms" below).

### Constants

```rust
const WORDS_PER_MINUTE: u32 = 200;
const MAX_DESCRIPTION_LEN: usize = 175;  // matches Q1 default
```

`MAX_DESCRIPTION_LEN` is the L7-bracketed Q1 constant; if the
author sets `listing-item.description` explicitly, no truncation
applies. If the author sets a custom listing's
`max-description-length` (L3+ schema work), that override applies
at *render* time, not here. L1 only emits a sane default the L1
fallback can use.

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

### Plain-text and image extraction

For each helper function, decide at implementation time whether
to:

1. **Reuse** an existing helper if its semantics match L1's
   needs exactly. Candidates: `inlines_to_plain_text` in
   `crates/quarto-core/src/transforms/metadata_normalize.rs`
   (line 110), `inlines_to_plain_text` in
   `crates/quarto-core/src/transforms/title_block.rs` (line
   144), `inlines_to_text` in
   `crates/quarto-core/src/template.rs` (line 626),
   `inlines_to_plain_text` in
   `crates/quarto-pandoc-types/src/config_value.rs` (line 22).
2. **Lift** one of them into a single shared helper before L1
   adds a sixth duplicate. Most likely placement:
   `quarto_pandoc_types::inline::Inlines::to_plain_text()` or
   `quarto_util::inlines_to_plain_text(&[Inline]) -> String`.
3. **Write** an L1-specific helper if the existing ones differ
   in subtle ways (handling of math, code, raw-inline, link
   text, image alt) that don't match L1's needs.

**Recommendation:** spend 30 minutes auditing the four duplicates
during L1's TDD setup. If they agree on Inline coverage, lift to
a shared module. If they disagree, document the disagreement,
file a follow-up bd issue ("consolidate inlines_to_plain_text
duplicates"), and L1 picks the closest match (probably
`metadata_normalize`'s).

### Helper specifications

#### `compute_description(blocks: &[Block], max_len: usize) -> Option<String>`

- Walk `blocks` in document order; find the first
  `Block::Para(p)` whose `inlines_to_plain_text(&p.content)`
  produces a non-empty string after `trim()`.
- Truncate to `max_len` characters using a word-boundary-safe
  truncation (Q1 uses `truncateText(s, n, "space")`). If a
  helper for word-boundary truncation already exists in
  `quarto-util` or similar, reuse; otherwise write a tiny one
  and consider it the smaller half of the helper-consolidation
  follow-up.
- If no paragraph is found, return `None`. Empty documents,
  documents starting with a heading and no paragraphs, etc.
  produce `None` and the field stays unset.
- If the truncated text would end mid-word, end at the last
  whole word and append `…`. (Q1 behavior; non-essential but
  keeps the comparison easy.)

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

- Concatenate `inlines_to_plain_text` of every `Block::Para`,
  `Plain`, `Header`, `BlockQuote`, etc. — same container set as
  `first_image_src`.
- Tokenize on whitespace runs (`split_whitespace().count()`).
  This matches Q1's `estimateReadingTimeMinutes` closely
  enough; Q1 uses a regex that's nearly equivalent.
- Return `None` if the count is `0` (empty document — let the
  caller leave the fields unset rather than report 0). Return
  `Some(n)` for any `n >= 1`.

#### `div_ceil(numerator: u32, denominator: u32) -> u32`

`(numerator + denominator - 1) / denominator`. Use
`u32::div_ceil` if the MSRV permits. Ceiling so a 1-word post
reports "1 min" not "0 min."

#### `filesystem_mtime_iso(path: &Path) -> Option<String>`

```rust
fn filesystem_mtime_iso(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
    Some(datetime.format("%Y-%m-%d").to_string())
}
```

`chrono` is already a workspace dependency (used elsewhere in
`quarto-core`); confirm in the worktree before adding any new
crate. If `chrono` isn't available, fall back to manual
`SystemTime` → seconds-since-epoch → date arithmetic, but check
first.

WASM consideration: `std::fs::metadata` works under
`wasm32-wasip1` (the WASI flavor we use); under
`wasm32-unknown-unknown` it doesn't exist. The hub-client's
WASM build uses `wasm32-unknown-unknown`. Two options:

1. **Gate the mtime read with `#[cfg(not(target_arch = "wasm32"))]`**
   per `.claude/rules/wasm.md`. WASM builds skip mtime, leave
   `date_modified` unset. Author can supply explicitly if
   needed in a hub-client preview context. Recommended.
2. Read mtime via `quarto-system-runtime` (which already
   handles native/WASM split for fs operations).

**Recommendation:** Option 2 if `quarto-system-runtime` exposes
mtime; Option 1 otherwise. Verify in the worktree.

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
   the description; truncated to ≤175 chars (use a paragraph
   ≥175 chars to exercise truncation).
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
12. **`autofill_date_modified_native`** — write a fixture
    file with a known mtime (use `filetime::set_file_mtime`),
    run the stage, assert `date_modified` matches
    `YYYY-MM-DD` of that mtime. **Native only**;
    `#[cfg(not(target_arch = "wasm32"))]`.
13. **`autofill_date_modified_skipped_when_path_missing`** —
    `doc.path` points at a nonexistent file; `date_modified`
    stays `None` (no panic, no error). Tests the
    graceful-degradation contract.
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
- [ ] **Audit the four `inlines_to_plain_text` duplicates**
      (`metadata_normalize`, `title_block`, `template`,
      `config_value`). Decide reuse / lift / write. If lift,
      file the consolidation as a separate bd follow-up
      issue and **don't block L1 on it** — pick one
      duplicate to reuse for now.
- [ ] **Audit `ConfigValue` mutation idioms** in
      `crates/quarto-pandoc-types/src/config_value.rs`. If
      mutation requires a contract-doc-level decision (e.g.
      adding `as_map_entries_mut`), stop and ask the user.
- [ ] **Audit WASM-fs availability for mtime.** Check
      `quarto-system-runtime` for an mtime accessor. If
      absent, gate the mtime read behind `#[cfg(not(target_arch =
      "wasm32"))]` per `.claude/rules/wasm.md`.

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
      `word_count`, `div_ceil`,
      `filesystem_mtime_iso` (with WASM gate per audit
      decision).
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
- **Risk: WASM build breaks because `chrono` or `std::fs` is
  unavailable.** *Mitigation:* the WASM-fs audit step;
  `cargo xtask verify` (full) catches it.
- **Risk: existing snapshots move because of a subtle ordering
  effect of the new stage.** *Mitigation:* L1 doesn't mutate
  AST blocks, only metadata not consumed by render today. If
  snapshots move, investigate per CLAUDE.md before proceeding.
- **Risk: `inlines_to_plain_text` reuse picks a variant whose
  edge-case behavior surprises listings consumers later.**
  *Mitigation:* audit step documents the disagreement; L3's
  sub-plan re-checks the choice when listings actually
  render.
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
- **D3 (constants):** `WORDS_PER_MINUTE = 200` (matches Q1),
  `MAX_DESCRIPTION_LEN = 175` (matches Q1 default).
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
- **D8 (no `categories` auto-fill):** L1 does not write
  `categories` into `listing-item`. The L0 D7 decision
  preserves `categories_raw` on both `profile` and
  `profile.listing_item`; merge is the listings
  consumer's job.
- **D9 (idempotence):** required, tested at 14.
- **D10 (helper consolidation):** out of scope for L1
  unless the audit reveals existing helpers can't be
  reused safely. File a follow-up bd if so.

## Open sub-questions (defer; do not block L1)

- **Plain-text helper consolidation.** Five copies in tree.
  L1 picks one to reuse and files a follow-up bd for the
  consolidation. Not L1's job to fix.
- **`ConfigValue` mutation API.** If the existing API is
  awkward, L1 may need a thin builder helper *inside the
  L1 module* rather than extending `ConfigValue`'s public
  surface. Sub-plan-level call; ask the user before
  extending the public API.
- **Listings-aware default for `MAX_DESCRIPTION_LEN`.**
  Q1's default is 175; L1 uses that. If L3's listing
  config later wants a per-listing override that
  *re-truncates* a longer L1-saved description, the host
  page's render can do that. Don't second-guess in L1.
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
   - Any new shared helpers added (or follow-ups filed for
     consolidation).
   - Any contract-doc edits beyond the planned `listing_item`
     row note.
   - Whether the WASM mtime gate landed in
     `quarto-system-runtime` or as a `#[cfg]` in the L1
     stage. L4's sub-plan will need to know.
   - Profile-cache invalidation observation: did
     `cargo nextest` re-derive any cached profiles? If
     so, that's the expected v3-cache-rebuild pattern.
