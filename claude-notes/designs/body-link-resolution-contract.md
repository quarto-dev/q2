# Body-link resolution contract

**Status:** Initial draft, 2026-04-27 (Phase 8 sub-phase 8.0).
**Code:**

- `crates/quarto-core/src/transforms/navigation_href.rs` —
  `resolve_doc_relative_target` (Pass-1 / static query) and
  `resolve_doc_relative_href` (Pass-2 / rewrite).
- `crates/quarto-core/src/stage/stages/link_resolution.rs` — Pass-1
  stage that walks the AST and writes results to
  `DocumentProfile.body_link_targets`.
- `crates/quarto-core/src/transforms/link_rewrite.rs` — Pass-2
  transform that rewrites `Inline::Link.target.0` in place.

## What this contract is for

Body-link resolution decides which **other project documents** a
given page links to from its body content. The Phase-8 dependency
graph reads this set — every entry becomes a graph edge from the
source page to the target — so it must be:

1. **Statically derivable.** Computable from the parsed AST + the
   project's file inventory, with no engine execution / Lua filter /
   user-controllable computation in the loop.
2. **Deterministic.** Same input → same set of targets, every run.
3. **Equivalent across Pass-1 and Pass-2.** The set of targets the
   Pass-1 stage records is the same set Phase 6's Pass-2 transform
   would rewrite. A unit test asserts equivalence.

This contract describes the algorithm so users (and future
implementations) have a stable target.

## Inputs

- `raw`: the link's `target.0` URL string as it appears in the AST
  immediately after `IncludeExpansionStage` runs (so transitive
  include content participates).
- `source_relative`: the page's project-relative source path,
  forward-slash separated (e.g. `chapters/intro.qmd`,
  `posts/2025/welcome.qmd`).

**Note (Phase 8.2):** the Pass-1 helper does *not* take a
`ProjectIndex` argument. Pass-1 runs *during* index construction
— at the per-page profile checkpoint, before the index exists —
so an index parameter would be `None` at the point of need. The
helper instead returns the resolved project-relative path for
any internal `.qmd` reference; the dependency-graph builder
applies the index-existence filter when emitting edges. Phase 6's
Pass-2 helper (`resolve_doc_relative_href`) does take an index
because it runs after the index is fully built.

## Output

A project-relative `PathBuf` (forward-slash, e.g. `other.qmd`,
`docs/api.qmd`) for any internal `.qmd` reference; otherwise
`None` (external URLs, fragment-only anchors, non-`.qmd` paths).
The result reflects path normalization only — it is *not* a
guarantee that the target exists in the project.

## Algorithm

1. **External URLs and fragment-only anchors return `None`.**
   Anything matching `is_external` (any scheme, `//host/...`,
   `mailto:`, `tel:`, `ftp:`) or starting with `#` is not a project
   reference. No further work.

2. **Strip `?query` and `#fragment` tails.** The lookup is on the
   path portion only. Tails are not retained at this layer (Pass-2
   re-appends them when rewriting; Pass-1 doesn't need them).

3. **Non-`.qmd` paths return `None`.** Static resources (images,
   CSS, downloadable files) are not project documents. Phase 6
   passes them through verbatim with no diagnostic; Phase 8's graph
   ignores them.

4. **Resolve to project root.** Apply the source-relative path
   resolution rule:

   - A leading `/` in `raw`'s path part means project-root-absolute.
     Strip the slash; the result is the project-relative path
     (no `source_relative` involvement).

   - Otherwise, the path is relative to the source document's
     *directory*: `dirname(source_relative)`. Walk the components,
     collapsing `.` (drop) and `..` (pop the most recent kept
     component). Extra `..` above the project root are clamped (no
     error).

5. **Return the resolved path.** Wrap as a `PathBuf` and return
   `Some(p)`. The caller (the Phase-8 dependency-graph builder)
   is responsible for any index-existence filter. Pass-2's
   helper does its own index lookup for diagnostics + rewriting.

## Examples

| Source page | Raw href | Resolved target |
|---|---|---|
| `index.qmd` | `about.qmd` | `about.qmd` |
| `index.qmd` | `docs/api.qmd` | `docs/api.qmd` |
| `docs/api.qmd` | `../about.qmd` | `about.qmd` |
| `docs/api.qmd` | `/other.qmd` | `other.qmd` |
| `docs/api.qmd` | `tutorial.qmd` | `docs/tutorial.qmd` |
| `posts/p.qmd` | `../about.qmd?ref=foo` | `about.qmd` |
| `posts/p.qmd` | `../about.qmd#section` | `about.qmd` |
| any | `https://example.com` | None (external) |
| any | `#section` | None (fragment-only) |
| any | `image.png` | None (non-`.qmd`) |
| any | `missing.qmd` | None (no index hit) |

## Equivalence with Pass-2 rewrite

The two helpers agree on path resolution but differ on index
gating:

- For internal `.qmd` references that path-resolve *and* exist in
  the index: Pass-1 returns `Some(p)`; Pass-2 rewrites the href to
  the target's `output_href`. Both produce the same `p`.

- For internal `.qmd` references that path-resolve but *don't*
  exist in the index: Pass-1 returns `Some(p)` (the resolved
  path); Pass-2 leaves the href unchanged and emits a diagnostic.
  The Phase-8 dependency-graph builder filters such targets out
  before emitting edges, so the dep-graph view of body-link
  edges still matches what Pass-2 actually rewrites.

- For external URLs / fragment-only anchors / non-`.qmd` paths:
  both return `None` / unchanged respectively.

A unit test in `navigation_href.rs`
(`pass1_pass2_agree_on_resolved_path_when_both_hit`) asserts this
equivalence on shared fixtures.

## What this contract does NOT cover

- **Image hrefs** (`Image::target.0`). Phase 6 leaves them alone;
  Phase 8 doesn't add edges for them. Images point at static
  resources, not project documents.
- **Reference-style markdown links** (`[text][1]`). qmd doesn't
  support them at all.
- **Cross-format reference resolution** (HTML→PDF). Out of website-
  epic scope; the index keys on `(source_path, format_id)` and
  cross-format edges are tracked separately if/when needed.
- **Draft-mode visibility filtering.** Phase 6's `bd-p4sc` (draft
  mode) layers on top — body-link resolution returns the target
  regardless of draft flag; the rewrite step decides what to do
  about drafts.

## When to bump this contract

Any change to the algorithm above that would alter the set of
targets returned for an existing `(raw, source_relative, index)`
input requires:

1. Updating this document.
2. Bumping `DOCUMENT_PROFILE_VERSION` (because
   `body_link_targets` would shift on cached profiles).
3. Bumping `PROFILE_KEY_VERSION` (because the set of targets is a
   profile-derived field and cache hits would otherwise serve
   stale data).

Pure refactors (private helper renaming, additional non-`.qmd` skip
shortcuts that don't change the result set) do not require a bump.
