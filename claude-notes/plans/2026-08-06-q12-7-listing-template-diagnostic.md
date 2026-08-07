# Diagnostics blame the wrong key: materialized map spans (Q-12-7 and siblings)

**Strand:** bd-9yh3pzfu (bug, p1) — child of bd-61cd (Listings epic)
**Folded in:** bd-2mxo (metadata materialization drops source_info provenance)
**Split out of this work:** bd-oywyaouf (EJS diagnostic), bd-lu16jgxq (Q-12-7 wording)
**Handoff memo (external, separate session):**
`claude-notes/scratch/2026-08-06-memo-quarto-source-map-default-sourceinfo.md`
**Status:** plan drafted, awaiting user review. **Do not implement yet.**

## Overview

Rendering a Q1 site ported to Q2 (Posit Connect docs at
`~/Desktop/daily-log/2026/08/05/q2-connect-docs/docs-quarto-2`) emits:

```
Warning: [Q-12-7] `template:` was set but `type:` is not `custom`; falling back to the built-in template for the declared type.
   ╭─[ …/cookbook/vanities/index.qmd:4:11 ]
   │
 4 │     sort: false
   │           ──┬──
   │             ╰──── `template:` was set but `type:` is not `custom`; …
```

The message talks about `template:` and `type:`; the caret points at
`sort: false`, an unrelated sibling key.

Investigating that render turned up three independent defects. **This
strand is now scoped to the first one only** — the source-mapping bug. The
other two are filed separately (see below); it is acceptable for the Connect
docs to render worse in the interim, since that site is several fixes away
from rendering well regardless.

| Defect | Where it lives now |
| --- | --- |
| **A. Diagnostics blame the wrong key** | **this strand (bd-9yh3pzfu)** |
| B. Q-12-7's text asserts a `type:` the user never declared | bd-lu16jgxq |
| C. Unsupported EJS templates render as raw `<% … %>`, silently | bd-oywyaouf |

The full investigation of B and C is preserved in the strand descriptions;
this document keeps only what bears on A. Two findings from that
investigation still matter here as *context*, because they explain why the
narrow fix below is the right one:

- Q1 makes `template:` **imply** `type: custom`
  (`external-sources/quarto-cli/src/project/types/website/listing/website-listing-read.ts:1316-1320`),
  so the YAML that triggered this is idiomatic Q1, not user error.
- Q2 drops EJS deliberately — doctemplate replaced it so untrusted contexts
  (hub-client) never execute arbitrary JS. The diagnostic gap around that is
  bd-oywyaouf.

## Diagnosis

**Confirmed by direct experiment.** With the fixture above the caret lands
on `false` (4:11, width 5). Reordering the keys so `template:` comes first
moves the caret to `../template.ejs` (4:15, width 15). The map's
`source_info` is *literally the first entry's value span*.

The Q-12-7 call site blames the whole listing map:

- `crates/quarto-core/src/project/listing/config.rs:534-541` — passes
  `value` (the `ConfigValueKind::Map` for `listing:`) to `push_diag`.
- `crates/quarto-core/src/project/listing/config.rs:928-940` — `push_diag`
  attaches `value.source_info` verbatim.

But the map's `source_info` is already wrong before listing code sees it:

- `crates/quarto-config/src/materialize.rs:142-158` — a materialized Map's
  `source_info` comes from `m.iter().next()`, the **first child's** value
  span. (If that child is itself a map, the span becomes the
  `programmatic_config` sentinel — no location at all.)
- `crates/quarto-config/src/materialize.rs:109-113` — a materialized
  Array's `source_info` comes from `.items.last()`, the **last** item.
- `crates/quarto-config/src/materialize.rs:130-137` — every
  `ConfigMapEntry.key_source` is replaced with
  `SourceInfo::generated(By::programmatic_config())`.
- The empty-map fallback is `SourceInfo::default()` —
  `Original { FileId(0), 0..0 }`, a bogus offset-0 span rather than a
  Generated sentinel, so it renders as a real-looking location in file 0.

Parsing is not at fault. pampa's YAML→ConfigValue bridge
(`crates/pampa/src/pandoc/meta.rs:175-193`) carries a real map span and a
real `key_source`, and `rawblock_to_config_value` (`meta.rs:384-389`) even
re-stamps the top-level frontmatter map to span the whole YAML body. All of
it is discarded when
`crates/quarto-core/src/stage/stages/metadata_merge.rs:266-288` replaces
`doc.ast.meta` wholesale with `merged.materialize()`. The project-config
path has its own parallel bridge (`crates/quarto-config/src/convert.rs:59-77`)
with the same fate. This is bd-2mxo.

### Blast radius

