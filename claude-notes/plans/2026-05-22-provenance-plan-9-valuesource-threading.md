# Provenance Plan 9 — ValueSource threading for metadata-derived content

**Date:** 2026-05-22
**Branch:** feature/provenance
**Status:** Research plan (pre-implementation; API surface not yet pinned)
**Milestone:** none directly — improves attribution / round-trip provenance
  reporting; does not gate M3.

## Epic context

Part of the **provenance epic** (Plans 3–10). Plan 6 stamps every
pipeline-synthesized node with `Generated { by, from }`; for most
synthesizers the `from` list is non-empty only when there's a
body-source token to anchor at (shortcode resolutions → `Invocation`).
**Several synthesizers consume metadata values (frontmatter,
`_quarto.yml`, `_metadata.yml`) and currently emit `from: []`** because
the value-side source info is discarded somewhere between the YAML
parser and the synthesizer's stamping point. Plan 9 threads it the
last hop and stamps `ValueSource` anchors on those consumers, so
attribution tooling can trace rendered content back to the YAML keys
that produced it.

Plan 9 is the **consumer wiring** half of the provenance epic. Plan 6
stamps the identity (`by`); Plan 9 stamps the origin (`ValueSource` in
`from`) on the metadata-derived subset. Together they make every
pipeline-produced metadata-derived node fully attributable.

## Goal

Thread per-value `SourceInfo` to where synthesizers can stamp it as
`ValueSource` anchors. Three target consumers:

1. **Meta/var shortcode resolutions** (closes bd-129m3) — `{{< meta
   footer >}}` → `Generated { by: shortcode("meta"), from:
   [Invocation -> token_si, ValueSource -> value_si] }`.
2. **DocumentProfile.title → nav-text** (closes bd-8pmq3) — sidebar /
   navbar entries built from `profile.title` carry a `ValueSource`
   anchor pointing at the source qmd's title metadata bytes.
3. **Appendix container metadata-derived sections** (currently
   unowned in beads) — per-section sub-Divs for license, copyright,
   citation each stamped with `ValueSource` pointing at
   `meta.license` / `meta.copyright` / `meta.citation`.

Plus the **Plan 7 deferred invariant tests** that depend on at least
one ValueSource consumer existing (the `preimage_in` role-asymmetry
unit test and the appendix-license end-to-end round-trip test).

When this plan lands, the `Invocation` vs `ValueSource` asymmetry
contract Plan 7 documents has real exercise — there are producers,
the writer correctly walks only the `Invocation` anchors, the
attribution machinery can light up the `ValueSource` data without any
writer changes.

## Scope

### In scope

#### Phase 1 — Infrastructure

- A provenance-aware conversion API alongside the existing
  `config_value_to_inlines(value: &ConfigValue) -> Vec<Inline>` in
  `crates/quarto-core/src/transforms/shortcode_resolve.rs:167`.
  **API shape (settled per user direction):**

  ```rust
  /// Convert a ConfigValue to inline content, returning both the
  /// inlines and the source_info pointing at the value's definition
  /// site. The caller decides how to stamp the source_info (typically
  /// as an `AnchorRole::ValueSource` on a surrounding `Generated`).
  ///
  /// For `PandocInlines` content, the returned source_info is the
  /// outer ConfigValue's; per-leaf source_info is preserved on the
  /// inlines themselves and is not flattened.
  fn config_value_to_inlines_with_provenance(
      value: &ConfigValue,
  ) -> (Vec<Inline>, SourceInfo);
  ```

  The existing `config_value_to_inlines` stays for legacy callers
  (template values, non-provenance contexts). New consumers route
  through the provenance-aware version.

