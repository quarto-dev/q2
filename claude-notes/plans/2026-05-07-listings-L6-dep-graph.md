# L6 — Dependency-graph integration (sub-plan)

**Date:** 2026-05-07
**Beads:** `bd-xbnf` (this phase). Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessors:**
- L0 (`bd-n8a4`, closed) — `DocumentProfile` substrate; bumped
  to v4 with `listing_item` and `categories_raw`.
- L2 (`bd-j60g`, closed) — listing data model; reference doc.
- L3 (`bd-ml8z`, closed) — generate/render transforms;
  `parse_listings`, `glob_match_path` (in `project/discovery.rs`),
  and the host-relative-first / project-relative-fallback glob-match
  rule (`crates/quarto-core/src/transforms/listing_generate.rs`
  `matches_any_glob`).
- Phase-8 dep-graph substrate
  (`claude-notes/plans/2026-04-27-websites-phase-8.md`):
  `ProjectDependencyGraph` + `force_render` + the
  `augment_targets_with_always_render` Mode-B augmentation.

**Status:** Draft. Awaiting user approval before hand-off.

## Goal of this phase

Make Mode B (`quarto render posts/foo.qmd`) automatically
re-render listing hosts when any of their content files are
named as targets. Today, Mode B picks up listing hosts only if
the user manually adds them to the targets, leaving stale
listing pages on every partial render — a real-world regression
relative to Q1's incremental behavior.

L6 ships:

1. **`listing_content_globs: Vec<String>` on `DocumentProfile`.**
   Extracted from `meta.listing.*.contents` at profile-extract
   time. Lists the *unresolved glob patterns* the host declared,
   not resolved paths (resolution requires the full project
   source set, which a per-document profile cannot safely cache).
2. **`DOCUMENT_PROFILE_VERSION` bump 4 → 5.** Matches the L0
   precedent: adding a default-empty field still bumps the
   version so stale cached profiles are invalidated rather than
   silently re-read with the field missing.
3. **New edge source in `ProjectDependencyGraph::build`.** For
   each profile with non-empty `listing_content_globs`, expand
   each glob against `ProjectIndex.profiles()` (host-relative
   first, project-relative fallback — same rule L3 uses at
   render time). Each match becomes a forward edge `host →
   content`. The host is added to `force_render`.
4. **Mode B integration via the existing `force_render`
   primitive.** No new augmentation method; the existing
   `augment_targets_with_always_render` already requires a
   `force_render` page to reach a target via forward edges
   before pulling it in, which matches the listing-host
   semantic exactly. The orchestrator's `compute_augmented_render_set`
   needs no changes.
5. **Pass `cargo xtask verify`** (full, including hub-client
   build — the profile-field addition affects WASM through
   `quarto-core` types).

**Out of scope for L6 (deferred):**

- **Resolved-targets caching on the profile.** Out of scope per
  the user's 2026-05-07 decision — globs are stored, resolution
  is recomputed at every graph build. If profiling shows the
  recompute is hot, a separate side-table cache keyed on the
  graph build's input set is a clean follow-up.
- **Diagnostics for empty-match globs.** Q-12 already gets
  `Q-12-12` (categories enabled but empty) from L5; matching
  for "listing has no items" is L3's render-time concern. Adding
  a graph-build-time diagnostic is redundant and noisy when the
  user is mid-edit.
- **`listing_content_globs` in the JSON output of `quarto
  inspect`.** Not asked for; will land if anyone surfaces a need.
- **Listing-host re-rendering when a sibling is *deleted*.**
  Phase-8 incremental builds already handle deletions through
  the cache key; L6 inherits that behavior. No new code.

## Reference material

Read first:

- Parent epic: `claude-notes/plans/2026-05-05-listings-epic.md`
  §"L6" + §"Architecture summary".
- Phase-8 dependency-graph plan:
  `claude-notes/plans/2026-04-27-websites-phase-8.md`
  §"Decision 5 — Dependency graph: shape and inputs" and
  §"Mode B with `always-render` siblings". The L6 work
  *extends* this design with one new edge source.
