# Phase 8 — Incremental rebuilds

**Date:** 2026-04-27 (redrafted after design discussion)
**Beads:** TBD (parent `bd-0tr6`).
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-27-websites-phase-7.md`
**Status:** Draft v2 — pending user review.

## Goal of this phase

Phase 8 makes single-document re-renders inside a project
cheap, *without caching any user-controllable computation*.
The mechanism is a **dependency graph** computed from each
page's `DocumentProfile` plus the project nav config, paired
with a Pass-1 (profile) cache. Concretely:

1. **Profile cache (Pass 1 skip).** Persist each `DocumentProfile`
   to `<project>/.quarto/cache/profiles/<key>` keyed by a content
   hash of the source plus every `_metadata.yml` / `_quarto.yml` /
   transitive include / extension contribution that participated in
   its merged metadata. Hit the cache for unchanged inputs; fall
   through to the Pass-1 head pipeline only on miss.
2. **Dependency graph.** For every page, compute the set of *other*
   pages whose profile content can affect this page's rendered
   output. Inputs:
   - Sidebar co-membership (any sibling in the same sidebar).
   - Page-navigation neighbors (prev / next from the resolved
     sidebar order).
   - Cross-doc body-link targets.
   - User-declared `project.nav-dependencies` in the page's merged
     metadata (escape hatch / Lua-filter disclosure).
   The graph carries forward `edges` and reverse `reverse_edges`
   for O(N) propagation on thousand-page sites.
3. **Two render modes.**
   - **Mode A — `quarto render`** (full project): re-render
     every page's Pass-2 unconditionally. Profile cache speeds
     up Pass-1; Pass-2 is unchanged from today. This phase makes
     no attempt to skip Pass-2 in Mode A — filters / engines /
     transforms in Pass-2 can have side effects, and silently
     caching past them is the wrong default.
   - **Mode B — `quarto render foo.qmd` / `foo/` / `a.qmd b.qmd`**
     (subset render): render exactly the user-named subset.
     Walk the dependency graph from the subset to find sibling
     profiles that need re-extracting (so the subset's nav
     features render correctly); run Pass-1 only on those.
     Other pages are untouched on disk. **No Pass-2 propagation
     onto dependents** — the user named the targets.
4. **Sidebar `auto:` and body-link resolution lifted to Pass-1**
   as deterministic helpers with prose contracts. Pass-2's
   sidebar/link transforms layer on top. Equivalence tests
   prevent drift.
5. **Sitemap incremental merge.** Read existing
   `_site/sitemap.xml`, replace entries for re-rendered pages,
   preserve entries for non-targets. Closes Phase 7's `bd-pphv`.
6. **CLI surface.** `--clean` wipes the `<project>/.quarto/`
   cache before rendering. No `--full`, no `--no-cache` — Mode
   A is already the full render, and disabling the profile cache
   adds failure modes without removing meaningful ones.

## Why no Pass-2 cache

A `DocumentProfile` is a faithful, side-effect-free static summary
of a document — caching it is always safe. Pass-2 is the opposite:
it runs user-supplied Lua filters, executes engines (R, Python,
Julia, Observable JS, …), reads environment / wall-clock /
network, and produces output that may legitimately differ between
identical inputs. Caching Pass-2 output by hashing inputs is a
silent soundness regression for any user whose filters or chunks
have side effects, and the failure mode is "stale but plausible
HTML on disk" — exactly the kind of bug Quarto's reproducibility
story is supposed to prevent.

The narrower `freeze` feature (separate epic) caches engine output
by *user opt-in* at a single well-understood boundary. Phase 8
leaves freeze untouched. Within Phase 8 there is no analogous
cache. **The win is not "Pass-2 ran but its output was cached."
The win is "Pass-2 didn't need to run at all."**

This is a deliberate sacrifice of one optimization (re-running an
unaffected page's Pass-2 to produce the same bytes) in exchange
for predictable correctness. If real users benefit from the
deterministic-filter case, an opt-in Pass-2 cache can be added
later as its own feature, with the user explicitly asserting
filter purity. Phase 8 doesn't try to guess.

## What this phase explicitly is **not**

- **Pass-2 output caching.** No cached HTML, no replay, no tarred
  Page-scoped artifact replay, no project-state snapshot.
  See §"Why no Pass-2 cache" above.
- **Engine output caching (`freeze`).** Separate epic, untouched
  by Phase 8. Sharing the serializable-checkpoint substrate is in
  scope; *executing* on it belongs to that epic.
- **Resumption from cached `AtProfile`** (`bd-ee4z`). Pass-1 cache
  reads serialized `DocumentProfile` JSON directly; Pass-2 always
  starts from Pass-2 head when it runs. Resumption is an
  optimization that's only meaningful once Pass-2 caching exists.
- **Hub-client cache integration.** Phase 9 layers a WASM-side
  store on the same `SystemRuntime::cache_get/set` interface
  Phase 8 uses. Phase 8 ships native-only; on WASM the default
  `cache_get` returns `Ok(None)` (always recompute — correct).
- **Cross-format invalidation.** Multi-format projects are out of
  website-epic scope; the profile cache key includes `format_id`
  so per-format caches don't collide.
- **Parallel Pass-1 / Pass-2.** `bd-pdwr` follow-up.
- **Watching the filesystem.** `quarto preview` territory.

## The bd-r82e prerequisite (DocumentProfile.includes)

`bd-r82e` (filed during `bd-xfwx`) is a hard prerequisite. After
`IncludeExpansionStage` was threaded ahead of the profile
checkpoint, a profile can reflect content spliced in from
`{{< include child.qmd >}}`. The profile *depends on* the child
file but carries no record of it; a content hash of the parent
source alone is insufficient.

Phase 8 closes `bd-r82e` as **sub-phase 8.0** before any caching
code is written:

1. Add `pub includes: Vec<IncludeEntry>` to `DocumentProfile`
   where `IncludeEntry { path: PathBuf, content_hash: [u8; 32] }`.
2. Populate from `IncludeExpansionStage` via a side-channel
   `DocumentProfileStage` drains.
3. Bump `DOCUMENT_PROFILE_VERSION` 1 → 2.
4. Update `claude-notes/designs/document-profile-contract.md`.
5. Tests: profile records every direct + transitive include with
   correct content hashes.

Sub-phase 8.0 also adds the two other new profile fields needed by
the dependency graph (Decision 4 below) — they ride the same
profile-version bump.

## Reference material

- **Parent epic plan** §"Phase 8 — Incremental rebuilds".
- **Phase 0 sub-plan** — `DocumentProfile` contract and
  project-root invariant.
- **Phase 1 sub-plan** §"`pass_one` / `pass_two` driver" — the
  insertion seam for cache lookups and dependency-aware
  Pass-2 selection.
- **Phase 2 sub-plan** §"Sidebar resolution" — sidebar membership
  is the largest source of dependency edges.
- **Phase 4 sub-plan** §"Prev/next derivation" — page-nav neighbors.
- **Phase 6 sub-plan** §"`LinkRewriteTransform`" — cross-doc body
  links produce dependency edges as a side effect of resolution.
- **Phase 7 sub-plan** §"Decision 9 — Sitemap algorithm" — the
  fresh-write Phase 8 turns into a merge.
- **`bd-r82e`** — includes-tracking blocker.
- **Q2 current code:**
  - `crates/quarto-core/src/project/orchestrator.rs:336-488` —
    `ProjectPipeline::run`, `pass_one`, `pass_two`. Phase 8
    wraps Pass-1 in a cache lookup and gates Pass-2 on the
    dependency graph.
  - `crates/quarto-core/src/document_profile.rs:46-…` —
    `DocumentProfile`. Phase 8 adds `includes`, `nav_dependencies`,
    `always_render` fields.
  - `crates/quarto-core/src/project/index.rs:35-…` —
    `ProjectIndex`. Phase 8 builds a `ProjectDependencyGraph`
    from the index plus nav config.
  - `crates/quarto-core/src/project/mod.rs:104-186` —
    `directory_metadata_for_document`. Layered `_metadata.yml`
    files contribute to the Pass-1 cache key.
  - `crates/quarto-core/src/stage/include_expansion.rs` —
    `IncludeExpansionStage`. Sub-phase 8.0 amends this stage.
  - `crates/quarto-core/src/transforms/link_rewrite.rs` (Phase 6)
    — link rewriting already resolves `.qmd` → `.html` via
    `ProjectIndex`; Phase 8 captures the resolved targets as
    body-link edges.
  - `crates/quarto-core/src/project/website_post_render.rs` —
    `write_sitemap`. Phase 8 turns fresh-write into merge.
  - `crates/quarto-system-runtime/src/traits.rs:686-714` —
    `cache_get` / `cache_set` / `cache_clear_namespace`. Phase 8
    persists the profile cache through this API; **no new file
    I/O paths**.
  - `crates/quarto-system-runtime/src/native.rs:459-540` —
    `NativeRuntime` cache backing. Already wired to
    `<project>/.quarto/cache/` for SASS in
    `commands/render.rs:108-110`.
  - `crates/quarto/src/commands/render.rs:97-127` — CLI flag
    surface. Phase 8 adds `--full` and `--clean`.
- **Q1 reference (negative space):** Q1's `--incremental` is
  user-pointed (the user lists which files to re-render); Q1 has
  no automatic dependency analysis. Phase 8's auto-detect
  dependency graph is Q2-original.

## Key decisions (to confirm with user)

### Decision 1 — Cache layout under `<project>/.quarto/cache/`

```
<project>/.quarto/cache/
├── sass/                       # existing (Phase 5 SCSS pre-compile)
└── profiles/                   # NEW: per-page DocumentProfile JSON
    └── <pass1-key>             # 64-char hex sha256