- `DocumentProfile` gains `title_source_info: Option<SourceInfo>`
  (per bd-8pmq3's detailed plan: ~30–50 LOC including `extract`
  change + `Default` impl at `crates/quarto-core/src/document_profile.rs`).
  Uses `#[serde(default, skip_serializing_if = "Option::is_none")]`
  — same pattern as `order: Option<i32>`. **No
  `DOCUMENT_PROFILE_VERSION` bump** (additive `Option<_>` with
  default; per document-profile-contract §"Serialization and
  versioning"). Update the contract's §Change log.

- New typed enum `AppendixSection { License, Copyright, Citation }`
  in `crates/quarto-source-map/src/source_info.rs`, with serde
  derive. Discriminator for `By::appendix` (see Phase 4).

#### Phase 2 — Meta/var shortcode ValueSource (closes bd-129m3)

- `MetaShortcodeHandler::resolve`
  (`crates/quarto-core/src/transforms/shortcode_resolve.rs:148`) and
  the matching `var` handler look up via
  `ctx.metadata.get_nested(&key)` which returns a `&ConfigValue`
  whose `.source_info` is the value's definition site.
- Construct resolved inlines via
  `config_value_to_inlines_with_provenance`, then stamp the
  surrounding `Generated` with both anchors:

  ```rust
  let (inlines, value_si) = config_value_to_inlines_with_provenance(value);
  let mut gen = SourceInfo::generated(By::shortcode(name));
  gen.append_anchor(AnchorRole::Invocation, Arc::new(token_si));
  gen.append_anchor(AnchorRole::ValueSource, Arc::new(value_si));
  // attach `gen` to each resolved inline
  ```

- Belt-and-suspenders for `ConfigValueKind::PandocInlines`
  (markdown-rich metadata like `title: "**Bold**"`): the `ValueSource`
  is attached on the wrapping shape, **not** pushed into every leaf
  — keeps Plan 7's multi-inline dedupe rule (which compares
  `invocation_anchor()` source_info structurally) trivially correct
  with no ValueSource cross-talk.

#### Phase 3 — DocumentProfile.title → nav-text (closes bd-8pmq3)

- Update `DocumentProfile::extract`
  (`crates/quarto-core/src/document_profile.rs:529`): replace
  `title: plain_text_field(meta, "title")` with code that also
  captures `meta.get("title")?.source_info.clone()` into the new
  `title_source_info` field.
- Three Plan-6 Phase-5 consumer sites attach
  `ValueSource(profile.title_source_info)` when present:
  - `crates/quarto-core/src/transforms/sidebar_generate.rs:228`
  - `crates/quarto-core/src/transforms/sidebar_auto.rs:311` (only
    when reading from `profile.title`; file-stem fallback at line 318
    keeps `from: smallvec![]`)
  - `crates/quarto-core/src/transforms/navigation_enrich.rs:59`
- Subtitle / description / date / image fields stay out-of-scope
  (not consumed by nav sites today). Inline-rich titles
  (`ConfigValue::PandocInlines`) preserved by Phase 1's API design.

#### Phase 4 — Appendix metadata-derived sub-Divs

- `create_license_section` / `create_copyright_section` /
  `create_citation_section` in
  `crates/quarto-core/src/transforms/appendix.rs` (lines 270–) read
  `meta.get("license")` / `.get("copyright")` / `.get("citation")` —
  the source_info is on those `ConfigValue` references and just
  needs to ride along.
- **Per-section sub-Div stamping (option A):** each per-section Div
  carries
  `Generated { by: By::appendix(AppendixSection::License), from: [ValueSource(license_si)] }`,
  with the outer container kept at
  `Generated { by: By::appendix_container(), from: [] }`.
- **`By::appendix` becomes parameterized** (settled per user
  direction): drops the existing nullary `By::appendix()`
  constructor in favor of `By::appendix(section: AppendixSection)`.
  See §Design decisions for backward-compat rationale (no
  production callers; no persisted wire artifacts).
- Need a separate `By::appendix_container()` (or similar) for the
  outer wrapper Div, since the wrapper isn't tied to a single
  metadata key. Tentative name `By::appendix_container()` —
  discriminate during implementation.
- Missing-key cases (no `license` in meta) gracefully skip — no
  ValueSource attempt, no synthesizer fires.

#### Phase 5 — Plan-7 invariant tests (deferred from Plan 7)

