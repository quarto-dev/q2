# Provenance, Plan 2 of 3: consumers (`quarto-error-reporting`, q2)

**Epic:** `bd-mxa44voa`.
**Depends on:** Plan 1 (`2026-08-20-provenance-1-foundations.md`). **Read Plan
1's § Design before starting** — this plan uses `ProvenanceBuilder`, the
lockstep *walker*, *desync*, `Concat`/`Substring` and `strict-provenance`
without redefining them.
**Sibling:** Plan 3 = `2026-08-20-provenance-3-audit-and-fix.md`.

**Absorbed strand** — closed, its content is Phase 5 here:
`bd-chmbr0zl` (panic boundary around diagnostic rendering).
Also closes `bd-ariadne-config-span-char-boundary-panic-rkqmhzrg` (Phase 1).

Background, the bug class, and the design rationale live in Plan 1. This plan
is the consumer side: take the corrected provenance, use it, and delete the
workarounds that existed because it was wrong.

### The publish chain, stated once

**Four** crates.io releases sit between the phases below, and three of them are
Plan 1's. Getting this wrong means a phase built against a version that does
not exist yet.

```
Plan1-P1 publishes quarto-source-map 0.1.2   (behavior fixes only)
        ├──► Plan2-P1 publishes quarto-error-reporting 0.2.2   (this plan)
        └──► Plan1-P2 publishes quarto-source-map 0.1.3        (ProvenanceBuilder)
                     └──► Plan1-P2 publishes quarto-yaml 0.1.3
                                  └──► Plan2-P2 refreshes q2's lock to all four
```

**`ProvenanceBuilder` is not in 0.1.2.** Plan 1 deliberately holds it out —
0.1.2 carries the four behavior fixes, and the builder ships as **0.1.3**, as
Phase 2's last act, after its walker has driven it green against a path patch.
An earlier revision of this section said three releases and named 0.1.2 as the
version this plan consumes; both were wrong.

So **Phase 1** needs Plan 1's *Phase 1* published — not merely its Phase 0
evidence, and 0.1.2 is the right version there, since Phase 1 needs only the
floor. **Phase 2** additionally needs Plan 1's *Phase 2* published, because it
takes `quarto-source-map 0.1.3` **and** `quarto-yaml 0.1.3`. Phase 3 onward
needs `content_source_info` and `ProvenanceBuilder`, i.e. everything above.
`^0.1.0` would pull 0.1.3 by luck, but the refresh target and the confirmation
step must name it.

## How the four defects relate

Useful orientation when reading the phases below, because each attacks a
different one:

| Repo | Defect | Consequence | Where fixed |
|---|---|---|---|
| `quarto-yaml` + q2 | decoded value paired with undecoded provenance | every quoted-scalar span is 1 byte left, and multi-line block scalars misreport the line — in **both** the front-matter and project-config paths | Plan 1 + Phase 3 here |
| `quarto-source-map` | `Location.column` is floored to a char boundary, `Location.offset` is not | callers reading `.offset` inherit an unsanitized value | Plan 1 Phase 1 |
| `quarto-error-reporting` | renderers slice at raw byte offsets with no boundary check | a wrong offset becomes a **process abort**, not a wrong caret | Phase 1 here (`4da3385`) |
| q2 | no panic boundary around diagnostic emission | one bad diagnostic kills an already-successful render and discards the rest of the stream | Phase 5 here |

The first is the **root cause**. The second is why the bad value survived
undetected. The third is why it was fatal rather than cosmetic. The fourth is
why the blast radius was the whole render.

## Workarounds that collapse

The clearest evidence that this is worth doing. **Three q2 authors
independently hit this bug, understood it, and routed around it** — each
paying in capability:

| Site | Workaround | Cost today | Disposition |
|---|---|---|---|
| `quarto-core/src/transforms/callout.rs:431-447` (inside `attribute_value_source`, `:401-448`) | derives the parent from lengths alone when nothing was collapsed; else falls back to the whole value span | loses precision on any escaped attribute | **deleted** (Phase 4) |
| `quarto/src/commands/use_cmd/config.rs:229` (`scalar_value_span`) | byte-compares the raw span's text against the decoded value, returns `None` on mismatch | **refuses to repoint the declaration at all** | **kept** — simplification optional (Phase 4) |
| `quarto-core/src/cell_options/mod.rs:196-228` | avoids the bug structurally by never decoding | constrains its input format | **untouched** |

The third is cited in `bd-mxa44voa` as the exemplary case — and it is
exemplary *precisely because* it already uses the concat-piece approach Plan 1
generalizes. Plan 3's Phase 7 asserts these three dispositions; tell that
session if any of them changes.

Two more sites pair a decoded value with a raw span but do no sub-offset
arithmetic, so they are wrong-span rather than drifting:
`transforms/theorem.rs:344-360` and `transforms/proof.rs:181-197`. Phase 4
fixes them for free and **changes their output** — see that phase's fallout
list.

## What Plan 1 hands us

Plan 1's § Hand-off to Plan 2 lists **nine**, and all nine are ours. An earlier
revision of this table said eight and claimed the ninth self-routed to Plan 3 —
that was true of an older Plan 1, which has since moved the `incremental.rs`
item into its § Hand-off to *Plan 3* and added a new #8. Numbering below
follows Plan 1's current copy.

| # | Obligation | Phase |
|---|---|---|
| 1 | Thread content provenance through `ConfigValue` | 3 |
| 2 | Test the zero-width renderer label | 1 |
| 3 | Re-anchor the two upstream crash regression tests | 1 |
| 4 | Release `quarto-error-reporting` 0.2.2 carrying `4da3385` | 1 |
| 5 | Re-gate this plan's Phase 1 on Plan 1 **Phase 1** publishing 0.1.2 | 1 |
| 6 | Absorb the two q2-side reactions to the `Location.offset` floor | 2 |
| 7 | Specify the desync report as warning-level and non-fatal | 3 |
| 8 | **Re-check `qmd-syntax-helper` after the `AttrSourceInfo` meaning change** | 4 |
| 9 | Carry the `AttrSourceInfo` meaning change into `annotated-qmd` | 4 |

Obligation 3 is settled as a unit test; see § In-session verification.
Obligation 8 was added after this plan's owner confirmed "all eight", so it is
the one most likely to be dropped — it is the item with the worst failure mode
in the epic (see Phase 4).

## In-session verification

Some evidence here **cannot** be recorded as a permanent test: it exists only
in a state the plan destroys. Reverting a fix to watch a test go red is not a
test; neither is exercising a crash that the next phase makes unreachable. The
standing rule for this plan:

> Anything we cannot record as a permanent test, we verify **in session** and
> paste the observation verbatim into § Evidence. An in-session verification is
> a checklist item like any other, with a named expected observation. "We
> reasoned it must be fine" is not a discharge. **Anything we *can* record
> permanently, we do** — see § Evidence's "make these permanent".

Two verifications here are genuinely unrepeatable:

1. **Phase 1** — that the two `..._does_not_panic` tests bound *before* the
   floor. After Plan 1's 0.1.2 no `SourceInfo`-carried location can deliver a
   mid-character offset to a renderer.
2. **Phase 6** — the whole revert matrix. Reverts are inherently in-session.

Phase 4's pre-fix TS range is recorded in session *and* pinned by a permanent
test, so it is not in this list.

## Test seam spec (frozen)

One row per test, bound before any code is written. **A test whose revert hunk
is not named here is not specified** — the executor will otherwise invent a
harness. Once a row goes green its assertion surface is frozen; never edit an
assertion to make it pass.

Tiers here are: **unit** (in-crate `#[test]`), **cli** (drives the `q2` binary
through `crates/quarto/tests/integration/`), **node** (`node --import tsx
--test` in `ts-packages/annotated-qmd`).

**"Hull"**, used throughout this plan: the single `(start, end)` source range
covering a discontiguous `Concat`, obtained from the `map_offset` pair —
`map_offset(0)` for the start and `map_offset(length())` for the end. It exists
only when the pieces tile the source without gaps.