Any diagnostic that blames a Map- or Array-kind `ConfigValue`, anywhere in
the tree, points at an arbitrary sibling. Within listing config alone:
`config.rs:337` (Q-12-4, duplicate id — `item` is a map), `config.rs:357`
and `config.rs:388` (Q-12-1), `config.rs:711` (Q-12-3).

Separately, the `key_source` loss silently breaks `config.rs:516-521`, where
L5 captures `entry.key_source` into `l.categories_source` *specifically* to
anchor Q-12-12. That anchor is the programmatic-config sentinel today, so
the feature it exists to serve does not work. Worth confirming during the
audit; the fix belongs to bd-2mxo, since there is no per-key span to restore
from (below).

### The spans are not lost — they are discarded in flight

**Correction to an earlier reading of this code.** An initial pass
concluded bd-2mxo was blocked on a `MergedCursor` API change, on the
grounds that `MergedMap` carries only `keys: Vec<String>` and so has no
spans to offer. That is true of `MergedMap` itself and false of the system:

- `MergedConfig` holds `layers: Vec<&'a ConfigValue>`
  (`merged.rs:100-107`) — **borrowed originals**, spans and `key_source`
  intact. `MergedMap` is a *virtual* map computed over them, not a copy.
- `MergedCursor::keys()` (`merged.rs:216-222`) already iterates the real
  `ConfigMapEntry` structs from those layers and keeps only `entry.key`,
  dropping `entry.key_source` on the floor.
- `MergedCursor::as_value()` / `navigate_to()` (`merged.rs:238-245`)
  already return the winning layer's `&ConfigValue`, `source_info` and all.

So the fix is **additive accessors on `MergedMap`/`MergedCursor` plus
rewiring the two synthesis sites in `materialize.rs`** — no redesign, and
entirely inside `crates/quarto-config` (~1,200 lines across `merged.rs` and
`materialize.rs`).

**No external-crate work is required.** quarto-yaml already emits correct
key spans — that is how the in-tree bridges capture them before merge
discards them. quarto-source-map is a data type we are failing to
propagate. The single genuinely-external item is its `Default` impl
returning a plausible-looking `Original { FileId(0), 0..0 }` instead of a
sentinel; that is written up for a separate session in
`claude-notes/scratch/2026-08-06-memo-quarto-source-map-default-sourceinfo.md`
and is **not** a prerequisite here. In-tree we avoid it by not routing
through `Default`.

### Both fixes are in scope, and neither subsumes the other

Fixing materialization does *not* make the call-site fix unnecessary:
quarto-yaml computes a mapping's span as first-key-start → `MappingEnd`
(quarto-yaml 0.1.0 `src/parser.rs:513-525`), so even with perfect
materialization, blaming the `listing:` map would underline the whole block
starting at `sort:`. A diagnostic *about `template:`* must point at
`template:`. Phase A fixes the blame target; Phase B fixes the spans that
every other map-blaming diagnostic in the tree depends on.

**Expected cost:** correcting container spans moves diagnostic locations
tree-wide, so snapshot churn is expected and must be reported explicitly per
the CLAUDE.md snapshot policy. This is a reason for care in Phase D, not a
reason to defer.

## Work Items

Per CLAUDE.md, each phase writes its test first and confirms it fails.

Phases A and B are independent and each is separately shippable; A is
smaller and fixes the reported symptom, so it goes first. Phase 0's
assertion helper is a prerequisite for both — without it neither phase can
be tested for the thing that actually broke.

### Phase 0 — Make spans assertable

The reason this bug survived: **154 assertions in `crates/` check a
diagnostic's `code`; ~12 touch its location.** `config.rs:1233` is typical —
`assert_eq!(diags[0].code.as_deref(), Some("Q-12-7"))` passes cheerfully
with a garbage span. Nothing here is testable until that changes, and the
helper is worth having independently of both fixes (it is what would catch a
regression from a future quarto-yaml/quarto-source-map bump).

- [x] Add a test helper that resolves a `DiagnosticMessage`'s `SourceInfo`
      to a concrete `(file, line, column, underlined text)` and asserts on
      it. → `crates/quarto-config/src/span_assert.rs`, behind a
      `span-assert` cargo feature that `quarto-core` enables as a
      dev-dependency.
      **Not `quarto-test`** as the plan guessed: that crate is a
      document-level `_quarto.tests` smoke runner and *depends on*
      `quarto-core`, so it sits above both consumers. `quarto-config` is
      the lowest crate in the graph carrying both `quarto-source-map` and
      `quarto-error-reporting`.
- [x] Make the helper fail loudly on a defaulted/sentinel span rather than
      reporting it as `file 0, line 1`. → `SpanProblem` enumerates each
      distinct failure (`SuspiciousDefault`, `Generated`, `Concat`,
      `UnknownFile`, `NoContent`, `OutOfBounds`) so a failing assertion
      says *why*. `resolve_span` checks for `Original { FileId(0), 0..0 }`
      before resolving, and a unit test pins that behavior so the helper
      can't silently regress into leniency.
