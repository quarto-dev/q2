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
- `index`: the project's `ProjectIndex` (the set of every
  rendered page's `DocumentProfile`).

## Output

A project-relative `PathBuf` (forward-slash, e.g. `other.qmd`,
`docs/api.qmd`) when `raw` resolves to a project document, otherwise
`None`.

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

5. **Look up in `index`.** If the project-relative path matches a
   `DocumentProfile.source_path`, return that profile's
   `source_path` (which is canonical project-relative,
   forward-slash, by Phase-0 invariant). Otherwise return `None`.

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

For a fixed `(raw, source_relative, index)`:

- `resolve_doc_relative_target(raw, source_relative, index) == Some(p)`
  if and only if Phase 6's `resolve_doc_relative_href(raw, source_relative, _resolver, Some(index), _label, _diags)`
  rewrites `raw` to a string starting with the target's
  `output_href`.

- `resolve_doc_relative_target(raw, source_relative, index) == None`
  if and only if `resolve_doc_relative_href` returns the raw href
  unchanged (modulo the Pass-2 diagnostic for `.qmd`-shaped misses).

This equivalence is enforced by a unit test in
`navigation_href.rs` that exercises the same fixtures through both
helpers and asserts agreement.

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
