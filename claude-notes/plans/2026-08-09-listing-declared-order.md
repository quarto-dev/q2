# Listings lose declared order of explicit `contents:` entries (bd-listing-declared-order-3ixcvc4o)

**Date:** 2026-08-09
**Braid:** bd-listing-declared-order-3ixcvc4o (origin: `br-listing-declared-order-qodof0f6` in the connect-docs porting skein)
**Checkout:** main worktree (`/Users/cscheid/rooms/room-1/q2`, branch `main`)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The mechanism is fully understood, Q1's target semantics are pinned from source, the fix site is localized to `ListingGenerateTransform`'s item-collection loop, and a minimal repro exists (copied into `listing-declared-order-investigation/repro/`). A handful of scope questions below need answers before implementation.

## Issue context

P1 bug, filed 2026-08-09 (today) by Carlos, label `listings`. A listing whose
`contents:` is a list of explicit paths renders items in project-index order
(effectively path-alphabetical) regardless of declaration order, even with
`sort: false`. Q1 preserves declared order; all 15 Posit Connect cookbook
listing pages rely on it ("Getting Started" renders 4th instead of 1st).
Second-order effect: post-render description previews extracted from
listing-only section pages pick up the wrong first item.

Related quirk to fix in the same pass: sorting by a `listing-item.extra`
custom field *works* (`sort.rs::field_value` falls through to the extra map)
but `is_known_sort_field` doesn't know extra fields, so a working
custom-field sort emits a misleading Q-12-3 "values will compare as equal"
warning.

## Dependency graph

**Empty** — no edges in the q2 skein. The strand was filed today from external
repro work (connect-docs porting), so the "why" lives in its description and in
the origin repro, not in graph context. There is an open **Listings feature
epic (bd-61cd)** this strand is *not* linked to; linking it (`parent-child` or
`related`) is cheap incidental hygiene — see incidental work below.

## What the code looks like today

All paths in the description exist at HEAD and the mechanism is confirmed by
reading source:

1. **Resolution** (`crates/quarto-core/src/project/listing/glob_resolve.rs`):
   `contents:` entries become `GlobPattern`s via the shared
   `crate::glob::resolve_patterns`. Order of the *patterns* is preserved in
   `GlobResolution.globs`.
2. **Matching** (`crates/quarto-core/src/transforms/listing_generate.rs`
   ~lines 162–173): items are collected by iterating
   `ctx.project_index.profiles()` (Pass-1 insertion order — project input
   enumeration order) and testing each candidate against the compiled
   `PatternSet::matches`. **This is where declaration order is lost**: the
   loop is candidate-major, so item order = index order, never pattern order.
3. **Sort** (`crates/quarto-core/src/project/listing/config.rs::parse_sort` +
   `sort.rs::apply_sort`): `sort: false` → `Some(vec![])` → `apply_sort`
   returns immediately (correct no-op), so the index order leaks through.

Precedent already in-tree: `PatternSet::excluded` (`glob/matcher.rs` ~line
171) exists precisely so `project.render` can walk its positive patterns in
the author's listed order while keeping exclusions global. The listing fix
can follow the same shape.

Bonus per-pattern machinery already present: the Q-12-19 "matched nothing"
diagnostic loop (`listing_generate.rs` ~lines 180–200) already compiles each
positive pattern individually — first-matching-pattern-index computation can
reuse (or share hoisted compiles with) that loop.

### Q1 target semantics (pinned from `external-sources/quarto-cli`)

- `src/core/path.ts::filterPaths`/`resolvePathGlobs`: **glob-major
  iteration** — for each glob in declared order, append its matching files;
  `ld.uniq` dedups keeping *first* occurrence. Net effect: items ordered by
  first-matching-pattern index; within one pattern, candidate-set order.
- `website-listing-read.ts::computeListingSort`: `sort: false` → `[]` (no
  sorting, contents order preserved); `sort: true`/absent → `undefined` →
  default sort.
- **Q1's default sort is NOT date-desc**: when `sort:` is absent, title is a
  hydrated field, and sources include document items, Q1 applies
  `[{field: "order", asc}, {field: "title", asc}]` (`website-listing-read.ts`
  ~line 637). `order` is a front-matter field (`kFieldOrder`) authors use for
  curated ordering. q2's `listing_generate.rs` ~line 207 applies **date
  desc** with a comment claiming it "Matches Q1 default" — that claim looks
  wrong against current Q1 source. Scope question below.
- q2's `is_known_sort_field` also doesn't include `order` (nor does
  `hydrate_item` obviously surface it other than via `extra`).

### Repro at HEAD (end-to-end, confirmed)

Minimal repro copied to
`claude-notes/plans/listing-declared-order-investigation/repro/` (declares
`contents: [./bravo/index.md, ./alpha/index.md]` with `sort: false`).
Rendered with the HEAD binary (2026-08-09, `main` @ 2f2f4be3, v0.14.0):

