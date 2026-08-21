# Provenance, Plan 3 of 3: fix the remaining instances

**Epic:** `bd-mxa44voa`.
**Findings:** `claude-notes/research/2026-08-21-provenance-audit-findings.md`.
**Read § 1–2 of it before touching anything here** — they carry the accessor
rule this plan applies everywhere, and the reason the same mistake has been made
five times.
**Siblings:** Plan 1 = `2026-08-20-provenance-1-foundations.md`, Plan 2 =
`2026-08-20-provenance-2-consumers.md` — both **alongside this file** and
current, since all three merged onto `feature/yaml-provenance` (integration
order 1 → 2 → 3, so this plan sits on top of both). Earlier revisions of this
file told you to read them from `.worktrees/workspace-1` / `-2`; those were
review worktrees, are no longer authoritative, and may be gone.

**Absorbed strands:** `bd-gx2mal69` (comrak `NodeValue::Text`), `bd-x0o0pem3`
(the `quarto.config.md` Lua path — **answered: inert**).

## Read this first

**The audit is done.** It was this plan's original content; it now lives in the
findings doc, phase by phase. What remains here is implementation. Do not
re-derive a finding — check it against its citation and say so if you disagree.

**Two upstream releases, not one.** `quarto-source-map` **0.1.2** carries the
four behavior fixes (including `preimage_in`'s blanket-`None`);
**`ProvenanceBuilder` ships separately in 0.1.3**, after Plan 1's Phase 2 drives
it. So Phase 1 needs 0.1.2 and Phase 6 needs 0.1.3 — do not read one gate for
both.

**Start today, before either release:** Phases 2, 3, 4, 5, all of Phase 1 except
its last two items, and all of Phase 7 except the Plan 2 cross-check. Comments,
tests and one classification pass against code that exists now.

| gated item | needs |
|---|---|
| Phase 1, "confirm blanket-`None` is regression-free" | **0.1.2** consumable by q2 |
| Phase 1, the doc-comment citation | Plan 1's Phase 1 doc rewrite (still open) |
| Phase 6, all implementation items | **0.1.3** — `ProvenanceBuilder` released |
| Phase 6, before writing any code | **read Plan 1 § The shared builder** — `ProvenanceBuilder`'s signature lives only there, so Phase 6 is not implementable from this plan alone |
| Phase 1, the **plain-scalar** fold fixture | Plan 2 Phase 3 landed — a fold `Concat` only reaches an emitted body byte range once q2 consumes `content_source_info` |
| Phase 1, the **attribute** variant of the same fixture | Plan 2 Phase 4 landed |
| Phase 7, cross-checking Plan 2's dispositions | Plan 2 Phases 3–4 landed |

To consume 0.1.2 before it publishes, use a local override — the upstream
checkout is `~/src/quarto-source-map`, and Plan 1's Phase 1 uses the same
mechanism:

```toml
[patch.crates-io]  # LOCAL DEV ONLY — do not commit
quarto-source-map = { path = "/Users/gordon/src/quarto-source-map" }
```

## Test seam spec (frozen — bind before dispatch)

Every test below is bound to the **exact production hunk whose revert reddens
it**. Once a test is green, its assertions and harness are frozen; never edit
one to go green. Three of the five originally specced tests failed this check
and are corrected here — see § Vacuity findings.

| id | tier | real unit mounted | seam: mount → events → assertion surface | mock boundary | **revert hunk → RED** |
|---|---|---|---|---|---|
| **T1** | integration, in-process | `quarto_core::pipeline::capture_untransformed_ast_json` | call it on a fixture that **has** config-derived body content (a `_quarto.yml` with a navbar `text:` containing markdown) → parse the returned JSON → assert **every** `astContext.p` entry is wire-code `Original` with the document's own `file_id` | none — real parse, real serializer | (a) `pipeline.rs:1013` `parent_source_info: None` → `Some(_)` ⇒ entries become `Substring` (code 2) ⇒ RED. (b) move the `:920` call below `run_pipeline` ⇒ entries appear with code 4 `Generated` ⇒ RED |
| **T2** | e2e, real binary | `q2 render` | fold-shaped `_quarto.yml` (`aaa`⏎`bbb` plain scalar) → render → assert emitted bytes are the **content** (`aaa bbb`) not the **source** (`aaa\nbbb`) | none | whichever newly-classified copy site is fixed; **only write T2 if Phase 1's classification finds one.** If all 24 are `locate`, T2 has no hunk and must not be written |
| **T3** | unit, in-crate | `comrak_to_pandoc::empty_source_info` | convert a node with no location → assert `matches!(si, SourceInfo::Generated { .. })` | none | `lib.rs:31` back to `SourceInfo::original(FileId(0), 0, 0)` ⇒ RED |
| **T5** | unit, in-crate | `comrak_to_pandoc` `Text` conversion + `ProvenanceBuilder` | `aa\*bb cc &amp; dd ee` → convert → assert `map_offset(0)` of the **`dd`** and **`ee`** `Str`s resolves to **16** and **19** | none | the lockstep walker → back to `base_offset + byte_idx` (`text.rs:90-140`) ⇒ `dd` resolves to 11 ⇒ RED |
| **T7** | unit, in-crate | `quarto_core::crossref::codeblock_shorthand` | a cell whose entire body is the word `python`, fenced ```` ```{python} ```` → assert the resolved span points at the **body**, not into `{python}` | none | the `map_offset` pair → back to `block_text.find(&cb.text)` (`:486`) ⇒ span lands in the info string ⇒ RED |

### Not regression tests — labelled, not smuggled in

Two items the plan listed as tests have **no q2 hunk to revert**. They are
legitimate but must not be counted as guarding anything:

| id | what it actually is | why no hunk |
|---|---|---|
| **T4** (Phase 5) | a **characterization probe** | Its own checklist says "if it goes red, file a strand" — i.e. it exists to *discover* whether a writer-provenance defect exists, not to guard a fix. If it goes green it guards nothing. Run it, record the result, keep it `#[ignore]`d if red. |
| **T6** (Phase 6 blockquote) | an **upstream-behavior pin** on comrak | Nothing in q2 makes drift reset at `SoftBreak`; comrak's per-line `Text` nodes do. Its "revert" is a comrak version bump. Keep it, and say so in the test name/comment, so nobody reads it as covering our code. |

### Vacuity findings — three corrections

**T6 was vacuous as originally specced, and it is the sharpest catch here.**
The plan said "assert drift resets at `SoftBreak`". Measured: pre-fix, line 2's
`dd` already reports **14..16, which is correct** — resetting is exactly what it
does. So `assert dd == 14..16` **passes before and after the fix** and survives
its own revert. The discriminator on line 2 is `ee`: pre-fix 19..21, true 23..25.
So the corrected assertion is *`dd` correct **and** `ee` correct*, and only the
`ee` half discriminates.

**T5's discriminator does not exist before the first replacement.** Pre-fix,
`aa*bb` reports 0..5 and `map_offset(0)` → 0, which is *right*; the drift only
accumulates after the first escape. A test asserting the first token passes
without the fix. Hence T5 asserts **`dd` and `ee`**, never `aa*bb` alone.

**T1's assertion (b) is vacuous without config-derived content in the fixture.**
If the fixture has no navbar/footer/title markdown, moving the capture below the
stages injects nothing and the pool stays all-`Original` — green without the
invariant. Hence the seam requires a `_quarto.yml` whose `text:` contains
markdown, so a transform *would* splice a `Generated`-carrying node in.

### Missing-test pass

Behaviour with no test, either specced or explicitly accepted:

- **Phase 4's Lua inertness is unguarded and is bindable — spec it.** Three
  independent grounds hold today, and nothing notices if one fails. Add
  **T8**: unit, in-crate; call `quarto.config.md("x")` through a Lua filter →
  assert the resulting node's `SourceInfo` gives `map_offset(0, ctx) == None`.
  Revert hunk: attach an `Invocation` anchor in `filter_source_info`
  (`types.rs:2291`) or add a production `append_anchor` ⇒ still `None` for
  `map_offset`, so **assert `resolve_byte_range() == None` as well** — that one
  reddens. This is the guard the plan's Phase 4 comment asks a reader to trust.
- **Accepted untested, with rationale:**
  - *Phase 2's drift amplifiers.* An ordering constraint on future work, not a
    behaviour. A comment is the only enforceable artifact.
  - *Phase 3's revived `parse_with_parent`.* Deliberately unguarded — we
    rejected a lint rule because the function is dead (§ 5). If it is ever
    revived, its doc comment is the warning; there is no live path to test.
  - *Phase 1's `postprocess.rs:660` combine-fallback.* Its success condition is
    "state whether any snapshot moves". If none move there is nothing to assert,
    and inventing an assertion would be theater.

## Phase 1 — `preimage_in` consumers

Findings: § 3. **Verdict: latent, not live** — the wrong-bytes path through
`incremental.rs:171` is closed by the shape of the preview capture, not by
anything about `preimage_in`. That is why this phase ships a **guard**, not a
fix: the site is not broken; what is missing is anything that would notice if
the invariant moved.

- [ ] **Classify the 24 unclassified calls** as **locate** (computes a position,
      compares identity, or bounds a search) or **copy** (slices source text
      that is then emitted). The enumerated call list is in § 3. Output: this
      table appended to § Evidence — the deliverable even if every answer is
      "locate".

      | file:line | locate / copy | what it does with the range |
      |---|---|---|
      | `incremental.rs:171` | copy | slices `original_qmd` → `CoarsenedEntry::Verbatim` |
      | `postprocess.rs:660` | locate | min/max span over a run |

      Plan 1's hypothesis, to test rather than assume: the split falls along the
      incremental-writer / span-computation line, with `incremental.rs`'s
      `Verbatim` arms the only copies.
- [ ] **Guard the invariant that makes the copy site safe.** One test, because
      one assertion covers both failure modes: **every `SourceInfo` in the
      captured baseline pool must be an `Original` rooted at the document's own
      `FileId`.** A parent threaded into the baseline parse makes them
      `Substring`; a transform-injected node carries `Generated`
      (`shortcode_resolve.rs:1175`, `appendix.rs`, `footnotes.rs`,
      `title_block.rs`) or a foreign file id. So the single pool-shape assertion
      catches both "someone added a parent" and "the capture moved after the
      stages" — which is why it beats asserting source order, a claim about a
      function body rather than about a value.
      The failure message must name **both** causes and point at
      `incremental.rs:171`, so whoever trips it lands on the copy site rather
      than on the capture. Note in the test that this invariant load-bears for
      provenance correctness while living in `quarto-core`, which neither
      `quarto-source-map` nor `quarto-yaml` owns.
- [ ] **Fix the call-site comment at `incremental.rs:162-169`.** It asserts the
      byte-identity reading Plan 1 is retracting upstream, so once 0.1.2 lands
      the codebase asserts both readings — worse than asserting only the wrong
      one. (Plan 1's ninth hand-off obligation, `7d799d623`.) Say the `.get()`
      guard checks **bounds, not identity**, and that the arm is safe only
      because the baseline AST is untransformed and parent-less.
- [ ] **Failing test first, for any *newly* discovered copy site:** a
      fold-shaped end-to-end fixture (`aaa`⏎`bbb` as a plain scalar) driven
      through the real binary, asserting the emitted bytes are the *content* and
      not the *source*. Observe red before fixing.
- [ ] **Confirm Plan 1's 0.1.2 blanket-`None` is regression-free.** Two shapes:
      **(a)** `postprocess.rs:660` documents at `:651-652` that it *relies* on
      `preimage_in` returning `Some(hull)` for a contiguous `Concat`, falling
      back to `combine(first, last)` on `None`. **Success condition:** state, in
      § Evidence, whether that fallback moves any existing snapshot, and if so
      which. If none move, say so — that is the answer. (Do not cite the
      `:1845-1852` module doc as evidence of harm: that bug was
      `combine(self, self)` specifically.)
      **(b)** `cell_options` is the one production `Concat` producer with
      length-matched pieces, but multi-option cells are *gappy*
      (`option_content_ranges` returns `content_start..line.len()`, skipping the
      next line's `#| ` prefix — `cell_options/mod.rs:250-263`), so they already
      return `None`. Only a **single-option cell** yields one piece with a hull
      that blanket-`None` removes. Plan 1 asserts both shapes in its own
      Phase 1; confirm the q2 side agrees.
- [ ] **Gated on Plan 1: cite the corrected `preimage_in` doc comment.** Plan 1
      has agreed to rewrite it (`source_info.rs:410-413`) *before* the 0.1.2
      release, because this phase's write-up depends on the wording — but the
      item is still open in Plan 1's Phase 1 and the old text is still live.
      Read the current wording from `~/src/quarto-source-map`; do not quote a
      remembered replacement.

## Phase 2 — the `SourceInfo::original(` surface

Findings: § 4. **All 17 production sites triaged; no further triage needed.**
Five are Phase 6's comrak defect; the rest are safe by shape or are the three
drift amplifiers below.

- [ ] **Comment the three drift amplifiers** (`postprocess.rs:317`, `:669`,
      `:1833`) with the ordering constraint from § 4: fix producers before these
      consumers, or the fix silently does not reach the output. Record it in the
      code; do not restructure.
- [ ] Separately at `postprocess.rs:1833`: note the hardcoded `attr_end + 1`
      assumption in the same comment.
- [ ] **Discharge Plan 1's hand-off** — its Phase 1 audit shipped no fixes
      outside `quarto-source-map`. Examine `offset_to_location_bytes`
      (`quarto-parse-errors/src/error_generation.rs:330`, a documented
      "bytes-aware sibling") plus `quarto-yaml`'s own `Location` uses
      (`~/src/quarto-yaml`). Plan 1 measured the two `offset_to_location`
      implementations in `quarto-source-map` disagreeing by one column for a
      mid-character offset; a third with its own rule is the same hazard.
      **Output:** for each, one line in § Evidence stating what it returns for a
      mid-character offset — floored, ceiled, raw, or overcounted — and whether
      that agrees with `FileInformation::offset_to_location` after Plan 1's fix.
      **Routing:** a q2-side disagreement is fixed here; a `quarto-yaml`-side
      one is **out of scope** — file a strand and notify Plan 1, which owns that
      release.
- [ ] **Change** `comrak-to-pandoc/src/lib.rs:31`'s `empty_source_info()` from
      `SourceInfo::original(FileId(0), 0, 0)` to a `Generated`, so "no location"
      stops being indistinguishable from "start of file 0" — the shape
      `span_assert` flags as `SpanProblem::SuspiciousDefault`
      (`quarto-config/src/span_assert.rs:152-157`). Out of this bug class but
      cheap and adjacent. **Expect snapshot movement** in `comrak-to-pandoc`
      tests; handle per CLAUDE.md (count, summary, file list).

## Phase 3 — `quarto-xml`

Findings: § 5. **`parse_with_parent` is dead code** — zero callers anywhere,
including tests, and `quarto-xml` is workspace-internal so there are no outside
consumers. `XmlParser::parent` and the `Substring` branch of `make_source_info`
are dead paths with it. `quarto-csl` and `quarto-citeproc` do no offset
arithmetic at all and need no work.

**Decision (2026-08-21): do not lint it.** An earlier draft proposed an
`xtask lint` rule restricting callers, modelled on `add-file-with-id`. Rejected
— guarding an API nobody calls is a door in a field. One doc comment instead, so
that whoever revives it learns the precondition.

- [ ] Note at `quarto-xml/src/parser.rs:55` (`parse_with_parent`) that it has no
      callers, and that its precondition if revived is: the content handed to it
      must be a **byte-identical slice** of the parent, because
      `make_source_info`'s `Substring` branch composes affinely. Say that
      attribute values are entity-decoded (`parser.rs:469` calls
      `unescape_value()`) while `value_source` spans raw text *including the
      quotes* (`parser.rs:548-558`), so an attribute value is exactly the input
      that would break it.

## Phase 4 — the `quarto.config.md` Lua path

Findings: § 6. **Inert on three independent grounds.** `ProvenanceBuilder` would
not fix it if it were live — `Generated { by: By::filter }` has no byte extent
to map into, so the fix would be an ephemeral `SourceFile`. The original "if
live, fix as in Phase 1" branch is deleted, not deferred.

- [ ] **T8 — guard the inertness, which is currently untested.** Three
      independent grounds hold today and nothing notices if one fails, so the
      comment below asks a reader to trust an unguarded invariant. Call
      `quarto.config.md("x")` through a Lua filter and assert the resulting
      node's `SourceInfo` yields **`resolve_byte_range() == None`** (assert
      `map_offset(0, ctx) == None` too, for documentation, but note it cannot
      redden — the `Generated` arm returns `None` unconditionally, so only
      `resolve_byte_range` discriminates). **Revert hunk:** attach an
      `Invocation` anchor in `filter_source_info` (`types.rs:2291`) ⇒
      `resolve_byte_range` starts resolving through it ⇒ RED.
- [ ] Add a comment at `config_value.rs:601` recording *why* it is safe — the
      unconditional `None` in `map_offset`'s `Generated` arm, and the absence of
      production anchor mutation — and pointing at T8 as the thing that notices
      if it changes. The safety depends on facts several crates away; the next
      auditor should not have to re-derive it.
- [ ] Name the forward risk in the same comment. `filter_source_info` returning
      `from: SmallVec::new()` is exactly what someone will later "improve" by
      anchoring to the filter invocation site, and
      `quarto-core/src/transforms/shortcode_resolve.rs:1175` already establishes
      that pattern in production. Say that doing so makes `resolve_byte_range`
      live on a base with no byte extent.

## Phase 5 — the engine `map_offset` pair

Findings: § 6. Two production sites, not three; the existing test is vacuous;
and the invariant is **writer provenance**, not this bug class.

- [ ] **T4 — a characterization probe, not a regression test.** Extend
      `test_build_source_map_maps_lines_to_file_provenance` (`ts_engine.rs:2977`)
      with a non-identity fixture: a document the QMD writer normalizes, so
      `input`'s coordinate space genuinely differs from `ctx.source_info`'s. One
      test covers both production sites. **It has no revert hunk** — it exists
      to find out whether a defect is there. If it goes green it guards nothing;
      say so rather than counting it as coverage.
- [ ] **If it goes red, do not fix it here** — that is a writer-provenance
      defect, outside this epic. Record the observed drift in § Evidence, file a
      strand citing `engine_execution.rs:732` and
      `pampa/src/writers/qmd.rs:2880-2903`, and leave the new test `#[ignore]`d
      with a comment pointing at the strand rather than deleting it.

## Phase 6 — comrak `NodeValue::Text`

Findings: § 7. **Sequenced last** — a real, test-verified correctness bug, but
its only consumer is JSON output nothing reads, making it the lowest-value item
here. The fix is **lockstep**, not re-deriving comrak's escape rules; § 7 has
the three measured facts that make it well-posed and the worked tiling.

- [ ] **Failing test first — T5.** The drift is measured (§ 8) but has no
      permanent test. Assert the **`dd`** and **`ee`** `Str`s, **not `aa*bb`**:
      pre-fix `aa*bb` already resolves correctly, so asserting it passes without
      the fix. Expected values in § 7's table. Observe red.
- [ ] **T6 — the upstream pin, with the corrected discriminator.** The
      blockquote fixture. Assert `dd` **and** `ee`; only `ee` discriminates,
      because `dd` reports 14..16 correctly *before* the fix — resetting at
      `SoftBreak` is precisely what it does, so `assert dd == 14..16` survives
      its own revert. Name the test and comment it as a **comrak-behaviour pin**
      (its "revert" is a comrak version bump, not a q2 hunk), so nobody reads it
      as covering our code. If the reset property ever breaks, lockstep needs a
      deletion rule and this design is wrong — it should fail loudly.
- [ ] Implement the lockstep walker in `comrak-to-pandoc`, driving
      `ProvenanceBuilder::in_file(file_id, anchor)` with two segmentation rules
      (backslash-punct; entity reference to its `;`), **escape before verbatim**.
- [ ] Have `tokenize_text_with_source` derive each token's span as a `substring`
      of the content provenance rather than `base + byte_idx`.
- [ ] Record the JSON-writer snapshot churn per CLAUDE.md — count, summary, file
      list — and state in the commit message that `r` changes coordinate space
      for escaped paragraphs on `--from commonmark`.
- [ ] Add code comments, do not fix: the entity sub-character offset, and the
      two `Code` / `Link` span caveats from § 7, so the next consumer of those
      spans is warned.

## Phase 7 — close the epic

Findings: § 6, "The workaround census". Six sites, **one deletion** — "the
workarounds collapse" is a claim about capability, not deletions.

- [ ] Record the `cell_options` constraint (§ 6) in that module's doc comment
      and close the question. **Do not lift it** — there is no consumer.
- [ ] Add a code comment at `codeblock_shorthand.rs:486` noting the first-match
      hazard: `find` returns the *first* occurrence and `cb.source_info`
      includes the fence line, so a code block whose text is a substring of its
      own info string (contrived but reachable: a cell containing only the word
      `python`) gets a span pointing into `{python}`. Different bug class,
      bounded within the block — record, do not fix.
- [ ] **Decide the sixth workaround's fate.** `codeblock_shorthand.rs:486` can
      use the `map_offset(0)`/`map_offset(length())` pair instead of `find()`.
      Either do it — with a failing test on a `python`-only cell first — or file
      a strand. Do not leave it recorded and undecided.
- [ ] **Decide the seventh site's fate** — the `shortcode_string` closure at
      `treesitter.rs:989`, filed to Plan 3 by Plan 2 as out of its Phase 4
      scope. Scoping is already answered in § 6: **wrong-span, not drifting**
      (its computed range is destructured away at `shortcode.rs:36` and no
      consumer offsets into the surviving whole-node range), so this is a
      tightening, not a correctness fix. Cheapest honest action is to **delete
      the dead range computation** at `treesitter.rs:1000-1005` and comment the
      decoded-value/raw-span pairing. Either do that or file a strand; do not
      leave it filed-and-undecided, which is how it reached this plan.
- [ ] **Cross-check Plan 2's dispositions against § 6's census table.** Confirm
      the `callout.rs:431-447` match block is gone (the enclosing function ends
      at `:448` and keeps its bd-3aolj guard — do not delete that); that
      `use_cmd/config.rs:229` still compiles
      and still returns `None` on mismatch (it is *kept*, so a deletion would be
      the regression); and that `theorem.rs` / `proof.rs` changed output as
      Plan 2 Phase 4 predicts. Plan 3 owns none of those edits — this item
      exists because Plan 2's fallout list is the only place the
      `theorem`/`proof` change is written down.
- [ ] Close `bd-mxa44voa` once its children and all three plans are done.

## Definition of done

Every box above checked; the Phase 1 classification table and the Phase 2
`offset_to_location_bytes` lines in § Evidence; `cargo xtask verify` green; the
epic's children closed.

## Out of scope

**ipynb's escape problem.** Its design doc chose one ephemeral `SourceFile` per
cell so users see cell-relative positions rather than offsets into a JSON blob
(`2026-07-20-ipynb-surface-syntax-design.md`, § "Why pointing into the .ipynb
bytes is the wrong root"), and the unescaping happens at ingestion *before*
source tracking begins. `ProvenanceBuilder` is irrelevant to it.

**ipynb's assembly problem was decided, not missed.** That design's step 3
hand-builds a `SourceInfo::Concat` alternating per-cell `Substring` pieces with
`Generated { by: By::raw("ipynb/scaffold"), from: [anchor] }` scaffold pieces.
`ProvenanceBuilder` cannot emit a `Generated` piece, so ipynb would hand-roll a
fourth `Concat` builder. Plan 1 was told and kept the flat exclusion, which is
its call. Note also that Plan 1's *synthesis* is a second, different
representation of "content with no source byte".

That design doc's § "What we give up, and the escape hatch" lists an upstream
`Transformed { parent, runs }` variant as "the principled fix… worth doing only
when a concrete consumer exists." This epic found the consumer and chose
`Concat` instead, so **that escape hatch is now stale** and should not be
reproposed from that document.

**Writer provenance.** The QMD writer's `Concat` pairs source spans with bytes
written (Phase 5). Real, adjacent, and not this bug class.

**`quarto-csl` / `quarto-citeproc`.** Audited, zero offset arithmetic (§ 5).

## Evidence

_A fix is not done until its evidence is here. The audit's evidence is in the
findings doc, § 8._

### Phase 1 · Phase 2 · Phase 5 · Phase 6 · Phase 7

_(pending — one subsection per phase as its fixes land)_