| # | Test | Tier | Assertion surface | Revert ⟨hunk⟩ → RED |
|---|---|---|---|---|
| T1 | snap helper widens to whole chars, incl. the re-anchored `21..28` pair | unit | `snap_span_to_char_boundaries` return value, literal offsets | the snap helper body (`diagnostic.rs:671-686`) |
| T2 | zero-width label renders, both renderers | unit | rendered text of a `7..8`-inside-`✨` diagnostic | **`Location.offset` floor** in `quarto-source-map` — cross-repo, needs a path override |
| T3 | exclusive-end pin | unit | `map_offset(length())` on the QMD writer's concat | `Concat` exclusive-end branch (`mapping.rs:64-70`) — cross-repo |
| T4 | quoted scalar, exact columns | cli | `start_column`/`end_column` in `--json-errors` | the three re-parse bases (`meta.rs:259/:303/:316`) |
| T5 | multi-line block scalar → `9:7`, `9:26` | cli | same | the carrier read in `config_markdown.rs:283-290` |
| T6 | front matter quoted `title:` | cli | same | the three re-parse bases (`meta.rs:259/:303/:316`) |
| T7 | plain + single-line block unchanged | cli | same | **none — see § Unbindable below** |
| T8 | caret underlines the right bytes | cli | rendered text path, not JSON | whichever `resolve_span` path Phase 3 takes |
| T9 | per-page config diagnostic still gets a snippet | cli | **snippet presence** — the config file's path in rendered output, not merely "a diagnostic printed" | `bind_source_candidates`' `root_file_id()` change |
| T10 | `fig-cap` inline resolves into the option line | cli | inline `SourceInfo` byte position | `caption_inlines` (`codeblock_shorthand.rs:644`) |
| T11 | attribute escape resolves to `86..92` | cli | inner node `SourceInfo` | the `AttrSourceInfo` content-provenance swap |
| T12 | serde round-trip keeps `{"Scalar": <value>}` and yields `None` | unit | serialized JSON shape + deserialized provenance | the hand-written `Serialize`/`Deserialize` arms (`config_value.rs:222`, `:305`) |
| T13 | TS range for a quoted and an escaped value | node | `getAnnotatedParseSourceFields` range | `resolveChain`'s `Substring` arm (`source-map.ts:301-315`) |
| T14 | `block-types.test.ts` asserts the unquoted value | node | `source.substring(value.start, value.end)` | same as T13 |
| T15 | injected panic: exit 0, others print, `_site/` written, named stderr line | cli | exit code + stderr + filesystem | the `catch_unwind` boundary |

### Vacuity checks (the assertion must still discriminate)

- **T5's discriminator is the line number, not just the column.** Today's
  wrong answer is `8:10`/`9:14`; the right answer is `9:7`/`9:26`. Assert
  **both** line and column — a column-only assertion would still pass if the
  fix corrected the column while leaving the line wrong, which is the exact
  symptom that makes this case worse than an off-by-one.
- **T9 must assert snippet presence, not diagnostic presence.** The regression
  is "the warning prints with no source snippet." A test that only checks the
  `Q-2-9` warning appeared passes in both states.
- **T2's binding is cross-repo and inverted.** The zero-width highlight only
  *exists* after the floor, so its revert lives in `quarto-source-map`, not in
  the crate that holds the test. Phase 6's "revert the floor → its own tests
  red" row does **not** cover it; add a row naming this test explicitly.
- **`accum/`'s third reported column numerically equals its second truth
  column** — both are `9:25`. A test asserting `25` for that element reads as
  fixed while sitting on the stale value. If T8's optional per-element check is
  added, assert all four positions as a set, or assert the third element by
  something other than its column.
- **T4 and T6 share one revert hunk**, so Phase 6's provenance row cannot tell
  a half-done Phase 3 from a complete one. Split the row: revert the three
  re-parse bases (reddens T4/T6) separately from the carrier read (reddens
  T5), or a Phase 3 that fixes only front matter still shows a fully red
  matrix.

### Unbindable and accepted-untested — logged, not silently omitted

- **T7 cannot be reddened by reverting our code**, by construction: it asserts
  values that are correct today and must stay correct. Bind it by **mutation
  instead of revert** — apply the content-provenance base to the plain-scalar
  path too and confirm T7 goes red. Record that mutation in § Evidence.
- **The two re-anchored `..._does_not_panic` tests are unbound after Phase 1**,
  and this is known in advance rather than discovered in Phase 6. Once the
  floor lands, no hunk's revert reddens them — that is precisely why their
  coverage moved to T1. Phase 6's closing rule ("any test that cannot be made
  to fail: fix it or delete it") would condemn them, so decide now: **keep as
  renamed smoke tests with an explicit accepted-unbound note in their doc
  comment**, naming T1 as where the real coverage went. Do not let Phase 6
  rediscover this.
- **Accepted-untested, with rationale:** the `None`-fallback inertness for CLI
  `-M`/Lua metadata (its safety comes from those producers carrying
  `Generated`, which offset arithmetic already refuses — a test would assert
  the absence of a symptom); the Lua-filter degradation path (same shape); and
  the `ConfigValueKind::Scalar` migration itself (compiler-guided across 206
  sites — the type system is the test).
- **If the `resolve_span` fallback path is taken**, T8 moves to rendered
  output and a *new* row is needed: the `Substring{parent: Concat}` arm
  reports `Concat` rather than `Generated`, reverted by that arm.

## Phases

### Phase 1 — release `quarto-error-reporting` 0.2.2

**Gate: Plan 1's Phase 1 must be *published* first** (obligation 5, and
§ The publish chain). Two items below need `quarto-source-map` 0.1.2 resolved
in this crate: the zero-width label only exists once `Location.offset` floors,
and the re-anchoring is a response to that same floor. A local
`[patch.crates-io]` override is fine for development, but the release must not
ship against an unpublished dependency.

Also do not start until Plan 1's Phase 0 has captured the crash evidence.

The fix is committed on `fix/diagnostic-span-char-boundary` (`4da3385`):
`snap_span_to_char_boundaries` (`src/diagnostic.rs:671-686`), snapping **four
offsets at three call sites** (`:878` covers both the ariadne `Report::build`
anchor and the ariadne main label — the code comment says "snap once, up
front"; `:937` the detail labels; `:1032` the annotate-snippets `clamp`
closure). Do not go looking for a fourth call.

**These first two items are order-dependent, and getting them backwards
destroys evidence that cannot be recovered.** The binding experiment must run
**while this repo is still on 0.1.0/0.1.1**. Both `..._does_not_panic` tests
build `SourceInfo::original(fid, 21, 28)`, and once 0.1.2's floor is in play
`map_offset` floors 21→19 *before the renderer is reached* — so reverting the
snap no longer panics and the test cannot be made red. That is the same
mechanism Plan 1's § Risks cost (c) describes, and it is why the re-anchoring
below exists. An earlier revision of this phase listed the lock update first;
it was wrong.

- [ ] **First, in-session: bind the ariadne test while it still binds.**
      On the current lock (0.1.0), revert the snap call at `:878` only, keeping
      the tests, and confirm
      `ariadne_span_starting_inside_multibyte_char_does_not_panic`
      (`src/diagnostic.rs:1601`) goes red. Paste the failure into § Evidence.
      **Scope note:** Plan 1's Phase 0 bullets 3-6 revert each hunk
      individually and already bind the *annotate-snippets* test (`:1635`) and
      the four hunks by observed panic location. This item adds the one thing
      those do not — that the *ariadne* test goes red — so run it in the same
      sitting as Plan 1's Phase 0 rather than as a second expedition
      (§ In-session verification, instance 1)
- [ ] **Then take 0.1.2 in this repo.** `Cargo.toml:28` needs no edit
      (`quarto-source-map = "0.1.0"`, and `^0.1.0` accepts 0.1.2), but
      `Cargo.lock` pins **0.1.0**. Run `cargo update -p quarto-source-map` and
      **commit the lockfile** — the release runs `cargo publish --locked`, so
      without this 0.2.2 ships built against 0.1.0, and the zero-width test
      below would measure the old behavior. Everything after this point in the
      phase requires the floor to be present
- [ ] **Re-anchor the two tests (obligation 3).** Settled: unit-level coverage
      is the answer, because integration-level coverage is *unreachable* after
      the floor — every path from a `SourceInfo` to a renderer offset runs
      through `map_offset` → `offset_to_location`, including `Concat` (which
      recurses to `Original`), and the public entry `to_text_with_renderer`
      takes its span from `self.location`. Concretely: extend
      `snap_span_widens_to_whole_characters` with the offset pair the two
      integration tests used (`21..28` over
      `"text: <span>Ask AI \u{2728}</span>"` — one pair, shared by both
      tests), and **rewrite the two integration tests' names and doc
      comments** to say what they now cover (a span-carrying diagnostic still
      renders under each renderer) plus why they no longer cover the snap. Do
      not delete them and do not leave the names claiming a guarantee they lost