```
$ cd claude-notes/plans/listing-declared-order-investigation/repro
$ cargo run --quiet --bin q2 -- render .
Rendering project: .../repro (type: website)
Rendered 3 of 3 files to .../repro/_site
$ grep -n -o 'alpha/\|bravo/' _site/index.html
36:alpha/
39:alpha/
45:bravo/
48:bravo/
```

Output inspected: Alpha renders before Bravo despite the declared
bravo-first order — **bug confirmed at HEAD**. (`_site/` is gitignored in
the repro dir; regenerate with the command above.)

### Pre-flight verify note

`cargo xtask verify --skip-hub-build` at HEAD failed on one test:
`quarto-hub::integration admin_collect_lifecycle::collect_reverification_skips_rereferenced_candidate`
(assertion diff `2xPD…` vs `2XPD…` — a single case-flip in a doc id). This
is the known macOS case-insensitive-filesystem flake tracked as
**bd-eb2wnxkp** (this worktree's other in-flight strand; plan
`claude-notes/plans/2026-07-28-doc-id-identity-from-paths.md` awaiting
review). All 9,697 other tests that ran passed; the failure is unrelated to
listings.

## Implementation notes (pre-work investigation, 2026-08-09)

- `DocumentProfile` **already extracts top-level `order:`** front matter
  (`order: Option<i32>`, `document_profile.rs:699`) — no profile_version
  bump needed; only `hydrate_item`/`ListingItem`/`field_value` must surface
  it.
- Q1's default-sort condition (`hydratedFields.includes(title)` + document
  sources) is satisfied by **all** built-in types — `kDefaultTableFields`
  includes title — so the Q1-parity default `order asc, title asc` applies
  uniformly, replacing both q2's `date desc` default *and* the table-type
  "no default sort" special case.
- Q1 `computeListingSort`: `sort: true` → default sort (same as absent).
  q2's `parse_sort` currently sends `Boolean(true)` through
  `as_plain_text`, which would produce a bogus `true` sort field —
  fold the `sort: true` fix into this pass (parse_sort → `Option<Vec<_>>`,
  `None` = use default).