Status: Plan 7 shipped on `feature/provenance` 2026-05-24 (phases
1-7 + 9; Playwright e2e matrix carried separately in `bd-3izo3`).
These tests are now unblocked — they need a real `ValueSource`
consumer (Phase 4's appendix synthesizer) to exercise the
`Invocation`-vs-`ValueSource` asymmetry that Plan 7's writer
implements. Until Phase 4 stamps `ValueSource` anchors on the
appendix synthesizer, the structural-only versions of these tests
remain in Plan 7's `quarto-source-map` test module (the `preimage_in
skips non-Invocation roles` unit test, lines 982-986 of Plan 7).


- **`preimage_in` role-asymmetry unit test**: build
  `Generated { by: By::appendix(AppendixSection::License), from: [ValueSource(meta_si)] }`
  where `meta_si` is `Original { file_id: 0, start: 10, end: 25 }`.
  Call `preimage_in(FileId(0))` and assert it returns `None` (NOT the
  byte range of the meta-key — that would copy YAML into the body).
  Pins the `Invocation` vs `ValueSource` asymmetry documented in
  Plan 7 §`preimage_in` semantics. Lives in `quarto-source-map`'s
  test module.

- **Appendix-license end-to-end round-trip test**: build a project
  fixture with frontmatter `license: MIT` and a synthesized
  appendix (no user-written `:::{.appendix}` block). Run the full
  q2-preview pipeline → write back to qmd. Assert:
  - (a) no `license: MIT` bytes outside the YAML frontmatter range
    (the meta YAML must not leak into the body);
  - (b) output qmd is byte-identical to input qmd (round-trip
    stability — the synthesized appendix Div is dropped from
    output and re-synthesized next pipeline run).

  Covers the Phase 4 shape end-to-end. Belt-and-suspenders against
  a future refactor that "leniently" tries `value_source_anchor()`
  when `invocation_anchor()` returns None.

- **Multi-inline dedupe-by-Invocation test**: build a Para with
  three inlines each carrying
  `Generated { by: shortcode("meta"), from: [Invocation -> token_si, ValueSource -> value_si] }`
  (Phase 2 shape). Reconcile against an identical Para. Assert
  Plan 7's writer emits the shortcode token bytes ONCE — confirms
  dedupe consults `Invocation` only, not the full anchor list, and
  doesn't mis-fire if ValueSource source_infos differ.

- **Inline-level role-asymmetry test**: similar to the unit test
  but at the inline level, e.g. a `Span` synthesized by some
  metadata-aware transform with `[ValueSource only]`. Assert
  `preimage_in` returns None at the inline level too.

#### Phase 6 — Plan 7 cross-reference cleanup

- Reword Plan 7's §`Invocation` vs `ValueSource` consumer asymmetry
  subsection (added by commit `6a2797b6`) to point at Plan 9's
  Phase 4 as the canonical example, rather than asserting that the
  shape "is stamped today." Small docs change; closes the
  wording-vs-reality gap.
- Cross-link Plan 7's §Test plan to Phase 5's tests' new homes.

### Out of scope (rationale per item)

- **bd-36fr9 (Dispatch anchor for Lua filter / handler-shortcode)** —
  Conceptually adjacent (another anchor role for diagnostic-only
  attribution), but the precondition is *register Lua filter files in
  `SourceContext` and assign them `FileId`s*, which touches the Lua
  engine bridge, cache-key surface, and SourceContext interning.
  Sized for its own plan: **Plan 10**. Plan 9 stays
  metadata-flavored.

- **bd-12vrr (callout default-title)** — Callout titles ("Note",
  "Tip", "Warning") come from a static list, not from metadata. The
  work needs `By::callout()` and an atomicity decision but doesn't
  fit the "thread source-info from metadata" thesis. Standalone
  follow-up — see bd-12vrr's comment on the popup-menu use case.

- **bd-1inj0 (code-block decoration synthesizers)** — Filenames and
  captions come from chunk options / Attr, not from `ConfigValue`.
  Different threading path (`AttrSourceInfo`, currently broken at the
  merge layer per bd-1e6a5 / bd-3aolj). Wait for those preexisting
  `Attr` bugs to land before doing decoration ValueSource. Standalone
  follow-up.

- **bd-2mxo (MergedConfig::materialize() strips source_info)** —
  Real P2 bug, but per the issue itself "Scalar values are preserved
  correctly." Plan 9's consumers read scalar values (`license: "MIT"`,
  `title: "Foo"`); the bug affects map and array container
  source_info, which Plan 9 doesn't need at the leaf level. Stays as
  a parallel P2 fix that doesn't block Plan 9. (See §Risk areas
  for the one corner where map-shaped metadata interacts.)

- **bd-z2j7o (`WithSourceInfo<T>` wrapper audit)** — Phase 1's
  threading work may surface a third or fourth ad-hoc `(value,
  source_info)` pair. If so, that's evidence for the audit but Plan 9
  doesn't pre-decide the refactor.

- **bd-hjv5o (source-location-driven path resolution)** — Different
  problem: uses `SourceInfo` to *change behavior* (resolving paths
  relative to declaration site), not to *stamp anchors*.

- **Hub-client UI consumption of ValueSource anchors** (hover-preview
  showing "this title came from `_quarto.yml:title`"). The
  Rust-side correctness is independently verifiable via tests; the
  hover-UX is a separate hub-client plan.

- **Subtitle / description / date / image source_info on
  DocumentProfile** — extend when a consumer needs them; this plan
  ships title only (the only field the three nav sites consume).

## Design decisions (settled)

- **Per-section sub-Div appendix attribution (option A).** Each of
  license, copyright, citation gets its own typed `By::appendix`
  variant carrying its own `ValueSource`. Enables fine-grained
  hover-attribution UX. Trade-off: more sub-Divs, but the
  structural cost is small.

- **`By::appendix(AppendixSection)` typed enum constructor.** Settled
  over the alternatives (string-keyed `by.data`, `&'static str`
  parameter) because the discriminator is load-bearing and a typed
  enum is checked at the compiler. Adding new appendix-section
  variants in the future is a deliberate enum change — the right
  kind of friction.