- [ ] **Test the zero-width label (obligation 2).** Measured by Plan 1 on
      content `"x = 'A✨B'"` (emoji at bytes 6..9) with
      `SourceInfo::original(fid, 7, 8)` — both ends inside the character:

      | | offsets reaching the renderer | rendered highlight |
      |---|---|---|
      | today | raw `7..8`; snap floors 7→6 and **ceils** 8→9 | `6..9`, the whole character |
      | after the floor | both arrive as `6`; the ceil finds 6 already on a boundary | `6..6`, **zero width** |

      Two things to pin in the assertion. At the `Location` level this is
      *consistency*, not regression — the columns already collapse to `6..6`
      today; the regression is purely at the rendered level, because the
      renderer ceils the **raw** offset and flooring upstream destroys the
      input its ceil needs. And ariadne's `Report::build` anchor is already
      `start..start`, so a zero-width label very likely renders — which is
      exactly the "very likely" this test exists to replace. Assert for both
      renderers
- [ ] Bump crate version to `0.2.2`
- [ ] `cargo test` green (this crate is not a nextest workspace), **plus**
      clippy on `--no-default-features`, ariadne-only, and
      annotate-snippets-only. The misplaced-`cfg_attr` bug found during
      development was visible *only* in the no-default-features configuration
- [ ] PR → CI green → merge. **Merging to `main` publishes — there is no human
      release step.** Verified 2026-08-21 for *this* repo (Plan 1 verified the
      other two): it carries the same repo-agnostic `release.yml` using
      crates.io Trusted Publishing over OIDC with no stored token, and its
      `release` environment returns `protection_rules: []`, so no approval gate
      fires. The workflow only acts when the workspace version leads the
      registry
- [ ] Close `bd-ariadne-config-span-char-boundary-panic-rkqmhzrg`

**Not a checklist item, a review note:** `annotate-snippets` is **not** the
safe sibling. It panics identically at
`annotate-snippets-0.12.16/src/renderer/source_map.rs:71` on the same input;
it simply was not the default renderer. Both paths are fixed and both are
covered — do not drop either as redundant during review.

### Phase 2 — q2 takes the updates

**Gate:** Plan 1's Phase 2 published `quarto-yaml 0.1.3`, and Phase 1 above
published 0.2.2. **The items below are order-dependent** — the pin must
precede the refresh, because the refresh is what would make the change
invisible.

- [ ] **First: pin the `Concat` exclusive-end change before it lands
      silently.** Plan 1's third 0.1.2 behavior change (the exclusive-end
      branch uses the last piece's *source* length) alters a q2 production
      path: the QMD writer's provenance concat
      (`pampa/src/writers/qmd.rs:2876-2903`) pairs each block's
      `source_info()` with bytes *written*, and its only production consumer is
      `quarto-core/src/stage/stages/engine_execution.rs:733`. Measured:
      `crates/pampa/tests/integration/qmd_writer_source_info.rs` exercises only
      interior offsets, so **nothing currently reacts**. Add an exclusive-end
      assertion there against today's behavior, so the refresh below turns it
      red rather than passing unnoticed
- [ ] Then refresh the lock to **`quarto-source-map 0.1.3`** (not 0.1.2 —
      that release deliberately excludes `ProvenanceBuilder`, which Phases 3
      and 4 require), `quarto-error-reporting 0.2.2`, and
      `quarto-yaml 0.1.3`. Note q2's lock currently pins
      `quarto-source-map 0.1.0` even though 0.1.1 is released, so it needs an
      explicit update regardless. Confirm the resolved version by name; `^0.1.0`
      would accept 0.1.2 and leave Phase 3 unable to compile
- [ ] **No `Cargo.toml` edit for `quarto-yaml`.** Plan 1 decided the change is
      *additive* and ships as **0.1.3**, so `^0.1.2` accepts it and this is a
      lock refresh only
- [ ] Confirm no `[patch.crates-io]` override for `quarto-error-reporting`
      remains. Plan 1's Phase 0 removes it as its exit condition; this is the
      confirmation. **Only the three added lines are local** — the section's
      four committed entries (`lua-src`, `tree-sitter-language`, `runtimelib`,
      `jupyter-protocol`) are load-bearing and must stay
- [ ] **Absorb the two q2-side reactions to the floor (obligation 6).** Plan 1
      deliberately leaves both to us:
      - `pampa/tests/integration/test_location_health.rs:448` compares the two
        `offset_to_location` implementations' row/column; Plan 1 aligns the
        free `utils::offset_to_location` with `FileInformation`, so this **may**
        react. It only diverges for mid-char inputs, and whether any current
        fixture reaches one is **unknown until Plan 1's Phase 1 smoke step
        runs**. Do not plan for green or red — read the smoke output and
        **record what it showed in § Evidence**, so "nothing moved" is
        distinguishable from "nobody looked"
      - JSON-writer snapshots: `.offset` is emitted as `"o"`
        (`pampa/src/writers/json.rs:550`, `:555`, `:2005`, `:2014`, `:2258`,
        `:2267`) and read as `file_offset` at
        `quarto-core/src/engine/ts_engine.rs:689`. Neither snaps, which is the
        affirmative case for flooring — and the reason snapshots move.
        Per CLAUDE.md the review needs a **count, a summary of what changed,
        any surprises called out, and the affected file list**
- [ ] `cargo nextest run --workspace` green