```

That's it. No `pages/` directory, no `project-state/` snapshot,
no tar bundles. The dependency graph is recomputed from scratch
each run from the (cached or fresh) profiles — it's cheap and
not worth caching.

`profiles/` reuses the existing `SystemRuntime::cache_get/set`
path validated at `commands/render.rs:108`. Single I/O abstraction,
already works for hub-client's eventual WASM backing.

### Decision 2 — Pass-1 cache key

```
sha256(
  PROFILE_KEY_VERSION       (4 bytes, currently = 1)
  | quarto_build_id()         (length-prefixed UTF-8 — release version
                               or, in dev builds, git short hash;
                               see Decision 3)
  | DOCUMENT_PROFILE_VERSION  (4 bytes, currently = 2 after bd-r82e)
  | format_id                 (length-prefixed UTF-8)
  | source_path               (project-relative, length-prefixed UTF-8)
  | source_bytes              (length-prefixed)
  | for each layered _metadata.yml from project root → doc dir:
      | path                  (length-prefixed)
      | bytes                 (length-prefixed)
  | _quarto.yml bytes         (length-prefixed; "" if absent)
  | for each format-extension contribution, sorted by name:
      | name                  (length-prefixed)
      | metadata bytes        (length-prefixed)
)
```

SHA-256 hex (64 chars). Crate: `sha2`. The `PROFILE_KEY_VERSION`
constant is the manual-override lever; `quarto_build_id()` is the
automatic one.

**Where transitive includes fit in.** During implementation it
became clear that putting the include set in the lookup key
creates a chicken-and-egg: to look up the cache *before* running
Pass-1 we'd need to know what files the document includes, but
discovering that is exactly Pass-1's job. **Resolved:** the cache
key omits the include set; the cached profile carries
`includes: Vec<IncludeEntry>` (Phase 8.0a's `bd-r82e`); on load,
`profile_cache::load` verifies each cached include's recorded
content_hash against the file's current bytes via a resolver
the orchestrator passes in. Any unreadable file or mismatched
hash degrades the load to a miss. Net invalidation behavior is
identical to the original plan; just check-on-load instead of
bake-into-key.

**Format-extension contributions.** In v1 these are passed
empty by the orchestrator. A user adding or changing a format
extension's metadata won't invalidate the cache automatically;
`--clean` (sub-phase 8.4) is the documented escape hatch. A
follow-up bead can add proper extension hashing once the
extension-discovery code path is convenient to consume from
the orchestrator.

### Decision 3 — Quarto version baked into the cache key

`quarto_build_id()` returns:
- Release builds: `env!("CARGO_PKG_VERSION")`.
- Dev builds: same plus `+<git short hash>` via `vergen` or a
  one-line `build.rs` (decided in sub-phase 8.1).

Distribution upgrades invalidate the cache transparently. Dev
hackers iterating on pipeline code see invalidation per commit.
Manual `PROFILE_KEY_VERSION` bumps remain available for
mid-version behavior changes.

### Decision 4 — Three new `DocumentProfile` fields (sub-phase 8.0)

Sub-phase 8.0 lands **all three** new profile fields in one
profile-version bump (1 → 2):

```rust
pub struct DocumentProfile {
    // … existing fields …

    /// Transitive include set (bd-r82e). Populated by
    /// IncludeExpansionStage; consumed by Pass-1 cache-key construction.
    pub includes: Vec<IncludeEntry>,

    /// Pages this document depends on, declared by the user in
    /// frontmatter / _metadata.yml / _quarto.yml as
    /// `nav-dependencies: [other.qmd, …]`. Populated by
    /// DocumentProfileStage from merged metadata. Default: empty.
    pub nav_dependencies: Vec<PathBuf>,

    /// User-declared "always re-render this page" flag. Set when the
    /// user knows their filters introduce non-deterministic content
    /// or undeclarable dependencies (e.g. a Lua filter that walks the
    /// whole project). Frontmatter / _metadata.yml / _quarto.yml key
    /// is `always-render: true`. Default: false.
    pub always_render: bool,
}

pub struct IncludeEntry {
    pub path: PathBuf,        // project-relative, forward-slash
    pub content_hash: [u8; 32],
}
```

**Why all three at once.** They share a profile-version bump and
share the `DocumentProfileStage` plumbing. Sub-phase 8.0 is a
clean atomic deliverable that makes Phase 8's machinery possible
without any Phase 8 caching code yet existing.

**Why `nav_dependencies` is path-based, not profile-id-based.**
Paths are stable across runs; profile identity is run-local.
Resolution to a `ProjectIndex` slot happens at graph-build time.

**Why a flat `Vec<PathBuf>` and not a richer `(path, reason)`
struct.** v1 simplicity. The graph-builder *also* contributes
edges with reasons (sidebar / prev-next / body-link); the
`nav_dependencies` field is purely the user's declaration channel.

### Decision 5 — Dependency graph: shape and inputs

```rust
pub struct ProjectDependencyGraph {
    /// For each source path, the set of source paths whose profile
    /// content can affect this page's rendered output.
    pub edges: HashMap<PathBuf, BTreeSet<PathBuf>>,

    /// Reverse index: for each source path, the set of source paths
    /// that depend on it. Built alongside `edges` in one pass and
    /// used by transitive-deps queries (Mode B's `needed_profiles`)
    /// for O(N) propagation instead of O(N²).
    pub reverse_edges: HashMap<PathBuf, BTreeSet<PathBuf>>,

