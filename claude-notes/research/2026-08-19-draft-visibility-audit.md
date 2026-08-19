# Draft visibility in q2: audit of enforcement sites

**Date:** 2026-08-19. Produced by a study agent during the
bd-sidebar-dir-index-md-5khf3lds investigation, after the user asked whether
q2 has a *structural* mechanism preventing draft pages from being linked, or
whether each feature must remember to filter. Answer: **ad-hoc per-feature,
and most features don't.** Follow-up strand: see "Recommendation" below.

## Where the flag lives

- `crates/quarto-core/src/document_profile.rs:444` — `pub draft: bool` on
  `DocumentProfile`; set in exactly one place (`document_profile.rs:793`)
  from front-matter/merged-metadata `draft: true` (strict bool, default
  false).
- q2 has **no** `website.drafts:` list and **no** `website.draft-mode:` key.
  `crates/quarto-core/src/transforms/draft_alert.rs:70-77` records this:
  every q2 draft is in Q1's `visible` mode (bd-w0o9 tracks the config
  surface). The only schema hits for those keys are inert pampa test
  fixtures copied from Q1.
- Drafts are **always fully rendered** (asserted as intended in
  `crates/quarto-core/tests/integration/llms_txt.rs:568`), get their
  resources copied, and receive only a banner + `<meta
  name="quarto:status" content="draft">` (`draft_alert.rs`,
  `template.rs:271`). There is no Q1-style `draft-remove` post-processor.
- The `_`/dot-prefix discovery exclusion
  (`crates/quarto-core/src/project/discovery.rs:378`) is genuinely
  structural but orthogonal: it makes files invisible to the whole
  pipeline; `draft: true` does not.

## Enforcement tally

`ProjectIndex` (`crates/quarto-core/src/project/index.rs`) has **zero**
draft-aware accessors (`new / profiles / lookup_by_source / lookup_by_href /
is_empty / len`). Non-test code reads `.draft` on only **5 lines** in the
workspace, in 3 subsystems:

| file:line | what |
|---|---|
| `sidebar_auto.rs:219,228,246` | `.filter(\|p\| !p.draft)` per `AutoSpec` arm |
| `aliases.rs:340` | `if profile.draft { continue }` in `plan_alias_stubs` |
| `llms.rs:152` | `!profile.draft` inside `profile_has_companion` |

Across the workspace: **25 `ProjectIndex` access sites; 6 apply a draft
check, 19 do not.** Enumeration sites (10): filtered — `sidebar_auto.rs:214`,
`llms_post_render.rs:121`, `website_post_render.rs:815`; unfiltered —
`website_post_render.rs:513-514` (sitemap), `listing_generate.rs:184`
(listings), `aliases.rs:527`, `project_resources.rs:901,1121`,
`dependency_graph.rs:138,209`. Lookup sites (15): checked — `llms.rs:288`,
`llms.rs:885`, `link_rewrite.rs:374` (all via `profile_has_companion`);
unchecked — `sidebar_auto.rs:357` (fixed by
bd-sidebar-dir-index-md-5khf3lds), `navigation_href.rs:184,354`,
`sidebar_generate.rs:268`, `navigation_enrich.rs:57`,
`llms_post_render.rs:443,605,608`, `website_canonical_url.rs:62`,
`resource_report.rs:102`, `dependency_graph.rs:152,164`.

The best-shaped existing piece is `profile_has_companion`
(`llms.rs:145-156`) — "the single answer shared by capture, `link-format`
resolution, and post-render so they can't drift" — the right shape, but
scoped to llms companion eligibility, not linkability.

## Live leak sites found

1. **Auto-sidebar section header** — `sidebar_auto.rs:357` promoted a draft
   `<dir>/index.qmd` to a linked, titled section header. *Fixed by
   bd-sidebar-dir-index-md-5khf3lds* (index now resolved among the
   draft-filtered members).
2. **Breadcrumbs** — second-order from (1): `breadcrumbs_render.rs` walks
   the resolved sidebar, so a leaked header reached the trail too.
3. **Sitemap** — `website_post_render.rs:513-514` enumerates all profiles;
   drafts get `<url><loc>` entries. Q1 gates this
   (`website-sitemap.ts:176-178`). Tracked: **bd-4zdf**.
4. **Listings / RSS** — `listing_generate.rs:184` has no draft check
   anywhere in `project/listing/**`; feed items derive from listing items,
   so drafts reach `index.xml`. Q1 filters
   (`website-listing-read.ts:1106`). **No strand covered this** before the
   centralization strand below.
5. **Body / cross-document links** — `navigation_href.rs:354` +
   `link_rewrite.rs` rewrite links to drafts normally; Q1 unlinks
   (`website-utils.ts:100`). Tracked: **bd-p4sc**.
6. **Explicit navbar/sidebar/footer items** — `navigation_href.rs:184`
   resolves author-written nav hrefs with no check, and
   `quarto_navigation::NavigationItem` has no `draft` field at all; Q1
   carries `draft?: boolean` on the item and gates in the nav templates.
7. **Enrichment** — `navigation_enrich.rs:57`, `sidebar_generate.rs:268`
   pull a draft's title into nav labels (mild; author asked for the item).
8. **llms.txt home metadata** — `llms_post_render.rs:443` reads
   `lookup_by_href("index.html")` uncheckd for site title/description.
9. **`warn_aliases_ignored`** (`aliases.rs:527`) — diagnostics-only.

## Q1's mechanism shape (for comparison)

Q1 is also per-feature at the call site, but factors the policy into shared
predicates in `website-utils.ts` (`projectDraftMode` — default **`gone`**,
forced `visible` under preview; `isDraftVisible`; `isProjectDraft`), and has
two backstops q2 lacks: `resolveInputTarget` returns `{outputHref, draft}`
so the bit travels with every resolution, and default `draft-mode: gone`
empties leaked pages at post-process. q2's default is Q1's `visible` mode
with none of the guards — strictest-content, weakest-enforcement corner.

## Recommendation (acted on)

Open a strand to centralize the policy: an `is_linkable` /
`linkable_profiles()` surface on `ProjectIndex` in the shape of
`profile_has_companion`, routed through every enumeration/lookup site; a
`draft: bool` on `NavigationItem` for explicit nav entries; landed *before*
bd-w0o9's `draft-mode` config so `visible`/`unlinked`/`gone` becomes one
policy value read in one place; bd-p4sc and bd-4zdf become thin consumers.
Include a guard test that no non-test code reads `.draft` outside the
predicate.
