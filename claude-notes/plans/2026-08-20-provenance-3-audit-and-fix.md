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
**Design decisions:** `claude-notes/research/2026-08-23-provenance-3-design-recommendations.md`
resolves the eight questions this plan and Plan 2's hand-off left open; its verdicts are
transcribed into Phases 6–8 below (revision of 2026-08-23, after a full-context read and a
blank-slate implementer review). Read it for the *reasoning*; the checklist is authoritative for
the *work*.

## Read this first

**The audit is done.** It was this plan's original content; it now lives in the
findings doc, phase by phase. What remains here is implementation. Do not
re-derive a finding — check it against its citation and say so if you disagree.

**All gates are closed (verified 2026-08-23).** Earlier revisions of this section gated four
phases on unreleased upstream work. Every one of those gates has since landed and is in both
lockfiles (`Cargo.lock`, `crates/wasm-quarto-hub-client/Cargo.lock`):

| former gate | status |
|---|---|
| `quarto-source-map` 0.1.2 (the four behaviour fixes incl. `preimage_in`'s blanket-`None`) | in the lock as **0.1.3** |
| `quarto-source-map` 0.1.3 (`ProvenanceBuilder`: `in_file`, `in_parent`, `verbatim`, `replacement`, `finish`) | in the lock |
| Plan 1's `preimage_in` doc-comment rewrite | shipped in 0.1.3, `source_info.rs:410-457` ("a `Some(hull)` licenses *locating*… it does not license *copying*") |
| Plan 2 Phases 3–4 (q2 consumes `content_source_info`; attribute path drives the builder) | landed on `feature/yaml-provenance` (Plan 2 § EXECUTION STATUS, session 4) |
| `quarto-error-reporting` 0.2.2 (the char-boundary snap) | in the lock; floors bumped in both manifests |

No `[patch.crates-io]` override is needed for anything in this plan. (Phase 6's founding-crash
pin deliberately does **not** use one — see that item.)

**Phase order is value order, not gate order.** Phases 1–5 are comments, tests and one
classification pass. Phase 6 discharges Plan 2's hand-off (cheap guards and tightenings) and
runs **before** the comrak fix (Phase 7), which the plan itself ranks lowest-value. Phase 8
closes the epic.

**Before writing any Phase 7 code, read Plan 1 § The shared builder** — `ProvenanceBuilder`'s
signature lives only there (and in `quarto-source-map-0.1.3/src/provenance_builder.rs`), so
Phase 7 is not implementable from this plan alone.

## Test seam spec (frozen — bind before dispatch)

Every test below is bound to the **exact production hunk whose revert reddens
it**. Once a test is green, its assertions and harness are frozen; never edit
one to go green. (T11 changes a green test's *access path*, not its assertion
— the asserted text is identical; that is the one sanctioned edit.) Of the
twelve ids, eight are bound (table below); four are pins or probes with no q2
hunk and are labelled as such in the next table. Several originally specced
tests failed the binding check and were corrected — see § Vacuity findings.

| id | tier | real unit mounted | seam: mount → events → assertion surface | mock boundary | **revert hunk → RED** |
|---|---|---|---|---|---|
| **T2** | e2e, real binary | `q2 render` | fold-shaped `_quarto.yml` (`aaa`⏎`bbb` plain scalar) → render → assert emitted bytes are the **content** (`aaa bbb`) not the **source** (`aaa\nbbb`) | none | whichever newly-classified copy site is fixed; **only write T2 if Phase 1's classification finds one.** If all 24 are `locate`, T2 has no hunk and must not be written |
| **T3** | unit, in-crate | `comrak_to_pandoc::empty_source_info` | convert a node with no location → assert `matches!(si, SourceInfo::Generated { .. })` | none | `lib.rs:31` back to `SourceInfo::original(FileId(0), 0, 0)` ⇒ RED |
| **T5** | unit, in-crate | `comrak_to_pandoc` `Text` conversion + `ProvenanceBuilder` | `aa\*bb cc &amp; dd ee` → convert → assert `map_offset(0)` of the **`dd`** and **`ee`** `Str`s resolves to **16** and **19** | none | the lockstep walker → back to `base_offset + byte_idx` (`text.rs:99`, inside `tokenize_text_with_source` `:91-160`) ⇒ `dd` resolves to 11 ⇒ RED |
| **T8** | unit, in-crate (`config_value.rs` tests; harness `quarto_config_md_inline` at `:1475`) | `quarto.config.md` through a Lua filter | `quarto.config.md("x")` → take the node's `SourceInfo` → assert `resolve_byte_range() == None` (also assert `map_offset(0, ctx) == None`, for documentation — it cannot redden, the `Generated` arm returns `None` unconditionally) | none | attach an `Invocation` anchor in `filter_source_info` (`types.rs:2291`) ⇒ `resolve_byte_range`'s `Generated` arm delegates to `invocation_anchor()` (0.1.3 `source_info.rs:403-406`) ⇒ `Some` ⇒ RED |
| **T7** | unit, in-crate | `quarto_core::crossref::codeblock_shorthand::body_source_for` | a cell whose entire body is the word `python`, fenced ```` ```{python} ```` → assert the resolved span is `12..18` (the body), not `4..10` (inside `{python}`; measured 2026-08-23) | none | the **bounded between-fences search** → back to whole-block `block_text.find(&cb.text)` (`:486`) ⇒ span lands in the info string ⇒ RED. (An earlier revision named "the `map_offset` pair" as the hunk; that pair on `cb.source_info` is the whole-block hull and cannot locate the body — it bound nothing.) |
| **T10** | e2e, real binary (`crates/quarto/tests/integration/`) | `q2 render` over the founding repro: `_quarto.yml` website with navbar `text: '<span id="x">Ask AI ✨</span>'`, `index.qmd` with a title | assert exit 0, two `Q-2-9` warnings, **and their caret positions** `_quarto.yml:7:16` and `_quarto.yml:7:37` (measured 2026-08-23, recommendations § 4 config G) | none | the carets bind to Plan 2 Phase 3's `content_source_info` consumption in the config path (`meta.rs:255`, `config_markdown.rs:326`): revert either `.unwrap_or(&…)` base to the raw span ⇒ the second caret moves to `:7:36` (one byte left, onto `✨`) ⇒ RED. The exit-0 half is an **upstream pin** (see next table) and has no q2 hunk |
| **T11** | unit, in-crate | `quarto_config::span_assert::resolve_span` | the existing `codeblock_shorthand.rs:~1417-1440` test whose 20-line NOTE explains why it *cannot* call `resolve_span`: replace the NOTE + hand-rolled `map_offset` pair with `resolve_span(inner, &sources).expect(..)` and assert the same text | none | the `is_gapless` narrowing (Phase 6) → back to whole-`Concat` contiguity ⇒ `Err(SpanProblem::Concat)` ⇒ RED |
| **T12** | unit, in-crate — an **evidence record** plus, if needed, one test | the two zero-drift guards from Plan 2's T7 (plain scalar and single-line block scalar keep their positions) | mutate `meta.rs:255` (`markdown_base`) **alone** → record which guard reddens; restore; mutate `config_markdown.rs:326` (`base`) **alone** → record which reddens. If either mutation reddens nothing, **add a guard on that path** (same shape as the existing one) and bind it | none | the two selective mutations above, one per path. Plan 2's audit row 3 applied one mutation and treated both guards as one site (hand-off (g)); this row is the selective replay |

> **Correction 2026-08-23 (Phase 6e, T10 as built).** The **T10** row above is
> left as written; this note corrects it rather than replacing it. **Its
> binding claim is overdrawn.** The row says "revert *either* `.unwrap_or(&…)`
> base to the raw span ⇒ RED", naming `meta.rs:255` and
> `config_markdown.rs:326`. Measured against the test as built:
>
> | revert | T10 |
> |---|---|
> | `config_markdown.rs:326` → `&value.source_info` | **RED** — carets move to `:7:15` and `:7:36` |
> | `meta.rs:255` → `&source_info` | **GREEN — no change at all** |
>
> The second row is a **negative result**: it has no failure transcript to
> quote, because nothing failed. It is corroborated from the other side by
> the **T12** row's outcome (that mutation reddens the front-matter guard and
> only the front-matter guard) and predicted by the path analysis below.
> Recorded as a negative result rather than dressed up with manufactured
> output.
>
> **Of the two bases this row names, T10's caret half therefore binds
> `config_markdown.rs:326` only.** (Scope set by the table above: the claim
> is about these two bases, not about everything that could conceivably move
> a caret.) The
> navbar `text:` value reaches the markdown re-parse through
> `ConfigMarkdownTransform`, not through `DocumentMetadata`, so the
> front-matter base is never on this fixture's path. A future reader relying
> on T10 to guard `meta.rs:255` would be relying on nothing.
>
> **`meta.rs:255` is bound by
> `json_errors::plain_scalar_raw_html_frontmatter_unaffected`**, established
> selectively by the **T12** row above — which is a *better* binding than
> T10 would have been, because T12 also shows the guard on the other path
> stays green under the same mutation. See § Evidence → Phase 6 → 6e.
>
> This is the **eleventh** instance of this plan's recurring defect shape — a
> claim that is plausible but scoped so a reader draws a broader conclusion —
> and the **first found inside the frozen seam table itself**, which is the
> artifact every other task in this plan has been told to trust. The freeze
> protects test assertions and harnesses from being edited into passing; it
> does not make a measurably wrong prose claim immune to correction.

### Not regression tests — labelled, not smuggled in

These have **no q2 hunk to revert** (or, for T10's exit-code half, none in
q2). They are legitimate but must not be counted as guarding anything:

| id | what it actually is | why no hunk |
|---|---|---|
| **T1** (Phase 1) | an **invariant pin** | unit, in-crate (`pipeline.rs` test module, harness as in `render_qmd_to_preview_ast_emits_inline_footnote_section` at `:3202`): render a document with **one inline footnote** through `render_qmd_to_preview_ast`, parse `output.untransformed_ast_json`, assert **every** `astContext.p` entry is wire-code `0` (`Original`) with the document's own `file_id`. **No line-level hunk exists**: the pool is all-`Original` because `capture_untransformed_ast_json` re-parses the raw bytes with a fresh context (`pipeline.rs:1007`) — *not* because of `:1013`'s `parent_source_info: None`, which is built after the parse and read only by the JSON writer (`parent_source_info` is consumed at parse time, `location.rs:214`). The honest hunk is the rewrite the comment at `:914-919` invites — "derive the baseline from the pipeline's own parse" — under which the footnote transform's `Generated { by: footnotes() }` section (code `4`) appears in the pool ⇒ RED. That is why the fixture is a footnote, not a navbar: `footnotes` runs in the preview pipeline (`:1534-1536`), `title-block` does not (`:1533`), and no project config is needed. |
| **T9** (Phase 6) | an **invariant pin** | e2e, real binary (`crates/quarto/tests/integration/diagnostic_render_panic_boundary.rs`): `q2 render` + `QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER=0` over `render_exit_codes.rs:28-69`'s duplicate-crossref fixture (exactly one `Q-15-1` **error**) → assert `!status.success()`, stderr contains `internal error rendering diagnostic Q-15-1`, and does **not** contain the diagnostic's title text. **No guard mutation can redden it**: `should_exit_nonzero(&summary)` (`render.rs:848`) counts the immutable summary, not what was printed (`:836`). The only hunk is "compute exit status from printed diagnostics", a refactor someone could make. |
| **T4** (Phase 5) | a **characterization probe** | Its own checklist says "if it goes red, file a strand" — i.e. it exists to *discover* whether a writer-provenance defect exists, not to guard a fix. If it goes green it guards nothing. Run it, record the result, keep it `#[ignore]`d if red. |
| **T6** (Phase 7 blockquote) | an **upstream-behavior pin** on comrak | Nothing in q2 makes drift reset at `SoftBreak`; comrak's per-line `Text` nodes do. Its "revert" is a comrak version bump. Keep it, and say so in the test name/comment, so nobody reads it as covering our code. |
| **T10**'s exit-0 half (Phase 6) | an **upstream-behavior pin** on `quarto-source-map`'s `offset_to_location` floor + `quarto-error-reporting`'s snap | Measured 2026-08-23 (recommendations § 4): the abort returns only if q2's mapping regresses **and** both upstream guards are gone. The caret half of T10 (table above) is what binds to q2; the exit-code half is the only witness of the founding abort anywhere, which is why it is asserted too. |
| **Phase 6's `q_2_28`/`q_2_33` accessor swap** | **accepted-untested** | Both codes are corpus-only (no Rust emission site) and `find_violation_offsets` takes an offset, not a location, so a `Concat`-rooted diagnostic cannot be injected. The `== ">}}}"` content check is the real splice guard; the comment is the artifact. |
| **Phase 6's `render.rs:904` wrap** | **accepted-untested** | The path cannot panic today (`ctx = None` never reaches a renderer); the wrap is uniformity, not a fix. |

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

**T1 had two no-op hunks (caught 2026-08-23, round-2 review).** Flipping
`pipeline.rs:1013` cannot change the pool (that `ASTContext` is post-parse and
writer-only), and moving the `:920` call cannot either (the capture consumes
raw `content`, never the pipeline's AST). The earlier "needs a navbar with
markdown" requirement was therefore inert too. T1 is now an invariant pin with
a footnote fixture and the rewrite-shaped hunk stated honestly (table above);
findings § 3's "(3)(a) … sets `parent_source_info: None` explicitly at `:1013`"
cites the wrong mechanism and is corrected in this revision.

### Missing-test pass

Behaviour with no test, either specced or explicitly accepted:

- **Phase 4's Lua inertness was unguarded — now specced as T8** (frozen table;
  it is a bound regression test, not a missing one). It is the guard the
  plan's Phase 4 comment asks a reader to trust.
- **Accepted untested, with rationale:**
  - *Phase 2's drift amplifiers.* An ordering constraint on future work, not a
    behaviour. A comment is the only enforceable artifact.
  - *Phase 3's revived `parse_with_parent`.* Deliberately unguarded — we
    rejected a lint rule because the function is dead (§ 5). If it is ever
    revived, its doc comment is the warning; there is no live path to test.
  - *Phase 1's `postprocess.rs:660` combine-fallback.* Its success condition is
    "state whether any snapshot moves". If none move there is nothing to assert,
    and inventing an assertion would be theater.
  - *Phase 6's shortcode-closure deletion.* Behaviour-preserving by construction
    (the surviving range is recomputed from the same node at `shortcode.rs:42-44`);
    no snapshot can move. The comment is the artifact.

## Phase 1 — `preimage_in` consumers

Findings: § 3. **Verdict: latent, not live** — the wrong-bytes path through
`incremental.rs:171` is closed by the shape of the preview capture, not by
anything about `preimage_in`. That is why this phase ships a **guard**, not a
fix: the site is not broken; what is missing is anything that would notice if
the invariant moved.

- [x] **Classify the 24 unclassified calls** as **locate** (computes a position,
      compares identity, or bounds a search) or **copy** (slices source text
      that is then emitted). The enumerated call list is in § 3. Output: a
      **26-row** table (the two rows below are already classified and lead it)
      appended to § Evidence — the deliverable even if every answer is
      "locate".

      | file:line | locate / copy | what it does with the range |
      |---|---|---|
      | `incremental.rs:171` | copy | slices `original_qmd` → `CoarsenedEntry::Verbatim` |
      | `postprocess.rs:660` | locate | min/max span over a run |

      Plan 1's hypothesis, to test rather than assume: the split falls along the
      incremental-writer / span-computation line, with `incremental.rs`'s
      `Verbatim` arms the only copies.
- [x] **T1 — pin the invariant that makes the copy site safe** (seam spec: an
      invariant pin, footnote fixture, no line-level hunk). Mount it on
      `render_qmd_to_preview_ast` via the in-crate harness. One test, because
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
      than on the capture. In the same commit, correct findings § 3 item 3(a):
      the baseline is parent-less because `capture_untransformed_ast_json`
      re-parses raw bytes through `qmd_to_pandoc` (`pipeline.rs:1007`), not
      because of the writer-only `ASTContext` at `:1013`. Note in the test that this invariant load-bears for
      provenance correctness while living in `quarto-core`, which neither
      `quarto-source-map` nor `quarto-yaml` owns.
- [x] **Fix the call-site comment at `incremental.rs:162-168`** (`:169` is the `match` head). It asserts the
      byte-identity reading Plan 1 is retracting upstream, so once 0.1.2 lands
      the codebase asserts both readings — worse than asserting only the wrong
      one. (Plan 1's ninth hand-off obligation, `7d799d623`.) Say the `.get()`
      guard checks **bounds, not identity**, and that the arm is safe only
      because the baseline AST is untransformed and parent-less.
- [ ] **T2 — failing test first, for any *newly* discovered copy site:** a
      fold-shaped end-to-end fixture (`aaa`⏎`bbb` as a plain scalar) driven
      through the real binary, asserting the emitted bytes are the *content* and
      not the *source*. Observe red before fixing.

      **Not written.** The classification did find two new copy sites
      (`assemble_inline_content`'s `KeepBefore` arm and
      `assemble_recursed_container`'s verbatim early return), but both are
      latent members of the very class this phase guards rather than fixes, so
      no fix exists to observe RED against and T2 would be bound to nothing.
      They are named alongside the original copy site in T1's failure message
      and in the corrected call-site comments instead. See § Evidence →
      Phase 1 → "T2 — not written". Box left unchecked deliberately: the work
      was resolved, not performed.
- [x] **Confirm Plan 1's 0.1.2 blanket-`None` is regression-free.** 0.1.3 is in
      the lock and the branch is green, so this is answerable **now by
      inspection** (run `cargo nextest run -p pampa` and read the snapshot
      list), not by a future measurement. Two shapes:
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
      next line's `#| ` prefix — `crates/quarto-core/src/cell_options/mod.rs:247-263`), so they already
      return `None`. Only a **single-option cell** yields one piece with a hull
      that blanket-`None` removes. Plan 1 asserts both shapes in its own
      Phase 1; confirm the q2 side agrees.
- [x] **Cite the corrected `preimage_in` doc comment** — shipped in 0.1.3
      (`quarto-source-map-0.1.3/src/source_info.rs:410-457`: a `Concat` hull is
      "an offset claim, not a byte-identity claim"; a `Substring` over a `Concat`
      returns `None`). Quote from the registry source or `~/src/quarto-source-map`
      at tag 0.1.3; do not quote a remembered replacement.

## Phase 2 — the `SourceInfo::original(` surface

Findings: § 4. **All 17 production sites triaged; no further triage needed.**
Five are Phase 7's comrak defect; the rest are safe by shape or are the three
drift amplifiers below.

- [x] **Comment the three drift amplifiers** (`postprocess.rs:317`, `:669`,
      `:1833`) with the ordering constraint from § 4: fix producers before these
      consumers, or the fix silently does not reach the output. Record it in the
      code; do not restructure.
- [x] Separately at `postprocess.rs:1833`: note the hardcoded `attr_end + 1`
      assumption in the same comment.
- [x] **Discharge Plan 1's hand-off** — its Phase 1 audit shipped no fixes
      outside `quarto-source-map`. Examine `offset_to_location_bytes`
      (`quarto-parse-errors/src/error_generation.rs:330`, a documented
      "bytes-aware sibling") plus `quarto-yaml`'s own `Location` uses
      (`~/src/quarto-yaml`). Plan 1 measured the two `offset_to_location`
      implementations in `quarto-source-map` disagreeing by one column for a
      mid-character offset; a third with its own rule is the same hazard.
      **Output:** for each, one line in § Evidence stating what it returns for a
      mid-character offset — floored, ceiled, raw, or overcounted — and whether
      that agrees with `FileInformation::offset_to_location` after Plan 1's fix.
      Examine `~/src/quarto-yaml` **at the tag q2 consumes** (`Cargo.lock`:
      0.1.3), not its HEAD. **Routing:** a q2-side disagreement is fixed here;
      a `quarto-yaml`-side one is **out of scope** — file a strand against
      `posit-dev/quarto-yaml` (Plan 1 is closed; there is no one to notify).
- [x] **T3 first, then change** `comrak-to-pandoc/src/lib.rs:31`'s
      `empty_source_info()` from `SourceInfo::original(FileId(0), 0, 0)` to a
      `Generated`, so "no location" stops being indistinguishable from "start of
      file 0" — the shape `span_assert` flags as `SpanProblem::SuspiciousDefault`
      (variant `quarto-config/src/span_assert.rs:74`, check at `:265`). Out of
      this bug class but cheap and adjacent. ~~**Expect snapshot movement** in
      `comrak-to-pandoc` tests~~ — **corrected 2026-08-23 (measured).**
      `crates/comrak-to-pandoc` contains **zero** `.snap` files; its only
      dependent is `pampa` (`crates/pampa/Cargo.toml:49`), which has 212, so
      any movement would land there. **None did, in either crate**, and the
      reason is structural, not luck: every `empty_source_info()` call sits on
      a `source_ctx == None` branch (`block.rs:26`, `inline.rs:23`,
      `text.rs:25`-`:90`), and pampa's only entry point
      (`readers/commonmark.rs:47`) always passes `Some(&source_ctx)`. The
      helper is therefore unreachable from every render and snapshot path;
      **this crate's own no-source tests are its only callers.**
      (*Corrected 2026-08-23, fix round 1:* an earlier revision also named
      `normalize.rs` as a caller. It is not one — its `empty_source_info` is a
      separate `#[cfg(test)]`-local helper still on the old `Original` shape,
      and the file has no `use crate` imports at all. Dropping the phantom
      caller makes the reachability conclusion **stronger**, not weaker: the
      helper's entire live surface is the three no-source branches, every one
      of which pampa's entry point bypasses.)
      Phase 7 is a different matter — see its item.

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

- [x] Note at `quarto-xml/src/parser.rs:55` (`parse_with_parent`) that it has no
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

- [x] **T8 — guard the inertness, which is currently untested.** Three
      independent grounds hold today and nothing notices if one fails, so the
      comment below asks a reader to trust an unguarded invariant. Call
      `quarto.config.md("x")` through a Lua filter and assert the resulting
      node's `SourceInfo` yields **`resolve_byte_range() == None`** (assert
      `map_offset(0, ctx) == None` too, for documentation, but note it cannot
      redden — the `Generated` arm returns `None` unconditionally, so only
      `resolve_byte_range` discriminates). **Revert hunk:** attach an
      `Invocation` anchor in `filter_source_info` (`types.rs:2291`) ⇒
      `resolve_byte_range` starts resolving through it ⇒ RED.
- [x] Add a comment at `config_value.rs:613-642` (the `quarto.config.md`
      constructor; `filter_source_info(lua)` is the base at `:626`) recording *why* it is safe — the
      unconditional `None` in `map_offset`'s `Generated` arm, and the absence of
      production anchor mutation — and pointing at T8 as the thing that notices
      if it changes. The safety depends on facts several crates away; the next
      auditor should not have to re-derive it.
- [x] Name the forward risk in the same comment. `filter_source_info` returning
      `from: SmallVec::new()` is exactly what someone will later "improve" by
      anchoring to the filter invocation site, and
      `quarto-core/src/transforms/shortcode_resolve.rs:1175` already establishes
      that pattern in production. Say that doing so makes `resolve_byte_range`
      live on a base with no byte extent.

## Phase 5 — the engine `map_offset` pair

Findings: § 6. Two production sites, not three; the existing test is vacuous;
and the invariant is **writer provenance**, not this bug class.

- [x] **T4 — a characterization probe, not a regression test.** Extend
      `test_build_source_map_maps_lines_to_file_provenance` (`ts_engine.rs:2977`)
      with a non-identity fixture: a document the QMD writer normalizes, so
      `input`'s coordinate space genuinely differs from `ctx.source_info`'s. One
      test covers both production sites. **It has no revert hunk** — it exists
      to find out whether a defect is there. If it goes green it guards nothing;
      say so rather than counting it as coverage.
- [x] **If it goes red, do not fix it here** — that is a writer-provenance
      defect, outside this epic. Record the observed drift in § Evidence, file a
      strand citing `engine_execution.rs:732` and
      `pampa/src/writers/qmd.rs:2880-2903`, and leave the new test `#[ignore]`d
      with a comment pointing at the strand rather than deleting it.

## Phase 6 — discharge Plan 2's hand-off: guards and tightenings

Plan 2 § Hand-off routed nine items here by name; earlier revisions of this plan
transcribed only two. Decisions: recommendations doc §§ 1–8, confirmed by the
owner on 2026-08-23 in two rounds (eleven choices in all; the T1/T10/T12/
`use_cmd` ones are recorded only here). Everything here is small; it runs
before the comrak fix because it is worth more.

- [x] **T7 first, then fix `codeblock_shorthand.rs:486`** (`body_source_for`,
      `crates/quarto-core/src/crossref/codeblock_shorthand.rs:470-490`). Replace
      the whole-block `find` with a search **bounded to the region between the
      fence lines** (and rewrite the function's doc comment, which currently
      describes the whole-block search): start after the first `\n` of `block_text`; end before the
      closing fence line when the block text ends with one (tree-sitter error
      recovery can omit it). Keep the existing fallback to the block span when
      the search fails (blockquote/list continuations). Comment the one
      remaining hole — a body consisting solely of fence characters — and that
      the span is then a few bytes off but still inside the block. **Do not**
      use the `map_offset(0)`/`map_offset(length())` pair on `cb.source_info`:
      that is the whole-block hull (measured: `4..10` vs truth `12` for a
      `python`-only cell).
      **Correction 2026-08-23 (in execution).** The hole named in this item is
      wrong in two ways, both measured in Phase 6a: a body consisting solely of
      fence characters resolves **correctly** when the closing fence is present
      (```` ````{python}\n```\n```` ```` → `13..16`); the real hole is a body
      whose *last line* is fence-only in a block with **no** closing fence, and
      it degrades to the **whole block**, not "a few bytes off". The landed doc
      comment states the measured shape. Same correction on
      `claude-notes/research/2026-08-23-provenance-3-design-recommendations.md`
      § 1, where it originated; full evidence in § Evidence → Phase 6 → 6a.
- [x] **Draft the producer-side strand** (outside this epic; title/body in
      recommendations § 1): carry `code_fence_content`'s provenance on
      `CodeBlock` as `text_source`, built with `ProvenanceBuilder` so elided
      `block_continuation` markers become gaps; `process_fenced_code_block`
      (`pampa/src/pandoc/treesitter_utils/fenced_code_block.rs:30`) currently
      discards that range. 74 construction sites + wire + TS schema — a type
      change, not a consumer fix. File it with `--deps discovered-from:bd-mxa44voa`.
- [x] **Delete the dead range computation in the `shortcode_string` closure**
      — `crates/pampa/src/pandoc/treesitter.rs:1002-1005` (**not** `:1000-1001`,
      which is the tail of the live `text` binding). Narrow
      `process_shortcode_string` (`treesitter_utils/shortcode.rs:31-46`) to take
      `&dyn Fn() -> String`, make the closure return `text` (so `:1006`'s
      `IntermediateBaseText(text, range)` goes too), drop the callee's
      `let … else { panic!() }`, and comment at
      the construction site that the arg's range is the quote-inclusive node span
      paired with the decoded string, and that no consumer offsets into it
      (`shortcode_resolve.rs:135, :171, :837, :848, :2232, :2265` take the
      string). No behaviour change, no snapshot movement. Closes Plan 2
      deferred-minor #5.
- [x] **`q_2_28.rs:80` and `q_2_33.rs:74-75`: replace `end_offset()` /
      `start_offset()` with `resolve_byte_range()`** — it returns
      `Option<(file_id, start, end)>`; use `end` / `start` respectively, and
      `continue` on `None` (the accessor rule, findings § 1). The `file_id` can
      be ignored: both conversions parse exactly one file, so a resolved span
      is in it. Comment in `q_2_28.rs` that the `== ">}}}"` comparison at
      `:121` is the splice-safety guard and must not be removed as redundant. **No generic "refuse non-`Original`" guard.**
      Accepted-untested (seam spec).
- [x] **T11 first, then narrow `is_gapless`**
      (`crates/quarto-config/src/span_assert.rs:234`; walker
      `concat_pieces_are_contiguous` `:199-227`; caller `resolve_span` `:252`):
      for a `Substring` over a `Concat`, check contiguity only of the pieces the
      queried content sub-range overlaps. Piece *selection* uses the declared
      per-piece content `length` (content offsets against content lengths — the
      one place that is right); piece *positions* stay `map_offset`. Test-only
      blast radius (`span-assert` is a `[dev-dependencies]`-only feature). Update
      the helper's "conservative over-approximation" comment (`:187-191`).
- [x] **T9 — the caught-panic-on-error-severity pin** (seam spec). Use the
      existing `run_q2_render_with_fault` helper (project invocation, `render .`).
      For "the rendered body is absent" assert the absence of the diagnostic's
      *title* text (the guard's own line contains the code string `Q-15-1`, so
      the code is not a usable discriminator). In the same commit, **append a
      dated correction** to Plan 2 § Hand-off item 9 (do not rewrite the
      record): printing (`render.rs:836`) precedes the exit gate (`:848`); the
      invariant is that both read `&summary` and the guard's closures are
      `UnwindSafe` (`:1264-1269`), not that counting happens first. The guard's
      own doc (`:1255-1256`) is already right.
      *(Anchor stale as of 2026-08-23 — noted, not rewritten: the claim holds,
      but on HEAD the guard's doc comment is `:1268-1270`, the `UnwindSafe`
      bound `:1290-1293`, and the rationale paragraph `:1278-1283`. See
      § Evidence → Phase 6 → 6d.)
- [x] **Wrap `render.rs:904`** in `render_diagnostic_guarded(code, ||
      diagnostic.to_text(None))`, with a one-line comment: safe today because
      `None` never reaches a renderer (`to_text_with_renderer`, upstream
      `diagnostic.rs:461-481`); `config_sources` is built at `:884-889` and the
      day it is bound and passed here, this site needs the guard like the other
      eight. Update Plan 2 Phase 5's `grep -c` evidence 8 → 9. Optionally append
      the document name to the `internal error rendering diagnostic` line (Plan 2
      deferred-minor #6) — two lines, same commit.
- [x] **T10 — the founding-crash e2e pin, with carets** (seam spec), in
      `crates/quarto/tests/integration/`. Write the fixture inline (the shape
      is in the seam spec; do not reference the external repro path). Comment
      it as two halves: the carets bind q2's config-path provenance; the exit
      code is an upstream pin.
- [x] **T12 — selective replay of Plan 2's audit row 3** (seam spec): one
      mutation per path, record both outcomes in § Evidence, add a guard only
      if a path has none. Closes Plan 2 hand-off (g).
- [ ] **Upstream doc-only PR in `~/src/quarto-error-reporting`** rewriting
      `snap_span_to_char_boundaries`' doc comment (`src/diagnostic.rs:654-670`):
      keep the two-renderer panic claim (still true — ariadne 0.6.0 aborts on a
      mid-char end index; measured A/C/E in recommendations § 4); state that since
      `quarto-source-map` 0.1.2 every offset arriving via `map_offset` is already
      floored (`file_info.rs:116-125`), so the snapping half is defense in depth
      and the clamp half guards the `length() - 1` fallback (`:842-851`) and
      inversion; point at commit `5e48166` and at q2's T10. No release, no floor
      bump. **Keep the snap.**
- [x] **Strand `bd-g7qh1ltt` re-scoped** (2026-08-23, comment `c-2edupaog`,
      `related` → `bd-1d6io`): provenance is a map, not a store; the fix is
      caller-supplied content, not decoded bytes on the wire nor a source-text
      fallback. It stays outside this epic; nothing further here.

## Phase 7 — comrak `NodeValue::Text`

Findings: § 7. **Sequenced last** — a real, test-verified correctness bug, but
its only consumer is JSON output nothing reads, making it the lowest-value item
here. The fix is **lockstep**, not re-deriving comrak's escape rules; § 7 has
the three measured facts that make it well-posed and the worked tiling.

- [x] **Failing test first — T5.** The drift is measured (§ 8) but has no
      permanent test. `map_offset` needs a `SourceContext`; the seven existing
      `text.rs` tests pass only a `FileId`, so T5 registers the fixture text in
      a context first. Assert the **`dd`** and **`ee`** `Str`s, **not `aa*bb`**:
      pre-fix `aa*bb` already resolves correctly, so asserting it passes without
      the fix. Expected values in § 7's table. Observe red.
- [x] **T6 — the upstream pin, with the corrected discriminator.** The
      blockquote fixture. Assert `dd` **and** `ee`; only `ee` discriminates,
      because `dd` reports 14..16 correctly *before* the fix — resetting at
      `SoftBreak` is precisely what it does, so `assert dd == 14..16` survives
      its own revert. Name the test and comment it as a **comrak-behaviour pin**
      (its "revert" is a comrak version bump, not a q2 hunk), so nobody reads it
      as covering our code. If the reset property ever breaks, lockstep needs a
      deletion rule and this design is wrong — it should fail loudly.
- [x] Implement the lockstep walker in `comrak-to-pandoc`, driving
      `ProvenanceBuilder::in_file(file_id, anchor)` with two segmentation rules
      (backslash-punct; entity reference to its `;`), **escape before verbatim**.
- [x] Have `tokenize_text_with_source` (`comrak-to-pandoc/src/text.rs:91`,
      currently `(text, base_offset: usize, file_id: FileId)`) derive each
      token's span as a `substring` of the content provenance rather than
      `base + byte_idx` (`:99`). **This changes its signature**: update the one
      production caller (`inline.rs:52`) and the seven in-file unit tests that
      pass the arguments positionally (`text.rs:263, 273, 292, 301, 309, 324,
      339`).
      **PLAN DEFECT (found in execution 2026-08-23, resolved; not an open
      item).** "Derive each token's span as a `substring` of the content
      provenance" is **incompatible as written** with this plan's
      frozen-test-seam rule. Taken literally —
      `SourceInfo::substring(whole_node_si, c0, c1)` — every commonmark text
      token becomes a `Substring` whose `start_offset()` is *content*-relative,
      and all **seven** of the frozen `text.rs` assertions named in this very
      item read `start_offset()`/`end_offset()` and expect **absolute file
      offsets**. The literal wording reddens the seams the same item orders to
      be preserved. It also changes the emitted shape for **all** unescaped
      text, not only for escaped paragraphs — the churn scope this phase
      predicts one item below. Resolved in favour of the binding constraint
      over the literal wording: a token's span is the **restriction of the
      node's tiling** to that token's content range, built with a fresh
      `ProvenanceBuilder::in_file`. Same semantics under `map_offset`; and a
      token lying wholly inside one verbatim run collapses back to a plain
      `Original`, so unescaped text keeps the shape it had and the seven
      assertions stay true **as written, unedited**.
      Ruled correct by the plan owner on 2026-08-23. Recorded here as a defect
      in the plan text rather than only as an executor deviation, so **Phase
      8's reconciliation does not read the ticked box as diverging from the
      checklist** — it satisfies its intent, not its letter. This is the third
      such pair in this plan (Phase 5's T4 "extend … / leave it `#[ignore]`d",
      and Phase 6's half-open "overlaps"). Nothing further is owed on it.
- [x] Record the JSON-writer snapshot churn per CLAUDE.md — count, summary, file
      list — and state in the commit message that `r` changes coordinate space
      for escaped paragraphs on `--from commonmark`. ~~This is the **second**
      `comrak-to-pandoc` snapshot wave (Phase 2's `empty_source_info` change was
      the first)~~ — **corrected 2026-08-23 (measured in Phase 2).**
      `crates/comrak-to-pandoc` has **no `.snap` files at all**; the snapshots
      to watch are **pampa's** 212. Phase 2 moved **none** of them, so there is
      no first wave to be the second of — say only what this phase moves. Note
      the asymmetry that explains it: Phase 2's `empty_source_info` is reachable
      only on the `source_ctx == None` branch, which pampa never takes, whereas
      **`tokenize_text_with_source` (`text.rs:91`-`:151`) is the `Some(ctx)`
      branch pampa does take** (`inline.rs:52`, from
      `readers/commonmark.rs:47`). Expect real pampa movement here — commonmark
      reader tests and any JSON-writer snapshot over `--from commonmark`.
      **Corrected 2026-08-23 (measured in Phase 7): this phase moved zero
      snapshots either.** The `Some(ctx)` reasoning above is right about
      *reachability* and wrong about *coverage*: reaching pampa is not the same
      as reaching a pampa snapshot. `convert_document_with_source`'s only
      non-test caller in the workspace **that passes `Some(ctx)`** is
      `readers/commonmark.rs:48` (`block.rs:37`'s `convert_document` is a second
      production caller, but it passes `None`, which routes `NodeValue::Text` to
      `tokenize_text` and never reaches the walker), whose
      only non-test caller is `main.rs:332`'s `--from commonmark` arm — and no
      snapshot test in the workspace invokes that arm. (Enumerated over every
      `.rs` file under `crates/`; `crates/pampa` has 212 `.snap` files and none
      moved.)
      **Do not read "no churn" as evidence the walker did nothing.** The two are
      unrelated: the walker's output changed, and the `--from commonmark` path
      simply has no snapshot coverage to record it. What the change *was* is
      recorded instead by direct observation through the binary — see
      § Evidence, Phase 7, where `pampa --from commonmark --to json` shows `dd`
      and `ee` moving to 16 and 19 and `aa*bb` emitting § 7's worked three-piece
      `Concat`. A future phase that wants snapshot coverage of this path has to
      add a `--from commonmark` snapshot test first; there is none to update.
- [x] Add code comments, do not fix: the entity sub-character offset, and the
      two `Code` / `Link` span caveats from § 7, so the next consumer of those
      spans is warned.

## Phase 8 — close the epic

Findings: § 6, "The workaround census". Six sites, **one deletion** — "the
workarounds collapse" is a claim about capability, not deletions.

- [x] Record the `cell_options` constraint (§ 6) in
      `crates/quarto-core/src/cell_options/mod.rs`'s file-header comment (`:1-…`,
      the "Shared cell-options facility" block) and close the question. **Do not
      lift it** — there is no consumer.
- [x] **Cross-check Plan 2's dispositions against § 6's census table.** Confirm:
      the `callout.rs` workaround match block is gone (it is — `transforms/callout.rs`
      now has the bd-3aolj guard at `:400-412`, function ending `:418`, `#[cfg(test)]`
      at `:420`; do not delete the guard); `use_cmd/config.rs:229`
      (`scalar_value_span`) still compiles and still returns `None` on mismatch
      (it is *kept*, so a deletion would be the regression). **Decision
      (2026-08-23): the `map_offset`-hull simplification Plan 2 declined (R-8,
      hand-off item 1) is declined permanently** — the function refuses rather
      than mis-points, and its `start_offset()`/`end_offset()` reads at
      `:233-234` are safe only because of the byte-equality check at `:235`;
      add that sentence as a comment at the site, no strand; `transforms/theorem.rs`
      / `transforms/proof.rs` changed output as Plan 2 Phase 4 predicts (tighter
      spans on decoded values — Plan 2 § Evidence Phase 4 is the only record of
      the prediction; quote it). **Add the sixth site to the census**: Plan 2's
      final fix wave found and fixed a decoded/raw pairing at
      `crates/quarto-core/src/project/website_post_render.rs:213-222` (FIX-2) that
      § 6's table predates; append it to the findings doc's table.

      > **Corrected 2026-08-23 (execution).** Three citation slips in this item,
      > none of which changes what it asks for; all three are measured in
      > § Evidence → Phase 8.
      > **(i)** The `callout.rs` guard's *aggregate* extent `:400-412` is exact,
      > but two interior ranges are not: the `debug_assert!` is `:404-409`, not
      > `:404-410`, and the `if … return generated()` is `:410-412`, not
      > `:412-414`. Function end `:418` and `#[cfg(test)]` `:420` are exact.
      > **(ii)** `website_post_render.rs` is the **seventh** site of the census,
      > not the sixth — § 6's table already listed six *sites* across five rows
      > (`theorem.rs` / `proof.rs` is one row, two sites). The heading was
      > updated from "six sites" to "seven sites"; "one deletion" is unchanged.
      > **(iii)** Plan 2's § Evidence Phase 4 does **not** mention
      > `theorem.rs`/`proof.rs`. The prediction is recorded in Plan 2's Phase 4
      > *checklist* (`:1197-1200`, and the corollary at `:1179-1182`) and in its
      > prose at `:340-342`; § Evidence Phase 4 holds the *measurement* for the
      > shared attribute path (column 27 vs 26) instead. Both are quoted in
      > § Evidence.
- [x] Record in § Evidence that `bd-49cbyqbt` (hand-off 4(c)'s second half) was
      closed 2026-08-22 as a duplicate of `bd-1d6io` — nothing to do here.
- [x] Close `bd-mxa44voa` once all three plans are done. Its four children
      (`bd-gx2mal69`, `bd-jmquuiqh`, `bd-th2ah982`, `bd-x0o0pem3`) are already
      closed (checked 2026-08-23), so the plans are the only remaining gate.

      **Closed 2026-08-23T18:30:46Z.** All four children re-verified closed
      individually *before* the close was attempted, and `braid dep tree` was
      read to confirm those four are the only children.

## Definition of done

Every box above checked; in § Evidence: Phase 1's classification table, T1
result, T2 (written, or "no copy site — not written", or — the outcome that
actually obtained — "copy sites found, none fixed (latent by the capture
invariant) — not written"), and the answer to item
5(a); Phase 2's `offset_to_location_bytes` lines and T3 result; Phase 3's
doc-comment landing (commit hash); Phase 4's T8 result; Phase 5's T4 result
(green or red-and-`#[ignore]`d with a strand); Phase 6's T7/T9/T10/T11/T12
results plus the upstream PR link and the strand id; Phase 7's T5/T6 results
and snapshot accounting; Phase 8's cross-check lines. `cargo xtask verify`
green; the epic closed.

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

### Phase 1

Executed 2026-08-23 on `feature/yaml-provenance`. **Verdict unchanged: latent,
not live.** The phase shipped a guard (T1) and three comment corrections; no
production behaviour changed.

#### Line-number rebase

The findings' enumeration was taken at `816f4ed47`. Two rebases apply:

1. `c9a77d18c` ("config_value: sweep call sites for Scalar's new struct-variant
   shape") added two lines at `incremental.rs:553`, moving every call site
   below `:555` by **+2**.
2. This phase's own comment corrections add 24 lines above `:171` and 6 more
   below, moving the sites again.

**The table below uses the line numbers as they stand after this phase's
commit**, with the findings § 3 number in parentheses. `postprocess.rs` is
unshifted by either. No call site moved relative to any other, and the count is
still 26. Code comments added by this phase deliberately name *functions and
arms*, not lines, so they do not rot.

#### The 26-row classification

**Decision rule.** `copy` = the *span's own bytes* are sliced out of
`original_qmd` and emitted as that node's text, so the **byte-identity** claim
`preimage_in` does not make is load-bearing. `locate` = only the **offset**
claim is used — the hull bounds a search, a containment/disjointness
comparison, a fused `SourceInfo`, or a slice of bytes lying strictly *outside*
every resolved span.

That last case is called out explicitly rather than buried. Seven sites slice
`original_qmd` at a **complement** range — the gap between two block hulls, the
prefix/suffix around a block's inlines, the delimiters around a container's
children — and emit those bytes. By the plan's literal wording ("slices source
text that is then emitted") they touch the `copy` definition. They are
classified `locate` because every byte they emit lies outside all resolved
spans: a hull that over- or under-claims *content* still marks the right
*source position*, which is exactly what `preimage_in` licenses. They are
labelled `locate (complement)` so the reading is auditable.

| file:line | locate / copy | what it does with the range |
|---|---|---|
| `incremental.rs:205` (171) | **copy** | slices `original_qmd` → `CoarsenedEntry::Verbatim` |
| `postprocess.rs:660` | locate | min/max span over a run |
| `incremental.rs:455` (421) | locate (complement) | `compute_separator`: `prev_span.end` — bounds the inter-block gap slice |
| `incremental.rs:458` (424) | locate (complement) | `compute_separator`: `curr_span.start` — the gap's other bound; gap bytes lie between the two hulls |
| `incremental.rs:723` (669) | locate (complement) | `assemble_inline_splice`: block hull; bounds the `## `-style prefix and the trailing suffix |
| `incremental.rs:726` (672) | locate (complement) | first inline's hull; prefix end + ordering guard |
| `incremental.rs:729` (675) | locate (complement) | last inline's hull; suffix start + ordering guard |
| `incremental.rs:816` (746) | **copy** | `InlineAlignment::KeepBefore`: `original_qmd.get(range)` pushed into the emitted result |
| `incremental.rs:868` (798) | **copy** | `assemble_recursed_container`: with no nested plan or no children, returns `original_qmd.get(orig_span)` as the container's text; otherwise bounds the delimiters |
| `incremental.rs:897` (821) | locate (complement) | first child's hull; bounds the opening delimiter slice |
| `incremental.rs:902` (826) | locate (complement) | last child's hull; bounds the closing delimiter slice |
| `incremental.rs:1200` (1116) | locate | tiling auditor: block hull passed down as the containment parent |
| `incremental.rs:1337` (1253) | locate | tiling auditor: table row hull → attr audit + containment |
| `incremental.rs:1348` (1264) | locate | tiling auditor: table cell hull → attr audit + containment |
| `incremental.rs:1383` (1299) | locate | tiling auditor: `CustomNode` Block slot → `check_containment` |
| `incremental.rs:1390` (1306) | locate | tiling auditor: `CustomNode` Inline slot → containment + tightness |
| `incremental.rs:1449` (1365) | locate | tiling auditor: inline hull → `check_tightness` |
| `incremental.rs:1456` (1372) | locate | tiling auditor: inline hull passed down as the containment parent |
| `incremental.rs:1648` (1564) | locate | `audit_attr_source`: kv key/value hull → tightness, containment, disjointness |
| `incremental.rs:1683` (1599) | locate | `resolve_units_from_iter`: hull → `AuditUnit.range`, else a census finding |
| `incremental.rs:1752` (1668) | locate | `classify_none_concat`: per-piece hulls → inter-piece gap classification |
| `postprocess.rs:314` | locate | `hull_source_infos`: `first.start` for a fused `SourceInfo::original` |
| `postprocess.rs:315` | locate | `hull_source_infos`: `last.end` for the same fusion |
| `postprocess.rs:1817` | locate | `math_with_attr_span_source_info`: math start offset |
| `postprocess.rs:1823` | locate | same: attr content end offset |
| `postprocess.rs:1828` | locate | same: max piece end for a non-contiguous attr `Concat` |

The tiling auditor reads `src` bytes at `check_tightness` (`:1852-1882`), but
only to test whether a boundary byte is a space or tab. The single offending
byte is echoed into the finding's message via `{:?}` on `b as char`; nothing
else of the source is copied, and no run of source text is emitted. That is a
comparison, not a copy.

#### Plan 1's hypothesis: **half right, and the correction matters**

The hypothesis was "the split falls along the incremental-writer /
span-computation line, with `incremental.rs`'s `Verbatim` arms the only
copies." The *line* holds exactly — all six `postprocess.rs` sites and all ten
tiling-auditor sites are `locate`, and every copy is in the incremental writer.
The *enumeration* did not: findings § 3 named one copy site, and there are
**three**.

- `incremental.rs:816` — `InlineAlignment::KeepBefore` in
  `assemble_inline_content`. The inline analogue of `:171`, one nesting level
  down.
- `incremental.rs:868` — `assemble_recursed_container`'s two early returns
  (`nested_plan` is `None`, or `orig_children` is empty), which keep the whole
  container verbatim.

Both are "Verbatim arms" in spirit, so the hypothesis's *shape* survives; what
did not survive is the count. Recorded because the guard's blast radius depends
on it — but see the next section for what that radius actually is: T1 does
**not** simply "guard three copy sites".

#### Reachability, and what T1 actually covers — pinned vs argued

Checked at the consumer, per findings § 2. `incremental_write` has exactly two
production callers, and both hand it an `original_ast` of the protected shape.
**The split is by path, not by site: all three copy arms sit on both paths.**

**Pinned by T1.** `pampa/src/apply_node_edit.rs:120` deserializes
`untransformed_ast_json` — which *is* `capture_untransformed_ast_json`'s
output, round-tripped through the frontend. T1 asserts that artifact's pool
shape at the producer, so this path **inherits** the guard. Naming the
inheritance matters: nothing on the `apply_node_edit` side is itself asserted.

**Argued, not pinned.** `wasm-quarto-hub-client/src/lib.rs:2952`
(`incremental_write_qmd`) reaches its **own**
`qmd_to_pandoc(original_qmd.as_bytes())` on raw bytes. The invariant there is
*analogous* — a fresh, parent-less parse of the very text being sliced, so the
body is all-`Original` for the same structural reason — but **T1 does not
exercise that call**, and no other test does either. Change it and nothing
fails.

So `:816` and `:868` are latent for precisely the reason `:205` is, on both
paths. No fix is warranted and none was made. What is *not* true, and is
deliberately not written anywhere in this plan or in the code, is that "T1
guards three copy sites": T1 pins one entry point, a second inherits it, and a
third is argued only. A guard whose blast radius is overstated is worse than
one whose limits are written down.

#### T2 — not written

T2's frozen seam row binds it to "whichever newly-classified copy site **is
fixed**". The classification found two, but both are latent members of the very
class this phase deliberately guards rather than fixes, so no fix exists to
observe RED against; a test written anyway would be green on arrival and bound
to nothing — the vacuity this plan's seam discipline exists to prevent. Instead
`:816` and `:868` are named alongside `:205` in T1's failure message and in the
corrected `incremental.rs` `KeepBefore` comment, so a reader who trips the guard
sees all three copy sites — under the pinned/argued split above, which T1's doc
comment and the call-site comments both state.

**Ruling (controller, 2026-08-23).** T2 was ruled out on exactly this basis:
the seam's *intent* ("whichever copy site **is fixed**") governs over its
binary phrasing ("if all 24 are `locate`…"), which did not anticipate a third
state — copy sites found, none fixable. The accepted cost is stated plainly: if
that judgement is wrong, a real copy-emission defect at `:816`/`:868` ships
unguarded, and whoever finds it has this table naming the site and the
invariant that was supposed to protect it.

#### T1 — invariant pin, green for the stated reason

`crates/quarto-core/src/pipeline.rs`,
`preview_untransformed_baseline_body_pool_is_all_original_own_file`. Renders a
one-inline-footnote document through `render_qmd_to_preview_ast`, parses
`output.untransformed_ast_json`, collects every pool id referenced by a
**body node's `s` key**, and asserts each of those `astContext.p` entries is
wire-code `0` (`Original`) with `d == 0`. The capture's filename
table holds exactly one entry, so `FileId(0)` is the document itself and any
other value is foreign. The id set is asserted non-empty first, so the check
cannot pass vacuously (12 body entries for this fixture). The failure message
names both causes and points at `incremental.rs:205`, `:816` and `:868`.

**The `s` key is the whole reach, and the test now says so.** Attr and
link/image target provenance are *also* body-reachable pool refs, but
`write_attr_source` (`pampa/src/writers/json.rs:694-720`) and
`write_target_source` emit them through `to_json_ref` (`json.rs:430-433`) as
**bare integers** with no `s` key, so `collect_pool_ids` never sees them —
including the kv key/value provenance that this table's
`incremental.rs:1648` row audits. Both failure modes still redden the test
through the `s` ids, so the pin holds; its reach is simply narrower than
"everything the body touches", and the doc comment and failure message now say
that instead of "every source-info the baseline body reaches". Extending
`collect_pool_ids` to bare numbers under `blocks` would be **wrong**, not
merely more work: header levels, alignment indices and column widths are bare
numbers too.

Its doc comment carries a **"What this pin does and does not cover"** section
stating both limits — the pinned/argued split by caller, and the `s`-key
scope — so they travel with the test rather than living only in this document.

**The plan's T1 spec was wrong about the pool, and this is the correction.**
It specced "every `astContext.p` entry is wire-code `0`". Measured, that is
false and the test as specced is unpassable: the pool also carries
**front-matter metadata** provenance, which is legitimately `Substring`
(wire-code `1`). For this fixture the pool is entries `0..=11` for the body
(`Original`, `[40,73]`), entry `12` for the front-matter block (`Original`,
`[0,39]`), and entries `13..=26` `Substring` chains rooted at `12` — the
`title:` and `format:` scalars. Verbatim first run:

```
The untransformed baseline pool must be all `Original` (wire-code 0) rooted at
the document's own FileId(0); found 14 entry/entries that are not:
  [13] {"d":12,"r":[4,34],"t":1}
  [14] {"d":13,"r":[20,30],"t":1}
  ...
```

Restricting to the body is not a weakening — it is the set that load-bears.
`coarsen` and `assemble` walk `original_ast.blocks` and their inlines and copy
bytes from those spans alone; metadata is never byte-copied, and a folded
scalar in front matter would put a genuine `Concat` in the pool and harm
nothing. The test's doc comment records this so nobody "fixes" it back to the
whole-pool form. (The test was named
`..._baseline_pool_is_all_original_own_file` in that first run and renamed to
`..._baseline_body_pool_...` with the correction; it was never green under the
old name, so no frozen seam was edited.)

**Green for the stated reason — both failure modes exercised.** The pin has no
line-level hunk, so its binding was demonstrated by two throwaway probes rather
than asserted:

- *Mode 1 (`Substring`).* The first run above, over the whole pool, shows the
  assertion discriminating wire-code `1` and rendering the message.
- *Mode 2 (`Generated`).* Repointing the collection at `output.ast_json` — the
  post-transform AST, i.e. exactly "the capture moved downstream of the
  stages" — reddens it with the transform-injected nodes:

  ```
  Every source-info the baseline body reaches must be an `Original` (wire-code 0)
  rooted at the document's own FileId(0); 5 of 23 are not:
    [9]  {"d":{"by":{"kind":"appendix"}},"r":[0,0],"t":4}
    [10] {"d":{"by":{"kind":"footnotes"}},"r":[0,0],"t":4}
    ...
  ```

  This also validates the fixture choice: `footnotes` (and `appendix`) do run
  in the q2-preview pipeline, so a drifted capture is demonstrably caught.

Both probes were reverted; the committed test asserts the baseline body and
passes.

#### `incremental.rs:162-168` — comment corrected (now `:162-212`)

The old comment asserted the byte-identity reading Plan 1 retracted upstream
("it must have a byte preimage in the target file"). Replaced with: the
`.get()` guard checks **bounds, not identity**; a `Some(hull)` from
`preimage_in` is an offset claim only (0.1.3 doc comment, quoted below); and
the arm is safe only because the baseline AST is untransformed and parent-less
— with a pointer to T1 and to the two sibling copy sites. Both siblings got a
short note of their own at `assemble_inline_content`'s `KeepBefore` arm and
`assemble_recursed_container`'s verbatim early return, each pointing back to
the long note rather than restating it. All three notes name *functions and
arms* rather than line numbers **for the copy sites**, so a rebase cannot make
them point at the wrong arm. They do cite line numbers for cross-file
references (`apply_node_edit.rs:120`,
`wasm-quarto-hub-client/src/lib.rs:2952`, `source_info.rs:410-425`), which is
the ordinary rot risk and not the one that matters here.

#### Two accessor-rule violations, recorded not fixed (strand `bd-6392eba3`)

Surfaced by review, outside the `preimage_in` surface this phase enumerates.
Two helpers in `incremental.rs` read a span as
`si.start_offset()..si.end_offset()` — the pair findings § 1 calls **silently
wrong** on a `Concat`, returning `0` and the *content* length rather than the
source hull:

| function | the pair | production callers |
|---|---|---|
| `block_source_span` (`:550`) | `:552` | one — `assemble`, via `first_block_start` (`:369`) |
| `inline_source_span` (`:1033`, `pub`) | `:1035` | **none**; only `tests/integration/inline_splice_safety_tests.rs` |

`inline_source_span`'s callers were checked because it is `pub`: every one is
in that single test file, none outside the crate. So nothing in production
inherits the risk from it — worth stating, because it changes who the guard
has to cover.

**The failure mode at `block_source_span` is worse than a wrong offset — it
silently drops the front matter.** `assemble` tests `start > 0` (`:376`) to
decide whether a front-matter region exists and then copies
`original_qmd[..start]` into the output (`:381`). A `Concat` first block makes
`start_offset()` return `0`, so the branch is skipped entirely: no panic, no
diagnostic, no wrong bytes — just missing bytes.

Latent by exactly the invariant T1 pins (all-`Original` body), with the same
pinned/argued split by caller as the copy sites. **Recorded, not fixed**: the
fix returns `Option` and threads it through callers — for `assemble`, `None`
must be distinguished from `Some(0)`, since "cannot tell" is not "no front
matter" — which needs its own tests and lies outside Phase 1's guard-not-fix
mandate. Filed as **`bd-6392eba3`** (`discovered-from: bd-mxa44voa`); a doc
comment at each function points here and at T1.

Unlike the two new copy sites, these *do* warrant a strand: they are
`start_offset`/`end_offset` consumers, which no phase of this epic enumerates,
so a fix for them is genuinely out-of-plan rather than a decision the plan
already made. Noted for the irony and the lesson: this same file already
carries two comments warning against this exact accessor pair (`:714`, `:805`),
both added when it caused earlier bugs — the warnings were written at the sites
that had been burned, never applied to the file's own helpers.

#### Findings § 3 item 3(a)

Already corrected in `1102211f4` (the round-2 plan revision), which replaced
the `:1013` `parent_source_info: None` mechanism with the `:1007` re-parse. No
further edit was needed. Findings § 3's "the other 24 are unclassified" and
"one confirmed copy site" sentences were updated to point at this table.

#### Item 5 — Plan 1's 0.1.2 blanket-`None` is regression-free

The shipped `preimage_in` (0.1.3, `source_info.rs:458-503`) makes the
`Substring`-over-`Concat` arm return `None`; the bare `Concat` arm still
returns `Some(hull)` for byte-contiguous pieces.

**(a) `postprocess.rs:660`'s `combine(first, last)` fallback moves no
snapshot.** `77bd9d6c0` ("chore: refresh lock onto quarto-source-map 0.1.3 and
quarto-yaml 0.1.3") touched `Cargo.lock` and `Cargo.toml` only — **zero `.snap`
files** — and the branch has been green since. `contiguous_hull_for_run` takes
the fallback only when a piece fails to resolve or when an element is a
`Substring` over a `Concat`; neither shape occurs in the tree's inline runs.
That is the answer: **none move.** (Per the plan, the `:1845-1852` module doc
is *not* cited here — that bug was `combine(self, self)` specifically.)

**(b) The q2 side agrees with Plan 1 on `cell_options`.**
`crates/quarto-core/src/cell_options/mod.rs:207-228` builds each `Concat` piece
as `(SourceInfo::substring(body_source, start, end), end - start)` — **length-
matched by construction**, the one production producer of that shape. Multi-
option cells are **gappy**: `option_content_ranges` returns
`content_start..line.len()` for a prefix-only language (`:260`), so the
next line's piece starts *after* its own `#| ` prefix and the two ranges are
not adjacent — `preimage_in`'s contiguity check already returned `None` before
Plan 1's change. Only a **single-option cell** yields one piece whose hull the
blanket-`None` removes, and only for nodes *beneath* the
`parse_with_parent(&yaml_text, yaml_parent)` re-parse (`:229`), which are
`Substring { parent: Concat }`. Both shapes match Plan 1's Phase 1 assertions.

#### The corrected `preimage_in` doc comment (0.1.3)

Quoted verbatim from
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quarto-source-map-0.1.3/src/source_info.rs:410-424`:

> Byte range in `target` that this `SourceInfo`'s preimage covers, if any.
>
> A `Some(hull)` licenses **locating** a position in `target` — it does not
> license **copying** bytes from it. For an `Original` or a `Substring` chain
> that bottoms out in one, the hull happens to be byte-identical to this
> node's content, so locating and copying coincide. For a `Concat`, the hull
> is an **offset claim only**: a piece's source run and its content run can
> have equal length and different bytes (a 1→1 fold, e.g. a decoded escape
> that happens to decode to the same length it was encoded in) — no length or
> contiguity check can detect this, and `SourceInfo` carries no
> verbatim/replacement tag once constructed.

and the `Substring` clause at `:431-435`:

> `Substring` → if the parent is a `Concat`, `None` — see above; the affine
> composition `parent_range.start + offset` is only valid when the parent is
> byte-identical to its content, which a `Concat` parent is not.

### Phase 2

**The three drift amplifiers are commented, not restructured.**
`hull_source_infos` (`postprocess.rs`) carries the full note — § 4's sentence
quoted verbatim, plus the mechanism (the two numbers `preimage_in` reports are
baked into a fresh `Original` and the inputs' chains dropped, so a producer
whose offsets drift yields a *confidently wrong* flat range with nothing left
downstream to say it was derived) and the operative instruction: **verify a
producer's provenance fix upstream of these calls**, because their output
cannot distinguish a corrected producer from an uncorrected one.
`contiguous_hull_for_run` and `math_with_attr_span_source_info` carry short
notes pointing at it. The latter additionally records that `attr_end + 1` is a
hardcoded assumption, not a measurement: it holds only while `attr_end` is a
raw-source coordinate (a `Concat` attr's `preimage_in` hull is an offset claim
only) and while the grammar admits nothing between the last attribute and the
closing `}`.

**The false sentence is corrected; the code is not touched.**
`hull_source_infos`'s pre-existing doc comment claimed it was "the only correct
way to fuse two spans into one `Original`", using `preimage_in` for the hull —
false under findings § 1, whose rule is the `map_offset(0)` /
`map_offset(length())` pair, *never* `preimage_in` for a hull. Correcting a
comment that asserts the byte-identity reading is this phase's mandate, not an
exception to it: it is the same retraction Phase 1 had to make at
`writers/incremental.rs:162-171` ("a `Some(hull)` is an **offset claim, not a
byte-identity claim**"). The comment now says outright that this is not the
sanctioned way to build a hull and that the returned `Original` is not a
byte-identity claim, and keeps the tension note — `preimage_in` on a `Concat`
supplies an offset claim while the returned `Original` reads downstream as
both; § 4's verdict is about today's inputs, not a property of the flattening;
and since 0.1.2 `preimage_in` also *refuses* for a `Substring` over a `Concat`,
so those inputs silently take the coarse `combine()` fallback where the
`map_offset` pair would have produced a tight hull.

**Migrating the three sites onto the `map_offset` pair is `bd-ostgyku0`**
(`discovered-from: bd-mxa44voa`), deliberately not done here: it is a behaviour
change on pampa's live postprocess path with real snapshot risk, outside a
comments-only phase. All three doc comments name the strand, and it carries
§ 4's safe-by-shape triage as the reason it is latent rather than broken, plus
the instruction to re-derive `math_with_attr_span_source_info`'s `+ 1` rather
than carry it across the migration.

**Plan 1's hand-off — the third and fourth `offset_to_location` rules, measured.**
Fixture `"aé b"` ('a' at 0, 'é' spanning 1..3); the discriminating offset is 2,
inside 'é'. Also measured on `"xx\nyé z"` for the `line_start != 0` path.

- `quarto_source_map::utils::offset_to_location` (0.1.3, the Plan 1 baseline) →
  `Location { offset: 1, row: 0, column: 1 }` — **floored**, enclosing char not
  counted.
- `FileInformation::offset_to_location` (0.1.3, after Plan 1's fix) →
  `Location { offset: 1, row: 0, column: 1 }` — **floored**. Agrees with the
  above, which is what Plan 1 achieved.
- `offset_to_location_bytes` (`quarto-parse-errors/src/error_generation.rs:330`)
  → **before this phase**, `Location { offset: 2, row: 0, column: 2 }` — offset
  **raw** (unfloored, and therefore not a char boundary) and column
  **overcounted by one**, because slicing mid-character leaves a truncated tail
  that `from_utf8_lossy` renders as a single `U+FFFD`. **Disagreed on both
  fields.** Its own doc comment claimed it "matches the source-map utility
  exactly" for valid UTF-8 — a premise that had never been exercised. This is a
  q2-side disagreement, so it is **fixed here** per the routing rule: the
  function now walks the line with `next_codepoint_size` (the same walker
  `advance_chars` already uses, so the file's two character-counting
  conventions are now one) and returns the floored offset with the enclosing
  character uncounted. Pinned by
  `offset_to_location_bytes_agrees_with_source_map_on_mid_character_offsets`,
  which asserts equality with *both* source-map implementations at every offset
  of both fixtures. RED before the change at exactly offset 2 (`left: Location
  { offset: 2, row: 0, column: 2 }`, `right: Location { offset: 1, row: 0,
  column: 1 }`); green after; `quarto-parse-errors` 18/18.
- **`quarto-yaml` 0.1.3 — no third rule exists; nothing to route.** The
  consumed 0.1.3 registry source is byte-identical to `~/src/quarto-yaml` at
  HEAD (`4734b46 Release 0.1.3`; the tag was never pushed, so `diff -rq`
  against the registry is the check that stands in for it). The crate has **no
  offset→location implementation at all** — it goes marker → *byte offset*
  (`byte_offset_of_char`, `parser.rs:270`, which advances only across UTF-8
  lead bytes and so cannot return a mid-character offset) and never computes a
  row or column from an offset. It constructs `Location` at exactly one
  production site, `parse_impl` (`parser.rs:133-142`), a whole-file hull; every
  other construction is under `#[cfg(test)]` (the test module starts at
  `:1242`). **No strand was filed**, and the reason is worth stating: that one
  site's `column: content.lines().last().map_or(0, |l| l.len())` is a *byte*
  count where `Location.column` is a char count everywhere else — but
  `SourceInfo::from_range` (`source_info.rs:185-191`) keeps only
  `range.start.offset` and `range.end.offset` and **discards row and column
  before they can be observed**. The arithmetic is dead on arrival, so there is
  no behavioural disagreement to route to `posit-dev/quarto-yaml` — but it is
  filed anyway as **`bd-ug9euvpk`** (`discovered-from: bd-mxa44voa`), because
  the deadness is a property of *who calls it today*, not of quarto-yaml: any
  consumer that reads `Location.column` off a quarto-yaml `Location` without
  going through `from_range` makes it live. The strand says plainly that it is
  currently unobservable, names `from_range` as the reason, and states what
  would make it live, so the receiving repo can schedule it as the latent API
  inconsistency it is rather than chase it as a live bug. (It also records a
  secondary defect in the same expression: `content.lines().count() - 1` drops
  the empty final line a trailing newline creates, so `row` is one short of the
  line `offset` actually points into.)

**T3 — `empty_source_info` is now a `Generated`.** RED first: the test asserted
`matches!(si, SourceInfo::Generated { .. })` on a block converted without a
`SourceLocationContext` and failed with `got Original { file_id: FileId(0),
start_offset: 0, end_offset: 0 }` — the seam row's predicted revert shape,
observed. The helper now returns `SourceInfo::generated(By { kind:
"comrak-to-pandoc", data: Null })`, so `root_file_id()` and `preimage_in()`
answer `None` instead of pointing at file 0, and the value stops matching
`SpanProblem::SuspiciousDefault`. `comrak-to-pandoc` 166/166 (14 pre-existing
`#[ignore]`s, none of them the proptest-roundtrip or differential suites —
both ran green); `pampa` 4501/4501.

*Fix round 1 additions.* Two doc claims in this phase's own output were
false and are corrected: `offset_to_location_bytes` and `advance_chars` both
claimed their character rule matched `from_utf8_lossy`'s. It does not — that
folds a maximal ill-formed subpart into one `U+FFFD`, so `[0xE2, 0x82]` is one
character to it and **two** to the walk. Measured: `[0xE2, 0x82]` 1→2,
`[0xF0, 0x9F, 0x98]` 1→3, while `[0xE2, 0x41, 0x42]` and `[0xFF, 0xFE, 0xFD]`
are 3 under both. So the invalid-UTF-8 column behaviour **did** change, and
the pre-existing `bd-6qbto` fixture is `[0xFF, 0xFE, 0xFD]` — the case where
the two rules coincide — so its passing across the change proved nothing.
`offset_to_location_bytes_counts_each_ill_formed_byte_separately` now pins the
rule and the divergence (verified bound: reverting the walk to `from_utf8_lossy`
gives `left: 1, right: 2` on `[0xE2, 0x82]`). `advance_chars`'s copy of the
same false analogy is corrected too, since unifying the conventions in code
while leaving one of them documenting the other rebuilds the trap one function
over.

*Fix round 2.* That correction itself shipped a false universal: it said the
two rules "coincide only when every ill-formed byte is independently invalid".
`[0xC2]` refutes it — `0xC2` is a *valid* 2-byte lead, merely truncated by end
of input, and the rules coincide there (1 and 1); so does `[0xC2, 0x41]` (2
and 2). The real boundary is subpart **length**: the walker advances one byte
per ill-formed byte while `from_utf8_lossy` emits one `U+FFFD` per maximal
ill-formed subpart, so the counts agree **iff every such subpart is exactly
one byte**. Verified exhaustively before being written down — 1554 sequences
over a 6-symbol alphabet (ASCII, 2-/3-/4-byte leads, a bare continuation, a
non-lead), **0 biconditional violations with 107 genuinely diverging**, so the
check discriminates rather than passing vacuously. Now pinned as
`ill_formed_counting_diverges_exactly_when_a_subpart_spans_multiple_bytes`,
with an anti-vacuity assertion that the alphabet still produces divergence.

*Retrospective — this phase produced three false doc claims, and they share a
shape.* The original `from_utf8_lossy` equivalence, the phantom `normalize.rs`
caller, and the "only when" above were all **universal or equivalence claims
about an adjacent system's behaviour, asserted without exercising it**. Every
claim in the same work that was *measured* — the four-row divergence table, the
revert-verified fixture binding, the three offset→location rules — was correct.
The failure was never analysis; it was asserting where measurement was cheap.
**Rule for anyone extending this phase: a sentence containing "only when",
"always", "exactly", "matches", or "never" is not finished until it is either
pinned by a test or narrowed to the cases actually run.** Phase 2's deliverable
is accurate documentation, so an unexercised claim in a comment is a defect of
the same kind as a bug in code — which is precisely what § 4's drift-amplifier
notes exist to prevent downstream. Also recorded on `offset_to_location_bytes`: flooring the *end* of a span
can collapse it to zero width on corrupt input (a 4-byte codepoint at `0..4`
with `byte_offset = 2`, `size = 1` floors both ends to 0 — measured), and
**only `offset` reaches production**, since all four call sites discard `row`
and `column` through `from_range`.

*T3's fixture was extended* from `"Hello world."` to
``"Hello *world* and `code`."`` because the original bound only one of the two
construction paths: it is all `Str`/`Space` from `tokenize_text`, and the
`source_info` from `inline.rs:23` is computed at `:46` but discarded by the
`NodeValue::Text` arm. Verified by reverting `inline.rs:23` alone — with the
old fixture every inline stayed `Generated` (no assertion fired); with the new
one the test reddens on `Emph`/`Code`. Also noted on the helper: an
anchor-less `Generated` is itself flagged, as `SpanProblem::Generated`
(`span_assert.rs:267`) — the intended trade, but not a clean bill.

*Manifest change, and why T3 needed one.* `By::data` is a `serde_json::Value`
and no existing `By` constructor describes this producer, so naming
`serde_json::Value::Null` required `serde_json` as a direct dependency of
`comrak-to-pandoc` — added as `{ workspace = true }`, resolving to the
workspace's existing `1.0.149`. It was already in the crate's compiled closure
transitively via `quarto-source-map`, so this adds no new crate to any build;
both lockfiles gained one line naming it as a direct dep of
`comrak-to-pandoc`. **No version floor moved anywhere**, so the preamble's
"all gates are closed" table needs no revision.

**Snapshot accounting: zero `.snap` files added, modified, or removed** — no
pending `.snap.new` either. See the Phase 2 checklist item above for why that
is structural rather than lucky, and the Phase 7 item for why that phase should
*not* expect the same.

### Phase 3

**The doc comment landed in `a7b2e8f96`** ("Plan 3 Phase 3 (T3): document
parse_with_parent as dead code"), refined by **`9d3bf333f`** ("Plan 3 Phase 3
(T3) follow-up: tighten the precondition wording"). It is live at
`crates/quarto-xml/src/parser.rs:50-81`, above the `pub fn` at `:82`, and
carries all three things the checklist asked for: the zero-callers status, the
byte-identical-slice precondition with the affine-composition reason, and the
attribute-value example (`unescape_value()` in `parse_attributes` against a
quote-inclusive `value_source` from `find_attribute_positions`).

_(Recorded 2026-08-23 in Phase 8's reconciliation: the work landed in Phase 3
and was verified in `task-3-report.md`; only this evidence line and the
checklist box were outstanding. The box is now ticked.)_

### Phase 4

T8 landed as `quarto_config_md_yields_no_byte_range`
(`crates/pampa/src/lua/config_value.rs`, `mod tests`). Measured shape of the
node returned by `quarto.config.md('x')`:
`Substring { parent: Generated { by: By::filter(..), from: [] }, 0..1 }`.

**Binding confirmed by mutation, not by reasoning.** Applying the specced
revert hunk — replacing `from: SmallVec::new()` in `filter_source_info`
(`crates/pampa/src/lua/types.rs`) with a single
`Anchor::invocation(SourceInfo::from_range(FileId(0), 0..10))` — turned T8
RED: `resolve_byte_range()` returned `Some((0, 0, 1))` against `None`. Hunk
reverted; T8 back GREEN.

**The `map_offset` half was also measured, and is confirmed inert.** With the
mutation still applied and only the `resolve_byte_range` assertion
neutralized, `map_offset(0, &ctx) == None` still passed — so that assertion
genuinely cannot redden, as the plan states.

**One narrowing.** Findings § 6 frames the three grounds as "any one
sufficient". Ground 1 (`map_offset`'s `Generated` arm returns `None`
unconditionally) is sufficient for `map_offset` only. `resolve_byte_range` is
safe *contingently*, on `from` staying empty — grounds 2 and 3 are what keep
it empty, and ground 1 does not cover it. The mutation above is the evidence.
The comment at the constructor states the two accessors separately for this
reason, and **findings § 6 now carries a dated correction (2026-08-23)** saying
so at the source — Phase 6 and Phase 8 both cite § 6, so the false "any one
sufficient" could not be left standing there.

Grounds 2 and 3 re-verified against the current tree: `append_anchor` now has
**8** call sites (§ 6 said 7 — `crates/quarto-config/src/span_assert.rs:577`
is new), and all 8 sit inside their file's sole `#[cfg(test)]` module, which
in each case is the last top-level item; `Arc::make_mut`/`Arc::get_mut` still
have zero matches in `crates/`.

No production behaviour changed in this phase — one test and one comment.

### Phase 5

**T4 went RED.** The probe is
`probe_build_source_map_over_writer_concat`, added next to
`test_build_source_map_maps_lines_to_file_provenance` in
`crates/quarto-core/src/engine/ts_engine.rs`. It is `#[ignore]`d, pointing at
**bd-8hrjqcx0** (`discovered-from: bd-mxa44voa`), and nothing was fixed — the
drift is writer provenance, a different bug class.

**PLAN DEFECT (found in execution, resolved; not an open item).** T4's two
instructions are **incompatible as written**: "extend
`test_build_source_map_maps_lines_to_file_provenance`" and, in the same
checklist item, "leave the new test `#[ignore]`d". `#[ignore]` is
per-*function*, so extending the existing test in place and then ignoring it
would have suppressed a live green test — which the frozen-test-seam rule
forbids. Resolved in favour of the binding constraint over the literal wording:
the probe is a **sibling** `#[test]` fn, and
`test_build_source_map_maps_lines_to_file_provenance` is untouched (the commit's
diff is a pure append, `@@ -3041,4 +3041,158 @@`; no assertion weakened,
none tightened, no harness change).

Ruled correct by the plan owner on 2026-08-23. Recorded here as a defect in the
plan text rather than only as an executor deviation, so **Phase 8's
reconciliation does not read the ticked boxes as diverging from the checklist**
— they satisfy its intent, not its letter. Nothing further is owed on it.

**The existing test is vacuous for this purpose, as findings § 6 says.** Its
`input` is `&file_content[7..]` — it asserts that identity outright
(`assert_eq!(&file_content[7..], input)`) — paired with a single `Original`
span over exactly those bytes. Its assertions were left alone: they do
discriminate a *stub* `build_source_map` (the pre-`6a5f80fc4` `Vec::new()`),
which is what they were written for.

**The two production sites, and how the others were excluded.** A single grep
for `map_offset` across `crates/quarto-core/src/` returns 23 lines in 8 files
(26 in 8 after this phase's probe adds 3 more prose mentions in `ts_engine.rs`);
every one was classified.

- `stage/stages/engine_execution.rs` — 6 occurrences, **all** past the file's
  sole `#[cfg(test)]` at `:777` (module opens `:778`). Three are identifiers or
  an assertion string (`:2282`, `:2296`, `:2307`); the three real calls are
  `:2293`, `:2315`, `:2320`. Matches findings § 6, which cites `:2293`.
- `engine/ts_engine.rs` — 3 occurrences: doc-comment prose at `:667` and
  `:2966`, and **one call at `:683`**, in `build_source_map`. **Production.**
- `engine/jupyter/text_execute.rs:494` — **one call**, in `describe_location`.
  **Production.**
- `cell_options/mod.rs` (`:653`, `:669`, `:682`), `crossref/codeblock_shorthand.rs`
  (`:1436`, `:1439`), `stage/stages/include_expansion.rs` (`:1655`, `:1978`) —
  every call is past its file's sole `#[cfg(test)]` (`:320`, `:685`, `:838`
  respectively), so all seven are test code.
- `cell_options/mod.rs:41`, `crossref/codeblock_shorthand.rs:1430`,
  `transforms/attribution_render.rs:53`, `engine/context.rs:83`, `:90`,
  `stage/stages/include_expansion.rs:332` — prose, no call.

So the **textual `map_offset` call sites in `quarto-core/src/`** reduce to
exactly those two, confirming findings § 6's "two production sites, not three".
(Grep-bounded: this is a claim about textual occurrences under `src/`, not about
reaches through some other accessor that calls `map_offset` internally.)

**What is new here is only the last four rows.** Findings § 6 already names both
production sites (`2026-08-21-provenance-audit-findings.md:381-386`) and already
excludes `engine_execution.rs:2293` as `#[cfg(test)]`. The classification above
adds that the other five files — `cell_options`, `codeblock_shorthand`,
`include_expansion`, `attribution_render`, `engine/context` — are *entirely*
test code or prose, which § 6 did not state.

**Only `build_source_map` was exercised.** That the probe also characterizes
`describe_location` is a **code-reading argument, not a measurement** — the
drift lives upstream of both consumers, in the writer's `Concat`, and that is
where it was measured. An earlier revision of this section said the two sites
"make the identical `info.map_offset(offset, &ctx.source_context)` call against
the same `ExecutionContext` provenance"; **that ground is stronger than the code
supports and is corrected here.** `describe_location` is never called on
`ctx.source_info` itself. Its three call sites (`text_execute.rs:314`, `:315`,
`:339`) pass either `body_source` — a `SourceInfo::substring` of the same
`ExecutionContext` provenance, built at `:305-309` — or a YAML error's
`e.location()` derived from it, and **always at `offset: 0`**. The receiver is a
derived `Substring` at a fixed zero offset, not the provenance at arbitrary
offsets.

The conclusion survives (a `Substring` over a `Concat` inherits the drift), but
the old wording *hid* something: the `Substring`'s bounds are
`block.code_start .. + block.code.len()`, and `code_start` comes from
`parse_code_blocks` regex-matching the **written** QMD
(`text_execute.rs:124-147`, `code_start: code_match.start()` at `:147`). Those
bounds are themselves in written-QMD coordinates — an **additional** exposure at
the jupyter site, not an identical one. Recorded on bd-8hrjqcx0 for the fixer.

**The fixture is genuinely non-identity — measured, not assumed.** Input
`"Intro paragraph.\n\n  - alpha\n  - beta\n  - gamma\n\nOutro.\n"` is 55 bytes;
`write_with_source_info` emits 49, rewriting each `  - x` to `* x`:

```
in : 496e 7472 6f20 7061 7261 6772 6170 682e  Intro paragraph.
     0a0a 2020 2d20 616c 7068 610a 2020 2d20  ..  - alpha.  -
     6265 7461 0a20 202d 2067 616d 6d61 0a0a  beta.  - gamma..
     4f75 7472 6f2e 0a                        Outro..
out: 496e 7472 6f20 7061 7261 6772 6170 682e  Intro paragraph.
     0a0a 2a20 616c 7068 610a 2a20 6265 7461  ..* alpha.* beta
     0a2a 2067 616d 6d61 0a0a 4f75 7472 6f2e  .* gamma..Outro.
     0a                                       .
```

The test asserts both `input != source` and `input.len() != source.len()`, so a
writer change that made the fixture round-trip reddens the probe rather than
silently making it vacuous.

**The measurement.** `build_source_map` over the writer's `Concat`:

```
line 0 "Intro paragraph." @ 0  -> reported 0  | true 0
line 1 ""                 @ 17 -> reported 18 | true 17
line 2 "* alpha"          @ 18 -> reported 19 | true 18
line 3 "* beta"           @ 26 -> reported 27 | true 28
line 4 "* gamma"          @ 33 -> reported 34 | true 37
line 5 ""                 @ 41 -> reported 48 | true 47
line 6 "Outro."           @ 42 -> reported 49 | true 48
```

Blank separator lines are printed but not asserted on: the writer synthesizes
them, so they are not guaranteed to correspond to any original byte. (The
exclusion is conservative, not convenient — this fixture's original *does* have
blank lines, at 17 and 47, and both drift by +1, so excluding them **removes**
violations.) Asserted violations:
`(line, reported, true) = [(2, 19, 18), (3, 27, 28), (4, 34, 37), (6, 49, 48)]`.

**Two mechanisms, isolated by an identity control.** The same chain was run
over `"Intro paragraph.\n\n* alpha\n* beta\n* gamma\n\nOutro.\n"`, which the
writer round-trips byte-for-byte (verified with `cmp`). It reports a *uniform*
`+1` on every line after the first block (`17→18, 18→19, 26→27, 33→34, 41→42,
42→43`). So:

1. **Constant +1 per block after the first**, present under a byte-identical
   round trip. `write_impl_tracked` writes the separator `writeln!` *inside*
   the measured piece (the comment "include preceding blank line in
   measurement" is at `pampa/src/writers/qmd.rs:2891`; the loop it heads runs
   `:2892-2900`) but pairs it with a span starting at the block's first
   *content* byte.
2. **Accumulating drift inside a rewritten block** — +1, −1, −3 across the
   three items, stepping by −2 each, exactly the bytes each `  - ` → `* `
   rewrite removes. This half exists *only* under the non-identity fixture,
   which is what makes the fixture load-bearing rather than decorative.

**Why it is not this bug class.** `write_impl_tracked` pushes
`(block.source_info().clone(), buf.len() - start)` — each block's *source* span
paired with the bytes the *writer* emitted. Piece lengths are in written-QMD
coordinates, the spans in original-source coordinates. That `Concat` is what
`serialize_ast_to_qmd` (`engine_execution.rs:729-732`) returns and
`engine_execution.rs:467` installs as `ExecutionContext::source_info`. It is a
writer-provenance defect; `ProvenanceBuilder` does not address it, and no
decoder is involved. Recorded in bd-8hrjqcx0 with the full measurement.

**The strand carries the same honesty label.** `describe_location` was reasoned
about, not exercised — only `build_source_map` was measured — so a second probe
against the jupyter path was declined (it would add a second red `#[ignore]`d
test for an already-measured *upstream* fact) and `braid comment c-bl1zsr0x`,
amended by `c-axifcrbt`, instead tells the eventual fixer to verify both
consumers, gives the corrected ground (a `Substring` at offset 0, not the same
call on the same receiver) and names the additional `code_start` exposure. The same
comment records why the strand was retitled from "drift within a block" to
"drift on every multi-block document" and raised to priority 1: **mechanism 1
has the broader reach** — it fires on every block after the first in every
multi-block document fed to an engine, normalization or not, as the identity
control shows. Mechanism 2 is the narrower half.

The control test was a throwaway measurement, not committed; the committed
probe covers the non-identity case and its doc comment records the control's
numbers.

No production behaviour changed in this phase — one ignored test.

### Phase 6

_(One sub-heading per task. 6a–6f all have their sub-heading below as of
2026-08-23; the phase's own checklist has one box still unticked — the upstream
PR — for the reason recorded under 6f.)_

#### 6a — T7 + the `codeblock_shorthand.rs` bounded body search

**T7 went RED against the whole-block `find`, then GREEN.** The test is
`body_source_for_locates_the_body_not_the_info_string`, in the
`codeblock_shorthand.rs` test module. Fixture ```` ```{python}\npython\n``` ````;
`cb.text` is `"python"` (no trailing newline — the earlier probe's
`cb.text="python"` is exact). It asserts the body span with the
`map_offset(0)`/`map_offset(length())` pair, per findings § 1.

```
$ cargo nextest run -p quarto-core body_source_for_locates_the_body_not_the_info_string
    FAIL ... assertion `left == right` failed: body span must be the body between
    the fences, not the `python` inside the `{python}` info string at 4..10
      left: (4, 10)
     right: (12, 18)
```

**Revert binding verified explicitly.** With the fix in place and only the
bound hunk reverted (bounded region search → whole-block
`block_text.find(&cb.text)`), the same test fails with the same
`left: (4, 10)` / `right: (12, 18)`; restoring it returns to PASS. The seam
discriminates.

**The fix.** `body_source_for` now searches
`block_text[region_start..region_end]`, where `between_fence_lines` (new,
adjacent) returns the region after the opening fence line up to the start of
the closing fence line **when the last line is a bare fence** (`is_fence_line`:
≥3 of one fence char, whitespace-trimmed, so CRLF is handled). The closing
fence is *detected*, not assumed, because tree-sitter error recovery emits
blocks without one. The block-span fallback is unchanged.

**`resolve_byte_range` at the old `:476` is preserved, deliberately**, and the
reason is now in the doc comment: a byte search needs a span that is
byte-identical to a contiguous run of one file, and `resolve_byte_range` is the
accessor that answers exactly that — `None` on a `Concat`, an *honest* failure
that lands on the fallback. The `map_offset` hull answers a different question
and would licence composing offsets over a parent that is not byte-identical to
its content.

**Holes: one confirmed, one retracted.** Probed (temporary probe, removed):

| fixture | `cb.text` | new span | truth |
|---|---|---|---|
| ```` ```{python}\npython\n``` ```` | `python` | 12..18 | 12..18 ✅ |
| ```` ````{python}\n```\n```` ```` | ```` ``` ```` | 13..16 | 13..16 ✅ |
| ```` ````{python}\n```\n ```` (no closing fence) | ```` ``` ```` | 0..17 (whole block) | 13..16 ⚠️ |
| ```` ````{python}\nx\n```\n ```` (no closing fence) | `` x\n``` `` | 0..19 (whole block) | 13..18 ⚠️ |
| `` - item\n\n  ```{python}\n    x\n  ```\n `` | `  x` | 24..27 | 24..27 ✅ |
| `` > ```{python}\n> > x\n> ```\n `` | `> x` | 16..19 | 16..19 ✅ (see below — this row does **not** exercise the fence bound) |
| `` ```{python}\nprint('hi')\n `` (no closing fence) | `print('hi')` | 12..23 | 12..23 ✅ |

**Row 7 is the reassuring one, and it belongs in the record.** An error-recovery
block with an *ordinary* body still resolves exactly (`12..23`), so rows 3–4's
fallback is specific to a **fence-shaped final line**, not to missing closing
fences in general.

**Row 6 measures the earliest-match property, not the fence bound** (correction
2026-08-23, fix round 1). Its closing fence is `` > ``` ``, and `is_fence_line`
trims only whitespace — so the marker-prefixed fence is **not** detected, the
region runs to the end of the block, and the bound never fires. The row is a
true ✅, but what carries it is the body matching earlier in the region, not the
fence bound working inside containers. Row 5 is the row that *does* exercise the
bound in a container: its `` ``` `` closing fence is whitespace-indented, and
`trim` removes indentation. Same verdict, different route; the
`between_fence_lines` doc comment now says which is which.

So the recommendations doc's named hole — *a body consisting solely of fence
characters* — is **not** a hole when the closing fence is present (row 2). The
real hole is narrower and differently shaped: a body whose **last line** is
made only of fence characters **in a block with no closing fence** (rows 3–4),
where the detection eats the body's own final line and the search then fails →
block-span fallback. That is **coarser than the plan predicted** ("a few bytes
off"): it degrades to the whole block, not to a shifted span. Both are inside
the block. The doc comment says this, and says it was measured rather than
proven.

The second hole an earlier draft of the doc comment asserted — a
list-indented body whose own text begins with spaces matching a few bytes
early — **was written, then measured, then retracted**: rows 5–6 were built
specifically to produce an early match and both resolved to the true offset
(a run of *k* spaces followed by content can only align one way). The comment
now says the uniqueness is *unbroken by these probes*, not proven.

**The `:1375` test did not move — measured, not assumed.**
`nested_concat_cell_options_caption_resolves_correctly` consumes
`body_source_for`'s output. Probing its fixture under both the new bounded
search and the reverted whole-block search gives the **identical** span:

```
PROBE1375 cb.text="#| label: fig-plot\n#| fig-cap: \"A *strong* claim.\"\nprint('hi')" span=12..74
```

Its body text does not occur in the fence line, so the whole-block search's
first hit was already the true body start. It passes untouched.

**CRLF is measured, not reasoned** (added after the first report flagged it as
the change's one unexercised claim). Every shape above re-probed with `\r\n`:

| fixture | `cb.text` | span | truth |
|---|---|---|---|
| ```` ```{python}\r\npython\r\n``` ```` | `python\r` | 13..20 | 13..20 ✅ |
| ```` ````{python}\r\n```\r\n```` ```` | ```` ```\r ```` | 14..18 | 14..18 ✅ |
| `` ```{python}\r\n#| label: fig-a\r\nprint('hi')\r\n```\r\n `` | `#\| label: fig-a\r\nprint('hi')\r` | 13..42 | 13..42 ✅ |
| ```` ````{python}\r\nx\r\n```\r\n ```` (no closing fence) | `` x\r\n```\r `` | 0..22 (whole block) | 14..21 ⚠️ (same hole as its LF twin) |

**Not pinned by a test** (the checklist does not ask for one, and the hole was
ruled not-to-pin): the probe was removed, so no committed artifact re-derives
these rows — in particular the premise that the parser retains the `\r` in
`cb.text`, on which every row's truth column depends. Treat them as a dated
measurement, as with T4's probe. Same label is on the two doc comments.

The parser keeps the `\r` in `cb.text`, and the region starts after the `\n`, so
no stray `\r` leads it — span `13`, not `12`, in row 1. Note row 1 also shows the
T7 hazard is LF-specific *in that fixture*: with CRLF `cb.text` is `"python\r"`,
which the info string's `python}` does not contain, so even the whole-block
search would have landed correctly there. The bounded search is right either way.

Only the last row **discriminates** `is_fence_line`'s `\r` trim: it falls back to
the whole block exactly as its LF twin does, whereas an untrimmed `"```\r"` would
not have read as a fence, the region would have run to the block's end, and the
search would have succeeded at `14..21`. The three well-formed rows resolve
correctly with or without the trim, and the doc comment says so rather than
claiming they exercise it.

**Snapshots: zero added, modified, or removed.** `git status` after the change
shows one file, `codeblock_shorthand.rs`. This is bounded to what was run:
`body_source_for` has exactly four references, all in that file, and the only
shape whose span moves is a body that is a substring of its own info string —
the phase-boundary workspace run is what would show any snapshot elsewhere.

**Tests:** `cargo clippy -p quarto-core --all-targets -- -D warnings` clean;
`cargo nextest run -p quarto-core` → 4045 passed, 31 skipped (baseline 4044 +
T7).

**Strand filed:** **bd-lm75ion7** — "Carry `code_fence_content` provenance on
`CodeBlock` instead of re-locating the body by search"
(`discovered-from: bd-mxa44voa`), recommendations § 1 option 2.

#### 6b — `shortcode_string` closure deletion + `q_2_28`/`q_2_33` accessor swap

**Part A — the dead range computation.** `treesitter.rs:1002-1005` (the
`range` binding plus the `IntermediateBaseText(text, range)` wrap) is
deleted; `extract_quoted_text` now returns `String` directly. Narrowed
`process_shortcode_string`'s parameter to `&dyn Fn() -> String` and dropped
the callee's `let … else { panic!() }` — that `else` arm called
`extract_quoted_text_fn()` a **second time** just to format the panic
message, so narrowing the signature incidentally removes that double
evaluation too (not a separate fix; noted so it isn't mistaken for one).
Added a comment at the construction site naming the surviving range as the
quote-inclusive node span paired with the decoded string, with no consumer
reading it.

**Six-call-site verification, done by reading call sites rather than
trusting the plan's list.** Read all six lines the plan named in
`crates/quarto-core/src/transforms/shortcode_resolve.rs` (now at :135,
:171, :837, :848, :2236, :2269 — line numbers drifted a few lines from the
plan's :2232/:2265 from unrelated earlier edits, same sites): every one is a
`match … { ShortcodeArg::String(s) => … }` arm that only reads the string.
**The list is complete, and provably so rather than just unfound-by-grep:**
`ShortcodeArg` (`crates/quarto-pandoc-types/src/shortcode.rs:11-17`) has no
`SourceInfo`/range field on any variant, and the range is discarded at the
intermediate layer before a `Shortcode`/`ShortcodeArg` is ever built —
`treesitter_utils/shortcode.rs:91` and `:102` both destructure
`IntermediateShortcodeArg(arg, _)`, dropping the `Range`. So no seventh
consumer can exist: the type a `ShortcodeArg::String` reader receives
structurally cannot carry the range past that point, in this file or any
other. Also checked the two sites that *construct*
`ShortcodeArg::String` from resolved text (`shortcode_resolve.rs:726,
:764`) — these build new values from shortcode-resolution output, not
reads of the parsed arg's range, so they're consumers of the string only,
same as the six.

**Part B — the accessor swap.** `q_2_28.rs` (now :80-90) and `q_2_33.rs`
(now :74-88) replace `end_offset()` / `start_offset()`+`end_offset()` with
`resolve_byte_range()`, `continue`-ing on `None`. `q_2_33.rs` reads **both**
ends, confirmed by re-reading the pre-change lines — that one call now
supplies both instead of two separate accessor calls is a real
simplification there, not only the correctness fix. `file_id` is ignored in
both: each conversion's `read_violations`/`get_violations` path calls
`pampa::readers::qmd::read` on exactly one file's content, so every
diagnostic location it produces is necessarily in that file — a second
file_id could never appear to be silently mismatched. Added the
splice-safety-guard comment at `q_2_28.rs`'s `== ">}}}"` check (now
:129-134): even a wrong `error_offset` cannot splice wrong bytes, because
the check either finds the real `>}}}` shape or finds nothing.

**Accepted-untested, per the frozen seam spec — not a gap I tried to
paper over.** Both `Q-2-28` and `Q-2-33` are corpus-only (no Rust emission
site: `grep -rl 'Q-2-28' crates` / `'Q-2-33'` hit only the error corpus,
catalog, and the conversions themselves), and `find_violation_offsets`
takes an `offset: usize`, not a `SourceInfo`, so there is no seam to inject
a `Concat`-rooted diagnostic location through. The existing green test
`q_2_33_replace_range_is_byte_correct_with_escaped_attributes`
(`attr_provenance_splice_test.rs:253`) exercises the non-`Concat` path with
escaped attributes preceding the link target in the file and still passes
unedited — it is not evidence for the `Concat` branch, only that the
ordinary path is unchanged.

**Snapshots: zero.** `git status --porcelain` after both parts shows only
the four source files touched (`treesitter.rs`, `shortcode.rs`,
`q_2_28.rs`, `q_2_33.rs`) — no `.snap` files.

**Tests:** `cargo clippy -p pampa --all-targets -- -D warnings` clean;
`cargo clippy -p qmd-syntax-helper --all-targets -- -D warnings` clean;
`cargo nextest run -p pampa` → 4503 passed, 2 skipped; `cargo nextest run -p
qmd-syntax-helper` → 183 passed, 4 skipped (including all `q_2_28_test::*`
and `attr_provenance_splice_test::q_2_33_replace_range_is_byte_correct_with_escaped_attributes`,
unedited).

#### 6c — T11 + narrowing `is_gapless`

**T11 (the one sanctioned frozen-seam edit).**
`nested_concat_cell_options_caption_resolves_correctly`
(`crates/quarto-core/src/crossref/codeblock_shorthand.rs:1480`) had its
*access path* converted, not its assertion: the 20-line NOTE and the
hand-rolled `map_offset(0)`/`map_offset(length())` pair are replaced by
`resolve_span(inner, &sources).expect(..)`. **The asserted text is
byte-identical** — `"strong"`, with the same failure message ("the emphasis
span must underline the true source text through the nested-Concat parent,
not a shifted window"); the diff touches only the left-hand side of the
`assert_eq!`. The removed `assert_eq!(start.file_id, end.file_id, ...)` is
not a lost check: `resolve_span` performs exactly that comparison itself
(`span_assert.rs:373`) and returns `SpanProblem::Concat` when it fails, so
the `.expect(..)` subsumes it. Fixture, harness and expected value are
untouched, and no other test was edited.

**TDD — RED observed before the narrowing.** With T11 applied and
`is_gapless` still whole-`Concat`:

```
panicked at crates/quarto-core/src/crossref/codeblock_shorthand.rs:1555:14:
emph span should resolve through the nested-Concat parent: Concat
```

i.e. `Err(SpanProblem::Concat)`, exactly what the seam-spec row predicted.
GREEN after: `PASS ... nested_concat_cell_options_caption_resolves_correctly`.

**The narrowing.** `is_gapless` now delegates to `is_gapless_over(info,
queried, ctx)`, which threads an optional content sub-range. A `Substring`
layer translates the range into its parent's content space
(`parent_offset = start_offset + own_offset`, the same arithmetic
`SourceInfo::map_offset` does) and clamps it to its own extent, so nested
`Substring` chains keep track of where they sit in the root `Concat`. A
`Concat` with a range hands `concat_pieces_are_contiguous` only the pieces
that range touches (`pieces_touching`); a bare `Concat` (no range) still
gets every piece, which is why `gappy_concat_is_reported_not_guessed`
(`span_assert.rs:767`) is untouched — **confirmed by running it, not
assumed**: it queries a bare `Concat`, and it PASSes.

**Piece selection is over a *closed* interval — this is load-bearing.**
The checklist says "the pieces the queried content sub-range overlaps"; read
naively as half-open `[start, end)` that is **unsound**.
`Concat::map_offset` resolves an offset landing exactly on a piece boundary
inside the *next* piece (offset 0 of it), falling back to the last piece's
own end only at the concat's total length. So a query ending on a boundary
still reads its end position out of the following piece, and a gap at that
boundary silently widens the hull. Measured: with a half-open selector, a
`Substring(gappy_concat, 0, 4)` — four content bytes — resolves to
`Ok(ResolvedSpan { text: "6789AB" })`, six source bytes spanning the gap.
With the closed interval it returns `Err(Concat)`. Selection uses the
declared per-piece `length` (content offsets against content lengths, the
one place that field is right); every *position* still comes from
`map_offset`.

**Two new unit tests, each bound to a distinct hunk** (both verified by
reverting that hunk and observing RED):

| test (`span_assert.rs`) | revert hunk | observed RED |
|---|---|---|
| `substring_inside_one_piece_of_a_gappy_concat_resolves` | `is_gapless_over`'s `Concat` arm back to whole-`Concat` contiguity | `a sub-range inside one piece never crosses the gap: Concat` |
| `substring_ending_on_a_gappy_piece_boundary_is_still_refused` | `pieces_touching`'s `<=` back to `<` (half-open) | `left: Ok(ResolvedSpan { … text: "6789AB" })  right: Err(Concat)` |

**Comment rewrite (`span_assert.rs:187-203`).** The "conservative
over-approximation" paragraph is replaced with what is now true: the helper
is only as broad as its caller makes it, and **two** over-approximations
remain, both stated explicitly and both in the refusing direction (they can
report a gap-free span as gappy, never the reverse) — (i) a *partially*
covered piece that is itself a nested `Concat` is recursed into in full, not
narrowed; (ii) the first and last selected pieces are checked in full, which
costs nothing because only the boundaries *between* selected pieces can
fail. The new text does not claim the check is exact.

**All `resolve_span` consumers checked — the brief's list of six was
incomplete.** Pre-existing direct call sites outside `span_assert.rs`:
ten, not the six named (eleven counting the one T11 creates) —
`codeblock_shorthand.rs:1462` and the one T11 creates,
`project/listing/config.rs:1205`, `transforms/callout.rs:688/:704/:721/:748`
(the six named), **plus four the brief did not list**:
`quarto-config/src/convert.rs:557` and
`quarto-config/src/materialize.rs:500/:544/:615`. There are also two
*indirect* layers: `resolve_diagnostic_span` (`span_assert.rs:490` →
`resolve_span`), called at `listing/config.rs:1228/:1248`; and
`assert_diagnostic_underlines` (→ `resolve_diagnostic_span`), called at
`pampa/src/pandoc/meta.rs` ×2 (`:1034`, `:1035`) and `listing/config.rs`
×2. **None moved.**

**Blast radius re-confirmed as test-only**: `span-assert` is declared at
`quarto-config/Cargo.toml:29` and enabled only under `[dev-dependencies]` in
`quarto-core/Cargo.toml:127`, `quarto-config/Cargo.toml:34`,
`pampa/Cargo.toml:93`.

**Tests:** `cargo clippy -p quarto-config --all-targets -- -D warnings`
clean; `cargo clippy -p quarto-core --all-targets -- -D warnings` clean;
`cargo nextest run -p quarto-config` → 115 passed, 0 skipped (was 113 —
+2 new); `cargo nextest run -p quarto-core` → 4045 passed, 31 skipped, 0
failed. Also ran pampa's indirect consumers
(`-E 'test(link_title_provenance) or test(meta::tests)'`) → 51 passed.
**Snapshots: zero** — no `.snap` files touched.

**Review round 1 (2026-08-23) — one Important, two Minors, all addressed.**

The review confirmed the closed-interval finding by independent derivation
(`quarto-source-map-0.1.3/src/mapping.rs:53-72` worked by hand over the same
fixture) and proved something stronger than "no call site moved": the narrowing
is **structurally monotone** — `pieces_touching` returns a contiguous sub-slice
and every predicate in `concat_pieces_are_contiguous` is per-piece or
per-adjacent-pair, so the sub-slice's predicate set is a *subset* of the full
slice's. It can therefore only turn `Err(Concat)` into `Ok`, never the reverse;
no previously-green assertion *could* have reddened.

*Important — the residual-imprecision paragraph claimed a property the helper
does not have.* It said both remaining over-approximations were "in the refusing
direction … a piece's own extent is contiguous by construction". That
justification is false for one constructible shape:
`concat_pieces_are_contiguous:217` recurses only into a **bare**
`SourceInfo::Concat` piece, so a piece whose `source_info` is
`Substring{parent: Concat{gappy}}` skips the check entirely and its own endpoints
straddle the gap — an error in the **accepting** direction, a wrong `Ok`. I
measured it rather than accepting the reading: a single-piece `Concat` wrapping
`Substring(Concat[(6..10, 4), (12..15, 3)], 0, 7)` resolves to
`Ok(ResolvedSpan { … text: "6789ABCDE" })` — nine source bytes for seven content
bytes, silently including the gap's `"AB"`; the honest content is `"6789CDE"` and
the honest answer is `Err(Concat)`. (Probe run and reverted; not committed.)

The hole **predates** this phase — `concat_pieces_are_contiguous`'s loop body is
unchanged by `63936764b` — but the same monotonicity that makes the narrowing
safe makes the hole *more reachable*: a top-level `Substring` over a gappy
`Concat` used to be refused wholesale, and now resolves whenever the touched
pieces pass, so such a piece among them can produce the `Ok`. The comment is
rewritten to split the list into two refusing-direction imprecisions (with the
"contiguous" claim now correctly conditioned on the piece being `Original`,
`Generated`, or a recursed-into `Concat`) and **one named accepting-direction
gap**, carrying the measurement and the strand id. Tightening the check is out of
this phase's scope and is filed as **bd-qnubn7s0** (`discovered-from:
bd-mxa44voa`, P1, blast radius test-only), which records that the reachability
increase is a *consequence* of a correct narrowing, not a defect in it.

*Minor — a miscount inside the passage correcting a miscount.* `meta.rs` has
**×2** `assert_diagnostic_underlines` calls (`:1034`, `:1035`); `:1000` is a
comment naming the function. Corrected above, as is the loose "ten, not six"
sentence (ten *pre-existing* sites; eleven rows counting T11's).

*Minor — kept the `Concat`/`None` arm distinct* from `Some((0, length))` rather
than collapsing them, with a comment saying why: it is what makes "a bare
`Concat` keeps its pre-narrowing behaviour" exactly true, including for
degenerate zero-length pieces.

**Re-verified after the round-1 edits:** `cargo clippy -p quarto-config
--all-targets -- -D warnings` clean; `cargo clippy -p quarto-core --all-targets
-- -D warnings` clean; `cargo nextest run -p quarto-config` → 115 passed, 0
skipped; `cargo nextest run -p quarto-core` → 4045 passed, 31 skipped.

#### 6d — T9 + the `render.rs:904` guard wrap + two Plan 2 corrections

**T9 — `caught_panic_on_an_error_keeps_exit_code_nonzero`**
(`crates/quarto/tests/integration/diagnostic_render_panic_boundary.rs`). No new
file and no `main.rs` registration: the module is already registered, and
`run_q2_render_with_fault` (`:43`) already does `render .` with
`QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER=<N>`. Fixture: a website `_quarto.yml`
plus an `index.qmd` carrying two tables that share `{#tbl-dup}` — one `Q-15-1`
**error**, no warnings. Three assertions, exactly as the seam spec froze them:
stderr contains `internal error rendering diagnostic Q-15-1`; stderr does **not**
contain `Duplicate crossref identifier`; `!status.success()`.

> **Correction 2026-08-23 (review fix round 1).** The absence assertion first
> shipped with **`Duplicate Crossref Identifier`** — the *catalog* title from
> `error_catalog.json:1152`. **q2 never prints that string.** The rendered title
> is `Duplicate crossref identifier` (lowercase `c`, lowercase `i`), built at
> `crates/quarto-core/src/transforms/crossref_index.rs:381`; nothing in the
> render path reads `ErrorInfo::title`, which is consumed only for `docs_url`.
> `str::contains` is case-sensitive, so the original assertion held
> **unconditionally** — including in the very failure mode its own comment
> claimed to guard. The test was vacuous on exactly the half the seam spec calls
> "the rendered body is absent". Fixed to the rendered spelling, and the comment
> reworded to say *the diagnostic's own title*.
>
> **The discriminator is now proven in both directions**, by running the same
> fixture twice through the real binary rather than by argument:
> with the fault **disarmed**, stderr contains
> `Error: [Q-15-1] Duplicate crossref identifier`; with it **armed at index 0**,
> a case-*insensitive* `grep -c` for that phrase returns **0**. So the string is
> present exactly when the diagnostic renders and absent exactly when the guard
> fires — which is what makes the assertion load-bearing.

**Labelled in-file as an invariant pin, not a regression test.** Its doc comment
says so in the first sentence and spells out why no guard mutation can redden
it. T9 drives `render .`, so the path is `execute_project` (`render.rs:854`),
which prints at `:1024` and gates at `:1041`; `execute_single_doc` (`:770`) has
the same shape (`:836` / `:848`). The comment cites both. Both calls take
`&summary`, and the guard requires `UnwindSafe` *without* `AssertUnwindSafe`
(the bound is in the signature at `:1290-1293`, the rationale in the doc
paragraph at `:1278-1283`), so a swallowed render panic cannot touch the summary
the gate counts. The comment
names the one hunk that *would* redden it — "compute the exit status from the
diagnostics that were actually printed", a refactor nothing in the tree does
today — and explicitly corrects the ordering: printing is first, and the
invariant is immutability, **not** "counting happens before printing".

*The fixture is a transcribed copy* of `DUPLICATE_ID_DOC`
(`render_exit_codes.rs:29-31`), not a `pub(crate)` re-export of it, so no green
test file is touched. The copy is self-checking: if it stopped producing exactly
the `Q-15-1` error, the `internal error rendering diagnostic Q-15-1` assertion
fails.

**E2E, inspected** (not inferred from a green test). `.scratch/t9-e2e/`, same
two files. **Re-run on HEAD after the wrap landed** — the transcript an earlier
revision of this section quoted was captured *before* the wrap (its frames read
`:1242` / `:1375`, fourteen lines below HEAD's), so it could not witness a
post-wrap property. This one can:

```
$ QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER=0 cargo run -q --bin q2 -- render .
Rendering project: .../.scratch/t9-e2e (type: website)
thread 'main' panicked at crates/quarto/src/commands/render.rs:1256:9:
fault injection: QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER=0 (diagnostic Q-15-1)
   ... backtrace, frames 16-25 ...
   16  render::fault_inject_diagnostic_render          render.rs:1256:9
   17  render::render_diagnostic_guarded::{closure#0}   render.rs:1296:9
   21  render::render_diagnostic_guarded                render.rs:1294:11
   22  render::print_render_diagnostics_text            render.rs:1389:33
   23  render::print_render_diagnostics                 render.rs:1149:9
   24  render::execute_project                          render.rs:1024:5
   25  render::execute                                  render.rs:738:13
internal error rendering diagnostic Q-15-1
Rendered 1 of 1 files to .../_site — 1 error
EXIT=1
```

Four things read out of that output rather than assumed:

1. The fault reached the **Q-15-1** render — so index 0 still lands on the
   target diagnostic *after* the wrap added a ninth guarded site.
2. It landed at the coalesced per-page site, `:1389` (frame 22) — HEAD's line
   number, not the pre-wrap one.
3. Frame 24 is `execute_project` at **`:1024`**, independently confirming the
   project path's print call site used throughout this section.
4. The rendered body is absent and the counts clause still says **1 error** —
   the exit code came from the summary, which is exactly the immutability the
   pin records.

**The `render.rs:904` wrap** (`:904` names the pre-wrap `eprintln!` the plan
routed here; on HEAD the guarded block is `:904-919`, the call at `:916`). The pre-render loop over
`underscore_typo_diagnostics` + `project_kind_diagnostics` +
`config_diagnostics` in `execute_project` now reads
`render_diagnostic_guarded(code, || diagnostic.to_text(None))`, with a comment
saying the guard is **uniformity, not a fix**: `ctx = None` takes
`to_text_with_renderer`'s structured-text branch and never reaches a renderer,
so the byte-slicing path — the only known panic mechanism — is structurally
unreachable here today. The comment also names what changes that:
`config_sources` is built just above (`:884-889`), and the day it is bound and
passed here this becomes a ninth slicing site. (An earlier revision said
"fourteen lines above" — inherited pre-wrap geometry; the wrap itself pushed the
loop down, so the count was stale the moment it was written. The `:884-889`
anchor is right; the count is gone.)

*The upstream claim was measured, not transcribed,* against the version actually
locked (`quarto-error-reporting 0.2.2`): `to_text_with_renderer` at
`diagnostic.rs:442`, the excerpt-renderer gate
`if let (true, Some(ctx_val)) = (has_any_location, ctx)` at `:460-481`, the
structured-text fallback `if !has_source_render` at `:486`. The recommendations
doc § 7 cited `:461-481` and `:508-517`; the first is one line narrow at the
front and the second names the *ctx-present* branch, not the `at offset` write
(which is `:522`). **The code comment ships the measured anchors**, and a dated
correction is appended to recommendations § 7 — a comment must not carry a
citation its author has measured to be wrong, and propagating a known-wrong
citation "for greppability" makes both records worse.

**Accepted-untested, per the seam spec's own table**, and no test was written
for it. There is nothing to exercise: the wrap is a no-op on a path that cannot
panic. That is why the upstream reading above had to be measured rather than
assumed — it is the *only* evidence the wrap's safety claim rests on.

Two consequences worth recording rather than assuming:

**(a) The fault counter now spans nine sites, not eight.** A fixture whose
project config produces one of the three diagnostics in the newly-wrapped loop
would consume index 0 and steal the fault from the diagnostic a test meant to
target. It does not happen today, on two independent grounds. *Producers:* the
three are `underscore_typo_diagnostics` (`render_scripts.rs`, fires only on a
`pre_render`/`post_render` underscore misspelling), `project_kind_diagnostics`
(`project/mod.rs`, fires only for `book`/`manuscript`), and
`config.config_diagnostics` (ambiguous/incomplete project-type extensions) — a
bare `project: {type, output-dir}` config yields none of them, which is what
every fixture in `diagnostic_render_panic_boundary.rs` uses. *Witness:* the
post-wrap e2e backtrace above shows index 0 still reaching the Q-15-1 render.

**(b) No existing fixture is perturbed.** The whole `quarto` suite is green
after the wrap. Note this is suite-level evidence, not an enumeration of every
fixture's config diagnostics; ground (a)'s producer analysis is what makes it a
reason rather than a coincidence.

**Plan 2 correction 1 — § Hand-off item 9** (appended as a dated block quote
below the original text; the record is not rewritten). It states that the item's
ordering claim is backwards (`:836` prints, `:848` gates), that the property
actually holding is immutability via `&summary` + the non-asserted `UnwindSafe`
bound (`:1290-1293`, rationale `:1278-1283`), that the item's *conclusion* is
right and only its reason was wrong, that the guard's own doc comment
(`:1268-1270`) was already correct and unchanged, and that T9 discharges the
item. It also gives the project path's real anchors — `execute_project` `:854`,
print `:1024`, gate `:1041` — which item 9 had as `:1010`/`:1027`.

**Plan 2 correction 2 — Phase 5's `grep -c` evidence, 8 → 9**, appended to that
checklist line rather than replacing it. It carries the **pattern**, not just
the number: the count is of `render_diagnostic_guarded(` *with the trailing
paren*; grepping without it returns **12** on the same tree — the 9 call sites,
plus the definition `fn render_diagnostic_guarded<T>(` at `:1290` (which the
tight grep misses because of the `<T>`), plus two prose mentions in the
fault-injection seam's doc comment (`:1228`, `:1232`) — so a reader running the
looser grep would "correct" a correct record. Measured on HEAD: **9** with the
paren (`:916`, `:1349`, `:1361`, `:1389`, `:1478`, `:1503`, `:1524`, `:1540`,
`:1554`), **12** without.

**Plan 2 deferred-minor #6 — the optional document name on the `internal error
rendering diagnostic` line — was NOT done**, and the "two lines" estimate is
wrong. `render_diagnostic_guarded` takes only `code: Option<&str>`; a document
name is not in scope at three of the nine sites (the `project_diagnostics` loop
`:1361`, the newly-wrapped project-config loop `:916`, and JSON's
`project_diagnostics` `:1540`), and the coalesced sites (`:1349`, `:1389`) cover
a *group* spanning several files by construction. Threading it would change the
signature at all nine call sites and still leave the line format inconsistent.
Left undone deliberately; it remains a deferred minor, not a strand.

**Gates.** `cargo clippy -p quarto --all-targets -- -D warnings` → clean.
`cargo nextest run -p quarto` → **453 passed, 1 skipped, 0 failed**.
Measured delta: `cargo nextest list -p quarto | grep -c '^quarto::'` = **452
before, 453 after** — exactly the one added test, nothing removed or duplicated.
The workspace run is the orchestrator's, at the phase boundary.

#### 6e — T10 (founding-crash e2e pin) + T12 (selective replay of Plan 2 row 3)

**T10 — `crates/quarto/tests/integration/founding_crash_config_span_e2e.rs`**
(new file; registered in `tests/integration/main.rs` between
`extension_config_spans` and `get_config_cli`). One test,
`founding_repro_renders_clean_with_correct_carets`, fixture written inline as
Rust string literals — **LF line endings on every host**, since the `\n`s are
literal and nothing is read from disk.

Fixture `_quarto.yml`, verbatim:

```yaml
project:
  type: website
website:
  title: "T"
  navbar:
    left:
      - text: '<span id="x">Ask AI ✨</span>'
        href: index.qmd
```

plus `index.qmd` = `---\ntitle: "Index"\n---\n\nbody\n`. This is
byte-identical to the founding repro (`.scratch/ariadne-emoji-panic/repro/`),
which was inspected to confirm the geometry rather than assumed.

**Column arithmetic, confirmed against the real binary.** Line 7 is
`······- text: '<span id="x">Ask AI ✨</span>'` — **six** leading spaces, so
fifteen characters precede the `<`. 1-based **character** columns (`✨` is one
column, three bytes): cols 1-6 spaces, 7 `-`, 8 space, 9-12 `text`, 13 `:`,
14 space, 15 `'`, **16** `<` of `<span`; `<span id="x">` spans 16-28,
`Ask AI ` spans 29-35, **36** is `✨`, **37** is the `<` of `</span>`. The
plan's `:7:16` / `:7:37` therefore hold for *this* indentation, and the binary
prints exactly those.

Assertions: exit 0; exactly two `[Q-2-9]`; stderr contains `_quarto.yml:7:16`
and `_quarto.yml:7:37`; `_site/index.html` written (so a regression that skips
the render cannot pass by printing nothing).

**Two halves, labelled in the module doc.** The **carets** are a bound q2
regression guard. The **exit code** is an **upstream-behaviour pin** in the T6
sense — no q2 hunk reverts it; per recommendations § 4 the abort returns only
if q2's mapping regresses *and* both upstream guards are gone
(`quarto-source-map`'s `offset_to_location` floor, `quarto-error-reporting`'s
`snap_span_to_char_boundaries`). It is asserted anyway because it is the only
witness of the founding abort that exists anywhere.

**Discrimination proof — run both ways, and one plan correction.**

| mutation | T10 |
|---|---|
| none (branch HEAD) | **PASS** |
| `config_markdown.rs:326` → `let base = &value.source_info;` | **FAIL** |
| `meta.rs:255` → `let markdown_base = &source_info;` | **PASS** |

Under the `config_markdown.rs:326` revert, observed verbatim:

```
first Q-2-9 must be anchored at the `<` of `<span` — _quarto.yml:7:16. stderr:
   ╭─[ …/_quarto.yml:7:15 ]
   ╭─[ …/_quarto.yml:7:36 ]
```

Both carets shift one byte left: `:7:15` lands on the `'` quote delimiter, and
`:7:36` is the founding crash's own offset — the correct end sits at the `<` of
`</span>`, one byte left of which is byte 37, *inside* `✨` (bytes 35..38). It
prints as column 36 rather than aborting only because `quarto-source-map`'s
floor walks it back to the character start. This matches
`config_markdown.rs`'s pre-existing comment ("a caret one byte left of ideal,
not a crash") and recommendations § 4's recorded `:7:36`.

> **Plan correction (measured, not argued).** This plan's T10 seam row says
> reverting **either** `.unwrap_or(&…)` base reddens the test. **Only
> `config_markdown.rs:326` does.** Reverting `meta.rs:255` alone leaves T10
> fully green: the navbar `text:` value reaches the markdown re-parse through
> `ConfigMarkdownTransform`, not through `DocumentMetadata`, so the
> front-matter base is never on this fixture's path. This is the same
> two-different-code-paths fact Plan 2 hand-off (g) recorded, arriving from
> the other direction. The module doc states the narrowed binding and names
> where `meta.rs:255` *is* bound. Both facts are recorded rather than the
> fixture being widened to force the prediction true.
>
> **Twelfth instance, 2026-08-23 (review fix round 1).** The test file
> written to *carry* the eleventh-instance correction itself shipped the same
> defect shape: its half-1 doc said "Measured RED **both ways** for this
> test", meaning the two caret assertions — but a reader arriving from the
> seam row (which claims *either base* reddens T10) reads "both ways" as
> "both bases", which is the precise false belief the paragraph fifteen lines
> below exists to retract. Fixed in fix round 1 by naming what "both" ranges
> over and saying explicitly that it is not the two bases. Worth recording
> because of where it landed: the correction and the defect were authored in
> the same commit, by someone who had just been told about the defect. That
> is the strongest evidence available that this shape is **a habit of
> writing**, not a property of any one claim — so the countermeasure has to
> be mechanical (name what a quantifier ranges over, every time), not
> vigilance.
>
> **Ruled 2026-08-23: correct the seam row.** The freeze protects test
> assertions and harnesses — it exists to stop a test being edited into
> passing — and does not make the plan's *prose* immune to correction when
> measurement disproves it. A dated correction is appended to the T10 row in
> § Test seam spec; the original row is left as written. This is the
> **eleventh** instance of this plan's recurring defect shape, and the first
> found **inside the frozen seam table itself** — the artifact every other
> task in this plan has been told to trust.
>
> **Do not "improve" T10 by widening its fixture to cover both paths.**
> Widening it would break the asserted caret geometry (the columns are a
> function of this exact line-7 indentation) *and* the "exactly two `Q-2-9`"
> count, trading a precise pin for a vaguer one. The `meta.rs:255` path is
> better bound by T12's selective mutation anyway, because that also shows
> the *other* guard stays green under the same mutation — a discrimination
> a widened T10 could not supply.

**T12 — selective replay of Plan 2's audit row 3.** Plan 2 applied the row-3
mutation at both bases at once and reddened all five CLI tests; hand-off (g)
routed the selective version here. Mutation applied, one site at a time, is
Plan 2's own: *behave as if one leading delimiter byte were always stripped,
whether or not the scalar was quoted* —
`SourceInfo::substring(<base>.clone(), 1, <base>.length())`.

Guards: `json_errors::plain_scalar_raw_html_frontmatter_unaffected` (P) and
`json_errors::single_line_block_scalar_raw_html_unaffected` (B).

| mutation site | P (plain scalar, front matter) | B (single-line block, `_quarto.yml`) |
|---|---|---|
| none | PASS | PASS |
| `meta.rs:255` (`markdown_base`) alone | **FAIL** | PASS |
| `config_markdown.rs:326` (`base`) alone | PASS | **FAIL** |

Observed values, verbatim:

```
# meta.rs:255 alone
assertion `left == right` failed: plain scalar positions must be unaffected …
  left: [(2, 11, 2, 14), (2, 15, 3, 1)]
 right: [(2, 10, 2, 13), (2, 14, 2, 18)]

# config_markdown.rs:326 alone
assertion `left == right` failed: single-line block scalar positions must be unaffected …
  left: [(6, 8, 6, 21), (6, 27, 7, 1)]
 right: [(6, 7, 6, 20), (6, 26, 6, 33)]
```

Both `left` values reproduce Plan 2 row 3's recorded numbers exactly — the
same one-column right-shift, now attributed to one site each.

**Outcome: each path already has exactly one guard, and the mapping is
one-to-one and selective. No guard needed to be added.** Plan 2 hand-off (g)
is closed: the audit's row 3 did cover two guards on different code paths, and
those paths are now individually bound.

**T12 and the T10 correction are the same fact from opposite directions.**
Hand-off (g) asked which guard each path owns; answering it selectively also
disproved the T10 seam row's "either base" binding claim, because the reason
`meta.rs:255` cannot redden T10 is precisely the reason it owns P and not B.
That is why the selective replay was worth doing rather than trusting Plan 2's
single-mutation audit: the non-selective mutation reddened everything, so it
could establish that both guards are bound but not *which path binds which* —
and it is exactly that missing half that the T10 row got wrong.

**Restoration.** Both files restored from pre-mutation copies; `git status
--short` shows only `tests/integration/main.rs` modified and the new test file
untracked — neither `crates/pampa/src/pandoc/meta.rs` nor
`crates/quarto-core/src/transforms/config_markdown.rs` appears. No mutation is
in the commit.

**Gates.** `cargo clippy -p quarto --all-targets -- -D warnings` → clean.
`cargo nextest run -p quarto` → **454 passed, 1 skipped, 0 failed** (6d's
recorded baseline was 453; delta **+1** = T10, nothing removed or duplicated).
T12 added no test, which is why the delta is 1 and not 2. The workspace run is
the orchestrator's, at the phase boundary.


#### 6f — the upstream `quarto-error-reporting` doc comment

_(Recorded 2026-08-23 in Phase 8's reconciliation; full detail in
`task-11-report.md`.)_

**Written and committed upstream, but there is no PR link, and that is this
plan's one Definition-of-done gap.** The rewritten doc comment for
`snap_span_to_char_boundaries` is commit **`fd60487`** ("Document what
snap_span_to_char_boundaries actually guards") on branch
`docs/snap-span-char-boundaries-rationale` in `~/src/quarto-error-reporting`,
one commit ahead of `origin/main` (`87f1d38`), **not pushed**. Verified
2026-08-23 with `git -C ~/src/quarto-error-reporting branch -vv`. Pushing is
the user's call (repo-root `CLAUDE.md`, § GIT PUSH POLICY), so the checklist
box stays unticked rather than being ticked on a local commit. **The snap is
kept**, as the item requires; the comment is doc-only, with no release, no
version bump and no floor bump.

The **strand id** the Definition of done pairs with the PR link *is* recorded:
`bd-g7qh1ltt`, re-scoped 2026-08-23 (comment `c-2edupaog`, `related` →
`bd-1d6io`), checklist box ticked. So of that clause's two halves, the strand
id is present and the PR link is not.

### Phase 7

**T5 — RED observed, then green.** `cargo nextest run -p comrak-to-pandoc -E
'test(t5_) or test(t6_)'` before the walker existed:

```
thread 'tests::t5_text_node_offsets_survive_escape_and_entity' panicked at
crates/comrak-to-pandoc/src/lib.rs:277:9:
assertion `left == right` failed
  left: 11
 right: 16
```

11 is exactly § 7's measured pre-fix value for `dd` (`base_offset + byte_idx`
over the decoded string: −1 for `\*`, −4 for `&amp;`), against a true 16.
After the walker both tests pass.

**T6 — an upstream-behaviour pin on comrak, and only its `ee` half
discriminates.** The same RED run failed T6 at `lib.rs:312`, the **`ee`**
assertion (`left: 19, right: 23`) — which means the `dd` assertion on the
preceding line had *already passed* with the unfixed code. That is the
measurement, not an inference: `dd` reports 14 before and after, because
resetting the drift at a `SoftBreak` is precisely what comrak's per-line
`Text` nodes do. The `dd` half is kept because the reset property is what the
pin is *about*; it is stated as non-discriminating in the test's own doc
comment, along with the fact that its "revert" is a comrak version bump rather
than a q2 hunk.

**The walker.** `crates/comrak-to-pandoc/src/text.rs`. Rule order is escape →
character reference → byte-verbatim (§ 7 fact 3). Segmentation for a reference
runs to its `;` and is *syntactic only*; whether comrak actually decoded it is
decided against the content by a resync check, which is what keeps the HTML5
named-entity table out of the crate (§ 7's explicit non-goal). `&amp;amp;`
(decoded once, then a literal `amp;`) and `&foo;bar` (well-formed syntax,
unknown name, left verbatim) are told apart that way, and both are unit-tested.
`ProvenanceBuilder::in_file` does the tiling.

**Token spans are the *restriction* of the node's tiling to the token's
content range**, not `SourceInfo::substring` over the whole-node provenance.
The two agree at **run** granularity, not byte-exactly: a token whose `c0`
falls *inside* a replacement maps to that run's `src.start` under the
restriction and to `src.start + (c0 - run_start)` under a literal wrapper.
Both land inside the same replacement's source range, which `span_for`'s
sub-character caveat already licenses, so this is a precision point rather than
a difference in correctness. The restriction additionally keeps a token that
lies wholly inside one verbatim run collapsing back to a plain `Original`, so
unescaped text keeps the shape it had before, and the seven frozen `text.rs`
assertions — which read `start_offset()`/`end_offset()` — stay true as written.
A literal `substring` wrapper would have made *every* commonmark text token a
`Substring{parent: …}` and reddened all seven.

**Signature.** `tokenize_text_with_source(text, raw, base_offset, file_id)`.
The eight call sites the brief enumerated are exactly the ones that existed:
`inline.rs:52` (now `:56`) and the seven positional unit tests. The seven were
updated by passing the fixture text twice (`("hello", "hello", 10, FileId(0))`)
— raw == content, so every run is verbatim and each token collapses to the same
`Original` it had before. **No assertion in those seven was edited.**
`test_source_with_base_offset` keeps its name; the parameter it was named for
still exists, so there is no mismatch to note after all.

`SourceLocationContext` now holds the source text and exposes
`raw_slice(&sourcepos)`; nothing else in the AST carries a text node's raw
bytes.

**Snapshot churn: zero files.** `git status --porcelain` after a full green
`-p pampa` run lists four modified `.rs` files and no `.snap`. The checklist's
"expect real pampa movement" is corrected above: `tokenize_text_with_source` is
reachable from pampa, but the only path in is `main.rs:332`'s `--from
commonmark` arm, and no snapshot test in the workspace invokes it. The
reachability enumeration ranges over every `.rs` file under `crates/`:
`convert_document_with_source` has one non-test caller **that passes
`Some(ctx)`** (`readers/commonmark.rs:48`) — `block.rs:37`'s `convert_document`
is a second production caller and passes `None`, which routes `NodeValue::Text`
to `tokenize_text` and cannot reach the walker — and `readers::commonmark::read`
has one non-test caller (`main.rs:332`) plus two test callers (its own module tests and
`test_diagnostic_path_normalization.rs:103`, fixture `"hello\n"` — no escape,
no reference, so its tiling is one verbatim run and its output is unchanged).

**End-to-end through the binary.** `cargo run --bin pampa -- --from commonmark
--to json <fixture>` on `aa\*bb cc &amp; dd ee`, pool inspected with `jq`:

```
{"c":"dd","s":11,...}  ->  p[11] = {"d":0,"r":[16,18],"t":0}
{"c":"ee","s":13,...}  ->  p[13] = {"d":0,"r":[19,21],"t":0}
{"c":"aa*bb","s":4,...} -> p[4] = {"d":[[1,0,2],[2,2,1],[3,3,2]],"r":[0,5],"t":2}
                            p[1]=[0,2] p[2]=[2,4] p[3]=[4,6]
```

`dd` and `ee` now resolve to 16 and 19, and `aa*bb`'s three-piece `Concat` is
§ 7's worked tiling restricted to that token. This is the `r` coordinate-space
change for escaped paragraphs on `--from commonmark`, observed rather than
inferred.

**Comments, not fixes.** The entity sub-character offset is documented on
`span_for`; the `Code` (backtick-inclusive span over a stripped literal) and
`Link`/`Image` (entity-decoded URL with `TargetSourceInfo::empty()`) caveats
are documented in the module header.

**Known degradation, documented in code — corrected 2026-08-23 (review round
1): it has two outcomes, not one.** When a character reference is immediately
followed by another `&` or by a `\`, there is no byte-comparable source text to
resynchronize against and `resyncs` accepts unconditionally. The **follower**
then decides the outcome, and "the failure mode" ranges over both of these:

- **Unknown reference** (`&foo;&bar;`) — the walk desynchronizes a step later
  and the whole node falls back to one honest run. Coarse, never misreported.
  This is the only case the first version of this paragraph, and the code
  comments it summarised, described.
- **Known reference decoding to more than one character**
  (`&NotEqualTilde;&amp;`) — the one-character candidate is accepted where the
  truth is two, and the walk **completes with a silently wrong tiling; there is
  no fallback**. Measured: 20 raw bytes → 6 content bytes tiles as
  `(0..15 → 3) | (15..20 → 3)` against a truth of `(0..15 → 5) | (15..20 → 1)`,
  so content bytes 3..5 are attributed to `&amp;`'s source range instead of
  `&NotEqualTilde;`'s. Swap the follower and the outcome swaps:
  `&NotEqualTilde;\*` *does* fall back, because the escape rule cannot match a
  continuation byte — which is precisely why the narrow claim read as true.

Both outcomes keep every offset inside the node's own source span and move only
*sub-token* positions; a whole-word token still anchors at its run's start, and
the only consumer is JSON output § 7 records as unread. Ruled not worth
backtracking (plan owner, review round 1); the finding was the **claim**, and
the code is unchanged.

**Gates.** `cargo clippy -p comrak-to-pandoc --all-targets -- -D warnings` →
clean; `cargo clippy -p pampa --all-targets -- -D warnings` → clean.
`cargo nextest run -p comrak-to-pandoc` → **179 passed, 14 skipped** (pre-task
baseline **166**; delta **+13** = T5, T6 and eleven walker unit tests — the
intermediate 168 recorded mid-task already included T5 and T6; the eleventh
walker test, pinning the empty-`raw` synthesis fallback, was added in review
round 1). `cargo nextest run -p
pampa` → **4503 passed, 2 skipped**, unchanged. The workspace run is the
orchestrator's, at the phase boundary.

### Phase 8

All four boxes closed 2026-08-23. Everything below was re-verified against the
tree at this branch's HEAD, not transcribed from the brief.

#### 1. The `cell_options` constraint — recorded, not lifted

Written into the file-header comment of
`crates/quarto-core/src/cell_options/mod.rs` (the "Shared cell-options
facility" block), citing findings § 6 by heading. The constraint as stated
there: *a language's option-line syntax may only elide spans, never transform
them, because every byte of the reassembled YAML must be a real source byte.*

**Why it holds, re-derived rather than quoted.** `partition_cell_options`
(`:189`) builds each concat piece as
`(SourceInfo::substring(body_source, start, end), end - start)` (`:237-240`,
line numbers after this phase's header addition), so content length equals
source length piece by piece; `option_content_ranges` (`:273`) returns *ranges
of the line* for both syntax families — one range for prefix-only languages, content +
newline ranges for suffix languages — so the suffix is elided, never rewritten.
**The claim ranges over every language `CommentSyntax` can describe**, prefix-only
and block-comment alike; it says nothing about languages q2 does not support.

**Not lifted, and the reason is a missing consumer, not a missing capability.**
`ProvenanceBuilder`'s `replacement(range, 0)` would express the deletion that
lifting it needs. No q2 language has a transforming option-line syntax, so
there is no consumer. The header comment says this in those terms.

#### 2. Cross-check of Plan 2's dispositions against § 6's census table

**`callout.rs` — the workaround is gone and the guard is intact.**
`attribute_value_source` now spans `:393-418`; there is **no**
`resolve_byte_range` / `map_offset` / `start_offset` / `end_offset` anywhere in
`crates/quarto-core/src/transforms/callout.rs` (grep, whole file, 2026-08-23),
which is the deletion the census records. The bd-3aolj / bd-1e6a5 guard
survives at `:400-412` and **was not touched**.

> **Two sub-citations in the Phase 8 checklist item are off by a line or two;
> the aggregate is exact.** Measured: comment `:400-403` ✓; `debug_assert!`
> `:404-409` (the item says `:404-410` — `:409` is the closing `);` and `:410`
> is already the `if`); `if attr.2.len() != … { return generated(); }`
> `:410-412` (the item says `:412-414`); function ends `:418` ✓;
> `#[cfg(test)]` at `:420` ✓; and the guard's overall extent `:400-412` ✓. The
> item's *conclusions* are all correct — only two interior line ranges drifted.

**`use_cmd/config.rs` — kept, and now commented.** Verified byte-exact:
`fn scalar_value_span` at `:229`, `start_offset()` at `:233`, `end_offset()` at
`:234`, and `self.text.get(start..end)? == parsed.as_str()` at `:235` — all
four lines are where the item says, measured *before* this phase's comment
insertion; the comment sits inside the function body, so `fn scalar_value_span`
stays at `:229` and the three body lines are now `:255`, `:256`, `:257`. It
compiles, it is called from exactly two sites (`:195`, the top-level `brand`
entry, and `:210`, the per-format one), and it still returns `None` on
mismatch. **Nothing was deleted**, which the item correctly
names as the regression to avoid.

The comment now at the site states the decision and the mechanism: the raw
accessors are safe **only** because of the check on the following line. Per
findings § 8, a `Concat` — or a `Substring` over one — reports *content*
coordinates from these accessors (`start_offset() == 0`,
`end_offset() == content length`), so the slice would be an unrelated prefix of
the file, the comparison would fail, and the function would return `None`: it
**refuses rather than mis-points**. Hence the `map_offset`-hull simplification
Plan 2 declined (R-8, hand-off item 1) is **declined permanently**, no strand.

> **One qualification the checklist item does not make, added to the comment
> rather than left implicit.** On *this* path the `Concat` shape is not live
> today: `ProjectConfigFile::load` parses through `quarto_yaml::parse_file`
> (`:153`), and `YamlHashEntry::value_span` is documented upstream
> (`quarto-yaml-0.1.3/src/yaml_with_source_info.rs:100-106`) as the value's
> **raw, delimiter-inclusive** span — `value.source_info.clone()`, *not*
> `content_source_info` — so it is an `Original` over `_quarto.yml` and the two
> accessors return true file offsets. What the byte-equality check catches
> **today** is therefore quoting/escaping/folding (`"a.yml"` vs `a.yml`), which
> is exactly what the function's own doc comment says. The `Concat` argument is
> what makes the reads safe **if that shape ever arrives here**; both readings
> are in the new comment, so neither can be mistaken for the other. The one
> case the check cannot catch — a file whose first `end` bytes literally spell
> the parsed value — is named there too.

**`theorem.rs` / `proof.rs` — unedited, and their output tightened as
predicted.** Both sites are still at the cited lines and still read
`attr_source.attributes[name_idx].1.clone()`: `theorem.rs:344-360`,
`proof.rs:181-197`. Neither was touched by Plan 3; the change reached them
through what `AttrSourceInfo` now carries.

*The prediction, quoted.* Plan 2 records it in its **Phase 4 checklist**, in
the fallout item (`2026-08-20-provenance-2-consumers.md:1197-1200`):

> **Update the fallout, don't just expect it.** These are work items, not
> predictions: … and `theorem.rs`/`proof.rs` spans tighten to exclude quotes,
> moving any location assertion over a theorem or proof `name=`.

and again as a check-this-first corollary in the same phase (`:1179-1182`):

> And note the corollary — this plan predicts `theorem.rs`/`proof.rs` and
> link/image `title` spans will "tighten to exclude quotes"; that prediction
> only holds if the quotes are left out of the tiling as above. If those spans
> do **not** tighten, this is the first thing to check.

> **Correction to this task's brief (2026-08-23).** The brief said "Plan 2's
> § Evidence Phase 4 is the only record of that prediction — quote it."
> Measured: **§ Evidence Phase 4 does not mention `theorem.rs` or `proof.rs`
> at all.** The three places Plan 2 records the prediction are the two quoted
> above plus its census-adjacent prose at `:340-342` ("Phase 4 fixes them for
> free and **changes their output** — see that phase's fallout list"). What
> § Evidence Phase 4 *does* hold is the **measurement** for the shared
> attribute path they consume — "Column **27** — the first byte inside the
> opening quote, underline stopping before the closing quote. Under the
> revert: column **26**, underlining both delimiters" — plus the one snapshot
> that moved, `table-caption-attr.snap`, `[104,113]` → `[105,112]`. So the
> prediction and its measurement are in the same plan but different sections.

*Independently measured here, through the binary*, so the tightening is
observed rather than inherited. Fixture
`::: {#thm-line name="Line \"crossing\""}`, in which the `name` value is quoted
and contains two escapes:

```
$ cargo run -q --bin pampa -- --to json --json-source-location full \
    .scratch/prov3-phase8/thm.qmd | jq -c '..|objects|select(has("a"))|.a'
{"classes":[],"id":1,"kvs":[[2,7]]}

$ … | jq -c '.astContext.p[2], .astContext.p[7], .astContext.p[3,4,5,6]'
{"d":0,"r":[15,19],"t":0}                                  # key `name`
{"d":[[3,0,5],[4,5,1],[5,6,8],[6,14,1]],"r":[0,15],"t":2}  # value: a Concat
{"d":0,"r":[21,26],"t":0}   # `Line ` — verbatim, 5 bytes
{"d":0,"r":[26,28],"t":0}   # `\"`   — 2 source bytes, 1 content byte
{"d":0,"r":[28,36],"t":0}   # `crossing`
{"d":0,"r":[36,38],"t":0}   # `\"`   — 2 source bytes, 1 content byte
```

The opening quote is file byte 20 and the closing quote byte 38: the value's
source extent is **21..38**, quotes excluded, with each escape carried as its
own piece. That is the value `theorem.rs:345` / `proof.rs:182` clone into the
`Str`'s `source_info`. Output inspected directly; no test asserts these
numbers, and none was added — this is a cross-check, not a new seam.

**The seventh site, appended to the census.** Plan 2's final fix wave (FIX-2)
fixed a decoded/raw pairing in
`crates/quarto-core/src/project/website_post_render.rs`'s `copy_footer_images`
that § 6's table predates. Verified 2026-08-23: `:222` now reads
`let base = content_source_info.as_ref().unwrap_or(&cv.source_info);` and the
comment at `:208-217` cites `config_markdown.rs:326`, which is byte-identical
in form (`let base = content_source_info.as_ref().unwrap_or(&value.source_info);`)
and **unmutated** — that is the same line Phase 6e mutated for T12 and
restored. The row is appended to the findings doc's table with a dated note.

> **Numbering, named.** The brief calls it "the sixth site"; counting each
> row's *sites*, § 6's table already listed **six** (the `theorem.rs` /
> `proof.rs` row is two), so this is the **seventh**, and the census heading
> was updated from "six sites" to "seven sites". **"One deletion" is
> unchanged** — `callout.rs` remains the only deleted row; this one was fixed
> in place. The findings doc's *next* subsection, "A seventh site: the
> shortcode-string closure", counts from the original six-row census and is
> **left as written**; the dated note says so explicitly, so the two
> enumerations cannot be conflated.

> **One census row is now stale, and the note says so.**
> `codeblock_shorthand.rs:486`'s disposition still describes the pre-fix state;
> Plan 3 Phase 6a fixed it (bounded between-fences search, guarded by
> `body_source_for_locates_the_body_not_the_info_string`). Recorded in the
> dated note rather than by rewriting the row, per that document's convention.

#### 3. `bd-49cbyqbt` — closed upstream of this plan, nothing to do here

Closed **2026-08-22T22:20:45Z** as a **duplicate of `bd-1d6io`** (failure #2,
"attr key range absorbs the inter-pair separator"); `bd-1d6io` is the superset
and its close reason records this strand's three durable contributions in
comment `c-qn11q3g6`. Verified by `braid show bd-49cbyqbt --json`. This is
hand-off 4(c)'s second half, and it needed **no work in this plan**. Note that
`bd-1d6io` itself is **`in_progress`, not closed** — it is outside this epic
(branch `braid/bd-1d6io-annotated-qmd-source-tracking`) and does not gate it.

#### 4. `bd-mxa44voa` closed

Children verified closed **before** the close was attempted, individually:
`bd-gx2mal69` (2026-08-21T01:46:21Z), `bd-jmquuiqh` (`:17Z`), `bd-th2ah982`
(`:19Z`), `bd-x0o0pem3` (`:23Z`). `braid dep tree bd-mxa44voa` lists **exactly
those four** as children — the three open follow-up strands this epic spawned
(`bd-6392eba3`, `bd-8hrjqcx0`, `bd-lm75ion7`) hang off it as `discovered-from`,
which is informational and does not gate the close, and `bd-g7qh1ltt` is
`related` to `bd-1d6io`, not a child here.

#### 5. Definition-of-done audit

| DoD clause | state |
|---|---|
| Phase 1 classification table | present (§ Evidence → "The 26-row classification") |
| Phase 1 T1 result | present ("T1 — invariant pin, green for the stated reason") |
| Phase 1 T2 written-or-not, with reason | present ("T2 — not written"); its box is deliberately unticked |
| Phase 1 item 5(a) | present ("Item 5", parts (a) and (b)) |
| Phase 2 `offset_to_location_bytes` lines + T3 | present |
| Phase 3 doc-comment commit hash | **was missing — filled here** (`a7b2e8f96`, refined `9d3bf333f`); box also ticked |
| Phase 4 T8 | present |
| Phase 5 T4 (green, or red + `#[ignore]` + strand) | present: RED, `#[ignore]`d, strand `bd-8hrjqcx0` |
| Phase 6 T7/T9/T10/T11/T12 | present (6a–6e) |
| Phase 6 strand id | present (`bd-g7qh1ltt`) |
| Phase 6 **upstream PR link** | **GAP** — commit `fd60487` exists locally, unpushed; no PR. See 6f |
| Phase 7 T5/T6 + snapshot accounting | present ("Snapshot churn: zero files") |
| Phase 8 cross-check lines | this section |
| `cargo xtask verify` green | **not run by this task** — the orchestrator owns the workspace/verify run |
| the epic closed | `bd-mxa44voa` closed 2026-08-23 |

**Two open items, both deliberate, neither a defect of execution:** T2's box
(resolved-not-performed, reasoned in the item itself) and the upstream PR
(needs a push, which is the user's decision).