**Deleted item, recorded so it is not re-derived:** an earlier draft had "fix
the 15 test-only `new_scalar` / `new_scalar_with_tag` call sites." Plan 1's
additive decision removed it — the public constructors keep their signatures
and provenance attaches via `with_content_provenance`, so q2 has **zero**
call-site edits there. The old semver argument ("a breaking 0.1.3 would be
pulled in silently; 0.2.0 forces an explicit edit") is retracted upstream
with it.

### Phase 3 — q2 consumes `content_source_info` (the YAML path)

**Scope, decided 2026-08-21:** this phase fixes **both** YAML paths — the
deferred project-config path that `ConfigMarkdownTransform` re-parses, *and*
document front matter, which is re-parsed immediately inside
`yaml_to_config_value`. Front matter is the larger user surface (every quoted
`title:`, `description:`, caption drifts by one today) and costs the same
one-line source.

**Tests first.**

- [ ] Failing test, in `crates/quarto/tests/integration/json_errors.rs`: a
      fixture with a **quoted** scalar containing raw HTML, asserting
      **exact** `start_column` / `end_column`. Nothing in q2 asserts on
      `start_column` today — measured — which is why this survived.
      **The recorded baseline "36/43 → 37/44" has no fixture attached and
      could not be reconstructed from either committed repro.** Either
      identify the document it came from and transcribe it, or derive fresh
      expected values from the fixture you write and record *those*. Do not
      copy 36/43 forward unexamined
- [ ] Failing test: a **multi-line block scalar**, using the fixture
      transcribed in § Evidence below. **Measured by Plan 1 on 2026-08-21
      against the released `q2 0.24.0`** — exit 0, two `Q-2-9` warnings (the
      `<span id="y">` open and close tags), both genuinely on **line 9** at
      columns **7** and **26**, reported at **`8:10`** and **`9:14`**. The
      drift is a constant **12** = 2 preceding content lines × 6 bytes of
      stripped indent: 26 − 12 = 14, and 7 − 12 underflows past the start of
      line 9, which is why the first warning claims **line 8**. Observe red on
      both, then green at 9:7 and 9:26. This is the only symptom in the epic
      that misattributes a diagnostic to the wrong *line*, and until this
      measurement the cited pair had never been tied to a file
- [ ] Optional, if a per-element check is wanted: Plan 1 also measured
      `.scratch/ariadne-emoji-panic/accum/` (transcribed in its § Evidence), a
      two-span variant of the same shape giving **four** warnings at
      `8:10` / `9:13` / `9:25` / `9:42` against truth
      `9:7` / `9:25` / `9:37` / `9:54`. It shows the constant −12 per element
      rather than inferred from two points. `blockscalar/` stays canonical
      here; `accum/` is the arithmetic demonstration. **Optional — record
      "added" or "declined" at Phase 7 rather than leaving the box ambiguous**
- [ ] Failing test: **front matter**. A quoted `title:` in a document's own
      front matter, asserting exact columns for an inline inside it. This is
      the path fixed at `meta.rs:259/:303/:316` below
- [ ] Regression tests for a **plain** (unquoted) scalar and a **single-line
      block** scalar — both correct today and must stay correct. Guards
      against over-correcting
- [ ] Add a **text-path** assertion pinning the caret position, not only the
      JSON columns. The absence of any such test is why this survived: the
      existing renderer tests assert on *style markers* (that ariadne drew its
      `╭` box, that annotate-snippets emitted `-->`) and every fixture is pure
      ASCII. Nothing asserted **which bytes get underlined**. See the
      `resolve_span` decision below for the mechanism

**The `resolve_span` decision (settle before writing the caret test).**

`assert_diagnostic_underlines` (`quarto-config/src/span_assert.rs:219`) is
built on `resolve_span` (`:147-166`), which short-circuits top-level
`SourceInfo::Concat` to `Err(SpanProblem::Concat)` at `:159` — and `Concat` is
exactly what corrected provenance produces. `Substring{parent: Concat}` is
worse: it falls through to `resolve_byte_range().ok_or(SpanProblem::Generated)?`
and reports **`Generated`**, sending a reader hunting for a filter-created
node. These are one decision, not two items:

- [ ] **Preferred: teach `resolve_span` piecewise resolution** — resolve the
      two ends via `map_offset(0)` and `map_offset(length())`, the same
      mechanism Phase 4's hull needs, so one implementation serves both. Then
      both `Concat` and `Substring{parent: Concat}` resolve,
      `SpanProblem::Concat` becomes dead, and the caret test can use
      `assert_diagnostic_underlines` directly. This is the direction that
      keeps the helper useful as this epic makes `Concat` the common shape
      across the assertion helpers built on it in `listing/config.rs`,
      `callout.rs` and `materialize.rs` (7 direct callers; more reach it
      transitively through `assert_diagnostic_underlines` and
      `resolve_diagnostic_span`)
- [ ] **Fallback, if piecewise resolution proves wrong:** a discontiguous span
      cannot be described by one `(start, end)` pair, so if asserting on the
      hull turns out to misrepresent what the caret covers, keep the refusal,
      **add the missing `Substring{parent: Concat}` arm** so the label says
      `Concat` rather than `Generated`, and have the caret test assert on
      rendered output instead. Record which path was taken and why in
      § Evidence — the two are mutually exclusive and a later reader must not
      try to do both

**Threading (obligation 1).**

- [ ] Carry content provenance as
      **`ConfigValueKind::Scalar { yaml, content_source_info: Option<SourceInfo> }`**
      — inside the variant, not as a fourth field on `ConfigValue`. Rationale
      in Plan 1's § Hand-off; the load-bearing half is that provenance must not
      be *separable* from the value it describes, because config merging picks
      winners per key and a sibling field can be carried forward while `value`
      is replaced — producing a pair whose string came from one file and whose
      provenance resolves cleanly onto a real offset in another
- [ ] **Budget the variant migration — it is the largest mechanical task in
      this epic and was previously invisible here.** `Scalar` is a **tuple**
      variant today, so making it a struct variant breaks every construction
      *and* every pattern match: **206 sites across 49 files** excluding
      `crates/*/tests/`, **240** including them (Plan 1's figures, confirmed
      independently twice). Plan for the sweep, its snapshot fallout, and the
      **WASM leg** — `wasm-quarto-hub-client` builds against these types on a
      separate target, so `cargo build --workspace` will not catch it and
      Phase 7's `cargo xtask verify` is the first thing that would.
      **Done when** `cargo xtask verify` (full) is green *and* the snapshot
      review below is filed — "planned the sweep" is not a done-condition
- [ ] **Snapshot gate for the front-matter expansion.** Fixing
      `meta.rs:259/:303/:316` moves the span of every quoted `title:`,
      `description:` and caption in every fixture whose spans serialize — a
      larger movement than the JSON-writer `.offset` churn Phase 2 gates. Give
      it the same CLAUDE.md treatment: **count, summary of what changed,
      surprises called out, affected file list.** An earlier revision mentioned
      "snapshot fallout" only in passing here, while gating the smaller churn
      one phase earlier
- [ ] Note for the desync report: content provenance is meaningful only for
      `Yaml::String`. A `Scalar(Yaml::Integer | Real | Boolean | Null)` carries
      `None` and **must not** trip the report below — the "None on a string
      scalar is a bug" rule is scoped to strings, and this is new user-visible
      output, so getting the scope wrong is noisy rather than silent
- [ ] **Set it in one place: `pampa::pandoc::meta::yaml_to_config_value`,
      `meta.rs:242`** (`let source_info = yaml.source_info.clone();`). Derive
      the content provenance there and use it for **both** consumers of that
      binding:
      - store it in the `Scalar` variant, for the deferred project-config path
      - **pass it as the re-parse base at `:259` (`!md`), `:303` (annotated
        `Markdown`) and `:316` (`DocumentMetadata` default)** — the three
        immediate re-parses that fix front matter. Today each pairs the decoded
        `&s` with the node span, which is the bug
- [ ] **Not** `quarto-config`'s `config_value_from_yaml` — that function has no
      production caller (its only call sites are its own tests, a
      `#[cfg(test)]` use at `materialize.rs:495`, and two locally-shadowed
      test helpers of the same name at `project_profile.rs:639` and
      `render_scripts.rs:712` whose bodies call the pampa converter). It is
      exported dead API
- [ ] `meta.rs:34` and `:59` are **forwarders**, not sites:
      `parse_yaml_string_as_markdown_to_config` receives `source_info` as a
      parameter and hands it to `readers::qmd::read`. Nothing there can call
      `content_source_info()`
- [ ] **Read the carried value in one place:** `parse_scalar_string_in_place`
      (`quarto-core/src/transforms/config_markdown.rs:284-290`)
- [ ] **Dispose of the dead converter.** Delete
      `quarto-config/src/convert.rs`'s `config_value_from_yaml` (and its
      `pub use` at `lib.rs:57`), or keep it in lockstep. Deleting is
      preferred: kept, it is a public constructor that can produce
      YAML-rooted scalars with no content provenance — a hole in the invariant
      that compiles. Deleting requires **retargeting `materialize.rs`'s
      `mod spans` tests** (the bd-2mxo / bd-9yh3pzfu span-preservation tests —
      the module opens at `:485` and runs past `:615`, so budget the whole
      module) at `yaml_to_config_value`. Those tests currently guard a
      converter no render uses, so this is a fidelity gain — and the
      `resolve_span` decision above applies to them too
- [ ] **Preserve the serialized wire shape.** `ConfigValueKind`'s
      `Serialize`/`Deserialize` are hand-written
      (`quarto-pandoc-types/src/config_value.rs:222`, `:305`) and the `Scalar`
      arm emits `{"Scalar": <value>}`. Keep that: drop provenance on
      serialize, `None` on deserialize. No `profile_version` bump is needed
      (`ConfigValue` is embedded in `DocumentProfile`, equality enforced at
      `document_profile.rs:842`). The loss is silent, so confirm no
      serialization boundary sits between producer and consumer — measured,
      none does: `pampa/src/readers/json.rs` constructs `Scalar` only on the
      Pandoc-JSON *input* path (`:2719-2867`), and the Lua bridge is Lua
      tables, not JSON. **Record that confirmation in § Evidence**

**The binding regression — do not ship Phase 3 without this.**

Measured by the Plan 3 session and re-verified here; the numbers are in
§ Evidence, so this does not depend on that session being consultable.
Threading keeps `ConfigValue.source_info` contiguous, but the *diagnostics*
produced by the re-parse take `content_source_info` as their parent, so their
locations are `Substring{parent: Concat}` for a multi-line block scalar.
Per-page diagnostics are bound at print time by `attach_config_source`
(`quarto/src/commands/render.rs:1184`, called at `:1270`) →
`bind_config_source` → `bind_source_candidates`, whose first statement is
`info.resolve_byte_range()?` (`quarto-core/src/config_sources.rs:90`) —
`None` for anything `Concat`-backed. The config file is then never registered
and the diagnostic renders with **no source snippet at all**. Today those two
`Q-2-9` warnings print a snippet with carets on the wrong line; after Phase 3
they would print with no snippet. That is a regression on the **block-scalar**
fixture (§ Evidence) — not the ariadne crash repro, which is a different file;
this plan uses "founding" for both and they are not interchangeable.

- [ ] Change `bind_source_candidates` to obtain the file id from
      `info.root_file_id()` rather than `info.resolve_byte_range()?`. It
      discards the range anyway and wants only the id, and `root_file_id`
      handles both `Concat` and `Substring{parent: Concat}`
      (`quarto-source-map/src/source_info.rs:521-532`). This also makes the
      binder agree with the **renderer**, which already resolves the file via
      `root_file_id()` (`quarto-error-reporting/src/diagnostic.rs:819`,
      `:1022`) — today they disagree about how to obtain the same value
