# `aliases:` is silently ignored — no redirect stubs written (bd-aliases-redirects-missing-sch7cd1g)

**Date:** 2026-08-12
**Braid:** `bd-aliases-redirects-missing-sch7cd1g` (p2, feature, label `website`)
**Duplicate of / duplicated by:** `bd-hzwecpyk` "Implement page aliases (URL redirects) for website projects" (cderv, 2026-06-23) — see § Duplicate strand
**Checkout:** invoked on `main` @ `1ba0f2ec` (no worktree created — this skill works in place)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The feature is cleanly unimplemented (not broken), the strand's
code pointers are accurate at HEAD, the Q1 reference implementation is small and fully
read, and q2's existing `website_post_render.rs` + `ProjectIndex` give us a *better*
substrate than Q1 had. The only things blocking a real plan are policy choices
(collision handling, stub template fidelity, draft interaction) and the duplicate-strand
question — all listed below.

## Issue context

`aliases:` in document front matter is a Quarto 1 website feature: each listed path gets
a small HTML redirect stub written into the output directory, so old URLs keep working
after pages are renamed or moved. q2 drops the key entirely — no stubs, **and no
diagnostic**. The silence is the expensive part: a porting project gets no signal that
its redirects are gone.

Filed 2026-08-12 by "Claude (q2-connect-docs)" while porting the Posit Connect docs.
Origin strand in that project's own skein: `br-aliases-redirects-8gk18exz`.

**Scale of the real-world hit.** 69 Connect-docs source files declare `aliases:`; the Q1
reference site has 99 redirect stubs the q2 render omits. That accounts for the *entire*
451-vs-352 HTML file-count gap between the two rendered sites — content-wise the port is
otherwise at 352/352 pages with zero errors. This is the single largest remaining
structural difference.

## Dependency graph

**Empty.** `braid dep tree` / `braid dep list` on this strand return no edges. It was
filed cross-project, so it carries no `discovered-from` chain inside the q2 skein.
That means no incoming pressure from the graph itself — the urgency argument is entirely
the Connect-docs port (`bd-wch2dotq`, open).

Neighbors found by search rather than by edge:

| Strand | Status | Relevance |
| --- | --- | --- |
| `bd-hzwecpyk` | open | **Duplicate.** Same feature, filed by cderv 2026-06-23, motivated by Hugo blog migration. Has a `blocks` edge to `bd-0tr6`. |
| `bd-0tr6` | open (epic) | Website projects epic. Its MVP scope statement **explicitly excludes `aliases`** — that exclusion is why this is unimplemented, and it is the thing we'd be revisiting. |
| `bd-wch2dotq` | open | "Make q2 render the posit-connect docs" — the umbrella this was discovered under, in the other project. Natural `related` parent here. |
| `bd-yu16` | open | Websites phase 7 (sitemap/favicon/site-url/title-prefix). The module we'd extend. |
| `bd-4zdf` | open | Draft-mode interaction with sitemap. The same question recurs for alias stubs. |

### Duplicate strand

`bd-hzwecpyk` and `bd-aliases-redirects-missing-sch7cd1g` are the same feature. The
older one has the correct graph position (`blocks bd-0tr6`); the newer one has all the
context (root-cause analysis, repro, file-count evidence). **Recommendation:** keep the
newer strand as the implementation strand, add its missing edges
(`blocks bd-0tr6`, `related bd-wch2dotq`), and close `bd-hzwecpyk` with
`--reason "Duplicate of bd-aliases-redirects-missing-sch7cd1g"` — but see design
question 1; closing another person's strand is the user's call.

## What the code looks like today

Every code pointer in the strand description checks out at `1ba0f2ec`.

**Nothing reads `aliases`.** Every grep hit under `crates/` is unrelated (SASS theme
aliases, YAML anchor aliases, highlight-language aliases, fnm path aliases). There is
also no `aliases` entry in any q2 YAML schema, which is why the key is dropped without
even an unknown-key warning.