    /// Pages explicitly forced into the render set regardless of
    /// dependency analysis (`always-render: true` OR `--full`).
    pub force_render: HashSet<PathBuf>,
}
```

Quarto websites can run into the thousands of documents, so the
reverse index ships day one. It's five extra lines at build
time and avoids the test that would catch the perf regression.

Built once between Pass 1 and Pass 2 from:

| Source | Edges contributed |
|---|---|
| Sidebar membership | for each sidebar with N entries, the complete sub-graph among those N pages (each entry depends on every other) |
| Prev/next neighbors | each page → its prev and next in resolved sidebar order |
| Body-link targets | source page → every project-relative `.qmd` it links to (captured during Phase 6's link resolution; see Decision 7) |
| User declarations | source page → each path in `profile.nav_dependencies` |

Sidebar membership is the heaviest single contributor — flatten
each sidebar's resolved entries and add the complete graph.
Conservative on purpose: a sidebar reorder might shift any
member's prev/next or active-item highlight, so any membership
co-listing implies mutual dependency. This is approximately
"O(sidebar-size²) edges per sidebar," which is fine — sidebars
are tens of entries, not thousands.

**force_render contents.** A page lands in `force_render` if any
of:

1. `profile.always_render == true`.
2. The project's nav config (the slices in `_quarto.yml` that
   affect every page: sidebar definitions, navbar, footer,
   website meta) changed since the previous run. We track this
   by hashing those slices and comparing against a one-line
   `<project>/.quarto/cache/nav-config-hash` file. Mismatch ⇒
   every page goes into `force_render`.
3. The user passed `--full`.

`force_render` is a coarse hammer; the dependency-graph edges are
the fine-grained mechanism. Both are needed.

### Decision 6 — Pass-2 selection: per-render-mode

The two render modes have meaningfully different shapes; the
algorithm reflects that.

**Mode A: full-project render (`quarto render` with no path).**

Re-render every page. Filters, engines, and Pass-2 transforms
all run for every page. The dependency graph and the profile
cache *still matter* — they keep Pass-1 cheap on the warm path
(profile cache hits) and they're the substrate for Mode B and
for Phase 9's hub-client. But Mode A doesn't try to skip any
page's Pass-2.

This matches how users think about `quarto render` ("rebuild my
site") and avoids the soundness fragility of trying to decide
which pages can safely skip Pass-2 in the presence of arbitrary
filters and engines. A future, separate caching epic can refine
this; Phase 8 doesn't.

**Mode B: single-doc / subset render (`quarto render foo.qmd`,
`quarto render foo/`, `quarto render a.qmd b.qmd c.qmd`).**

The user named a specific subset. Render exactly that subset
plus the minimum set of *DocumentProfile re-extractions* needed
for that subset's nav features to be correct. **No other pages'
Pass-2 runs.** Concretely:

```
// 1. Resolve the user-specified targets into a path set.
let targets: HashSet<PathBuf> = expand_user_args(args);

// 2. Run Pass-1 with cache for `targets`.
//    Pages outside `targets` don't run Pass-1 yet.
for page in targets:
    profile_with_cache(page);

// 3. Walk the dependency graph from `targets` to find which
//    *other* pages' profiles are needed for `targets` to render
//    correctly. (Sidebar membership, prev/next, body-link, user
//    nav-dependencies — all the same edges as Mode A.)
let needed_profiles: HashSet<PathBuf> = graph.transitive_deps(&targets);

// 4. Run Pass-1 with cache for `needed_profiles`.
for page in needed_profiles - targets:
    profile_with_cache(page);

// 5. Pass-2 over `targets` only.
for page in targets:
    render_document(page, project_index);
```

The reduced `quarto render foo/` form expands to the set of
`.qmd` files under `foo/` and reduces to the same algorithm.
Multiple-arg form `quarto render a.qmd b.qmd` is the union of
their target sets.

**Why no closure-from-changed-profiles in Mode B.** The user
told us *exactly* which pages to render — `targets`. Other
pages aren't re-rendered even if their dependents (the targets)
changed in ways that would normally propagate; that's beyond
the scope of what the user asked for. If they want propagation,
they invoke Mode A.

**Why we still build the full graph in Mode B.** Step 3 needs
it. The graph builder is cheap (linear in edges) and runs on
the in-memory `ProjectIndex`; the cost is dominated by Pass-1
profile re-extractions, which are exactly what we minimize via
`needed_profiles`.

**Cold project, Mode A.** Every Pass-1 cache misses; every page
re-extracts; every page renders Pass-2. Same as today.

**Warm project, Mode A.** Most Pass-1 hits; every page renders
Pass-2. Profile cache wins on Pass-1 cost; Pass-2 cost is
unchanged from today. Net win is the Pass-1 head pipeline
(parse + metadata merge + include expansion + profile extract).

**Cold project, Mode B (single doc).** Pass-1 the target plus
its dependency closure (sidebar siblings, etc.). Pass-2 the
target only. Sibling pages that share a sidebar with the target
do run Pass-1 (their profiles are needed) but do not run Pass-2.

**Warm project, Mode B.** Pass-1 the target (cache miss if its
inputs changed; hit otherwise) plus its dependency closure
(cache hits if their inputs are unchanged). Pass-2 the target
only.

**Sidebar `auto:` and Pass-1.** The dependency graph needs a
*fully-resolved* sidebar — not the raw `auto:` directive — to
emit co-membership edges. Resolution depends on knowing which
pages exist and their profiles (titles, draft flags, etc.).
Phase 8 treats this in two parts (Decision 7 below): a static
"which pages would this sidebar contain?" query that runs in
Pass-1, and the existing Pass-2 sidebar render that produces
HTML.

### Decision 7 — Pass-1 vs Pass-2 split for derived nav data

Two pieces of nav-relevant data have to be computable without
running engines / filters / Pass-2 transforms, so the dependency
graph can use them at Pass-1 time:

1. **Body-link targets** for a given page — which project-relative
   `.qmd` files does this page link to?
2. **Sidebar membership** — which pages does each declared
   sidebar contain (after `auto:` expansion)?

Both are *static* queries over a parsed document and the
project's file inventory. Phase 8 specifies them as deterministic
algorithms with prose contracts in
`claude-notes/designs/`, then implements each twice:

- **A "which pages?" pass** that runs in Pass-1 and produces
  paths only. Cheap, deterministic, no AST mutation.
- **The existing Pass-2 transform** that produces the actual
  HTML / metadata using the same logic plus per-page rendering
  context.

The two implementations *must* agree on the page set; a unit
test asserts equivalence on shared fixtures. If they ever
diverge that's a bug in one of them, not a design choice.

**Body-link resolution.** Phase 6's `LinkRewriteTransform`
already resolves `.qmd` → `.html` via `ProjectIndex` lookups
during Pass-2. Phase 8 extracts the *resolution* logic (path
normalization, `ProjectIndex` lookup, hash-fragment stripping)
into a shared helper `resolve_doc_relative_links` in
`crates/quarto-core/src/project/`. The new `LinkResolutionStage`
calls it at Pass-1 to populate `profile.body_link_targets`;
Phase 6's transform calls the same helper at Pass-2 to do the
rewrite. Single source of truth, asserted equivalent.

**Sidebar membership resolution.** The current sidebar-resolve
code (Phase 2) lives inside the Pass-2 sidebar transform. Phase 8
extracts the membership-only portion (which pages belong to
which sidebar, in what order, ignoring rendering specifics like
`expanded:` styling) into a Pass-1-callable helper:

```rust
pub fn resolve_sidebar_membership(
    config: &SidebarConfig,
    index: &ProjectIndex,
) -> Vec<ResolvedSidebar>;