- [ ] **Test on the right population.** `project_diagnostics` are
      pre-registered unconditionally by `config_source_context`
      (`render.rs:1172`, printed at `:1246-1248`) and need no binding, so a
      project-level fixture comes back green and proves nothing. The
      regression test needs a **per-page** config diagnostic from a multi-line
      block scalar — the same seam as the block-scalar test above
- [ ] Scope check, **recorded in § Evidence**: only this one call site is
      exposed. The other binders act on `ConfigValue` spans, which stay
      contiguous. `rebase_source_candidates` (`config_sources.rs:140`, used
      from `website_post_render.rs`) is the exception — it genuinely needs the
      range to rebuild an `Original{fid,start,end}`, so `root_file_id` does not
      help it; it is on the inert path (see the ruled-out list below) and stays
      as is

**The two `None`s (obligation 7).**

- [ ] `content_source_info()` returning `None` on a node q2 has already
      established is a string scalar is a **bug** — Plan 1 merged "not a
      scalar" and "derivation desynced" into one `None`, and both are bugs at
      that call site. Report it, but **warning-level and non-fatal**: Plan 1
      rejected `Err` precisely because a walker bug must not turn a working
      render into a hard failure, and a wrong caret beats no output
- [ ] **No `Q-` code for that report.** Decided here: it is an internal
      consistency failure a user cannot act on, so it gets a plain internal
      diagnostic rather than a catalog code. This is not only taste —
      `cargo xtask lint`'s `error-docs-page-missing` and
      `error-docs-sidebar-unlisted` would then require a
      `docs/errors/<subsystem>/<code>.qmd` page **and** an in-code-order
      sidebar entry in the same commit, mechanically enforced
- [ ] `ConfigValueKind::Scalar { content_source_info: None }` at the
      `ConfigValue` layer is **not** a bug — CLI `-M`, Lua and defaults-file
      metadata have no YAML origin. Falling back to `source_info` is inert
      there *because* those producers carry
      `SourceInfo::generated(By::programmatic_config())`
      (`quarto-core/src/stage/stages/metadata_merge.rs:48`, `:71`, `:460`,
      `:463`), where offset arithmetic already yields `None`. **Put that
      reason in a code comment**, so the fallback is never extended to
      YAML-rooted values
- [ ] Note the degradation path in the same comment, so it is not mistaken for
      a bug later: `UserFiltersStage::pre()`
      (`quarto-core/src/pipeline.rs:349`) runs before `ConfigMarkdownTransform`
      (`:1176`), and the Lua bridge discards provenance outbound
      (`pampa/src/lua/config_value.rs:150`) and rebuilds `Scalar` without it
      inbound (`:324-341`). A filter that touches `website.page-footer`
      therefore drops to today's behavior — a caret one byte left, not a
      crash. This is the safe direction, and a direct consequence of putting
      provenance *inside* the variant

**The third consumer.**

- [ ] Fix `caption_inlines` (`quarto-core/src/crossref/codeblock_shorthand.rs`,
      fn at `:644`). `OptionValue.value_source` is set at `:551` from
      `entry.value.source_info` — a live quarto-yaml node — and used as the
      offset base for markdown-parsing the decoded value, so
      `fig-cap: "A *strong* claim"` drifts exactly like the config path. It
      needs no carrier: Plan 1's `content_source_info()` serves it directly.
      **It is also the only path that exercises a `Concat` parent**: cell
      options hands `parse_with_parent` a `SourceInfo::concat`
      (`quarto-core/src/cell_options/mod.rs:227-229`), so content provenance
      there is a Concat *inside* a Concat-parented parse. That is the case
      Plan 1's builder contract ("never call `resolve_byte_range` on the
      parent") exists for — test it here
- [ ] Ruled-out sites, **listed in § Evidence** so nobody re-investigates:
      `project_resources.rs:215/224` (whole-node span, `as_plain_text`, no
      sub-offset arithmetic), `use_cmd/config.rs:229` (already defensive — see
      Phase 4), `lua/config_value.rs:620` (base is `filter_source_info(lua)` =
      `Generated` with no anchors — Plan 3), `theme_diagnostic.rs:331`
      (synthetic test span), `project/website_post_render.rs:217` (same shape
      as the `config_markdown` reader, on the same page-footer data, but its
      parse diagnostics are discarded and only image URLs are collected —
      inert today, and a latent second reader the moment anyone surfaces them)

### Phase 4 — q2's attribute path drives the builder

This is what proves the builder is general rather than YAML-shaped. It gives
`ProvenanceBuilder` a **second real consumer** in the same cycle it ships.

**Decided:** `AttrSourceInfo.attributes[i].1` **changes meaning** to content
provenance. No sibling field, no widening of the struct. Every Rust consumer
wants content — `callout.rs:427` (re-parse base), `theorem.rs:345` and
`proof.rs:182` (a span for a `Str` whose text is already decoded), and
`llms.rs:374-379`/`:1082` only filters the vec in parallel with `attr.2`.

**A note on when this is testable.** § Risks calls a `callout.rs` failure here
a stop-and-fix-Plan-1 signal — but by Phase 4, `ProvenanceBuilder` has already
shipped as `quarto-source-map` 0.1.3, so "fix Plan 1" means another release.
Drive `unescape_punctuation` against a local path override **during Plan 1's
Phase 2 development window**, which already runs under a `quarto-source-map`
path patch and already expects the API to change before it publishes 0.1.3.
*Not* during Plan 1's Phase 1 design-review item, as an earlier revision said:
that item is explicitly "on paper before writing it", and the builder does not
exist yet at that point — you cannot drive a decoder against an API that has
not been written.

- [ ] Failing test first: a div attribute whose value contains a collapsed
      escape, asserting an inner node's `SourceInfo` resolves to the true byte
      position. Measured baseline from `bd-mxa44voa`: for
      `title="Use \`renv\` today"` the code span sits at inner bytes `4..10`,
      maps to `85..91`, and is actually at `86..92` — off by one *before* any
      escape is involved, and one more byte per collapsed escape
- [ ] Drive `ProvenanceBuilder` so `AttrSourceInfo` carries content
      provenance. **The plumbing is larger than "drive it from
      `unescape_punctuation`."** That function
      (`pampa/src/pandoc/treesitter_utils/text_helpers.rs:41`) is private,
      takes a `&str`, and has exactly one caller — `extract_quoted_text` at
      `:28`, which discards offsets. `extract_quoted_text` in turn has **two**
      callers: the div-attribute value at `treesitter.rs:1208-1213` and the
      link/image `title` at **`:1301-1305`** — both producing
      `IntermediateBaseText(String, Range)`
      (`treesitter_utils/pandocnativeintermediate.rs:26`), and a
      `quarto_source_map::Range` is a start/end pair that **cannot express a
      `Concat`** — the same argument Plan 1 uses to reject the content-span
      design. The `SourceInfo` is not built until
      `commonmark_attribute.rs:44-49` (`SourceInfo::from_range`). So either the
      intermediate carries a `SourceInfo`, or the decoder returns a piece list
      bound later; `ProvenanceBuilder::in_file` exists for this shape.
      **Done when** the failing test above goes green — this item is the
      plumbing analysis behind it, not a separately checkable deliverable
- [ ] **A third decoder shadows the identifier — out of scope, and it is not a
      defect.** `treesitter.rs:989` defines a **local closure also named
      `extract_quoted_text`** which open-codes the same strip-and-unescape for
      `shortcode_string`, feeding `IntermediateBaseText` at `:1006` to
      `process_shortcode_string` (`treesitter_utils/shortcode.rs:31`).
      It **cannot drift, by construction**: that function destructures the
      closure's range away — `let ... IntermediateBaseText(id, _) = ...` at
      `:36`, verified — and (per Plan 3, who traced the downstream half)
      recomputes the range from the whole node, with no
      `ShortcodeArg::String` consumer offsetting into it. So the closure's
      range arithmetic at `:1000-1005` is dead code; Plan 3 owns deleting it.
      Unlike the `title` caller it is also **not** reached by changing the
      shared function's return type, so it is separable on both counts.
      Recorded because a Phase 4 implementer greps `extract_quoted_text`, finds
      two definitions, and needs to know which one this phase means — and that
      the other one is not a bug they are declining to fix
- [ ] **The `title` caller is in scope.** Decided here rather than left open:
      changing `extract_quoted_text`'s return type reaches
      `treesitter.rs:1301-1305` whether or not you use the provenance there, so
      the only real choice is use-it-or-discard-it, and discarding would
      deliberately leave a known-wrong span behind a change that already
      touched it. It is a **span-tightening, not a drift fix** — measured, the
      only consumer of `TargetSourceInfo.title`
      (`quarto-pandoc-types/src/attr.rs:121`) is the JSON writer's source-ref
      stream (`pampa/src/writers/json.rs:2079`), which does no sub-offset
      arithmetic, so today's span is quote-inclusive rather than drifting —
      the same shape as `theorem.rs`/`proof.rs`. Expect the same kind of
      fallout: any assertion pinning a link or image title's span moves.
      **It does not widen the 0.2.0 break** — measured, `annotated-qmd` reads
      `attrSource.kvs[i]` and never reads target/title spans, so the TS
      consumer is untouched by this half