**`DocumentProfile`** (`crates/quarto-core/src/document_profile.rs`) has no `aliases`
field. The extraction site at `:695` already does
`categories: extract_string_list(meta, "categories")` — the alias list is exactly that
shape. `DOCUMENT_PROFILE_VERSION` is `9` (`:83`); adding a field bumps it to `10`, which
is *free* correctness-wise because `crates/quarto-core/src/project/cache_key.rs` folds
the version into the cache-key hash domain — stale cached profiles self-invalidate.
There is also a `assert_eq!(DOCUMENT_PROFILE_VERSION, 9)` guard test at `:1662` that
must be updated deliberately.

**`crates/quarto-core/src/project/website_post_render.rs`** (757 lines) is the right
home. It already owns this exact class of work: `copy_favicon`, `write_sitemap`,
`write_robots_txt`, all native-only (`#[cfg(not(target_arch = "wasm32"))]`), all
short-circuiting when their config is absent, all called from
`orchestrator.rs:380-383`. `write_sitemap` in particular already walks
`index.profiles()` and maps each to `project.output_dir.join(&profile.output_href)`.

**`ProjectIndex`** (`crates/quarto-core/src/project/index.rs`) exposes
`lookup_by_href(&str)` over a `by_output_href` map — that is precisely the
"would this stub overwrite a real page?" guard, available for free.

### q2 has a structural advantage over Q1 here

Q1's `updateAliases` spends ~60 of its ~120 lines on an incremental-render workaround: on
an incremental build it re-walks *every* project input, re-reads each one's `aliases`,
and re-adds them to the map (with `allowNewAnchors=false`) so that a redirect file
claimed by several pages isn't rewritten with only the subset's entries.

**q2 does not need any of that.** Pass 1 profiles *every* project file unconditionally;
`RenderMode::Subset` / `ActivePage` only filter Pass 2
(`compute_augmented_render_set`, `orchestrator.rs:1183+`). So `index.profiles()` in
`post_render` always carries the complete alias set regardless of render mode. The q2
implementation should be roughly half the size of Q1's.

### Q1 reference behavior (read in full; copies preserved under the investigation dir)

Source: `external-sources/quarto-cli/src/project/types/website/website-aliases.ts`
and `.../resources/projects/website/templates/redirect-map.ejs`. Both are copied to
`claude-notes/plans/aliases-redirect-stubs-investigation/` so the plan is readable
without an `external-sources/` checkout.

1. **Path fixup** (`toAnchor` / `fixupHref`): alias ending in `/` → append `index.html`;
   alias with no extension → append `/index.html`; otherwise used as-is.
2. **Resolution**: alias starting with `/` is site-root-relative → `join(outputDir, alias[1..])`.
   Otherwise it is relative to the *declaring page's output file* → `join(dirname(outputFile), alias)`.
3. **Hash fragments**: an alias may carry `#frag`. Multiple aliases pointing at the same
   stub path but different fragments collapse into one stub with a `{frag: href}` map;
   the fragment-less one lands under key `""`.
4. **Href in the stub** is `relative(dirname(stubPath), targetOutputFile)` — a relative
   href, so the site works under any base path.
5. **Collision with a real output**: Q1 **warns and skips** —
   `` `Requested alias ${targetFile} -> ${offendingAlias.outputFile} would overwrite the target. Skipping.` ``
   *This corrects the strand description*, which says "Q1 lets the stub overwrite". It
   does not. (What Q1 *doesn't* catch is a case-insensitive-filesystem collision, since
   the guard is an exact string compare — that is the confusing Connect-docs result the
   strand is remembering.)
6. **Two pages claiming the same alias with no fragment**: last writer into
   `redirects[""]` wins, silently. No warning.
7. **Template**: `<title>Redirect</title>`, a JS-only `window.location.replace`,
   preserving hash and query string. No `<meta http-equiv="refresh">`, no `<noscript>`
   fallback, no `rel="canonical"`.

### Ordering hazard

`post_render` runs **before** the resource-copy pass
(`orchestrator.rs:937-960`, "Runs after every project type's post_render"). A stub
written into a path that a declared `resources:` glob also targets would be silently
clobbered by the later copy. Q1 has no equivalent guard either. Worth a decision, not
necessarily a fix.