- **No backward-compat carve-out for `By::appendix`.** The shape
  change is clean. Reasons (verified):
  1. No production callers today — only test sites in
     `source_info.rs` itself. `transforms/appendix.rs` still emits
     `SourceInfo::default()`; Plan 6 will add stamping after this
     plan finalizes the constructor.
  2. `By` is workspace-internal Rust — no FFI, no extension SDK,
     no TS-side mirror. The hub-client's TS hand-mirror is
     `atomicCustomNodes` for `CustomNode` types, not `By` kinds.
  3. Wire format: `By` serializes to `{kind, data}` via serde. No
     persisted artifact contains `By::appendix` today (Plan 6
     stamping hasn't shipped). No migration needed.

- **Idiomatic API: `(inlines, source_info)` returned for caller to
  wrap.** `config_value_to_inlines_with_provenance` does not stamp
  `Generated` itself, because `by` varies by caller (meta-shortcode
  vs. appendix sub-Div have different `By` kinds). Parallels how
  other source-info helpers in this codebase work.

- **`AnchorRole::Other` policy explicit (per user direction):** the
  `preimage_in` walker walks `Invocation` only; **all other roles,
  existing or future, are not consulted by the writer.** Documents
  the intent so an extension introducing `AnchorRole::Other("preimage-source")`
  knows it'll be ignored. Stated in the doc-comment on
  `preimage_in` and re-asserted in §`Invocation` vs `ValueSource`
  consumer asymmetry in Plan 7.

- **`ValueSource` is wrapper-level for `PandocInlines`-shaped
  metadata, not per-leaf.** Phase 2 attaches ValueSource on the
  surrounding `Generated` (one wrapping each resolved inline), not
  inside the rich-content inlines themselves. Two reasons:
  (a) keeps Plan 7's multi-inline dedupe rule clean (it consults
  Invocation, not anchors on inlines);
  (b) maps the user mental model: "this shortcode resolution came
  from there" — not "this individual Str came from there".

- **Plan posture: research plan.** This document settles the API
  shape (constructors, function signatures, enum variants); it does
  not yet commit to the implementation order or unit-test names.
  A subsequent review pass converts it to a development plan with
  checklisted phases.

## API surface to settle (research-plan deliverables)

By the time this plan converts to a development plan, the following
must be pinned:

1. **`config_value_to_inlines_with_provenance` signature** — exact
   return type, behavior for nil values, behavior for
   `PandocInlines` (returns `(inlines, value.source_info.clone())`,
   confirmed). Edge: `Concat`-shaped ConfigValue source_info — does
   the consumer get the Concat or just the start range? Recommend
   passing the full `source_info` regardless of shape; consumers
   that need a single range can call `resolve_byte_range`.

2. **`AppendixSection` enum variants** — `License`, `Copyright`,
   `Citation` are the three sections `transforms/appendix.rs` knows
   about today. If there are more synthesized sections (or planned
   ones), enumerate them now. Verify against
   `crates/quarto-core/src/transforms/appendix.rs:135-170`.

3. **`By::appendix_container` (or equivalent) for the outer
   wrapper** — name and signature. `By::appendix_container()` is
   tentative; could also be `By::appendix(AppendixSection::Container)`
   if treating "container" as a section variant feels right. Pick.

4. **`DocumentProfile.title_source_info` field placement and
   accessor surface** — direct field access (current convention) or
   a typed accessor (`fn title_with_source(&self) -> Option<(&str,
   &SourceInfo)>`)?

5. **`AnchorRole::Other` doc-comment text** — exact wording of the
   "future roles default to non-walked" policy. Lives on
   `AnchorRole::Other` and on `SourceInfo::preimage_in`.