- [ ] **Obligation 8: re-check `qmd-syntax-helper` after the meaning change,
      and record the result.** § Risks rules it out *by reachability* — its
      diagnostics come from its own `pampa::readers::qmd::read` call, so it
      never sees this provenance. That conclusion is mine, it is expected to
      hold, and **nothing currently re-tests it**. It matters because its 23
      `start_offset()` sites across 22 files **write to the user's files**
      (`conversions/q_2_33.rs:74-75`, `attribute_ordering.rs:74`,
      `div_whitespace.rs:77`, …) using the accessor that is silently `0` on a
      `Concat`, and this phase changes what `attributes[i].1` means. Failure
      mode is **corrupted source files, not a bad caret** — the worst in the
      epic. Concretely: after the swap, confirm no diagnostic reaching
      `qmd-syntax-helper`'s conversions carries an attribute-derived span, and
      write that confirmation into § Evidence rather than re-deriving the
      reachability argument
- [ ] **Tag verbatim by bytes, not by length.** Plan 1's walker had this bug
      and fixed it; our decoder must not reintroduce it. A piece is verbatim
      **iff its source run is byte-identical to its content run** — equal
      lengths are not sufficient, and a 1→1 piece with differing bytes tagged
      verbatim claims a byte-identity it lacks, which `preimage_in` would let
      the incremental writer act on by Verbatim-copying the wrong bytes. Only
      verbatim pieces coalesce. Zero-content pieces are **stored, never
      dropped** — reversed by Plan 1 on 2026-08-21 after measuring that
      dropping one leaves a source gap and `preimage_in` then returns `None`
      where storing gives `Some(4..14)`. Storing is what keeps the tiling
      gap-free, which the hull below depends on. Our decode is simpler than
      YAML's — `unescape_punctuation`'s cases are `\X`→`X` (2→1) and a
      preserved `\Y` (2→2, byte-identical), so no 1→1 non-identical case
      exists today — but emit pieces from the decode rather than inferring
      tags from lengths, so it stays true if the escape table grows
- [ ] **Delete the `callout.rs:431-447` workaround** — the
      `match value_source.resolve_byte_range()` block. Note the criterion
      precisely: the *function* `attribute_value_source` (`:401-448`)
      survives. It also does the key-index lookup (`:409-411`), the `Attr.2` ↔
      `AttrSourceInfo.attributes` positional-alignment guard for the
      duplicate-key bug bd-3aolj / bd-1e6a5 (`:413-425`), and the
      `Option` → `generated()` fallback. Only the length-arithmetic block is
      the provenance workaround. If *that* block cannot go, the builder is not
      general enough — stop, and see the note above about when this is
      testable
- [ ] **Update the fallout, don't just expect it.** These are work items, not
      predictions: `callout.rs:718`, `:734`, `:750` call `resolve_span` on
      attribute-derived inline spans (their disposition follows the Phase 3
      `resolve_span` decision — they resolve if piecewise lands, else they need
      rewriting); `quarto-ast-reconcile/src/remap.rs:526`'s test-only
      `file_id_of` panics on non-`Original` and must accept the new shape; and
      `theorem.rs`/`proof.rs` spans tighten to exclude quotes, moving any
      location assertion over a theorem or proof `name=`. Reconcile itself is
      clean — `remap_file_ids` is variant-complete
      (`quarto-source-map/src/source_info.rs:485-511`)

**Obligation 8 — the TypeScript boundary.**

`AttrSourceInfo` is serialized as `"a"` (`pampa/src/writers/json.rs:694-708`)
and read by `ts-packages/annotated-qmd/src/block-converter.ts:287` and
`inline-converter.ts:322`, which pull `attrSource.kvs[i]` and resolve it
through `sourceReconstructor.getAnnotatedParseSourceFields`.

**Corrected diagnosis (an earlier draft of this plan had this wrong, and the
error was relayed to Plan 1 — do not reinstate it).** The earlier claim was
that `getSourceLocation` reads the serialized `r` field and therefore hands TS
a range starting at byte 0. It does not: `getSourceLocation`
(`source-map.ts:150-157`) delegates to `resolveChain`, whose `case 2: //
Concat` (`:317-375`) **already walks the pieces** — it builds the concat via
`toMappedString(id)` and returns the range from `map(0)` and
`map(len-1) + 1`. `info.r` is used only on error and empty-concat paths. So a
well-formed `Concat` resolves correctly today, and TS is already doing the
`map_offset`-pair thing this plan prescribes elsewhere.

**The real defect is `resolveChain`'s `Substring` arm** (`:301-315`):

```ts
range: [parentStart + localStart, parentStart + localEnd]
```

— affine composition over the parent's resolved range, which is precisely the
`preimage_in` bug Plan 1 fixes in Rust, in TypeScript. And
`Substring{parent: Concat}` is exactly the shape these value spans take once
provenance is correct, so this arm is where the work is.

**Decided 2026-08-21: TS moves to content semantics.** The quote-inclusive
behavior is the bug, not the contract. `@quarto/annotated-qmd` is a published
public package (`publishConfig.access: public`, v0.1.1), so this is a breaking
behavior change and gets a minor bump.

- [ ] Fix `resolveChain`'s `Substring` arm to compose through the parent's
      mapping rather than affinely over its hull
- [ ] Update `ts-packages/annotated-qmd/test/block-types.test.ts:428-437`,
      which today asserts `source.substring(value.start, value.end)` ∈
      `['"42"', '"test"']` with the comment "values include quotes in source".
      It must assert the **unquoted** value. This test is the reason the
      change is breaking rather than invisible
- [ ] Regenerate the committed `ts-packages/annotated-qmd/examples/*.json` —
      they are Rust JSON-writer output carrying the `kvs` ids and ranges
      (`examples/README.md:33-35`), so Phase 4 makes them stale. Note the
      README's regeneration command names `--bin quarto-markdown-pandoc`,
      which no longer exists; the binary is `pampa`. Fix the README in passing
- [ ] Bump `@quarto/annotated-qmd` to **0.2.0** — the 0.x signal for a
      breaking behavior change
- [ ] Add a permanent test pinning the range for a quoted value and for an
      escaped value, and record the pre-fix range in § Evidence.
      **The runner is `node --import tsx --test test/*.test.ts`**
      (`package.json:30`) — this package has no vitest dependency and no
      vitest config; earlier drafts of this plan said "vitest" and were wrong
- [ ] **Gap-free tiling is a precondition, not an incidental.** Measured: a
      gappy `Concat` has no single range at all. Our decode is gap-free —
      `\X`→`X` consumes the 2 source bytes it replaces, and quote stripping
      trims the content range's ends rather than leaving an interior gap — but
      state it as a requirement on the decoder, because a dropped zero-content
      piece would open a source gap and silently remove this remedy's
      precondition

- [ ] **Reinstated, low priority — close it explicitly either way.**
      `use_cmd/config.rs:229` can be simplified to repoint declarations it
      currently refuses: a gap-free `Concat` does have a hull, obtainable from
      the `map_offset` pair. (Not from `preimage_in` — its `Substring` arm
      composes affinely, `source_info.rs:448-455`; Plan 1 makes it return
      `None` for a `Concat` parent in 0.1.2, so this is compiler-visible
      rather than a trap.) The function is correct today, merely limited, so
      this is optional — but Phase 7 reconciles this checklist, so record
      "done" or "declined" rather than leaving it ambiguous

### Phase 5 — panic boundary (`bd-chmbr0zl`)

Hardening, not a live defect. The specific panic is fixed; this addresses the
**class**. A diagnostic render that aborts an already-successful render, and
discards every diagnostic queued behind it, is disproportionate however the
bad offset arose.

Verified prerequisites, so the phase does not rest on assumption:

- `panic = "unwind"` holds. No profile in the root `Cargo.toml` sets
  `panic = "abort"` (`[profile.release]` sets only `strip`), and wasm32 sets
  `-C panic=unwind` explicitly in `.cargo/config.toml` for the Lua shim.