- L3 sub-plan:
  `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
  §"Generate transform: item discovery (step 3a)" — describes
  the host-relative / project-relative glob-match rule that L6
  reuses at graph-build time.
- L5 sub-plan:
  `claude-notes/plans/2026-05-06-listings-L5-categories-sidebar.md`
  §"Hand-off summary" — the source-of-truth state of
  `feature/listings` after L5's merge (commit `9e8afa0d`).
- Existing Q2 surface L6 builds on:
  - `crates/quarto-core/src/document_profile.rs` line 46 —
    `DOCUMENT_PROFILE_VERSION` (currently `4`, bumps to `5`).
  - `crates/quarto-core/src/document_profile.rs` lines 360–373 —
    `body_link_targets` is the closest precedent: a profile
    field populated by a Pass-1 stage and consumed by the
    graph builder. L6 follows the *layout* but **inverts the
    timing**: globs are resolved at graph-build, not at
    Pass-1. Reason: glob resolution needs the full source set.
  - `crates/quarto-core/src/document_profile.rs` lines 494–532
    — `DocumentProfile::extract`. Adding `listing_content_globs`
    here mirrors how `nav_dependencies` and `always_render` are
    pulled from `meta.project.*` (lines 522–523).
  - `crates/quarto-core/src/project/dependency_graph.rs`
    lines 91–174 — `ProjectDependencyGraph::build`. L6 adds one
    block to the per-profile pass at line 137.
  - `crates/quarto-core/src/project/dependency_graph.rs` lines
    270–289 — `augment_targets_with_always_render`. **No
    changes required**; the existing implementation handles
    listing hosts the moment they appear in `force_render`.
  - `crates/quarto-core/src/project/discovery.rs` lines 201–202
    — `glob_match_path`. The L6 graph-build glob-expander
    invocation.
  - `crates/quarto-core/src/transforms/listing_generate.rs`
    lines 153–170 — `matches_any_glob`. The host-relative-first
    / project-relative-fallback rule. L6 ports this *logic*,
    not the function (the inputs differ — L3 walks per-profile,
    L6 walks per-host).
  - `crates/quarto-core/src/project/orchestrator.rs` lines
    961–1015 — `compute_augmented_render_set`. **No changes
    required** for the listing-host pull-in; the call to
    `augment_targets_with_always_render` already does the
    right thing once L6 marks listing hosts in `force_render`.
- L3-merged listing parser:
  `crates/quarto-core/src/project/listing/config.rs`
  `parse_listings`. L6 needs a **smaller, narrower** extractor
  that pulls out just the glob strings, not the full `Listing`.
  See §"Where the globs come from" below.

## Settled inputs

These are decisions, not open questions:

- **`listing_content_globs: Vec<String>` lives on
  `DocumentProfile`; the graph builder expands them at build
  time.** User-confirmed 2026-05-07. Glob expansion needs the
  full source set, so resolved paths can't safely cache on
  the per-doc profile.
- **`DOCUMENT_PROFILE_VERSION` bumps 4 → 5.** User-confirmed
  2026-05-07. Stale Phase-8 caches invalidate cleanly.
- **Listing hosts fold into `force_render`.** User-confirmed
  2026-05-07. Reuses the existing
  `augment_targets_with_always_render` primitive without
  modification. The semantic match: a `force_render` page is
  pulled in when reachable from a target via forward edges,
  which is exactly "this listing host has an edge to a content
  file the user named."
- **Glob-match rule mirrors L3's render-time rule.** Try the
  candidate's host-relative form first (default `*.qmd` is
  host-dir-relative), fall back to project-relative for explicit
  patterns like `posts/**/*.qmd`. Reuses
  `crate::project::discovery::glob_match_path`. No new walker.
- **Multi-listing host: globs are flattened.** A page with
  `listing: [{contents: a/*.qmd}, {contents: b/*.qmd}]`
  produces `listing_content_globs == ["a/*.qmd", "b/*.qmd"]`.
  The graph cares about edges, not which listing produced them;
  flattening simplifies the field shape.
- **Inline-record `contents:` entries are dropped.** Same
  treatment as L3 (`Q-12-2` already emitted at render time);
  no glob string available to add to the field, no edge added.
- **Self-edges are dropped.** Already handled by
  `ProjectDependencyGraph::build`'s `add_edge` closure.
- **No new diagnostic codes in v1.** Empty-match globs produce
  no edges and no warning. L3 already handles "listing renders
  empty" at render time. Adding a graph-build-time diagnostic
  for the same condition is redundant and noisy mid-edit.

## Architecture

### Where the globs come from

`DocumentProfile::extract` runs in Pass-1 and reads from `ast.meta`.
The host's `listing:` config is at `meta.listing` (or `meta.listings`
plural; L3's parser handles both). L3's `parse_listings` already
parses the full `Listing` struct from a `ConfigValue`; that's
heavyweight for L6's purposes (we only need the glob strings).

L6 adds a **narrower extractor** — `extract_listing_content_globs(meta)`
— that walks `meta.listing` and pulls out just the `contents:`
glob strings from each listing entry. Pseudocode:

```rust
fn extract_listing_content_globs(meta: &ConfigValue) -> Vec<String> {
    let Some(listing) = meta.get("listing") else { return Vec::new(); };
    let mut out = Vec::new();
    for listing_entry in iterate_listing_entries(listing) {
        // listing_entry is a ConfigValue::Mapping for one listing,
        // or a ConfigValue::Bool/String for shorthand.
        let Some(contents) = listing_entry.get("contents") else {
            // Shorthand `listing: default` ⇒ default contents `*.qmd`
            // applies. We capture the default here so the dep-graph
            // edge source matches L3's `apply_type_defaults` behavior.
            out.push("*.qmd".to_string());
            continue;
        };
        for glob in iterate_contents_globs(contents) {
            out.push(glob);
        }
    }
    out
}
```

Defaults handling: when the host has a listing but no explicit
`contents:`, L3's `apply_type_defaults` substitutes `*.qmd` (the
host-dir-relative default). The L6 extractor inserts that same
default so the dep-graph edges reflect what L3 will resolve at
render time.

**Where this code lives.** Two reasonable homes:

- **Inside `crate::document_profile`** as a private helper next
  to `extract_listing_item`. Pros: keeps profile-extract
  self-contained. Cons: duplicates logic from
  `crate::project::listing::config`.
- **Inside `crate::project::listing::config`** as a new
  `pub fn extract_content_globs(meta) -> Vec<String>`, called
  from `document_profile::extract`. Pros: single source of
  truth for "what does `listing.contents` look like in YAML?"
  Cons: small cross-module dependency (`document_profile`
  imports `project::listing`).

Recommend the second placement — listings is the only feature
that reads `listing.contents`, and `project::listing::config`
already parses the full structure. The L6 session author
verifies during impl that this doesn't introduce a circular
module dependency (it shouldn't:
`document_profile` is a leaf with no current dependency on
`project::listing`, but the dependency direction —
`document_profile` → `project::listing` — only adds a use, no
cycle).

### The graph-build expansion

`ProjectDependencyGraph::build` already loops over
`index.profiles()` to read each profile's
`body_link_targets` / `nav_dependencies` / `always_render`
(lines 137–167). L6 adds one more block to that loop:

```rust
for profile in index.profiles() {
    let from = &profile.source_path;

    // … existing body_link_targets / nav_dependencies / always_render …

    // === Listing-content edges (L6) ===
    if !profile.listing_content_globs.is_empty() {
        // Mark the host as force_render so Mode B's existing
        // augmentation pulls it in when any of its content
        // files lands in the target set.
        force_render.insert(from.clone());

        // Expand each glob against the index's source set.
        // Host-relative first (default `*.qmd` semantics),
        // project-relative fallback for explicit `posts/**/*.qmd`
        // patterns. Mirrors L3's matches_any_glob rule.
        let host_dir_str = host_dir_forward_slash(from);
        for glob in &profile.listing_content_globs {
            for candidate in index.profiles() {
                if candidate.source_path == *from { continue; }
                let cand_str = path_to_forward_slashes(&candidate.source_path);
                let cand_host_relative = relative_to(&cand_str, &host_dir_str);
                let host_match = cand_host_relative
                    .as_deref()
                    .map(|hr| glob_match_path(glob, hr))
                    .unwrap_or(false);
                let project_match = glob_match_path(glob, &cand_str);
                if host_match || project_match {
                    add_edge(from, &candidate.source_path);
                }
            }
        }
    }
}
```

Performance: `O(H × G × N)` where `H = listing hosts`,
`G = avg globs per host`, `N = project profile count`. For a
1k-page project with 5 listing hosts and 1 glob each,
`5 × 1 × 1000 = 5000` glob-match calls — sub-millisecond. Fine.

The `path_to_forward_slashes`, `glob_match_path`, and
`relative_to` helpers all already live in
`crate::project::discovery` (the first two `pub`) or
`crate::transforms::listing_generate` (the last one,
currently private). The L6 session promotes `relative_to`
to `pub(crate)` in `crate::project::discovery` (or wherever
the public glob helpers cluster) so both call sites share it
— **no copy-paste duplication.**

### Why no changes to `compute_augmented_render_set`

The orchestrator's `compute_augmented_render_set`
(`crates/quarto-core/src/project/orchestrator.rs:961`) already
calls `graph.augment_targets_with_always_render` and trusts its
output. The augmentation is:

```text
reachable = reverse_closure(targets)            # pages that reach a target via forward edges
implicit  = { q ∈ force_render | q ∈ reachable }
effective = targets ∪ implicit
```

A listing host `H` with edges `H → C1, H → C2` and
`H ∈ force_render` (after L6) gets added to `effective`
exactly when at least one of `{C1, C2}` is in `targets`. That's
the listing-host pull-in semantic.

**Empty-match listing hosts:** a host with `listing_content_globs:
["*.qmd"]` but a project that has no other `.qmd` next to the
host gets `force_render.insert(host)` but no outgoing edges. The
augmentation requires the host be reachable from a target via
forward edges — with no edges, it never gets pulled in. Harmless.

## Profile version bump

`DOCUMENT_PROFILE_VERSION` increments from 4 → 5. The struct
gains one field:

```rust
/// Glob patterns from the host's `listing.*.contents` config,
/// flattened across all listings declared on the page. Each
/// entry is a raw glob string (e.g. `"*.qmd"`,
/// `"posts/**/*.qmd"`) — *not* a resolved path.
///
/// The dep-graph builder expands these at graph-build time
/// against `ProjectIndex.profiles()` (host-relative first,
/// project-relative fallback) to produce forward edges.
/// Listing hosts with non-empty entries are also added to
/// `ProjectDependencyGraph.force_render` so Mode B pulls them
/// in when any of their content files is in the user-named
/// target set.
///
/// Resolution is **not** cached on the profile because it
/// depends on the full project source set, which a per-doc
/// profile cannot represent safely (a new sibling .qmd added
/// to the project would not invalidate the host's profile
/// cache, leaving the resolution stale).
///
/// Default empty; serializer omits empty lists.
/// Added v4 → v5 (`bd-xbnf`).
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub listing_content_globs: Vec<String>,
```

The `Default` impl gets `listing_content_globs: Vec::new()`.
The contract doc
(`claude-notes/designs/document-profile-contract.md`) gets a
new row + change-log entry per the L0 precedent.

## Module layout

```
crates/quarto-core/src/
  document_profile.rs               ← bump VERSION; add field;
                                      extract via project::listing helper
  project/
    listing/
      config.rs                     ← add `extract_content_globs(meta)` pub fn
    dependency_graph.rs             ← add the listing-edges block in build()
                                      + tests
    discovery.rs                    ← promote `relative_to` to pub(crate)
                                      (or whatever the impl decides)

claude-notes/designs/
  document-profile-contract.md      ← change-log entry + field row
```

No new files. The L6 changes are surgical against the existing
phase-8 substrate.

## Diagnostic codes

L6 adds none. Empty-match globs are silent at graph-build time
(matches L3's render-time empty-listing behavior, which is also
silent). Glob-typo cases surface naturally as "listing renders
empty" at render time, where the user is more likely to act on
the feedback.

## Open questions

These are non-blocking but the L6 session author should resolve
inline rather than punt:

1. **Where does `extract_content_globs` live?** Recommended:
   `crates/quarto-core/src/project/listing/config.rs` as a new
   pub fn. Verify during impl that
   `crate::document_profile` doesn't already participate in a
   reverse-direction module dependency on `project::listing`.
2. **Should the L6 edge source share the listing-generate
   transform's `matches_any_glob`?** No — the inputs differ
   (transform iterates profiles per listing; graph iterates
   listings per profile). The shared piece is `glob_match_path`
   and `relative_to`, both already factored. Don't over-share.
3. **Should the `--clean-cache` CLI flag (Phase-8) note the
   L6 bump?** Probably no documentation change needed; the
   `--clean-cache` semantics are "blow it all away," and the
   v4→v5 mismatch on read produces a clean error +
   regeneration anyway. If the L6 session sees a user-facing
   doc that mentions specific profile versions, update it
   (unlikely; the doc tends to talk about behavior).
4. **Listing host that ALSO has `project.always-render: true`.**
   Double-counted in `force_render` set (a `BTreeSet` so the
   dedup is automatic). No behavioral surprise.
5. **Listing host whose `listing.contents` glob points outside
   the project root** (e.g. `../sibling-project/*.qmd`). The
   glob-match against `index.profiles()` will simply produce no
   edges — `index` only knows project-internal files. Matches
   L3's render-time behavior. No new diagnostic. **Additional
   reason this is the only viable behavior:** on the
   hub-client / WASM path, the runtime has access only to a VFS
   populated with project documents — there is no filesystem to
   walk outside the project root, so out-of-project paths are
   not just unsupported but architecturally unrepresentable.
   The native and WASM paths converge here naturally because
   both go through `ProjectIndex.profiles()`. Any future
   "include sibling project files" feature would have to add a
   separate index source on both sides; until then, silently
   dropping out-of-project globs is the **single uniform
   semantic** L3 and L6 share.

## Decisions log

- **D1 (`listing_content_globs: Vec<String>` on profile):**
  user-confirmed 2026-05-07. Globs are stable per host;
  resolution is graph-time-only.
- **D2 (`DOCUMENT_PROFILE_VERSION` 4 → 5):** user-confirmed
  2026-05-07. Matches L0 bump precedent; invalidates stale
  caches.
- **D3 (fold listing hosts into `force_render`):**
  user-confirmed 2026-05-07. Reuses
  `augment_targets_with_always_render` without modification.
- **D4 (defaults handling: shorthand `listing: default` →
  glob `"*.qmd"`):** matches L3's `apply_type_defaults` so
  graph edges line up with render-time matches.
- **D5 (multi-listing host: flatten globs):** graph edges are
  union; the field is a flat `Vec<String>`.
- **D6 (no new diagnostics):** empty-match cases are silent;
  L3 handles "listing renders empty" at render time.
- **D7 (glob-match rule mirrors L3 — host-relative first,
  project-relative fallback):** same `glob_match_path` call,
  same fallback order. Avoids divergence between graph and
  render.
- **D8 (worktree on `feature/listings`):** branch
  `beads/bd-xbnf-listings-dep-graph` at
  `.worktrees/bd-xbnf-listings-dep-graph/`, branched off the
  current `feature/listings` head (`4e271dee` at the time of
  writing — confirm at impl start). Same convention as L1 / L3
  / L5.

## Branch / worktree

L6 starts from the current `feature/listings` head. The L6
worktree lives at:

```
.worktrees/bd-xbnf-listings-dep-graph/
```

Branch: `beads/bd-xbnf-listings-dep-graph`, branched off
`feature/listings`.

Per `.claude/rules/worktrees.md`:

```bash
cd .worktrees/bd-xbnf-listings-dep-graph
echo "../../../.beads" > .beads/redirect
npm install
cargo xtask verify --skip-hub-build  # baseline before changes
```

Before starting, the L6 session must record:

- Current `feature/listings` HEAD hash (was `4e271dee` at plan
  time).
- Baseline test count (was 8621 at L5 close-out; may have moved
  if other branches landed).

## Tests (TDD)

Per CLAUDE.md: write tests, watch fail, implement, watch pass.

### Unit tests — `extract_content_globs`

In `crates/quarto-core/src/project/listing/config.rs`:

1. **`extract_globs_from_single_listing_default_shorthand`**
   — `meta.listing == ConfigValue::Bool(true)` ⇒ `["*.qmd"]`.
2. **`extract_globs_from_single_listing_with_explicit_contents`**
   — `meta.listing == { contents: ["posts/*.qmd"] }` ⇒
   `["posts/*.qmd"]`.
3. **`extract_globs_from_single_listing_no_contents_shorthand`**
   — `meta.listing == { type: grid }` (no `contents:`) ⇒
   `["*.qmd"]`.
4. **`extract_globs_from_array_of_listings`** —
   `meta.listing == [{ contents: a/*.qmd },
                     { contents: [b/*.qmd, c/*.qmd] }]`
   ⇒ `["a/*.qmd", "b/*.qmd", "c/*.qmd"]`.
5. **`extract_globs_drops_inline_records`** — `meta.listing
   == { contents: [{title: "foo"}, "*.qmd"] }` ⇒ `["*.qmd"]`
   (inline record dropped without diagnostic — L3 already
   surfaces `Q-12-2` at render time).
6. **`extract_globs_listing_false_is_empty`** —
   `meta.listing == false` ⇒ `[]`.
7. **`extract_globs_no_listing_key_is_empty`** — meta has
   no `listing` ⇒ `[]`.
8. **`extract_globs_handles_string_shorthand_contents`** —
   `meta.listing == { contents: "*.qmd" }` (single string,
   not array) ⇒ `["*.qmd"]`.

### Unit tests — `DocumentProfile::extract`

In `crates/quarto-core/src/document_profile.rs`:

9. **`profile_v5_default_has_empty_listing_content_globs`**
   — `DocumentProfile::default().listing_content_globs.is_empty()`.
10. **`profile_extract_populates_listing_content_globs_from_meta`**
    — fixture meta with a listing produces a non-empty field.
11. **`profile_v4_json_rejected_with_clean_error`** —
    deserialize a v4-shaped JSON, get
    `DocumentProfileError::VersionMismatch { expected: 5, found: 4 }`.
12. **`profile_v5_round_trip`** — serialize a profile with
    a non-empty `listing_content_globs`, deserialize, fields
    match.
13. **`profile_v5_round_trip_empty_omits_field`** —
    `to_json` of a default profile contains no
    `"listing_content_globs"` key (skip_serializing_if).

### Unit tests — `ProjectDependencyGraph::build` (listing edges)

In `crates/quarto-core/src/project/dependency_graph.rs`:

14. **`listing_globs_become_edges_host_relative_default`** —
    Host `index.qmd` with `listing_content_globs: ["*.qmd"]`,
    siblings `a.qmd`, `b.qmd`, host's own `index.qmd`. Edges:
    `index.qmd → a.qmd`, `index.qmd → b.qmd`. Self-edge
    suppressed.
15. **`listing_globs_become_edges_project_relative`** — Host
    `index.qmd` with globs `["posts/**/*.qmd"]`, siblings
    `posts/foo.qmd` and `posts/bar.qmd`. Edges:
    `index.qmd → posts/foo.qmd`, `index.qmd → posts/bar.qmd`.
16. **`listing_globs_no_match_no_edges`** — Host has
    `listing_content_globs: ["nope/*.qmd"]`, project has no
    matches. No edges added; host is still in `force_render`
    (so an empty listing host with no edges is harmless to
    Mode B; the augmentation requires reachability).
17. **`listing_host_added_to_force_render`** — Host with any
    non-empty `listing_content_globs` is in
    `graph.force_render` after build, regardless of edge count.
18. **`listing_host_with_empty_globs_not_in_force_render`** —
    A `Vec::new()` field doesn't add the host to `force_render`.
19. **`listing_globs_dont_self_edge`** — Host `posts/index.qmd`
    with glob `"*.qmd"` siblings `posts/foo.qmd` and itself —
    matches `posts/foo.qmd` only.
20. **`listing_globs_combine_with_body_link_edges`** — Same
    host has both `listing_content_globs` and
    `body_link_targets` pointing at overlapping pages; the
    edge sets dedup via `BTreeSet`.
21. **`listing_host_with_always_render_double_force_render_no_dup`**
    — Host has both `always_render: true` and
    `listing_content_globs: ["*.qmd"]`. `force_render` set
    contains the host once (BTreeSet dedup).
22. **`listing_globs_multi_glob_host`** — One host, two globs
    matching disjoint sibling sets, both edge sets present.

### Augmentation tests (Mode B)

23. **`augment_pulls_in_listing_host_when_content_targeted`** —
    Build a graph: `index.qmd` is a listing host with edge
    `index.qmd → posts/foo.qmd` and `force_render` set. User
    target = `posts/foo.qmd`. `augment_targets_with_always_render`
    returns `{posts/foo.qmd, index.qmd}`. (This validates that
    L6's force_render insertion + existing augmentation
    interoperate without orchestrator changes.)
24. **`augment_does_not_pull_in_listing_host_when_unrelated_target`**
    — Host `index.qmd → posts/foo.qmd`. User target =
    `unrelated.qmd` (no edge to `index.qmd`).
    `augment_targets_with_always_render` returns
    `{unrelated.qmd}` only.
25. **`augment_pulls_in_listing_host_only_via_listing_edges_not_body_links`**
    — Sanity check: a non-listing-host with a body link to a
    target page does *not* get pulled in (it's not in
    `force_render`). Confirms force_render gating still
    discriminates.

### Integration test (CLI Mode B)

26. **`mode_b_re_renders_listing_host_when_content_targeted`**
    — End-to-end fixture: `index.qmd` (listing host),
    `posts/foo.qmd`, `posts/bar.qmd`. First do
    `quarto render` (Mode A) to populate the cache. Then
    `quarto render posts/foo.qmd` (Mode B). Assert:
    - `_site/posts/foo.html` rebuilt (mtime updated).
    - **`_site/index.html` rebuilt** (mtime updated).
      Pre-L6, `_site/index.html` would not rebuild and would
      show stale data.
    - `_site/posts/bar.html` *not* rebuilt (mtime unchanged
      from the Mode-A pass).

    **End-to-end CLI verification per CLAUDE.md.**

### Snapshot tests

None — the rendered output isn't what L6 changes (L3/L5 already
cover the listing rendering). L6's contract is "the right files
re-render under Mode B," which is the integration test above.

### End-to-end CLI verification record

Recorded 2026-05-07 by the L6 implementation session.

**Fixture** (`/tmp/l6-cli-fixture/`):

```
_quarto.yml         # project.type: website, output-dir: _site
index.qmd           # listing host: contents: posts/*.qmd
posts/foo.qmd       # title: "Foo Original", date: 2026-05-01
posts/bar.qmd       # title: "Bar Original", date: 2026-05-02
```

**Invocation sequence:**

```bash
# Mode A — cold full render
cargo run --bin q2 -- render /tmp/l6-cli-fixture

# Edit foo's title only (simulates user editing one post):
#   title: "Foo UPDATED L6 PROOF"

# Mode B — render only the edited file
cargo run --bin q2 -- render /tmp/l6-cli-fixture/posts/foo.qmd
```

**Pre-Mode-B mtimes** (all from the cold Mode-A render):

```
May  7 12:28:28 2026 /tmp/l6-cli-fixture/_site/index.html
May  7 12:28:28 2026 /tmp/l6-cli-fixture/_site/posts/foo.html
May  7 12:28:28 2026 /tmp/l6-cli-fixture/_site/posts/bar.html
```

**Post-Mode-B mtimes:**

```
May  7 12:29:00 2026 /tmp/l6-cli-fixture/_site/index.html        ← refreshed via L6
May  7 12:29:00 2026 /tmp/l6-cli-fixture/_site/posts/foo.html    ← explicit target
May  7 12:28:28 2026 /tmp/l6-cli-fixture/_site/posts/bar.html    ← unchanged ✓
```

**Listing content in `_site/index.html` after Mode B**
(grepped to confirm the listing actually re-rendered):

```
<h3><a href="posts/bar.html" class="…">Bar Original</a></h3>
<h3 id="-1"><a href="posts/foo.html" class="…">Foo UPDATED L6 PROOF</a></h3>
```

`bar` still says `Bar Original` (correct — bar wasn't re-extracted),
`foo` now says `Foo UPDATED L6 PROOF` — proves the listing host
re-rendered against fresh sibling profiles. Pre-L6, index.html
would show stale `Foo Original` because the host wouldn't be in
the Mode-B render set.

Output **inspected** by hand: mtime table + grep output above.

### Hub-client smoke

L6 doesn't change rendered HTML, only Mode B selection.
Hub-client uses `RenderMode::ActivePage` (skips the
augmentation entirely), so L6 has no hub-client behavioral
effect. The hub-client smoke-test bar is **just `cargo xtask
verify` passing the WASM build** (the profile-field addition
flows through `quarto-core` types, so a serde regression would
surface there). No browser smoke required.

## Pipeline-builder wiring

None. `DocumentProfile::extract` runs in
`DocumentProfileStage` (Pass-1) which is already wired into
`build_html_pipeline_stages_with_apply_config` and
`build_wasm_html_pipeline`. Adding a field to the struct is
transparent to both. The graph builder is invoked from the
orchestrator, which is already wired.

## Risks and mitigations

- **Risk: a stale Phase-8 cache from before the v4→v5 bump
  silently re-reads as v4 and crashes.** *Mitigation:*
  `from_json` already returns
  `DocumentProfileError::VersionMismatch { expected: 5, found: 4 }`
  with a clean error; the orchestrator catches that and
  regenerates. Tested via #11.
- **Risk: glob expansion at graph-build is hot in projects
  with hundreds of listings.** *Mitigation:* the inner loop is
  `O(H × G × N)`. For a 1000-page project with 5 listing
  hosts and 1 glob each, that's 5000 string-pattern matches —
  microseconds. If profiling later shows it as a hotspot, a
  side-table cache keyed on the index identity is a clean
  follow-up.
- **Risk: `extract_content_globs` and `parse_listings` drift
  apart on what counts as a valid `contents:` shape.**
  *Mitigation:* both functions live in
  `crate::project::listing::config` (per D1's recommended
  placement). Co-located source pressure plus shared unit
  tests keep them aligned. If they ever do diverge, the L6
  integration test (#26) will catch it — a glob shape that
  L3's `parse_listings` accepts but L6's
  `extract_content_globs` drops will produce missing edges
  and the listing host won't re-render.
- **Risk: hub-client picks up the v5 bump and live-preview
  behavior changes.** *Mitigation:* hub-client doesn't
  participate in the dep-graph augmentation
  (`RenderMode::ActivePage`). The profile-field change is
  serde-only on the WASM side. `cargo xtask verify` covers
  the WASM build.
- **Risk: a sibling extracts file that's not part of the
  project's render set (e.g. excluded via `project.render`)
  is matched by a listing glob, but ProjectIndex doesn't
  carry it.** *Mitigation:* the L3 plan's D10 already
  documents this: files outside `ProjectIndex` are silently
  dropped at render time. L6 inherits this — no edge is
  added. Same trade-off, same behavior.
- **Risk: a Lua filter at Pass-1 mutates `meta.listing`
  before `DocumentProfileStage` runs.** *Mitigation:*
  `DocumentProfileStage` runs after the pre-checkpoint Lua
  filter slot; whatever the filter writes is what L6
  extracts. Matches existing `nav_dependencies` /
  `always_render` behavior.

## Implementation steps

Follow CLAUDE.md TDD: write tests, watch fail, implement,
watch pass.

### Preparation

- [x] Re-read
      `claude-notes/instructions/testing.md` and
      `claude-notes/instructions/coding.md`.
- [x] Re-read `.claude/rules/wasm.md` (`?Send`,
      WASM-cfg gating).
- [x] Re-read Phase-8 plan §"Decision 5" + §"Mode B with
      `always-render` siblings" for the contract L6 extends.
- [x] Confirm `feature/listings` head is the post-L5 merge
      (record HEAD hash + baseline test count). HEAD `4e271dee`,
      baseline 8621 tests passing, full `cargo xtask verify
      --skip-hub-build --skip-hub-tests` green.
- [x] Create the worktree at
      `.worktrees/bd-xbnf-listings-dep-graph/` per
      §"Branch / worktree". Branch
      `beads/bd-xbnf-listings-dep-graph`.
- [x] `npm install` in the worktree.
- [x] Add `.beads/redirect` per worktree rules.
- [x] Baseline: `cargo xtask verify --skip-hub-build
      --skip-hub-tests`; record test count. (8621 tests, all
      passing.)

### TDD phase 1 — `extract_content_globs`

- [x] Write tests #1–8 in
      `crates/quarto-core/src/project/listing/config.rs`'s
      test module (or a new submodule). Fail.
- [x] Implement `pub fn extract_content_globs(meta:
      &ConfigValue) -> Vec<String>`. Tests pass. Implementation
      delegates to `parse_listings` and discards diagnostics —
      single source of truth on shape; followup `bd-bqf2` tracks
      a future shared-shape-walker refactor.

### TDD phase 2 — `DocumentProfile` v5 + extract wire-up

- [x] Write tests #9–13. Fail (struct field missing,
      version still 4).
- [x] Add `listing_content_globs: Vec<String>` to
      `DocumentProfile`. Update `Default`, the `extract`
      function (calling
      `crate::project::listing::config::extract_content_globs`),
      and the version constant.
- [x] Update
      `claude-notes/designs/document-profile-contract.md`:
      change-log entry, field row, version-history block.
- [x] Tests pass. Workspace tests pass (8634 = 8621 baseline + 13
      new tests; phase 1 = 8 + phase 2 = 5).

### TDD phase 3 — Graph-build edges + force_render

- [x] Write tests #14–22. Fail.
- [x] Add the listing-edges block to
      `ProjectDependencyGraph::build`. Promote `relative_to`
      from `transforms::listing_generate` to
      `pub(crate)` somewhere shared (recommend
      `crate::project::discovery`). Renamed to `relative_to_dir`
      to disambiguate from the more generic name.
- [x] Tests pass.

### TDD phase 4 — Augmentation interaction

- [x] Write tests #23–25. Tests pass against the existing
      augmentation primitive (no augmentation code change
      required — phase 3's `force_render` insertion is the
      only adapter needed). All three guard the existing
      semantics against future regression.

### TDD phase 5 — Mode B integration test

- [x] Write test #26 (CLI end-to-end). Inline fixture in
      `crates/quarto-core/tests/incremental_rebuild.rs`
      (matches the existing `mode_b_*` pattern; no separate
      `tests/fixtures/listings/` directory needed).
- [x] Test passes on first try — phase 3's `force_render`
      insertion is the load-bearing fix and the orchestrator's
      existing `augment_targets_with_always_render` does the
      rest. Validates via mtime + output count.

### Verification and close-out

- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — all pass; record
      test-count delta. **8647 tests passing**, up from
      8621 baseline (+26: phase-1 = 8 + phase-2 = 5 +
      phase-3+4 = 12 + phase-5 = 1).
- [x] `cargo xtask lint` clean. (693 files checked.)
- [x] `cargo xtask verify` (full, including hub-client +
      WASM build) — all 9 steps green. The v4 → v5 profile
      bump flows through `wasm-quarto-hub-client` cleanly.
- [x] End-to-end CLI verification fixture rendered;
      output inspected; recorded above in the
      §"End-to-end CLI verification record".
- [x] Hub-client browser smoke **not required** for L6
      (see §"Hub-client smoke"). `cargo xtask verify`
      covers the WASM serde regression risk.
- [ ] Stop and request user permission before any push
      (per CLAUDE.md §"GIT PUSH POLICY").
- [ ] After user approval: `br update bd-xbnf --status
      closed`.
- [ ] `br sync --flush-only && git add .beads/ && git
      commit` from the **main repo** (per
      `.claude/rules/worktrees.md`).
- [ ] Update the listings epic table
      (`claude-notes/plans/2026-05-05-listings-epic.md`)
      to mark L6 closed with the merge commit hash.

## Filing reminder

This sub-plan corresponds to **one** bd issue:

- `bd-xbnf` — L6, the dependency-graph integration.

After impl, close with a reason that references the landed
commit. Update the issue description with a one-line link to
this file.

### Follow-up bd issues (file during impl if they trigger)

1. **Resolved-target side-table cache** *(conditional)* —
   only if profiling shows graph-build glob expansion is a
   hotspot for very large projects. Out of scope today
   (5k-page projects with hundreds of listings would still
   resolve in milliseconds).
2. **Graph-build diagnostic for empty-match listing
   `contents:` glob** *(conditional)* — only if user
   feedback shows that "listing renders empty" at render
   time isn't enough signal and earlier surfacing helps.
   Recommend filing only after at least one user complaint;
   premature noise is worse than no diagnostic.
3. **`extract_content_globs` source-info threading** —
   today the extractor returns plain strings; a
   future enhancement could carry source spans for
   richer diagnostics later. Filed only if needed.