## Open questions for implementation

- **Granularity of `ValueSource` for nested `meta.license` shapes.**
  YAML like `license: {name: MIT, url: ...}` produces a
  `ConfigValueKind::Map`. bd-2mxo notes the merge step strips map
  container source_info. Recommended approach for Phase 4: anchor
  at the **first scalar leaf** (`name`) when the value is map-shaped,
  falling back to the outer key when materialize has already
  stripped the container. Notes the limitation; full fix waits for
  bd-2mxo.

- **Multi-anchor cost on Phase 2's two-anchor shape.** Every
  meta-shortcode resolution gains a second anchor. Memory: 2 ×
  `Anchor` per inline. With `SmallVec<[Anchor; 2]>` already in place
  (Plan 4), this stays on the stack. Verify no allocation regression
  in a perf-sensitive document benchmark.

- **Cross-reference test fixtures for Phase 4.** The
  appendix-license e2e fixture needs to exercise the
  YAML-meta-only form (not user-written `:::{.appendix}`). Phase 4
  needs to ensure the synthesizer fires only on the metadata path,
  not on user-written appendix blocks. Confirm by reading
  `appendix.rs:135-170` carefully during implementation.

- **`PandocInlines`-shaped metadata behavior in Phase 2.** When
  `title: "**Bold**"` resolves to `[Strong[Str], Space, Str]`, each
  resolved inline gets a wrapping `Generated` with the ValueSource
  on the wrapper. The Bold's children (Str) themselves carry their
  own source_info (the parsed positions inside the YAML string).
  Test: an edit to the resolved Bold inline goes through Plan 7's
  soft-drop because the wrapper is atomic-kind (shortcode); the
  user-edit is reverted with Q-3-42. Confirm.

## References

- `crates/quarto-pandoc-types/src/config_value.rs:155,170` —
  `ConfigValue.source_info` and `ConfigMapEntry.key_source` (already
  in place; Plan 9 just propagates them to consumers).
- `crates/quarto-core/src/transforms/shortcode_resolve.rs:148-167` —
  `MetaShortcodeHandler::resolve` and `config_value_to_inlines`;
  Phase 1/2's primary edit site.
- `crates/quarto-core/src/transforms/appendix.rs:135-260` —
  `create_*_section` functions; Phase 4's edit site.
- `crates/quarto-core/src/document_profile.rs:271,487,529` —
  `DocumentProfile` field declaration, Default impl, `extract`
  helper; Phase 3's edit site (+ doc-contract Change Log).
- `crates/quarto-core/src/transforms/sidebar_generate.rs:228`,
  `sidebar_auto.rs:311,318`, `navigation_enrich.rs:59` — Plan-6
  Phase-5 nav consumers; Phase 3's stamping sites.
- `crates/quarto-source-map/src/source_info.rs:91-118` —
  `AnchorRole` enum (`Invocation`, `ValueSource`, `Other`);
  Phase 1 adds `AppendixSection` here.
- `crates/quarto-source-map/src/source_info.rs:529` — `By::appendix`
  constructor; Phase 4 modifies (signature change).
- Plan 6 §"ValueSource follow-up" (line 509-547) — Plan 9's
  scope-pickup point.
- Plan 7 §`Invocation` vs `ValueSource` consumer asymmetry
  (added by commit `6a2797b6`, not yet on `feature/provenance`)
  — Plan 9 Phase 5 lands the tests; Phase 6 cleans up wording.
- bd-129m3 (closes), bd-8pmq3 (closes).

## Test plan

(See Phase 5 above for Plan-7-deferred tests.) Additional unit /
integration tests by phase:

- **Phase 1**: `config_value_to_inlines_with_provenance` unit tests
  for scalar, bool, int, `PandocInlines`, `PandocBlocks` (rejection
  in inline context), missing key (None returned), nested via
  `get_nested`.

- **Phase 2**: meta-shortcode resolver produces two-anchor shape;
  `Invocation` source_info matches the token range; `ValueSource`
  source_info matches the metadata-key value range. `var` shortcode
  symmetrically. Test with both flat-string and PandocInlines
  metadata values.

- **Phase 3**: each of the three nav consumer sites produces
  `Generated` with `from: [ValueSource(profile.title_source_info)]`
  when title is present; produces `from: []` when title is None.
  Fixture extends Plan 6's multi-page audit-completion test.