### Repro — confirmed at HEAD

The external repro is copied into
`claude-notes/plans/aliases-redirect-stubs-investigation/repro/` (so nothing depends on
an absolute path outside the repo). `current/index.qmd` declares:

```yaml
aliases:
  - /old-name.html
  - ../previous/index.html
```

Re-run at `1ba0f2ec`:

```
$ cargo run --bin q2 -- render claude-notes/plans/aliases-redirect-stubs-investigation/repro
Rendering project: .../repro (type: website)
Rendered 2 of 2 files to .../repro/_site

$ find _site -name '*.html'
_site/current/index.html
_site/index.html
```

Two files where Q1 writes four (`old-name.html` and `previous/index.html` are the
missing stubs). **No diagnostic of any kind is emitted.** Output inspected directly;
`_site/` was removed afterward so only the sources are committed.

### Alias corpus in the Connect docs (measured)

Extracting only the front-matter `aliases:` blocks from the 69 declaring files in
`docs-quarto-2` yields **106 unique alias entries**. Shape breakdown:

| Shape | Count | Rule needed |
| --- | --: | --- |
| Site-root-relative (`/…`) | 76 | join against `output_dir` |
| Page-relative (`../…`, `./…`) | 30 | join against the page's own output dir |
| Trailing slash (`…/`) | 77 | append `index.html` |
| Ends in `.html` | 18 | use as-is |
| Extensionless, no slash | ~11 | append `/index.html` |
| Carries a `#fragment` | 5 | multi-entry redirect map |

**Every one of Q1's `fixupHref` branches is exercised** — the trailing-slash rule alone
covers 77 of 106 entries, so it is not an edge case.

**Fragments are not deferrable, and the multi-page-one-stub case is real.** Two
*different* source files both claim the stub `/cookbook/custom-execution-environments/index.html`:

- `cookbook/off-host-execution/creating-execution-environments/index.qmd` declares both
  `/cookbook/custom-execution-environments` (no fragment) and
  `/cookbook/custom-execution-environments/#create-the-image`
- `cookbook/operations/deploying-content/index.qmd` declares
  `/cookbook/custom-execution-environments/#deploying-the-content`

The correct single stub therefore has three entries pointing at **two different target
pages**. A fragment-less first cut would either drop entries or silently send
`#deploying-the-content` to the wrong page. `/cookbook/runtime-caches/` has the same
two-fragment shape.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Port the external repro into
  `crates/quarto-core/tests/integration/website_post_render.rs`, which already has a
  `render_project(...)` harness driving the real project pipeline. Failing tests first:
  absolute alias, page-relative alias, extensionless alias, trailing-slash alias,
  hash-fragment alias, two pages claiming one alias, alias colliding with a real page.
  Plus a `document_profile.rs` unit test for extraction.
- **Phase 1 — `DocumentProfile::aliases`.** Add `pub aliases: Vec<String>`, extract via
  `extract_string_list(meta, "aliases")`, bump `DOCUMENT_PROFILE_VERSION` 9 → 10, update
  the version-guard test and the contract doc
  (`claude-notes/designs/document-profile-contract.md`) change log.
- **Phase 2 — Alias resolution.** Pure function: `(alias, profile) -> (stub_output_path, fragment, href_back_to_page)`.
  Unit-testable with no filesystem. Implements the Q1 fixup + resolution rules.
- **Phase 3 — `write_alias_redirects` in `website_post_render.rs`.** Build the stub map
  from `index.profiles()`, apply the collision policy (design question 2), render the
  template, write via `runtime`. Wire into `orchestrator.rs` alongside `write_sitemap`.
- **Phase 4 — Diagnostics.** Whatever design questions 2/5 settle on: collision warning,
  same-alias-two-pages warning, and possibly a "`aliases:` ignored outside website
  projects" notice. New `Q-*` codes need catalog entries **and** `docs/errors/` pages in
  the same commit (see the `error-docs-page-missing` lint).