- Q-12-3 stays scoped to **unknown** fields only (per-user rule "warn only
  when no information can determine a sort"): warn iff the field is not
  built-in AND no item's `field_value` yields a value. Not extended to
  built-in fields with all-absent values, because the internally-generated
  default sort (`order asc`) would then warn on every listing whose items
  lack `order:`.
- Ordering fix lives in the **consumer** (`ListingGenerateTransform`), not
  in shared glob resolution: track, per candidate, the index of the first
  matching positive pattern (candidates iterate positive `PatternSet`s
  individually, exclusions stay global), then stable-sort collected items
  by that index. The per-positive compiles replace the Q-12-19 loop's
  existing per-pattern recompiles (track `matched_any` per pattern —
  every matching pattern gets credit, not just the first, preserving
  current Q-12-19 semantics).

## Work items

### Phase 0 — Tests (TDD: write first, verify failures)

- [ ] `sort: false` + explicit two-entry `contents:` preserves declared
  order (unit, `listing_generate.rs` harness)
- [ ] Mixed literal + glob `contents:` orders by first-matching-pattern
  index; within a glob, index order (unit)
- [ ] Item matching multiple patterns counts for its **first** pattern only
  (dedup / no duplicate items) (unit)
- [ ] Q-12-19 still fires for a pattern whose only matches were claimed by
  an earlier pattern? — NO: it must NOT fire (every matching pattern gets
  credit); regression test (unit)
- [ ] `sort: true` behaves like absent `sort:` (default sort) (unit)
- [ ] Default sort (absent `sort:`) is `order asc, title asc` — items with
  `order:` first (numeric asc), missing-order items after, title asc
  tie-break (unit)
- [ ] Table listings get the same default sort (unit)
- [ ] Explicit `sort: date desc` still works (existing test keeps passing)
- [ ] `sort: order` works as an explicit known field (unit, sort.rs)
- [ ] Working `extra`-field sort emits **no** Q-12-3 (unit)
- [ ] Typo'd sort field (no item has it) still emits Q-12-3 (unit)
- [ ] End-to-end: repro fixture renders bravo-then-alpha
  (smoke-all fixture or integration test + manual `q2 render` verification)

### Phase 1 — Order-preserving item collection

- [ ] `listing_generate.rs`: per-positive-pattern `PatternSet`s hoisted;
  candidate loop records first-match index + per-pattern `matched_any`
- [ ] Stable-sort collected items by first-match pattern index
- [ ] Q-12-19 loop consumes `matched_any` (drop per-item recompiles)
- [ ] Phase-0 ordering tests pass; full workspace suite green

### Phase 2 — Sort semantics

- [ ] `parse_sort` → `Option<Vec<ListingSort>>` (`true`/absent → `None`,
  `false` → `Some([])`); config plumbing updated
- [ ] `ListingItem.order: Option<i32>` hydrated from `profile.order`;
  exposed in template binding (`order` key) for Q1-compatible templates
- [ ] `field_value("order")` → item.order (extra fallback preserved);
  `order` added to `is_known_sort_field`
- [ ] Default sort → `[order asc, title asc]`, all listing types (replaces
  date-desc + table special case); update `default_sort_is_date_desc_*`
  test per decision Q3
- [ ] Q-12-3: warn only when field is unknown AND no item has a value
  (`apply_sort` gains access to items — it already has them)
- [ ] Full workspace suite green; snapshot changes reported per policy

### Phase 3 — End-to-end verification + docs

- [ ] `q2 render` on the committed repro fixture: bravo before alpha in
  `_site/index.html`; output inspected and recorded in this plan
- [ ] Sweep existing listing snapshots/fixtures for ordering churn; report
- [ ] docs/ listings page: document declared-order semantics, `order:`
  front matter, default sort, `sort: false`/`true` (coordinate with
  bd-2nb6i1qv scope — keep this entry minimal)
- [ ] `braid` bookkeeping: comment + close after user sign-off

## Design decisions (user, 2026-08-09)

1. **Ordering rule scope:** first-matching-pattern index for **all** patterns
   (literal and wildcard alike), Q1-style. ✅ decided
2. **Tie-break churn under sorts:** accepted — stable sorts over
   first-pattern order; snapshot churn from ties only. ✅ decided
3. **Default-sort parity:** default **can change** to Q1's
   (`order asc, title asc`), provided the old behavior stays configurable.
   **Working assumption** (flagged to user, overridable): per-listing
   `sort: date desc` — which already works — satisfies "configurable"; no
   new project-level default-sort knob in this pass. Consequence: `order`
   joins `is_known_sort_field` and sorts via the front-matter `order` field
   (through `extra` or hydrated explicitly).
4. **Q-12-3 shape:** any-item suppression — "we only want the warning if
   there's no information that can be used to determine an appropriate
   sort." I.e. skip the warning when at least one item has a value for the
   field (built-in or `extra`); keep it when no item does. ✅ decided
5. **Description-preview regression fixture:** minimal repro is enough; no
   connect-docs fixture. ✅ decided

## Open design questions for the user (original)

1. **Ordering rule scope.** Order by first-matching-pattern index for *all*
   patterns (literal and wildcard alike — this is exactly Q1's rule), or only
   when every entry is a literal path? Recommendation: all patterns,
   Q1-style; it's simpler and strictly more compatible.
2. **Does the fix change ordering under an explicit sort or the default
   sort?** With a stable sort applied afterwards, first-pattern order becomes
   the tie-break order (Q1 gets the same effect for free). Any snapshot
   churn would come from ties only. OK to accept that?
3. **Default-sort parity.** q2's absent-`sort:` default is `date desc`,
   commented as "Matches Q1 default", but current Q1 source applies
   `order asc, title asc`. Fix here, file separately, or leave (perhaps q2
   deliberately chose date-desc as the saner blog default)? This also decides
   whether `order` should join `is_known_sort_field` / hydrated fields.
4. **Q-12-3 fix shape.** Suppress the warning when *any* item's `extra` has
   the field? When *all* items have it? Or downgrade the message to mention
   the extra-field fallback? (Any-item suppression is the least chatty and
   matches "the sort visibly works".)
5. **Second-order description-preview effect.** The strand notes listing-only
   section pages get wrong first-item previews. That should fall out of the
   ordering fix automatically — is there a connect-docs page worth adding as
   a regression fixture, or is the minimal repro enough?

## Risks / tradeoffs (draft)

- **Snapshot churn**: any existing listing snapshot whose item order depended
  on index order under `sort: false` (or on tie order under sorts) may
  change. Expected small; must be called out per snapshot policy.
- **Per-pattern matching cost**: first-match-index needs per-pattern
  `PatternSet`s; the Q-12-19 loop already pays this per listing, so hoisting
  the compiles is likely a net wash or a win.
- **Default-sort parity (Q3)** is behavior-visible for every listing without
  an explicit `sort:` — riskier than the core fix; that's why it's split
  into its own phase / possibly its own strand.
- `ProjectDependencyGraph::build` also consumes `resolve_content_globs` —
  the ordering fix must live in the *consumer* (listing generate), not in
  shared glob resolution, to avoid perturbing the dependency-graph edge set
  or other glob consumers (`project.render`, `resources:`, `sidebar.auto:`).