- [x] Add a minimal in-repo fixture. → Inline in the test rather than
      on-disk: `parse_from_yaml` in `config.rs`'s test module drives the
      **real** path (YAML → `yaml_to_config_value` → `MergedConfig` →
      `materialize` → `parse_listings`), matching what
      `transforms/listing_generate.rs:72` reads at render time. Going
      through `materialize` is the point — skipping it skips the defect.
- [x] Failing test confirmed: `q_12_7_underlines_the_template_key_not_a_sibling`

      ```
      diagnostic [Q-12-7] underlines the wrong text
        expected: "../template.ejs"
        actual:   "false"
        at:       index.qmd:3:11
      ```

      This is the user-reported bug reproduced in-process, down to the
      underlined text. Note the pre-existing `template_with_non_custom_type_emits_q_12_7`
      test passes against the same broken behavior — the code-only
      assertion cannot see it.

### Phase A — Blame the right key (listing call sites)

- [x] In `parse_one_listing`, capture the `template:` entry's `ConfigValue`
      while walking the map (`template_source`), and blame it in the Q-12-7
      `push_diag`.
- [x] Phase 0's test passes.
- [x] Audit the other map-blaming `push_diag` sites in the same file.

**Audit result.** Of the sites that blame a container, only one besides
Q-12-7 had a better span available at the call site:

| Site | Code | Verdict |
| --- | --- | --- |
| `parse_listings` array loop | `Q-12-4` | **Fixed.** Blamed the whole listing map; now blames the offending `id:` value via a new `map_entry_value` helper. Confirmed failing first — it underlined `"contents: ./b.qmd\n      id: dupe\n"`. |
| `parse_contents` inline record | `Q-12-2` | **Already correct** — see correction below. Regression test added. |
| `parse_listings` fallthrough | `Q-12-1` | **No change.** Blames the offending value, which is a scalar-ish kind in this branch (the code notes these never occur in practice). |
| `parse_one_listing` non-map | `Q-12-1` | **No change.** Blames the right value; array items keep real spans (see correction). |
| `parse_sort` fallthrough | `Q-12-3` | **Fixed by Phase B.** Confirmed failing pre-fix — underlined `"title"`, the first entry's value. |

The pattern: call-site fixes handle "blaming the wrong *node*"; Phase B
handles "blaming the right node, which carries a wrong *span*".

**Correction to this audit (found during Phase B verification).** The audit
assumed every container carried a synthesized span. It does not:
`materialize_cursor`'s Array arm **clones array items verbatim**
(`item.value.value.clone()`, `item.value.source_info.clone()`) rather than
recursing through the cursor, because array items have no path to navigate
to. So **any container nested inside an array kept its original, correct
span all along.** Only map-valued *keys* — the paths a cursor can navigate —
went through the synthesizing arm.

That reclassifies two rows: `Q-12-2`'s inline record lives inside the
`contents:` array and was never broken, and `Q-12-4`'s pre-fix span was the
*genuine* mapping span rather than a synthesized one. Phase A's `Q-12-4`
change is still right — it blames the `id:` the message names instead of the
whole record — but it was a wrong-node fix, not a wrong-span fix. Verified
empirically by running each test against a stashed working tree rather than
by re-reading the code.

- [x] Span assertions added for both changed sites
      (`q_12_7_underlines_the_template_key_not_a_sibling`,
      `q_12_4_underlines_the_duplicate_id_not_a_sibling`), each confirmed to
      fail before its fix.

### Phase B — Preserve real spans through materialization (bd-2mxo)

- [x] Failing tests first, in `materialize.rs`'s new `tests::spans` module,
      parsing real YAML so the spans mean something. All four failed with
      the predicted diagnosis — map container = `"false"` (first entry's
      value), array container = `"./b.qmd"` (last item), nested-map
      container and every `key_source` = `Generated` sentinel. A fifth test
      (winning layer supplies the span) **passed from the start**, which
      usefully bounds the defect: scalar spans always survived
      materialization.
- [x] Add accessors on `MergedCursor`: `container_source()` and
      `key_source(key)`, both walking `config.layers` in reverse so the span
      follows the same highest-priority layer that `as_value`/`as_scalar`
      already pick for the winning value. They return the layer's span
      verbatim — a programmatically-built layer keeps saying it was
      generated instead of borrowing a neighbour's location.
- [x] Rewire `materialize_cursor`'s Map and Array arms to use them.
- [x] Replace the `unwrap_or_default()` fallbacks with
      `SourceInfo::generated(By::unknown())`, via a `container_source`
      helper documenting why. `By::unknown()` is the contract's sanctioned
      "we don't know" marker; `Default` would fabricate
      `Original { FileId(0), 0..0 }` — the plausible-looking lie that hid
      this bug in the first place.