- **Phase 5 — End-to-end verification.** `cargo run --bin q2 -- render` on the repro
  fixture; inspect the actual stub bytes; diff against the Q1 render. Then re-check the
  Connect-docs file-count gap (451 vs 352) and report the new number.
- **Phase 6 — Docs.** User-facing page under `docs/` describing `aliases:`.

## Open design questions for the user

1. **Duplicate strands.** `bd-hzwecpyk` (cderv, 2026-06-23) is the same feature and
   already carries the `blocks bd-0tr6` edge. Close it as a duplicate in favor of this
   one, close *this* one in favor of it, or merge the context into the older strand and
   keep that id? (I did not touch either — closing someone else's strand is your call.)

2. **Collision policy.** Q1 warns-and-skips when a stub path equals a real output path,
   and silently last-write-wins when two pages claim the same *fragment-less* alias.
   Note the corpus measurement above: two pages sharing one stub **already happens in the
   Connect docs**, though there it is benign (the collision is between distinct
   fragments, which merge correctly). A genuine fragment-less collision would be silent.
   Options: (a) match Q1 exactly; (b) match Q1 but *also* warn when two pages claim the
   same fragment key; (c) escalate to a hard error with a catalogued `Q-*` code. My
   inclination is (b) — silent last-write-wins is how you get a redirect pointing at the
   wrong page and never find out. Which do you want?

3. **Case-insensitive collisions.** Q1's guard is an exact string compare, so on macOS a
   stub at `Old-Name.html` happily overwrites an output at `old-name.html`. The strand
   says we have already hit this in the Connect docs. Do we add a case-insensitive
   collision check (diverging from Q1, catching a real bug), or match Q1 and leave it?

4. **Stub template fidelity.** Copy Q1's template byte-for-byte, so the Connect-docs
   Q1-vs-q2 diff comes out clean? Or write a better stub — `<meta http-equiv="refresh">`
   plus `<link rel="canonical">` plus a `<noscript>` link, so the redirect survives
   JS-disabled clients and is legible to crawlers? Byte-parity makes port verification
   trivial; the better stub is, well, better. I'd suggest byte-parity *now* (it's the
   thing that closes the Connect-docs gap) with a follow-up strand for the improved stub.

5. **Silence outside website projects.** Once this lands, a `default`-type project (or a
   single-file render) with `aliases:` still drops the key silently — the exact complaint
   the strand was filed about, just narrower. Emit a warning there, or accept the silence
   as Q1-compatible?

6. ~~**Hash fragments** — defer or not?~~ **Answered by the corpus measurement: not
   deferrable.** 5 of the Connect docs' 106 aliases carry fragments, and two of them
   merge into a stub shared by two different pages pointing at two different targets.
   A fragment-less first cut would produce wrong redirects, not merely incomplete ones.
   Flagging rather than asking — say so if you disagree and want a staged landing anyway.

7. **Drafts.** Should a page marked `draft: true` emit its alias stubs? `bd-4zdf` has the
   same open question for the sitemap. Same answer for both, presumably — but which?

8. **Stale stubs.** If an alias is removed from front matter, its stub lingers in
   `_site/` across incremental renders. Q1 has the same behavior. Out of scope, or track
   as a follow-up strand?

## Risks / tradeoffs (draft)

- **`DOCUMENT_PROFILE_VERSION` bump is cheap but not invisible.** `cache_key.rs` folds
  the version into its hash, so every project's profile cache invalidates once on the
  first render after this lands. Expected, worth mentioning in the changelog.
- **Revisiting an explicit epic exclusion.** `bd-0tr6`'s MVP scope statement names
  `aliases` in its exclusion list. Implementing it isn't a conflict — the MVP shipped —
  but the epic description should be amended so the exclusion list doesn't read as
  current policy.
- **The repro lives outside the repo.** Phase 0 must copy the fixture in; a test that
  reaches into `~/repos/github/cscheid/q2-connect-docs/` is not a test.
- **Post-render ordering.** Stubs are written before the resource copy pass, so a
  resource can silently clobber a stub. No consumer has hit this; flagging rather than
  fixing.