- `AssertUnwindSafe` is probably unnecessary. Neither **borrowed type** has
  interior mutability: `SourceContext`/`SourceFile`
  (`quarto-source-map/src/context.rs:11-33`) are plain data, and
  `CoalescedDiagnostic` holds no cells. The one exception, checked and
  discounted: `quarto-error-reporting` has
  `static CATALOG: OnceLock<Box<dyn CatalogProvider>>`
  (`src/catalog.rs:67`, `use` at `:15`) — a global static, not a field of
  either type we borrow across the boundary, so it does not make
  `&CoalescedDiagnostic` or `&SourceContext` non-`RefUnwindSafe`.
- Prior art to match: q2 already wraps per-document renders in
  `catch_unwind(AssertUnwindSafe(..))` at
  `quarto-core/src/project/pass2_renderer.rs:499` and
  `orchestrator.rs:1868`. They did not catch this panic because it fires in
  `print_render_diagnostics`, on main, after the render.

- [ ] Add an env-gated fault-injection hook to q2's diagnostic-emission loop,
      `cfg(debug_assertions)`-gated so it **cannot** be armed in a release
      build
- [ ] Failing test first: with the hook armed, assert **exit code 0**, that
      the *other* queued diagnostics still print, that `_site/` is still
      written, and that stderr carries an explicit
      `internal error rendering diagnostic <CODE>` line — the last of which is
      also what makes "surface loudly, do not swallow" testable rather than
      aspirational. Observe red. Note the default panic hook prints
      `thread '…' panicked at …` to stderr before `catch_unwind` returns — the
      assertion must tolerate it, and in production it will be there too,
      which is the intent
- [ ] Implement `catch_unwind` around the **per-diagnostic** render at every
      site, named rather than implied:
      `crates/quarto/src/commands/render.rs:1238-1240` (coalesced pass-2
      failures), `:1246-1248` (`project_diagnostics`), `:1269-1272`
      (coalesced per-page), and the `--json-errors` branch's
      `diagnostic_to_json` calls at `:1356` and `:1394` plus the
      project-level ones below them. The JSON path reads `.column` and cannot
      panic today — that is an argument, not an exemption
- [ ] Verify the `UnwindSafe` obligations and **write the outcome into
      § Evidence**, including "not needed" if that is what it turns out to be
- [ ] Assert a caught panic while printing a **warning** does not change the
      exit code. Sequencing already favors this:
      `print_render_diagnostics` runs after `_site/` is written and before
      `should_exit_nonzero` (`render.rs:836`/`:1010`, `:1026-1028`)
- [ ] Close `bd-chmbr0zl`

### Phase 6 — shadowing audit

Each fix removes the evidence for the one below it:

```
Plan 1 (correct provenance)  ──shadows──►  the char-boundary snap
the char-boundary snap       ──shadows──►  the panic boundary
```

That is shadowing **in the field**, not in the tests. The governing principle
is that **each level's test injects the defect at that level's own input
boundary**, so no upstream fix can remove it — which holds for Phase 3/4 (real
fixtures), Phase 5 (deliberate fault injection), and the snap's unit test
(literal offsets, never touching `map_offset`).

**Decision taken, recorded so it is not relitigated:** after Plan 1 lands, q2
has no known producer of mid-character offsets, so there is deliberately **no
end-to-end crash test** in q2. Coverage for the char-boundary snap lives in
`quarto-error-reporting`'s own unit test, which constructs a mid-char span
directly — a real bad input at that crate's real API boundary.

**Superseded:** an earlier draft accepted that the two `..._does_not_panic`
integration tests would become vacuous and treated their vacuousness as a
matrix observation. Plan 1's obligation 3 rejected that — it is the epic's only
regression coverage for its founding crash — and Phase 1 above re-anchors them
at unit level instead. The matrix below no longer expects a vacuous pass.

This whole phase is in-session verification (§ In-session verification,
instance 2); reverts cannot be permanent tests. **Every change this plan makes
needs a row** — the closing item demands each test be shown to fail, and a
change with no row exits the audit unbound.

- [ ] Revert **only the three re-parse bases** (`meta.rs:259/:303/:316`) →
      expect T4 (quoted column) and T6 (front matter) red, T5 still green.
      Split from the carrier revert below on purpose: bundled, the matrix
      cannot tell a half-done Phase 3 from a complete one
- [ ] Revert **only the carrier read** (`config_markdown.rs:284-290`) →
      expect T5 (block scalar) red, T4/T6 still green. Prefer both of these to
      reverting Plan 1's quarto-yaml fix: they bind the code we wrote and need
      no cross-repo override
- [ ] **Mutate, don't revert, for T7**: apply the content-provenance base to
      the plain-scalar path as well → expect the plain / single-line-block
      regression assertions red. T7 cannot be bound by reverting our code,
      because it asserts values that are already correct
- [ ] Revert the `Location.offset` floor only → expect **T2 (the zero-width
      label test, in `quarto-error-reporting`)** red, in addition to
      `quarto-source-map`'s own tests. T2 lives in a different repo from the
      hunk that binds it, so it needs naming here or it is audited by nobody
- [ ] Revert the `bind_source_candidates` → `root_file_id()` change only,
      **keeping** the provenance swap → expect the binding regression test
      red. Reverting the swap instead makes it pass vacuously, since there is
      then no `Concat` to refuse
- [ ] Revert the `caption_inlines` fix only → expect its `fig-cap` assertion red
- [ ] Revert the `resolve_span` change only (whichever of the two paths was
      taken) → expect the caret test red
- [ ] Revert the Phase 4 attribute swap only → expect Phase 4's Rust
      assertions red
- [ ] Revert the TS `Substring`-arm fix only → expect the annotated-qmd range
      test red
- [ ] Revert the char-boundary snap only → expect its **unit** test red
      (including the offset pair re-anchored into it in Phase 1)
- [ ] Revert the panic boundary only → expect the injection test red
- [ ] Revert the **desync warning report** only → expect its test red. It is
      injectable (feed a scalar whose provenance derivation is stubbed to
      `None`), therefore revertible, therefore it needs a row like everything
      else
- [ ] Revert the **verbatim-by-bytes tag rule** only (tag by length instead) →
      expect a test asserting a non-identical 1→1 piece is *not* verbatim to go
      red. If no such case exists in our escape table, say so here and mark
      this row accepted-unbindable rather than silently dropping it
- [ ] Revert the `Concat` exclusive-end fix only → expect the
      `qmd_writer_source_info.rs` assertion added in Phase 2 red
- [ ] **Where each revert runs, and what it costs.** The matrix spans three
      repos plus the TS package, so say it plainly rather than discovering it
      mid-audit: the provenance, binder, caption, `resolve_span`, attribute and
      TS reverts are in-tree; the snap revert runs in
      `~/src/quarto-error-reporting` alone; the floor and exclusive-end
      reverts are in `~/src/quarto-source-map` and require a temporary
      `[patch.crates-io]` override in whichever workspace runs the tests.
      Those overrides are scaffolding — Phase 7 confirms none survive
- [ ] Record every observation in § Evidence
- [ ] Any test that cannot be made to fail: fix it or delete it, and say which

### Phase 7 — land

- [ ] Reconcile this checklist against reality before handoff — re-read it,
      verify each box against what actually landed, correct any that are
      wrong, and commit the updated plan
- [ ] `cargo xtask verify` (full, not `--skip-hub-build`) green — the WASM leg
      matters here, because the `ConfigValueKind::Scalar` migration reaches
      `wasm-quarto-hub-client` through `quarto-pandoc-types`
- [ ] `cd hub-client && npm run build:all`; and run the annotated-qmd tests
      directly with `node --import tsx --test test/*.test.ts` —
      `cargo xtask verify` *builds* ts-packages but does not run their tests
- [ ] Confirm no `[patch.crates-io]` override for these crates remains, in any
      worktree used for the Phase 6 matrix
- [ ] Commit the plan; hand off to `finishing-a-development-branch`. The epic
      `bd-mxa44voa` stays **open** — Plan 3 is outstanding

## Risks

- **The fault-injection hook is a new permanent seam** in q2's diagnostic
  path. It must be impossible to arm in a release build.
- **`catch_unwind` is a no-op under `panic=abort`.** Verified unwind for every
  profile that matters (Phase 5), but re-check if a profile is ever added.
- **Phase 4 is the load-bearing test of Plan 1's API, and it runs after Plan 1
  has shipped.** If `callout.rs`'s length-arithmetic block cannot be deleted
  against correct provenance, the builder is not general enough — but by then
  fixing Plan 1 costs two more releases. That is why Phase 4 asks for the
  attribute decoder to be driven against a path override during Plan 1's
  design-review item.