- [x] The nested-map sentinel case is gone: with a real container span
      available there is nothing left to special-case, so that branch was
      deleted rather than re-decided.
- [x] L5's `categories_source` **was inert**, as suspected. Confirmed
      empirically: against a stashed tree the new test fails with
      "categories_source should be a real key span, not a sentinel:
      Generated". It now resolves to the `categories` key itself. This is a
      latent feature bug (Q-12-12 could never anchor anywhere) fixed as a
      side effect.
- [ ] Consider an xtask lint for `unwrap_or_default()` on `SourceInfo`-typed
      expressions — the crate's own doc comment promises "separate grep
      tooling" that does not exist (`crates/xtask/src/lint/` has only
      `external_sources.rs` and `metadata_as_str.rs`). **Deferred**: doing
      this accurately needs type information a grep-level rule lacks, and
      the in-tree `SourceInfo` `unwrap_or_default` sites are now gone.
      Filing separately rather than half-doing it here.

### Phase C — Verification

- [x] `cargo nextest run --workspace` — **10877 passed, 197 skipped**, zero
      failures.
- [x] `cargo xtask verify --skip-hub-build` clean (full verify pending, see
      below).
- [x] End-to-end through the real binary. Invocation:

      ```
      cargo run --bin q2 -- render .tmp-e2e/index.qmd
      ```

      on a fixture reproducing the report (`sort:` before `template:`).
      Output inspected in the terminal; the caret moved from `sort: false`
      to the template path:

      ```
      Warning: [Q-12-7] `template:` was set but `type:` is not `custom`; …
         ╭─[ …/.tmp-e2e/index.qmd:5:15 ]
       5 │     template: ../template.ejs
         │               ───────┬───────
      ```

- [x] Connect docs re-rendered
      (`cargo run --bin q2 -- render .` in `docs-quarto-2`). **All 15
      Q-12-7 instances** now point at `template: ../template.ejs`,
      including the originally-reported `cookbook/vanities/index.qmd`,
      which moved from `4:11` (`sort: false`) to `5:15`. Full run: 186 of
      186 files rendered, 9 errors / 310 warnings — still poor, as expected;
      bd-oywyaouf and bd-lu16jgxq address the rest. Spot-checked Q-16-3 and
      Q-5-3 spans in the same run: both point at real shortcode
      invocations, so nothing was collaterally moved.
- [x] **Snapshot report: zero `.snap` files changed.** This contradicts the
      plan's prediction of tree-wide churn, and the reason is worth
      recording: no existing snapshot ever captured a materialized
      *container* span. The 154-vs-12 assertion imbalance that hid the bug
      is the same thing that made the fix invisible to the suite. Scalar
      spans — the ones snapshots do capture — were never affected, which
      the Phase B "winning layer" test independently confirms.

## Key references

| What | Where |
| --- | --- |
| Q-12-7 emission (blames the map) | `crates/quarto-core/src/project/listing/config.rs:534-541` |
| `push_diag` (attaches span verbatim) | `crates/quarto-core/src/project/listing/config.rs:928-940` |
| Other map-blaming sites to audit | `config.rs:337`, `:357`, `:388`, `:711` |
| L5 `categories_source` capture (likely broken) | `crates/quarto-core/src/project/listing/config.rs:516-521` |
| Map span borrowed from first child | `crates/quarto-config/src/materialize.rs:142-158` |
| Array span borrowed from last item | `crates/quarto-config/src/materialize.rs:109-113` |
| `key_source` discarded | `crates/quarto-config/src/materialize.rs:130-137` |
| Correct spans upstream (pampa) | `crates/pampa/src/pandoc/meta.rs:175-193`, `:384-389` |
| Project-config bridge (same fate) | `crates/quarto-config/src/convert.rs:59-77` |
| Where the good spans are thrown away | `crates/quarto-core/src/stage/stages/metadata_merge.rs:266-288` |
| `MergedConfig` borrows the original layers (spans intact) | `crates/quarto-config/src/merged.rs:100-107` |
| `keys()` sees real `ConfigMapEntry`s, drops `key_source` | `crates/quarto-config/src/merged.rs:216-222` |
| `as_value()` returns the winning layer's `&ConfigValue` | `crates/quarto-config/src/merged.rs:238-245` |
| `SourceInfo::default()` = `Original{FileId(0),0..0}` | quarto-source-map 0.1.1 `src/source_info.rs:139-147` |
| Provenance contract (§10: `default()` is a bug) | `claude-notes/designs/provenance-contract.md` |
| YAML mapping span = first-key-start → MappingEnd | quarto-yaml 0.1.0 `src/parser.rs:513-525` |
| Catalog entry | `crates/quarto-error-catalog/error_catalog.json:926-932` |