pub struct ResolvedSidebar {
    pub id: Option<String>,
    pub members: Vec<PathBuf>,   // in declared order, project-relative
}
```

Pass-2's sidebar transform layers HTML rendering on top of this
same helper's output. Same equivalence test pattern.

**Prose contracts.** Sub-phase 8.0 produces
`claude-notes/designs/sidebar-auto-expansion-contract.md` and
`body-link-resolution-contract.md` describing the deterministic
algorithms in user-facing prose. The sub-plan includes their
generation as an explicit deliverable so users / Lua-filter
authors / future implementations have a stable target.

**New profile field.** Body-link targets land on the profile:

```rust
/// Project-relative .qmd targets this page links to.
/// Populated by LinkResolutionStage during Pass-1.
pub body_link_targets: Vec<PathBuf>,
```

Sidebar membership does *not* land on individual profiles — it's
a project-level computation, recomputed each run from
`_quarto.yml` + `ProjectIndex`. (Storing it per-page would
duplicate the same data on every member.)

**Why on the profile for body-link targets.** The dependency
graph builder needs them per-page; storing them on the profile
means the profile cache covers them (a page's body-link set is
a function of the page's source + includes — already in the
cache key). One source of truth.

### Decision 8 — Nav config hash (informational, future-use)

A one-line file `<project>/.quarto/cache/nav-config-hash` records
the SHA-256 of the project's nav-relevant config slices:

```
sha256(
  | navbar slice          (canonical JSON of meta.navbar)
  | sidebar slice         (canonical JSON of meta.website.sidebar)
  | footer slice          (canonical JSON of meta.website.page-footer)
  | website slice         (canonical JSON of meta.website {title, site-url, favicon})
)
```

Written every successful run. **Phase 8 does not act on this
hash** — Mode A re-renders everything, and Mode B renders only
`targets` regardless of nav-config drift, so there's no
"force re-render on nav change" path to gate. The hash is
recorded for two purposes:

1. **Diagnostic.** Trace logs report the value, useful for
   debugging "did the user edit the sidebar this run?"
2. **Future-use.** A later refinement (smarter Mode B that
   detects "a nav edit means I need to re-render this page even
   though the user only asked for foo.qmd") can consult it
   without a schema migration.

If the file is missing on read, that's not an error — the next
run writes it.

### Decision 9 — `--clean` semantics; no other new flags

| flag | profiles/ cache | nav-config-hash | this run reads cache | this run writes cache |
|---|---|---|---|---|
| (default) | kept | kept | yes | yes |
| `--clean` | wiped | wiped | yes (empty) | yes |

**`--clean` semantics.** Wipes `profiles/` and the
`nav-config-hash` file. Preserves `sass/` (SCSS recompiles are
expensive and almost never the source of incorrectness). Effect:
cold cache; every page's profile re-extracts; in Mode A every
page renders Pass-2 anyway, so `--clean` is mostly meaningful as
"throw away cached state I no longer trust."

**No `--full` flag.** Mode A (full-project `quarto render`)
already re-renders every page's Pass-2 unconditionally — there's
no per-page Pass-2 skip in Phase 8 to override. The "force every
page" intent is just "run `quarto render` with no path argument."

**No `--no-cache` flag.** Without a Pass-2 cache, the only thing
`--no-cache` could disable is the profile cache. Disabling that
saves a tiny amount of work and creates more failure modes than
it removes. `--clean` covers the "throw away cached state"
intent.

**Per-page `always-render: true`.** Still meaningful in Mode B:
a page with `always-render: true` is implicitly added to
`targets` if anything in its dependency closure is rendered.
This catches the "Lua filter introduces non-deterministic content;
re-render this page whenever its neighbors change" use case. Mode A
is unaffected (every page renders anyway).

### Decision 10 — Sitemap incremental merge (closes `bd-pphv`)

```
1. Read existing <output_dir>/sitemap.xml. Parse into
   BTreeMap<loc, SitemapEntry>.
2. For each profile in ProjectIndex:
   - If the page was rendered this run (in `changed`), update its
     entry with the current input mtime.
   - If the page was skipped, leave its entry untouched.
3. Write back, sorted by loc.
```

If reading fails (missing, malformed, version mismatch), fall
through to Phase 7's fresh-write. No regression.

### Decision 11 — Error policy: cache errors never abort a render

A corrupted profile cache file, JSON parse failure, version
mismatch — none abort. Single warning ("cache miss for X due to
load error"), fall through to live extraction. The next run
regenerates the cache cleanly. One exception: a *write* failure
on `nav-config-hash` is a hard error (future runs would
incorrectly assume nav config didn't change).

### Decision 12 — `nav-dependencies` and `always-render` plumbing

Both are user-facing metadata keys under the `project.` namespace
(matching `project.cache` precedent from earlier discussion).
Read by `DocumentProfileStage` from `meta.project`:

```yaml
# foo.qmd frontmatter
---
title: Foo
project:
  nav-dependencies:
    - a.qmd
    - subdir/b.qmd
  always-render: true   # rare; usually omitted
---
```

**Subtree application via `_metadata.yml`.** Setting
`always-render: true` in `_metadata.yml` applies to every doc in
the subtree (free from Q2's existing metadata merge). Same for
`nav-dependencies`, though the latter is less natural at subtree
scope (every doc in the subtree declaring the same dependency is
unusual). Document the subtree behavior in user docs.

**Frontmatter precedence.** Frontmatter > `_metadata.yml` chain >
`_quarto.yml`. Q2's existing merge precedence; Phase 8 inherits
without change.

**Path resolution.** `nav-dependencies` paths are resolved
project-relative (a leading `/` strips and re-roots at the
project; a bare path is relative to the document's directory).
The single helper used by Phase 6's link rewriter (`page_url_for`
+ relatives) is reused.

**Validation.** A `nav-dependency` that doesn't resolve to a
project document emits a diagnostic warning at graph-build time
and is dropped. Phase 8 doesn't fail the render — the user's
declaration is ignored, which is conservatively correct (we
might miss an edge but won't add a wrong one).

## Architecture sketch

### Module shape

```
crates/quarto-core/src/project/
    cache_key.rs           # NEW — Pass-1 key hasher + nav-config hasher
    profile_cache.rs       # NEW — DocumentProfile JSON read/write
    dependency_graph.rs    # NEW — ProjectDependencyGraph builder + closure
    orchestrator.rs        # MODIFIED — Pass-1 cache lookup; gates Pass-2
                           #            on dependency-aware `changed` set
    website_post_render.rs # MODIFIED — sitemap fresh-write → merge

crates/quarto-core/src/document_profile.rs
                           # MODIFIED — bd-r82e + nav_dependencies +
                           #            always_render + body_link_targets;
                           #            DOCUMENT_PROFILE_VERSION 1 → 2

crates/quarto-core/src/stage/include_expansion.rs
                           # MODIFIED — bd-r82e: record IncludeEntry side-channel

crates/quarto-core/src/stage/document_profile.rs
                           # MODIFIED — drain include side-channel; read
                           #            nav-dependencies / always-render from meta

crates/quarto-core/src/stage/link_resolution.rs
                           # NEW — LinkResolutionStage runs at end of Pass-1;
                           #       walks AST for project-relative .qmd links,
                           #       writes results to profile.body_link_targets

crates/quarto-core/src/transforms/link_rewrite.rs
                           # UNCHANGED — Phase 6's transform stays as-is;
                           # its body-link logic moves to LinkResolutionStage
                           # at Pass-1 (resolution is pure) while the actual
                           # rewrite stays in Pass-2 (mutation). See Decision 7.

crates/quarto/src/commands/render.rs
                           # MODIFIED — --full, --clean flags;
                           #            partial-render summary line
```

### Mode A — full project render (`quarto render`)

```
discover .qmd files in project
       │
       ▼
pass_one: for every page → profile_with_cache (hit / miss as inputs change)
       │
       ▼