- **Phase 4 breaks a published npm package.** `@quarto/annotated-qmd` v0.1.1
  is public, its documented invariant is
  `source.value.substring(start, end)`, and its own test pins quote-inclusive
  text. Moving to content semantics is deliberate (decided 2026-08-21) and
  carries a 0.2.0 bump, a test rewrite and regenerated example fixtures. This
  is the one place in this plan where getting it wrong produces a *wrong range
  in a shipped consumer* rather than a wrong caret.
- **The `ConfigValueKind::Scalar` migration is 206 non-test sites.** It is
  mechanical and compiler-guided, but it is the largest single piece of work
  here and the WASM target will not fail until `cargo xtask verify`.
- **`start_offset()` / `end_offset()` on a `Concat` stay unhardened, and that
  is a decision rather than an oversight.** They return 0 and the content
  length. An earlier round proposed a `debug_assert` in 0.1.2; Plan 1 declined
  and is right to. Those values are the *correct* answers in content space —
  `end_offset()` returning the content length is documented behavior — so an
  assert would fire on legitimate callers and cannot distinguish them from
  callers who conflated content space with file space. The defect lives at the
  call sites. Two live *drifting* Rust consumers are known:
  `bind_source_candidates` (fixed in Phase 3) and `use_cmd/config.rs:229`,
  which reads both and is safe only because of its read-back check. **Plus 23
  `start_offset()` reads across 22 files in `qmd-syntax-helper`**, ruled out by
  reachability rather than by inspection of each — and re-confirmed in Phase 4
  under obligation 8, because that crate writes to user files and this epic
  changes what the accessor is reading. (An earlier revision listed
  `getSourceLocation` here; that was a consequence of the retracted byte-0
  diagnosis — it is a TypeScript function and reads these accessors' *values*
  only through the serialized `r`, on error paths. See Phase 4.) Fix a third
  at its own call site if one appears.
- **Do not make `quarto-source-map`'s `Concat` arms consistent with each
  other.** Not our repo, but our Phase 3 removes one of the call sites that
  depends on the current asymmetry, so it is worth carrying. `resolve_byte_range`
  refuses (`None`) for a `Concat` while `preimage_in`, three functions away in
  the same file, returns a hull — and `resolve_byte_range`'s `Substring` arm is
  `parent_start + start_offset`, arithmetic **identical** to `preimage_in`'s.
  It is safe only because of that refusal, not because its arithmetic is
  better. Anyone "fixing" the apparent oversight by teaching
  `resolve_byte_range` to return a hull converts every one of its call sites
  from safe to silently wrong in a single commit, with no test failing.
  **The reverse direction is worse.** After 0.1.2, `preimage_in` refuses for a
  `Substring{parent: Concat}`, so git history shows it "used to" answer and
  restoring the hull reads as undoing an over-conservative change rather than
  proposing a new behavior — it arrives disguised as a revert. Its tell,
  per Plan 1: if the argument is *"we only return the hull when every piece is
  length-matched"*, that is a withdrawn proposal verbatim, and the
  counterexample is the 1→1 fold. Plan 1 records both directions as an explicit
  do-not; the membership test for the whole defect family is "someone wrote `+`
  over a parent willing to hand back a flattened range," which is why
  `map_offset` — which recurses instead of flattening — is not a member.
- **Two of this plan's verifications are unrepeatable.** If the Phase 1
  in-session revert is skipped, the binding evidence for the ariadne crash
  regression test is gone permanently — the floor makes it unreproducible.

## Evidence

_A phase is not done until its evidence is here._

### The block-scalar fixture (transcribed here and in Plan 1)

Phase 3's block-scalar test and the binding regression test both need this.
It is **not** the ariadne-emoji-panic repro — that one is a single-line
single-quoted navbar `text:` and cannot produce accumulating drift at all. It
lives untracked at `workspace-1/.scratch/blockscalar/_quarto.yml`, under a
`.git/info/exclude`'d directory in another worktree.

Plan 1 has since transcribed it too, along with a second `accum/` variant, so
it no longer depends on `.scratch/` surviving in any one worktree. The copy
below is deliberate redundancy, not a claim of exclusivity — an earlier
revision of this section said it existed nowhere else, which was true of an
earlier Plan 1 and is no longer.

```yaml
# _quarto.yml
project:
  type: website
website:
  title: "T"
  page-footer:
    center: |
      line one
      line two
      <span id="y">Footer</span>
```

```markdown
<!-- index.qmd -->
---
title: "Index"
---

body
```

`center: |` is line 6; the `<span id="y">Footer</span>` is on **line 9**, its
two tags at columns **7** and **26**. Both `Q-2-9` warnings belong there;
today they are reported at `8:10` and `9:14` — a constant drift of 12, being
2 preceding content lines × 6 bytes of stripped indent.

**Settled.** An earlier revision flagged that the three documents named three
different keys for this measurement (Plan 1's seam said navbar `text:`; this
plan and the epic said `page-footer.center`). Plan 1 built and measured both
candidates on 2026-08-21 and corrected its seam to `page-footer.center`; the
numbers above come from that run, which is the first time the epic's cited
pair was tied to a file rather than carried forward.

### Pre-execution baselines (`quarto-source-map` 0.1.1)

Measured by the Plan 3 review session, 2026-08-21, using a throwaway Rust test
that constructed the `SourceInfo` shapes below by hand and printed each
accessor's answer. Reproduced here because **that probe was reverted after
capture** — the code is no longer in the tree,
so this record plus the fixture spec below is the only reconstruction path.

**Read with the caveat:** the `Concat` was **hand-built**, not
`ProvenanceBuilder`-produced (the builder does not exist yet). These numbers
measure 0.1.1's behavior *given that shape*, not the eventual pipeline. The
shape is faithful to Plan 1's derivation — a 2→1 replacement flanked by
verbatim runs, gap-free — so the real builder is expected to produce it, but
that is inference, not measurement.

Fixtures, `fid = quarto_yaml::file_id_for_filename(<temp path>)`, candidate a
real file on disk, each `bind_config_source` call given a fresh
`SourceContext` with the file **not** pre-registered (modelled on
`quarto-core/tests/integration/project_profile_overlays.rs:447-465`):

- **A** — gap-free, models `'it''s'`:
  `concat(vec![(Original{fid,1,3}, 2), (Original{fid,3,5}, 1), (Original{fid,5,6}, 1)])`;
  content 4 bytes, source extent 1..6, piece 2 the doubled quote
- **B** — gappy, models a naive block scalar with the indent dropped as a
  standalone deletion: `concat(vec![(Original{fid,6,10}, 4), (Original{fid,12,15}, 3)])`,
  source gap at 10..12
- **C** — `substring(A, 0, 4)`, the whole content. **This is the shape a
  diagnostic carries after Phase 3.**

```
1.  A.resolve_byte_range()          = None
2.  C.resolve_byte_range()          = None
3.  C.root_file_id()                = Some(FileId(9415328668825900988))
4.  A.preimage_in(fid)              = Some(1..6)
5.  B.preimage_in(fid)              = None
6.  C.preimage_in(fid)              = Some(1..5)      # WRONG; truth is 1..6
7a. C.map_offset(0, &ctx)           = Some(offset: 1, row: 0, column: 1)
7b. C.map_offset(4, &ctx)           = Some(offset: 6, row: 0, column: 6)
8.  bind_config_source(&C, [path])  = None
9.  bind_config_source(&Original(1,6), [path]) [control] = Some("…/probe.yml")
10. C.start_offset() = 0   C.end_offset() = 4   C.length() = 4
    A.start_offset() = 0   A.end_offset() = 4   A.length() = 4
```

What each line is load-bearing for: (8) vs (9) is the Phase 3 binding
regression. (6) vs (7a/7b) is why a hull must come from the `map_offset` pair.
(4) vs (5) is why gap-free tiling is a decoder requirement. (10) is why
content coordinates must never be read as file offsets.

**Make these permanent** — tracked as items in the phases that own them, not
here: 7a/7b wherever a hull is computed (Phase 4), 8/9 as the binding
regression test (Phase 3), 10 as the TS-facing assertion (Phase 4). Line 6 is
a permanent test too, of the *opposite* value: Plan 1 fixes `preimage_in`'s
`Substring`-over-`Concat` arm in 0.1.2, so the wrong `Some(1..5)` becomes
`None` — assert the `None`. Plan 1 owns that assertion in its own Phase 1;
do not duplicate it here.

### Phase 1
_(pending)_

### Phase 2
_(pending)_

### Phase 3
_(pending)_

### Phase 4
_(pending)_

### Phase 5
_(pending)_

### Phase 6
_(pending)_