- **Phase 4**: each per-section sub-Div carries its own ValueSource;
  missing-key cases gracefully degrade (no Div, no panic);
  outer-container Div carries `Generated { by:
  By::appendix_container(), from: [] }`. Audit-completion test
  (Plan 6) extended.

- **Phase 5**: see Phase 5 description above.

## Dependencies

### Hard dependencies

- **Plan 6** — establishes `Generated` stamping convention; Plan 9
  builds the consumer wiring on top. Plan 6 stamps with `from: []`;
  Plan 9 enriches to `from: [ValueSource]` (Phases 3 and 4) or
  `from: [Invocation, ValueSource]` (Phase 2).
- **Plan 4** — `AnchorRole::ValueSource` already exists; this plan
  consumes it.

### Soft dependencies

- **Plan 7** — Phase 5's appendix-license e2e round-trip test needs
  Plan 7's writer + soft-drop infrastructure. The unit-level
  asymmetry test (Phase 5 first bullet) doesn't.
- **bd-2mxo** — affects map/array container source_info; relevant
  only for nested metadata shapes (`license: {name: MIT, ...}`).
  Workaround in Phase 4 lets Plan 9 ship without bd-2mxo.

### Blocks

- Future hub-client hover-attribution UX work (separate plan, not
  yet scoped).

### Does not block

- **Plan 7 implementation** can start without Plan 9 — Plan 7 ships
  without ValueSource anywhere; its `Invocation` vs `ValueSource`
  asymmetry section is forward-looking. Plan 9 Phase 6 retroactively
  cleans up Plan 7's wording.

## Risk areas

- **API shape churn between Phases 1, 2, 4.** All three depend on
  the `config_value_to_inlines_with_provenance` decision. If the
  API shape changes mid-implementation, all three phases revisit.
  Mitigation: settle the API as part of this research plan (above);
  the development plan starts with the API frozen.

- **Map-shaped metadata interaction with bd-2mxo.** Phase 4's
  "first scalar leaf" workaround degrades gracefully but produces a
  less-precise ValueSource for nested licenses. Acceptable for v1;
  bd-2mxo's fix tightens later. Document as a known limitation in
  the `By::appendix` doc-comment.

- **Two-anchor cost in Phase 2.** Every meta-shortcode resolution
  gains a second anchor. `SmallVec<[Anchor; 2]>` keeps it on the
  stack. Add a perf-sensitivity check during implementation if a
  document heavy in meta-shortcodes regresses.

- **Forgetting `AnchorRole::Other` policy in extensions.** A future
  extension that adds `Other("attribution-source")` and expects
  `preimage_in` to walk it would silently be ignored. Mitigation:
  the policy is doc-commented at multiple sites; reviewers catch
  the case if it comes up.

## Estimated scope

| Phase | Lines (rough) |
|---|---|
| 1: Infrastructure (`config_value_to_inlines_with_provenance` + `DocumentProfile.title_source_info` + `AppendixSection` enum) | ~150 |
| 2: Meta/var shortcode (bd-129m3) | ~80 |
| 3: Nav-text ValueSource (bd-8pmq3) | ~60 |
| 4: Appendix sub-Div ValueSource | ~180 |
| 5: Plan-7 invariant tests | ~120 |
| 6: Plan 7 docs reword | ~20 |
| Tests across phases | ~250 |
| **Total** | **~860** |

One focused session, possibly two if Phase 4's per-section
discrimination surfaces unexpected interactions. Comparable scope to
Plan 6.

## Notes

This plan is the "consumer wiring" half of the provenance epic. Plan 6
stamped the *identity* (`by`) on synthesizers; Plan 9 stamps the
*origin* (`ValueSource` in `from`) on the metadata-derived subset.
Together they make every pipeline-produced metadata-derived node
fully attributable.

Future plans in the same family:
- **Plan 10** — Dispatch anchor for Lua filter / handler-shortcode
  (closes bd-36fr9). Requires Lua-file registration in SourceContext.
- **bd-12vrr** and **bd-1inj0** — standalone follow-ups for
  synthesizers whose source isn't metadata-shaped.

### File naming convention

This is the first plan to use the `provenance-plan-N-<slug>.md`
naming convention (dropping the `q2-preview-` prefix). The
provenance epic has outgrown the original q2-preview framing — it
serves attribution, round-trip writing, error reporting, and (via
the Dispatch role in Plan 10) Lua-source pointing. Plans 3–8 keep
their existing q2-preview filenames for git-history continuity;
plans 9+ adopt the new convention.