ProjectIndex built from profiles
       │
       ▼
build ProjectDependencyGraph (edges + reverse_edges)
                ↑ informational only in Mode A
       │
       ▼
pass_two: render every page in full (engines, filters, transforms)
       │
       ▼
post_render → flush_site_libs → sitemap fresh-write → nav-config-hash write
```

Mode A's only Phase-8-induced acceleration is the profile cache
on the warm path. Pass-2 always runs; correctness is the same as
pre-Phase-8.

### Mode B — partial render (`quarto render foo.qmd` etc.)

```
expand args → targets: HashSet<PathBuf>
       │
       ▼
pass_one(targets): profile_with_cache for each target
       │
       ▼
build ProjectDependencyGraph from in-memory ProjectIndex with
the targets' profiles plus any cached profiles from previous runs
       │
       ▼
needed_profiles = transitive_deps(targets)   // via reverse_edges-aware walk
       │
       ▼
pass_one(needed_profiles - targets): profile_with_cache to ensure
the index is fresh enough for targets' nav rendering
       │
       ▼
pass_two: render only `targets` — engines + filters + transforms run
       │
       ▼
post_render → sitemap MERGE (targets' lastmods refresh, others preserved)
              → nav-config-hash write
```

Mode B touches zero Pass-2 work for non-target pages. Their
existing output on disk is left untouched.

### Mode B with `always-render` siblings

```
… same as above, plus:
implicit_targets = { p ∈ project.pages | p.always_render
                                       && reverse_edges_intersect(p, targets) }
targets ← targets ∪ implicit_targets
… continue as above with the augmented targets …
```

A page with `project.always-render: true` gets pulled into the
render set if any of its dependents (i.e. any page that links
to / co-shares-a-sidebar-with / lists it as a nav-dep) is in
the user-named targets. This catches the "Lua filter inserts a
random quote on this page; re-render whenever neighbors change"
case.

### Single-doc behavior (regression)

A single `.qmd` render outside a project still constructs
`NativeRuntime::new()` with no cache_dir. Cache calls
short-circuit. Dependency graph is trivially empty. No behavior
change vs. pre-Phase-8.

## Tests (TDD: write and fail first)

Every test authored before the code that makes it pass.

### Unit tests — sub-phase 8.0 (`bd-r82e` + new profile fields)

1. `profile_includes_records_direct_include`.
2. `profile_includes_records_transitive_includes`.
3. `profile_includes_dedupes_repeat_includes`.
4. `profile_includes_handles_cycles`.
5. `profile_v1_json_rejected_with_clean_error`.
6. `profile_v2_round_trip_with_includes`.
7. `profile_records_nav_dependencies_from_frontmatter`.
8. `profile_records_nav_dependencies_from_metadata_yml`.
9. `profile_records_always_render_true`.
10. `profile_always_render_default_false`.
11. `profile_records_body_link_targets` —
    `[link](../other.qmd)` → profile.body_link_targets contains
    "other.qmd" (project-relative).
12. `profile_body_link_targets_excludes_external_urls`.

### Unit tests — `cache_key`

13. `pass1_key_stable_for_identical_inputs`.
14. `pass1_key_changes_on_source_edit`.
15. `pass1_key_changes_on_metadata_yml_edit`.
16. `pass1_key_changes_on_quarto_yml_edit`.
17. `pass1_key_changes_on_include_content_change`.
18. `pass1_key_changes_on_include_path_change`.
19. `pass1_key_changes_on_format_id`.
20. `pass1_key_changes_on_extension_metadata`.
21. `pass1_key_changes_on_quarto_build_id`.
22. `pass1_key_independent_of_unrelated_files`.
23. `nav_config_hash_stable_for_identical_config`.
24. `nav_config_hash_changes_on_sidebar_edit`.
25. `nav_config_hash_changes_on_navbar_edit`.

### Unit tests — `profile_cache`

26. `profile_cache_miss_returns_none`.
27. `profile_cache_round_trip`.
28. `profile_cache_load_with_corrupt_json_returns_none_with_warning`.
29. `profile_cache_load_with_version_mismatch_returns_none`.
30. `profile_cache_no_op_without_cache_dir`.

### Unit tests — `dependency_graph`

31. `graph_includes_sidebar_co_membership_complete_subgraph` — three
    pages in one sidebar → 6 directed edges (each → other two).
32. `graph_includes_prev_next_neighbors`.
33. `graph_includes_body_link_targets`.
34. `graph_includes_user_declared_nav_dependencies`.
35. `graph_warns_on_unresolved_nav_dependency`.
36. `graph_reverse_edges_match_forward_edges` — for every
    `(u, v) ∈ edges` there is `(v, u) ∈ reverse_edges`. Property test.
37. `graph_force_render_includes_always_render_pages` —
    `project.always-render: true` → page in `force_render`.
38. `transitive_deps_finds_closure_via_reverse_edges` — Mode B's
    `needed_profiles` query: targets={X}, X depends on Y, Y on Z
    → result includes {X, Y, Z}.
39. `transitive_deps_terminates_on_cycles` — pathological cyclic
    `nav-dependencies` declaration; query returns finite set.
40. `implicit_target_pulls_in_always_render_dependents` — Mode B
    augmentation: page Q has `always-render: true` and depends on
    target X via reverse edge → Q joins `targets`.

### Unit tests — body-link / sidebar resolution equivalence

40a. `body_link_resolution_pass1_pass2_equivalent` — same fixture
     fed to `LinkResolutionStage` (Pass-1) and Phase 6's
     `LinkRewriteTransform` (Pass-2) → same target set.
40b. `sidebar_membership_pass1_pass2_equivalent` — same `auto:`
     fixture fed to `resolve_sidebar_membership` (Pass-1) and
     Phase 2's sidebar render (Pass-2) → same member list.

### Integration tests — Mode A (full project)

41. `mode_a_cold_run_renders_all_and_populates_cache` — fresh
    project → every page rendered; profile cache populated;
    nav-config-hash written.
42. `mode_a_warm_run_no_edits_still_renders_all` — render twice;
    Pass-2 still runs for every page (Mode A is full); but
    Pass-1 hits cache for every page. Reported summary like
    `"5 pages, 5 rendered (5 profile-cache hits)"`.
43. `mode_a_warm_run_after_body_edit_still_renders_all` — edit
    one page's body → every page Pass-2 runs anyway; that page's
    profile cache misses; siblings' profile caches hit.
44. `mode_a_warm_run_after_metadata_yml_edit_invalidates_subtree_profiles`
    — edit `chapters/_metadata.yml` → only chapters/* profile
    cache misses; every page still re-renders Pass-2 (Mode A).
45. `mode_a_warm_run_after_include_change_invalidates_parent_profile`
    — `bd-r82e` regression: parent's profile cache misses on
    child include change.

### Integration tests — Mode B (partial render)

46. `mode_b_single_target_renders_only_that_page` —
    `quarto render foo.qmd` in a project; foo has no nav
    dependencies → `_site/foo.html` is the only Pass-2 output.
    Other pages' output files unchanged on disk.
47. `mode_b_walks_dependency_closure_for_pass1` — foo has a
    body link to bar.qmd → bar's profile is re-extracted (or
    cache-hit) so foo's Pass-2 link rewriting can resolve;
    bar is *not* itself rendered.
48. `mode_b_user_declared_nav_dependency_is_followed_for_pass1` —
    foo declares `project.nav-dependencies: [b.qmd]`; b's profile
    is loaded for foo's render; b is not itself rendered.
49. `mode_b_always_render_pulls_dependent_into_targets` — q is
    in foo's reverse-dep set and has `project.always-render: true`
    → render `quarto render foo.qmd` → q is also rendered.
50. `mode_b_directory_arg_expands_to_targets` —
    `quarto render foo/` renders every `.qmd` under `foo/`.
51. `mode_b_multi_target_arg_renders_union` —
    `quarto render a.qmd b.qmd` renders {a, b} only.
52. `mode_b_unrelated_pages_outputs_byte_identical_to_pre_render` —
    confirm Mode B doesn't accidentally touch other pages.

### Integration tests — cache behavior, common to both modes

53. `pipeline_clean_flag_wipes_profile_cache_and_nav_hash` —
    pre-populate cache → `--clean` → cache empty before render.
54. `pipeline_corrupt_profile_cache_falls_through_to_live_extract`.
55. `pipeline_sitemap_merge_preserves_skipped_entries` —
    Mode B edit-one-render-one → other pages' sitemap
    `<lastmod>` is the *original* timestamp.
56. `pipeline_default_project_no_cache_io` — single-doc render
    (no cache_dir) → no `.quarto/cache/` writes.
57. `pipeline_unresolved_nav_dependency_warns_does_not_fail` —
    a `project.nav-dependencies: [missing.qmd]` declaration →
    diagnostic, edge dropped, render proceeds.

### CLI end-to-end (per CLAUDE.md §End-to-end verification)

58. **Mode A / Mode B smoke** at `/tmp/q2-phase8-smoke/`:
    ```
    _quarto.yml:
      project: { type: website, output-dir: _site }
      website:
        title: "Phase 8 Test"
        site-url: "https://example.com/site"
        sidebar:
          contents: [index.qmd, a.qmd, b.qmd, c.qmd]
    index.qmd, a.qmd, b.qmd, c.qmd  (a.qmd has [link to b](b.qmd))
    d.qmd  (not in any sidebar)
    ```
    Sequence:
    1. `quarto render` (Mode A, cold). Assert all 5 pages
       rendered; profile cache populated; `_site/` populated.
    2. `quarto render` (Mode A, warm). Assert all 5 pages
       rendered (Mode A always re-runs Pass-2); profile cache
       hits everywhere; HTML byte-identical.
    3. Edit `b.qmd` body. `quarto render` (Mode A). Assert all
       5 rendered; only b's profile cache misses on Pass-1.
    4. `quarto render a.qmd` (Mode B). Assert: only `_site/a.html`
       mtime advances; b's profile is loaded for a's link
       resolution but b is not rendered; index/c/d untouched.
    5. `quarto render a.qmd b.qmd` (Mode B multi). Assert:
       only a.html and b.html advance.
    6. `quarto render foo/` (Mode B, directory) on a fixture
       with `foo/x.qmd` and `foo/y.qmd`. Assert: only those
       two render.
    7. `quarto render --clean` then `quarto render` (Mode A).
       Assert: cache wiped before render; full re-render.
    8. Add `project.always-render: true` to d.qmd; add d.qmd to
       a's body links (so reverse-edges connect them);
       `quarto render a.qmd` (Mode B). Assert: d is also
       rendered (implicit target via always-render +
       reverse-edge).
    Record observed outputs and summary lines in close-out.

59. **Regression smokes**: re-run `/tmp/q2-phase{2..7}-smoke/`
    after `--clean`; assert byte-identical output.

60. **`bd-r82e` smoke**: parent → child include; Mode A render;
    edit child only; Mode A re-render → parent's profile cache
    misses; render correct.

### Snapshot tests

None — inline asserts cover the vocabulary.

## Work items (checklist)

### Preparation
- [x] Re-read `claude-notes/instructions/testing.md`,
      `coding.md`, `review.md`.
- [x] Confirm user agreement with Decisions 1–12.
- [x] Resolve open questions §"Open questions" below.
- [x] File `bd` issues:
      - `bd-fegm` parent under `bd-0tr6`.
      - `bd-r82e` (already filed) marked as a *blocker* of
        `bd-fegm`.

### Sub-phase 8.0 — `DocumentProfile` v2 + lifted helpers
- [x] Add `IncludeEntry` and `includes: Vec<IncludeEntry>` to
      `DocumentProfile`.
- [x] Add `nav_dependencies: Vec<PathBuf>`, `always_render: bool`,
      `body_link_targets: Vec<PathBuf>` to `DocumentProfile`.
- [x] Bump `DOCUMENT_PROFILE_VERSION` 1 → 2.
- [x] `IncludeExpansionStage` records spliced child paths +
      content hashes via the new `DocumentAst.recorded_includes`
      side-channel; merged back transitively from sub-recursion.
- [x] `DocumentProfileStage` drains include side-channel; reads
      `project.nav-dependencies` and `project.always-render` from
      merged metadata.
- [x] Extract `resolve_doc_relative_target` shared helper in
      `transforms/navigation_href.rs`. Phase 6's existing
      `resolve_doc_relative_href` is unchanged; both share the
      same path-normalization logic via `resolve_to_project_root`.
      Equivalence test `pass1_pass2_agree_on_target_set` passes.
- [x] `LinkResolutionStage` (new) in
      `stage/stages/link_resolution.rs`: walks the post-include
      AST, calls `resolve_doc_relative_target`, writes results
      to `profile.body_link_targets`. Inserted between
      `DocumentProfileStage` and `UnwrapProfileStage` in both
      the standard HTML pipeline and the WASM HTML pipeline,
      and in the orchestrator's `pass_one` stage list.
- [x] Extract `resolve_sidebar_membership` Pass-1-callable
      helper in `project/sidebar_membership.rs`. Reuses
      `expand_auto` from `transforms/sidebar_auto.rs` (lifted
      from `mod` to `pub(crate) mod`). Returns
      `Vec<ResolvedSidebar { id, members }>`.
- [x] Write prose contracts:
      - `claude-notes/designs/body-link-resolution-contract.md`.
      - `claude-notes/designs/sidebar-auto-expansion-contract.md`.
- [x] Update `claude-notes/designs/document-profile-contract.md`
      (four new fields + version bump + change-log entry).
- [x] Tests 1–12 (DocumentProfile v2), 40a (Pass-1/Pass-2
      link-resolution equivalence), plus 11 LinkResolutionStage
      stage tests, 7 sidebar_membership unit tests, and 2
      `bd-r82e` integration tests in
      `tests/document_profile_pipeline.rs`. Net delta: +42
      tests across the sub-phase.
- [x] Verified Phases 2–7 transforms still pass — added
      `Default` impl for `DocumentProfile` and updated all
      transform-test fixtures to use `..DocumentProfile::default()`.
      All 1337 quarto-core tests green.
- [x] WASM build clean (`hub-client && npm run build:wasm`).
- [x] `cargo xtask lint` and `cargo fmt --check` clean.

### Sub-phase 8.1 — Cache infrastructure
- [x] `cache_key.rs`: `pass1_key` (sha256 with length-prefixed
      encoding) + `quarto_build_id` (CARGO_PKG_VERSION) +
      `hex_encode` helpers via `sha2`. (Note: `nav_config_hash`
      deferred — Phase 8 doesn't act on it per Decision 8;
      will land if a future refinement needs it.)
- [x] `profile_cache.rs`: `load(runtime, key, include_resolver)`
      verifies cached profile's includes against current bytes;
      `save(runtime, key, &profile)`. Both swallow recoverable
      errors so the orchestrator never aborts on cache hiccups.
- [x] 30 tests (15 cache_key + 15 profile_cache including the
      4 include-verification tests).

### Sub-phase 8.2 — Dependency graph + render mode selection
- [x] `dependency_graph.rs`: `ProjectDependencyGraph` struct
      with forward `edges` and `reverse_edges` (built in one
      pass over the index).
- [x] Builder consuming `ProjectIndex` + sidebar membership
      (via lifted helper) + body-link targets (via profile) +
      user-declared `project.nav-dependencies`.
- [x] `force_render` includes pages with
      `project.always-render: true`.
- [x] `forward_closure(targets)` and `reverse_closure(targets)`
      queries.
- [x] `augment_targets_with_always_render(targets)` Mode B
      augmentation: `force_render` pages whose reverse-closure
      intersects `targets` join the render set.
- [x] Orchestrator profile cache wiring: `pass_one` calls
      `profile_with_cache`, which computes the cache key from
      source bytes + layered _metadata.yml + _quarto.yml +
      format id + source path, looks up via
      `profile_cache::load` (with include verification), and
      falls back to a live head pipeline on miss.
- [x] 13 dependency_graph unit tests + 10 incremental-rebuild
      integration tests (4 Mode B specific).
- [x] `RenderMode { Full, Subset(HashSet<PathBuf>) }` enum on
      `ProjectPipeline`; `with_mode()` builder method. Default
      `Full` keeps the existing CLI behavior backward-compatible.
- [x] Mode A wiring: `pass_one` over all pages with profile
      cache; `compute_augmented_render_set` returns `None`;
      `pass_two` filter no-ops.
- [x] Mode B wiring: full Pass-1 (cache makes it cheap),
      build dependency graph, augment user targets with
      always-render dependents whose reverse-closure intersects
      them, filter `pass_two` to render only the augmented set.
      Per-target absolute paths translated to project-relative
      for the graph query and back to absolute for the filter.
- [x] **Deviation from plan:** the original plan called for a
      "partial Pass-1 walk" in Mode B (profile only targets +
      their dependency closure). Implementation found a
      chicken-and-egg with sidebar `auto:` expansion: the
      membership resolver consults the index, which doesn't
      exist for non-target pages until they've been profiled.
      v1 ships full Pass-1 in both modes, leveraging the cache
      for warm-path speedup. The Pass-2 skip is the bigger
      perf win anyway (filters and engines cost more than
      profile extraction). Filing as a follow-up bead.
- [x] **`LinkResolutionStage` no-index resolution.** Discovered
      while wiring Mode B that `LinkResolutionStage` was reading
      `ctx.project_index` to resolve links — but the index
      doesn't exist during Pass-1 (it's *built from* Pass-1
      profiles). Fix: `resolve_doc_relative_target` now does
      pure path normalization (no index lookup); the dependency
      graph builder applies the index existence check when
      emitting edges. Updated tests + the contract doc.

### Sub-phase 8.3 — Sitemap merge (closes `bd-pphv`)
- [x] `website_post_render::write_sitemap` reads existing
      `<output_dir>/sitemap.xml`, parses each `<url>` block into
      `loc → lastmod`, refreshes entries for pages rendered this
      run (matched via the `outputs: &[RenderToFileResult]`
      param), preserves entries for skipped pages, and writes
      back sorted by `loc`.
- [x] `parse_sitemap_locs` is tolerant of malformed input —
      malformed `<url>` blocks are skipped, root-level parse
      failures degrade to fresh-write.
- [x] `RenderToFileResult.output_path` matched against
      `project.output_dir.join(profile.output_href)` to identify
      rendered pages.
- [x] 6 unit tests on the parser (round-trip, missing-lastmod,
      malformed input, escape preservation, extract_inner_tag
      simple+missing) + 1 integration test
      (`mode_b_sitemap_preserves_untouched_entries_lastmod`)
      end-to-end via Mode B render.
- [x] Close `bd-pphv`.

### Sub-phase 8.4 — CLI surface
- [ ] `--clean` flag: wipes `profiles/` and `nav-config-hash`,
      preserves `sass/`.
- [ ] CLI arg parsing: distinguish "no path arg" (Mode A) from
      "one or more path args" (Mode B); directory-arg expansion
      to constituent `.qmd` files.
- [ ] Summary line:
      - Mode A: `"5 of 5 rendered (4 profile-cache hits)"`.
      - Mode B: `"2 of 5 rendered (3 untouched, 4 profile-cache hits)"`.

### Sub-phase 8.5 — Integration tests + smoke
- [ ] Mode A tests 41–45.
- [ ] Mode B tests 46–52.
- [ ] Cache-behavior tests 53–57.
- [ ] CLI smoke (test 58) at `/tmp/q2-phase8-smoke/`.
- [ ] Regression smokes (test 59).
- [ ] `bd-r82e` smoke (test 60).

### Sub-phase 8.6 — Hub-client / WASM impact check
- [ ] Audit `crates/wasm-quarto-hub-client/src/`: confirm new
      cache paths are no-ops on WASM.
- [ ] Confirm new profile fields don't break WASM build.

### Verification and close-out
- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace`.
- [ ] `cargo xtask lint`.
- [ ] `cargo fmt --all -- --check`.
- [ ] `cargo xtask verify` (full, including WASM build).
- [ ] No snapshot drift from Phase 7.
- [ ] Follow-ups filed (each `discovered-from:bd-<phase8>`,
      parent-child to `bd-0tr6`):
      * `nav-dependencies` glob support
        (`[posts/*.qmd]`, `[chapters/**/*.qmd]`).
      * Smarter Mode B: detect "user-named target had a nav-config
        edit between runs that affects it" and pull in only the
        affected sidebar members rather than relying on the user
        to know.
      * Open-question follow-up: opt-in Pass-2 caching for users
        who explicitly assert filter purity (separate epic, not
        in Phase 8).
- [ ] Close `bd-pphv`, `bd-r82e`.
- [ ] Update epic plan §"Work items".
- [ ] Update §"Follow-up beads report (running log)".
- [ ] `br close bd-<phase8>`.
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** A user's Lua filter introduces a cross-doc dependency
  the graph builder can't see; warm renders show stale output.
  *Mitigation:* `nav-dependencies` declaration channel; `--full`
  escape hatch; `always-render: true` per-doc opt-out; `--clean`
  nuclear option. Warn loudly in user docs about the situation
  and the knobs.

- **Risk:** Body-link resolution has to run in Pass-1 (so
  `body_link_targets` lands on the profile), but
  `LinkRewriteTransform` runs in Pass-2 — these go out of sync.
  *Mitigation:* `LinkResolutionStage` (Pass-1) and
  `LinkRewriteTransform` (Pass-2) share a single resolver helper
  with a docstring stating the expected invariant; a unit test
  asserts both produce the same target set for the same AST.

- **Risk:** Sidebar resolution today requires Pass-1 to have
  completed (it reads profiles for `auto:` expansion). The
  dependency graph builder needs sidebar resolution. So the
  build order is: Pass-1 all → resolve sidebars → build graph
  → `changed` → Pass-2 over changed. Verify the existing
  sidebar resolver runs at the right point.
  *Mitigation:* sub-phase 8.2 task: confirm (or relocate) the
  sidebar resolver to run pre-graph.

- **Risk:** `nav_dependencies` field bloat. Most pages declare
  none; `Vec` default is `vec![]` which serializes as `[]`.
  *Mitigation:* `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
  on all three new collection fields. Same for `IncludeEntry`
  list.

- **Risk:** Closure algorithm's worst case is O(N²) per
  iteration; pathological projects could hit this.
  *Mitigation:* a real project has tens of pages, not
  thousands. Document the bound; if a real user hits it, add a
  reverse-edge index (page → pages-that-depend-on-it) for O(N)
  closure. Not v1.

- **Risk:** `previous_profile` PartialEq comparison is sensitive
  to field-order changes during serde deserialization.
  *Mitigation:* `DocumentProfile` derives `PartialEq` already
  (Phase 0). Add a property test: round-trip serialize/deserialize
  preserves PartialEq.

- **Risk:** Cache write succeeds for a page whose Pass-1 actually
  failed (e.g. partial write before crash).
  *Mitigation:* the existing `cache_set` in `NativeRuntime` is
  atomic-rename. Phase 8 piggybacks. Plus the version-check
  guard on load.

- **Risk:** A page that's never been profiled before (cold add)
  but isn't in any sidebar / nav config / referenced by anyone:
  is it picked up?
  *Mitigation:* file discovery still lists every `.qmd`. Pass-1
  runs on every discovered file; cold profile (no previous) →
  `changed` includes the page. Independent of the dependency
  graph entirely.

- **Risk:** `--clean` implementation isn't atomic; partial wipe
  leaves the cache in a half-state.
  *Mitigation:* wipe `profiles/` first (removes the harder
  half), then `nav-config-hash` (a single file). If wipe fails,
  abort with a clear error.

## Explicit non-goals for this phase

- No Pass-2 output cache.
- No engine output cache (`freeze`).
- No project-state cache.
- No tarred Page-scoped artifact replay.
- No hub-client / WASM cache backing (Phase 9).
- No cross-format invalidation.
- No parallel rendering.
- No filesystem watcher.
- No per-page Pass-2 skip *cache* — the skip is decided per run
  from the dependency graph; it's not a stored decision.
- No fine-grained nav-config invalidation (sidebar edit forces
  full re-render via the coarse hash; refinement is a follow-up
  if real users notice).
- No automatic detection of filter-introduced dependencies — the
  user declares them via `nav-dependencies`.

## Open questions (resolved 2026-04-27)

1. ~~**Body-only edits**~~ — *Resolved: not a concern in Phase 8.*
   Mode A always re-renders every page; Mode B renders exactly
   the user-named subset. There is no "Pass-2 skip for unchanged
   pages" path that body-only edits could fall through. The
   over-rendering Decision 6 v1 worried about (B's dependents
   re-render after a body-only edit to B) doesn't happen because
   Mode B never propagates to dependents at all — the user's
   `targets` is the rendered set. Decision 6 redrafted to mode-A
   / mode-B form.

2. ~~**`LinkResolutionStage` placement.**~~ *Resolved: lift the
   resolution helper.* Phase 8 extracts the project-relative
   link-resolution logic from Phase 6's `LinkRewriteTransform`
   into a shared `resolve_doc_relative_links` helper.
   `LinkResolutionStage` (Pass-1) and `LinkRewriteTransform`
   (Pass-2) call the same helper; a unit test asserts they
   produce the same target set. Captured in Decision 7.

3. ~~**Sidebar `auto:` expansion timing.**~~ *Resolved: prose
   contract + two implementations.* Phase 8 specifies sidebar
   `auto:` expansion as a deterministic algorithm with a prose
   contract at
   `claude-notes/designs/sidebar-auto-expansion-contract.md`,
   then implements two variants: a Pass-1 membership-only query
   (paths only, used by the dependency graph) and the existing
   Pass-2 sidebar transform (HTML rendering) layered on top.
   Equivalence test asserts agreement on shared fixtures.
   Captured in Decision 7.

4. ~~**`nav-dependencies` namespace.**~~ *Resolved: under
   `project.`.* So `project.nav-dependencies:` and
   `project.always-render:`. Captured in Decision 12.

5. ~~**Globs in `nav-dependencies`?**~~ *Resolved: no globs in
   v1.* Filed as a follow-up bead at close-out (`bd-<phase8>`-
   adjacent). Intent: add when a real user need surfaces; not
   testing it in the middle of the big feature.

6. ~~**Changes to `nav-dependencies` declarations.**~~ *Resolved:
   works automatically.* Declarations live in the page's
   metadata, hashed into the Pass-1 key. Edit triggers a Pass-1
   cache miss → profile re-extracted with new declarations →
   graph builder uses them. Sub-phase 8.2 has a test (test 48)
   confirming this end-to-end.

7. ~~**Reverse edge index.**~~ *Resolved: yes, day one.* Quarto
   websites can reach thousands of documents; O(N²) closure is a
   real concern. The `ProjectDependencyGraph` ships with
   `reverse_edges` built alongside `edges`. Captured in
   Decision 5.

## Decisions log (to confirm 2026-04-XX)

1. Cache lives at `<project>/.quarto/cache/profiles/` plus a
   one-line `nav-config-hash` file.
2. Pass-1 cache key is sha256 over source + layered metadata +
   project config + transitive includes + format extensions +
   versions + `quarto_build_id()`.
3. `quarto_build_id()` (release version, or git short hash on
   dev builds) baked into every cache key.
4. `DocumentProfile` v2 adds `includes`, `nav_dependencies`,
   `always_render`, `body_link_targets`. `DOCUMENT_PROFILE_VERSION`
   1 → 2 (covers all four).
5. `ProjectDependencyGraph` ships forward `edges` plus
   `reverse_edges` day one for O(N) propagation on
   thousand-page sites. Built from sidebar co-membership +
   prev/next + body-link targets + user-declared
   `project.nav-dependencies`. `force_render` triggered by
   `project.always-render: true`.
6. Two render modes: Mode A (`quarto render`, full project)
   re-renders every page's Pass-2 unconditionally; Mode B
   (`quarto render foo.qmd` / `foo/` / `a.qmd b.qmd c.qmd`)
   renders exactly `targets`, walks the graph to find which
   sibling profiles need re-extracting, runs Pass-1 only on
   those, no closure of Pass-2 onto dependents.
7. Body-link resolution and sidebar `auto:` membership both
   factored into Pass-1 helpers with prose contracts + Pass-2
   transforms layered on top + equivalence tests.
8. Nav-config-hash file written every run for diagnostics /
   future use; **does not** force re-render in Phase 8 since
   Mode A renders everything anyway.
9. `--clean` wipes `profiles/` + `nav-config-hash`, preserves
   `sass/`. No `--full` (Mode A is the full render). No
   `--no-cache`.
10. Sitemap fresh-write becomes read-merge-write; closes
    `bd-pphv`. In Mode A every entry's lastmod refreshes; in
    Mode B only `targets` entries refresh, others preserved.
11. Cache errors warn, never abort. Nav-config-hash *write*
    failure is the lone hard error.
12. `project.nav-dependencies` and `project.always-render` live
    under the `project.` namespace.

## Epic-level impact

Phase 8 closes the **rebuild-economy surface** for websites:

- Site navigation, resources, links, post-render outputs — Phases 2–7.
- **Dependency-aware partial rebuilds — Phase 8.**
- Hub-client live preview — Phase 9.

The dependency graph is the core deliverable. The profile cache
is supporting infrastructure (without it, Pass-1 dominates the
warm-path cost). Together they enable `quarto render foo.qmd` in
a project to render foo and only foo's transitively-affected
neighbors — the headline single-doc-preview use case.

`freeze` (separate epic) caches engine outputs at user opt-in;
Phase 8 leaves that surface untouched. The two compose: `freeze`
makes Pass-2 cheaper *when it runs*; Phase 8 makes Pass-2 *not
run* for unaffected pages. They address different costs.

`bd-pphv` (sitemap merge) closes as a side-effect of the
incremental loop. `bd-r82e` (DocumentProfile.includes) closes as
sub-phase 8.0. `bd-pdwr` (parallel rendering) becomes attractive
once Pass-2 cost dominates the warm path — orthogonal to Phase 8.

The dependency graph is also the substrate for Phase 9: hub-client
asks "if I edit page foo, which pages need re-rendering?" and
gets a precise answer from the same graph builder, with
`force_render` set to empty (no `--full`, no nav-config change in
the live-edit case).
